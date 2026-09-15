pub mod call_hierarchy;
pub mod code_actions;
pub mod code_lens;
pub mod completion;
pub mod diagnostics;
pub mod hover;
pub mod implementation;
pub mod inlay_hints;
pub mod navigation;
pub mod position;
pub mod references;
pub mod scope;
mod source_functions;

pub mod signature_help;
pub mod state;
pub mod symbols;

use std::collections::BTreeSet;
use std::sync::Arc;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use state::WorldState;

/// Log level for the LSP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Parse a log level from an environment variable value.
    fn from_env(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// Determine the log level from CLI verbose flag and DOVETAIL_LSP_LOG env var.
    /// CLI --verbose overrides to Debug. Env var takes precedence if set.
    fn resolve(verbose: bool) -> Self {
        if let Ok(val) = std::env::var("DOVETAIL_LSP_LOG") {
            if let Some(level) = Self::from_env(&val) {
                return level;
            }
        }
        if verbose { Self::Debug } else { Self::Warn }
    }
}

struct DovetailLanguageServer {
    client: Client,
    state: Arc<WorldState>,
    log_level: LogLevel,
    progress_counter: std::sync::atomic::AtomicU32,
}

impl DovetailLanguageServer {
    fn log(&self, level: LogLevel, msg: &str) {
        if level <= self.log_level {
            let tag = match level {
                LogLevel::Error => "ERROR",
                LogLevel::Warn => "WARN",
                LogLevel::Info => "INFO",
                LogLevel::Debug => "DEBUG",
                LogLevel::Trace => "TRACE",
            };
            eprintln!("[dovetail-lsp {tag}] {msg}");
        }
    }

    /// Send a show_message notification without blocking the current task.
    /// Uses fire-and-forget to avoid deadlocking on a full client channel.
    fn show_message_nonblocking(&self, typ: MessageType, message: String) {
        let client = self.client.clone();
        tokio::spawn(async move {
            client.show_message(typ, message).await;
        });
    }

    async fn cancel_progress(&self, params: WorkDoneProgressCancelParams) {
        let latest = self
            .progress_counter
            .load(std::sync::atomic::Ordering::Relaxed)
            .wrapping_sub(1);
        if params.token == NumberOrString::Number(latest as i32) {
            self.state.cancel_analysis();
        }
    }

    /// Allocate a fresh, unique progress token.
    fn next_progress_token(&self) -> NumberOrString {
        let id = self
            .progress_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        NumberOrString::Number(id as i32)
    }

    /// Run a full workspace check and publish diagnostics to the client.
    async fn check_and_publish(&self) {
        let cancel = self.state.begin_dependency_analysis();
        let task_cancel = cancel.clone();
        let start = std::time::Instant::now();

        // Set up LSP work-done progress (fire-and-forget to avoid deadlock when the
        // client socket is not actively drained, e.g. in tests).
        let token = self.next_progress_token();
        {
            let client = self.client.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let _ = client
                    .send_request::<request::WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                        token,
                    })
                    .await;
            });
        }

        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title: "Dovetail".into(),
                        cancellable: Some(true),
                        message: Some("Checking workspace...".into()),
                        percentage: Some(0),
                    },
                )),
            })
            .await;

        // Channel to bridge progress from the blocking thread to async notifications
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(usize, usize, String)>();

        let client_for_progress = self.client.clone();
        let progress_token = token.clone();
        let forwarder = tokio::spawn(async move {
            while let Some((index, total, name)) = rx.recv().await {
                let percentage = if total > 0 {
                    ((index as u32) * 100) / (total as u32)
                } else {
                    0
                };
                client_for_progress
                    .send_notification::<notification::Progress>(ProgressParams {
                        token: progress_token.clone(),
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                            WorkDoneProgressReport {
                                cancellable: Some(true),
                                message: Some(if total == 0 {
                                    name
                                } else {
                                    format!("Checking {} ({}/{})", name, index + 1, total)
                                }),
                                percentage: Some(percentage),
                            },
                        )),
                    })
                    .await;
            }
        });

        let state = Arc::clone(&self.state);
        let join_result = tokio::task::spawn_blocking(move || {
            let callback = move |index: usize, total: usize, name: &str| {
                let _ = tx.send((index, total, name.to_string()));
            };
            state.analyze_workspace(Some(Arc::new(callback)), false, task_cancel)
        })
        .await;

        // Wait for all queued progress notifications to be sent
        forwarder.await.ok();

        let _publication = self.state.publication_gate.lock().await;
        let join_result = if self.state.is_current_analysis(&cancel) {
            join_result
        } else {
            Ok(None)
        };
        let diags_by_file =
            match join_result {
                Ok(Some(d)) => {
                    self.log(LogLevel::Debug, &format!(
                    "check_and_publish: workspace check succeeded, {} files with diagnostics",
                    d.len()
                ));
                    d
                }
                Ok(None) => {
                    self.client
                        .send_notification::<notification::Progress>(ProgressParams {
                            token,
                            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                                WorkDoneProgressEnd {
                                    message: Some("Analysis superseded or cancelled".into()),
                                },
                            )),
                        })
                        .await;
                    return;
                }
                Err(e) => {
                    self.log(LogLevel::Error, &format!("workspace check panicked: {e}"));
                    self.client
                        .send_notification::<notification::Progress>(ProgressParams {
                            token,
                            value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                                WorkDoneProgressEnd {
                                    message: Some("Check Failed".into()),
                                },
                            )),
                        })
                        .await;
                    self.show_message_nonblocking(
                        MessageType::ERROR,
                        format!("Dovetail workspace check panicked: {e}"),
                    );
                    return;
                }
            };

        // Collect URIs that previously had diagnostics so we can clear stale ones
        let previously_published = {
            let prev = self.state.previously_published_files.read().unwrap();
            prev.clone()
        };

        let mut now_published = std::collections::HashSet::new();

        // Publish new diagnostics
        let has_errors = diags_by_file.values().any(|diags| {
            diags
                .iter()
                .any(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        });
        for (uri, diags) in &diags_by_file {
            now_published.insert(uri.clone());
            self.client
                .publish_diagnostics(uri.clone(), diags.clone(), None)
                .await;
        }

        // Clear diagnostics for files that no longer have errors
        for uri in &previously_published {
            if !now_published.contains(uri) {
                self.client
                    .publish_diagnostics(uri.clone(), vec![], None)
                    .await;
            }
        }

        // Update tracking set
        {
            let mut prev = self.state.previously_published_files.write().unwrap();
            *prev = now_published;
        }

        let file_count = diags_by_file.len();
        let elapsed = start.elapsed();
        self.log(
            LogLevel::Info,
            &format!("check_and_publish: {file_count} files with diagnostics in {elapsed:.1?}"),
        );

        // End progress
        let end_message = if has_errors {
            format!("Check failed ({elapsed:.1?})")
        } else {
            format!("Check passed ({elapsed:.1?})")
        };
        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token,
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some(end_message.clone()),
                })),
            })
            .await;

        if has_errors {
            self.show_message_nonblocking(MessageType::WARNING, format!("Dovetail: {end_message}"));
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DovetailLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.log(
            LogLevel::Debug,
            &format!("initialize: root_uri={:?}", params.root_uri),
        );

        // Extract workspace root
        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                let mut root = self.state.workspace_root.write().unwrap();
                *root = Some(state::physical_path(&path));
            }
        }

        // Emit prelude sources to disk for goto-definition
        self.state.emit_prelude_sources();

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into()]),
                    retrigger_characters: Some(vec![",".into()]),
                    ..Default::default()
                }),
                inlay_hint_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
                call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "dovetail-language-server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.log(LogLevel::Debug, "initialized: starting workspace check");
        self.client
            .log_message(MessageType::INFO, "Dovetail language server initialized")
            .await;

        // Register a file watcher for Dovetail.toml so we re-check the workspace
        // when the manifest changes outside the editor (e.g. via the CLI).
        let watcher_registration = Registration {
            id: "dovetail-toml-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                watchers: vec![FileSystemWatcher {
                    glob_pattern: GlobPattern::String(
                        self.state
                            .workspace_root
                            .read()
                            .unwrap()
                            .as_ref()
                            .map(|root| format!("{}/{{Dovetail.toml,Dovetail.lock}}", root.display()))
                            .unwrap_or_else(|| "{Dovetail.toml,Dovetail.lock}".into()),
                    ),
                    kind: None,
                }],
            })
            .ok(),
        };
        let client = self.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client.register_capability(vec![watcher_registration]).await {
                eprintln!("[dovetail-lsp WARN] failed to register Dovetail.toml watcher: {e}");
            }
        });

        self.check_and_publish().await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.log(
            LogLevel::Debug,
            &format!(
                "did_change_watched_files: {} change(s), triggering check",
                params.changes.len()
            ),
        );
        self.check_and_publish().await;
    }

    async fn shutdown(&self) -> Result<()> {
        self.log(LogLevel::Debug, "shutdown");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.log(
            LogLevel::Debug,
            &format!("did_open: {}", params.text_document.uri),
        );
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let content = params.text_document.text;
        let mut docs = self.state.documents.write().unwrap();
        docs.insert(uri, state::DocumentState { version, content });
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.log(
            LogLevel::Debug,
            &format!(
                "did_change: {} version={}",
                params.text_document.uri, params.text_document.version
            ),
        );
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        // FULL sync: the entire content is in the first change event
        if let Some(change) = params.content_changes.into_iter().next() {
            let mut docs = self.state.documents.write().unwrap();
            docs.insert(
                uri,
                state::DocumentState {
                    version,
                    content: change.text,
                },
            );
        }

        let cancel = self.state.begin_source_analysis();

        // Cancel any pending debounced analysis
        {
            let mut handle = self.state.analysis_handle.lock().unwrap();
            if let Some(h) = handle.take() {
                h.abort();
            }
        }

        // Spawn a debounced re-check: wait 300ms then check workspace.
        // Diagnostics are only published on save.
        let state = Arc::clone(&self.state);
        let new_handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let cancel = match cancel {
                Some(cancel) => cancel,
                None => loop {
                    if let Some(cancel) = state.begin_source_analysis() {
                        break cancel;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                },
            };
            tokio::task::spawn_blocking(move || state.analyze_workspace(None, true, cancel))
                .await
                .ok();
        });

        {
            let mut handle = self.state.analysis_handle.lock().unwrap();
            *handle = Some(new_handle);
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.log(
            LogLevel::Debug,
            &format!("did_close: {}", params.text_document.uri),
        );
        let mut docs = self.state.documents.write().unwrap();
        docs.remove(&params.text_document.uri);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.log(
            LogLevel::Debug,
            &format!("did_save: {}, triggering check", params.text_document.uri),
        );
        self.check_and_publish().await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        self.log(
            LogLevel::Debug,
            &format!("document_symbol: {}", params.text_document.uri),
        );
        let uri = params.text_document.uri;
        let symbols = self.state.get_document_symbols(&uri);
        self.log(
            LogLevel::Debug,
            &format!("document_symbol → {} symbols", symbols.len()),
        );
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!("goto_definition: {} {}:{}", uri, pos.line, pos.character),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_definition → None (no workspace root)",
                );
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "goto_definition → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        // Count how many typed items exist for this file
        let func_count = tm
            .functions
            .values()
            .filter(|f| f.span.file == file_path)
            .count();
        let global_count = tm
            .globals
            .values()
            .filter(|g| g.span.file == file_path)
            .count();
        let test_count = tm.tests.iter().filter(|t| t.span.file == file_path).count();
        self.log(LogLevel::Debug, &format!(
            "goto_definition: file_path={:?}, pos={}:{}, typed items: {} functions, {} globals, {} tests",
            file_path.as_ref(), pos.line + 1, pos.character + 1, func_count, global_count, test_count
        ));

        if func_count == 0 && global_count == 0 && test_count == 0 {
            // Show a sample of known file paths to help diagnose path mismatches
            let known: Vec<_> = tm
                .functions
                .values()
                .map(|f| f.span.file.as_ref())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .take(10)
                .collect();
            self.log(
                LogLevel::Debug,
                &format!(
                    "goto_definition: no typed items for this file. Sample known paths: {:?}",
                    known
                ),
            );
        }

        let node = position::find_node_at_position(tm, &file_path, pos.line + 1, pos.character + 1);
        let node = match node {
            Some(n) => {
                self.log(
                    LogLevel::Debug,
                    &format!("goto_definition: found node {:?}", n),
                );
                n
            }
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_definition → None (no node at position)",
                );
                return Ok(None);
            }
        };

        let location = navigation::goto_definition(&node, tm, reg, workspace_root);
        match &location {
            Some(loc) => self.log(
                LogLevel::Debug,
                &format!(
                    "goto_definition → {}:{}:{}",
                    loc.uri, loc.range.start.line, loc.range.start.character
                ),
            ),
            None => self.log(
                LogLevel::Debug,
                "goto_definition → None (no definition found)",
            ),
        }
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!(
                "goto_type_definition: {} {}:{}",
                uri, pos.line, pos.character
            ),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_type_definition → None (no workspace root)",
                );
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "goto_type_definition → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        let func_count = tm
            .functions
            .values()
            .filter(|f| f.span.file == file_path)
            .count();
        self.log(
            LogLevel::Debug,
            &format!(
                "goto_type_definition: file_path={:?}, pos={}:{}, {} functions in file",
                file_path.as_ref(),
                pos.line + 1,
                pos.character + 1,
                func_count
            ),
        );

        let node = position::find_node_at_position(tm, &file_path, pos.line + 1, pos.character + 1);
        let node = match node {
            Some(n) => n,
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_type_definition → None (no node at position)",
                );
                return Ok(None);
            }
        };

        let location = navigation::goto_type_definition(&node, reg, workspace_root);
        match &location {
            Some(loc) => self.log(
                LogLevel::Debug,
                &format!(
                    "goto_type_definition → {}:{}:{}",
                    loc.uri, loc.range.start.line, loc.range.start.character
                ),
            ),
            None => self.log(
                LogLevel::Debug,
                "goto_type_definition → None (no type definition found)",
            ),
        }
        Ok(location.map(GotoDefinitionResponse::Scalar))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!("hover: {} {}:{}", uri, pos.line, pos.character),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "hover → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "hover → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        let func_count = tm
            .functions
            .values()
            .filter(|f| f.span.file == file_path)
            .count();
        self.log(
            LogLevel::Debug,
            &format!(
                "hover: file_path={:?}, pos={}:{}, {} functions in file",
                file_path.as_ref(),
                pos.line + 1,
                pos.character + 1,
                func_count
            ),
        );

        let node = position::find_node_at_position(tm, &file_path, pos.line + 1, pos.character + 1);
        let node = match node {
            Some(n) => n,
            None => {
                self.log(LogLevel::Debug, "hover → None (no node at position)");
                return Ok(None);
            }
        };

        let result = hover::hover_for_node(&node, tm, reg);
        match &result {
            Some(_) => self.log(LogLevel::Debug, "hover → Some(hover info)"),
            None => self.log(LogLevel::Debug, "hover → None (no hover info for node)"),
        }
        Ok(result)
    }

    #[allow(deprecated)]
    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        self.log(
            LogLevel::Debug,
            &format!("symbol: query={:?}", params.query),
        );
        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "symbol → None (no workspace root)");
                return Ok(None);
            }
        };

        let contexts = self.state.contexts.read().unwrap();
        let mut ws_symbols = Vec::new();
        for context in contexts.iter() {
            ws_symbols.extend(symbols::workspace_symbols(
                &params.query,
                &context.module,
                &context.registry,
                workspace_root,
            ));
        }
        let mut seen = std::collections::HashSet::new();
        ws_symbols.retain(|s| seen.insert(format!("{:?}", s.location)));
        let results: Vec<SymbolInformation> = ws_symbols
            .into_iter()
            .map(|s| SymbolInformation {
                name: s.name,
                kind: s.kind,
                tags: None,
                deprecated: None,
                location: s.location,
                container_name: None,
            })
            .collect();

        self.log(
            LogLevel::Debug,
            &format!("symbol → {} results", results.len()),
        );
        Ok(Some(results))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        self.log(
            LogLevel::Debug,
            &format!("completion: {} {}:{}", uri, pos.line, pos.character),
        );

        // Detect dot vs scope completion
        let is_dot = params
            .context
            .as_ref()
            .and_then(|ctx| ctx.trigger_character.as_deref())
            == Some(".");

        if is_dot {
            self.log(
                LogLevel::Debug,
                &format!(
                    "completion: {} {}:{} (dot trigger)",
                    uri, pos.line, pos.character
                ),
            );
            return self.dot_completion(uri, pos);
        }

        self.log(
            LogLevel::Debug,
            &format!(
                "completion: {} {}:{} (scope/auto-import)",
                uri, pos.line, pos.character
            ),
        );
        self.scope_and_auto_import_completion(uri, pos)
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!("signature_help: {} {}:{}", uri, pos.line, pos.character),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "signature_help → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "signature_help → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        let result = signature_help::signature_help_at_position(
            tm,
            reg,
            &file_path,
            pos.line + 1,
            pos.character + 1,
        );
        match &result {
            Some(sh) => self.log(
                LogLevel::Debug,
                &format!("signature_help → {} signatures", sh.signatures.len()),
            ),
            None => self.log(LogLevel::Debug, "signature_help → None"),
        }
        Ok(result)
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;
        let range = params.range;
        self.log(
            LogLevel::Debug,
            &format!(
                "inlay_hint: {} {}:{}-{}:{}",
                uri, range.start.line, range.start.character, range.end.line, range.end.character
            ),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "inlay_hint → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        // Get document content for type annotation detection
        let document_content = {
            let docs = self.state.documents.read().unwrap();
            docs.get(uri).map(|d| d.content.clone())
        };

        let tm = self.state.typed_module_for(uri);
        let tm = match tm.as_ref() {
            Some(t) => t,
            None => {
                self.log(
                    LogLevel::Debug,
                    "inlay_hint → None (typed_module not available)",
                );
                return Ok(None);
            }
        };

        // Debug: show computed file path and how many functions match
        let matching_funcs: Vec<_> = tm
            .functions
            .values()
            .filter(|f| f.span.file == file_path)
            .map(|f| f.display_name.clone())
            .collect();
        self.log(
            LogLevel::Trace,
            &format!(
                "inlay_hint: file_path={:?}, matching functions={}, total functions={}",
                file_path.as_ref(),
                matching_funcs.len(),
                tm.functions.len()
            ),
        );
        if matching_funcs.is_empty() {
            // Show a sample of file paths in the typed module for debugging
            let sample_paths: std::collections::BTreeSet<String> = tm
                .functions
                .values()
                .map(|f| f.span.file.to_string())
                .collect();
            self.log(
                LogLevel::Debug,
                &format!(
                    "inlay_hint: no functions match file_path={:?}. Known file paths: {:?}",
                    file_path.as_ref(),
                    sample_paths.iter().take(20).collect::<Vec<_>>()
                ),
            );
        }

        let hints =
            inlay_hints::inlay_hints_for_file(tm, &file_path, range, document_content.as_deref());

        if hints.is_empty() {
            self.log(LogLevel::Debug, "inlay_hint → None (no hints)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("inlay_hint → {} hints", hints.len()),
            );
            Ok(Some(hints))
        }
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        if self.state.is_dependency(uri) {
            return Ok(None);
        }
        let diagnostics = &params.context.diagnostics;
        let range = &params.range;
        self.log(
            LogLevel::Debug,
            &format!(
                "code_action: {} {}:{}-{}:{} ({} diagnostics)",
                uri,
                range.start.line,
                range.start.character,
                range.end.line,
                range.end.character,
                diagnostics.len()
            ),
        );

        let mut actions = Vec::new();

        // Auto-import code actions (from diagnostics)
        if !diagnostics.is_empty() {
            let reg = self.state.registry_for(uri);
            if let Some(registry) = reg.as_ref() {
                let scope_info = self.state.get_import_scope_for_file(uri);
                if let Some((_, source_file, _)) = scope_info {
                    actions.extend(code_actions::auto_import_code_actions(
                        diagnostics,
                        registry,
                        &source_file,
                        uri,
                    ));
                }
            }
        }

        // Organize imports code action (always available)
        {
            let scope_info = self.state.get_import_scope_for_file(uri);
            if let Some((_, source_file, _)) = scope_info {
                if let Some(action) = code_actions::organize_imports_action(&source_file, uri) {
                    actions.push(action);
                }
            }
        }

        // Add type annotation code actions
        {
            let workspace_root = self.state.workspace_root.read().unwrap().clone();
            if let Some(workspace_root) = &workspace_root {
                let file_path = uri_to_file_path(uri, workspace_root);
                let file_path: crate::common::span::FilePath = file_path.into();

                let document_content = {
                    let docs = self.state.documents.read().unwrap();
                    docs.get(uri).map(|d| d.content.clone())
                };

                if let Some(content) = &document_content {
                    let tm = self.state.typed_module_for(uri);
                    if let Some(tm) = tm.as_ref() {
                        actions.extend(code_actions::add_type_annotation_actions(
                            tm, &file_path, range, content, uri,
                        ));
                    }
                }
            }
        }

        if actions.is_empty() {
            self.log(LogLevel::Debug, "code_action → None (no actions)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("code_action → {} actions", actions.len()),
            );
            Ok(Some(
                actions
                    .into_iter()
                    .map(CodeActionOrCommand::CodeAction)
                    .collect(),
            ))
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        self.log(
            LogLevel::Debug,
            &format!("references: {} {}:{}", uri, pos.line, pos.character),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "references → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "references → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        let node = position::find_node_at_position(tm, &file_path, pos.line + 1, pos.character + 1);
        let node = match node {
            Some(n) => n,
            None => {
                self.log(LogLevel::Debug, "references → None (no node at position)");
                return Ok(None);
            }
        };

        let include_declaration = params.context.include_declaration;
        let mut locations = references::find_references(
            &node,
            tm,
            reg,
            &file_path,
            workspace_root,
            include_declaration,
        );

        if let Some(definition) = navigation::goto_definition(&node, tm, reg, workspace_root) {
            let owner = context
                .as_ref()
                .and_then(|context| context.declaration_owner(&definition.uri));
            for context in self.state.contexts.read().unwrap().iter() {
                if owner.is_none() || context.declaration_owner(&definition.uri) != owner {
                    continue;
                }
                if navigation::goto_definition(
                    &node,
                    &context.module,
                    &context.registry,
                    workspace_root,
                )
                .as_ref()
                    == Some(&definition)
                {
                    locations.extend(references::find_references(
                        &node,
                        &context.module,
                        &context.registry,
                        &file_path,
                        workspace_root,
                        include_declaration,
                    ));
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        locations.retain(|location| seen.insert(format!("{location:?}")));

        if locations.is_empty() {
            self.log(LogLevel::Debug, "references → None (no references found)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("references → {} locations", locations.len()),
            );
            Ok(Some(locations))
        }
    }

    async fn goto_implementation(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!(
                "goto_implementation: {} {}:{}",
                uri, pos.line, pos.character
            ),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_implementation → None (no workspace root)",
                );
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "goto_implementation → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        let node = position::find_node_at_position(tm, &file_path, pos.line + 1, pos.character + 1);
        let node = match node {
            Some(n) => n,
            None => {
                self.log(
                    LogLevel::Debug,
                    "goto_implementation → None (no node at position)",
                );
                return Ok(None);
            }
        };

        let locations = implementation::goto_implementation(&node, tm, reg, workspace_root);

        if locations.is_empty() {
            self.log(
                LogLevel::Debug,
                "goto_implementation → None (no implementations found)",
            );
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("goto_implementation → {} locations", locations.len()),
            );
            Ok(Some(GotoDefinitionResponse::Array(locations)))
        }
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        self.log(
            LogLevel::Debug,
            &format!(
                "prepare_call_hierarchy: {} {}:{}",
                uri, pos.line, pos.character
            ),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "prepare_call_hierarchy → None (no workspace root)",
                );
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let tm = self.state.typed_module_for(uri);
        let tm = match tm.as_ref() {
            Some(t) => t,
            None => {
                self.log(
                    LogLevel::Debug,
                    "prepare_call_hierarchy → None (typed_module not available)",
                );
                return Ok(None);
            }
        };

        let result = call_hierarchy::prepare_call_hierarchy(
            tm,
            &file_path,
            pos.line + 1,
            pos.character + 1,
            workspace_root,
        );
        match &result {
            Some(items) => self.log(
                LogLevel::Debug,
                &format!("prepare_call_hierarchy → {} items", items.len()),
            ),
            None => self.log(LogLevel::Debug, "prepare_call_hierarchy → None"),
        }
        Ok(result)
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        self.log(
            LogLevel::Debug,
            &format!("incoming_calls: {}", params.item.name),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "incoming_calls → None (no workspace root)");
                return Ok(None);
            }
        };

        let context = self.state.context_for(&params.item.uri);
        let tm = context.as_ref().map(|context| context.module.clone());
        let tm = match tm.as_ref() {
            Some(t) => t,
            None => {
                self.log(
                    LogLevel::Debug,
                    "incoming_calls → None (typed_module not available)",
                );
                return Ok(None);
            }
        };

        let mut results = call_hierarchy::incoming_calls(tm, &params.item, workspace_root);
        let owner = context
            .as_ref()
            .and_then(|context| context.declaration_owner(&params.item.uri));
        for context in self.state.contexts.read().unwrap().iter() {
            if owner.is_some()
                && context.declaration_owner(&params.item.uri) == owner
                && call_hierarchy::owns_item(&context.module, &params.item, workspace_root)
            {
                results.extend(call_hierarchy::incoming_calls(
                    &context.module,
                    &params.item,
                    workspace_root,
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        results.retain(|result| seen.insert(format!("{result:?}")));

        if results.is_empty() {
            self.log(LogLevel::Debug, "incoming_calls → None (no callers)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("incoming_calls → {} callers", results.len()),
            );
            Ok(Some(results))
        }
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        self.log(
            LogLevel::Debug,
            &format!("outgoing_calls: {}", params.item.name),
        );

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "outgoing_calls → None (no workspace root)");
                return Ok(None);
            }
        };

        let tm = self.state.typed_module_for(&params.item.uri);
        let tm = match tm.as_ref() {
            Some(t) => t,
            None => {
                self.log(
                    LogLevel::Debug,
                    "outgoing_calls → None (typed_module not available)",
                );
                return Ok(None);
            }
        };

        let results = call_hierarchy::outgoing_calls(tm, &params.item, workspace_root);

        if results.is_empty() {
            self.log(LogLevel::Debug, "outgoing_calls → None (no callees)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("outgoing_calls → {} callees", results.len()),
            );
            Ok(Some(results))
        }
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let snapshot = self
            .state
            .documents
            .read()
            .unwrap()
            .get(uri)
            .map(|document| (document.version, document.content.clone()));
        let Some((version, source)) = snapshot else {
            return Ok(None);
        };
        let input = source.clone();
        let file_path = uri.as_str().into();
        let output =
            tokio::task::spawn_blocking(move || crate::formatter::format_source(&input, file_path))
                .await
                .map_err(|_| tower_lsp::jsonrpc::Error::internal_error())?
                .map_err(|error| tower_lsp::jsonrpc::Error::invalid_params(error.to_string()))?;
        if output == source {
            return Ok(None);
        }
        let documents = self.state.documents.read().unwrap();
        if documents
            .get(uri)
            .is_none_or(|document| document.version != version)
        {
            return Ok(None);
        }
        let line = source.bytes().filter(|byte| *byte == b'\n').count() as u32;
        let character = source
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .encode_utf16()
            .count() as u32;
        Ok(Some(vec![TextEdit {
            range: Range::new(Position::new(0, 0), Position::new(line, character)),
            new_text: output,
        }]))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;
        self.log(LogLevel::Debug, &format!("code_lens: {}", uri));

        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "code_lens → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let tm = self.state.typed_module_for(uri);
        let tm = match tm.as_ref() {
            Some(t) => t,
            None => {
                self.log(
                    LogLevel::Debug,
                    "code_lens → None (typed_module not available)",
                );
                return Ok(None);
            }
        };

        let lenses = code_lens::test_code_lenses(tm, &file_path);

        if lenses.is_empty() {
            self.log(LogLevel::Debug, "code_lens → None (no lenses)");
            Ok(None)
        } else {
            self.log(
                LogLevel::Debug,
                &format!("code_lens → {} lenses", lenses.len()),
            );
            Ok(Some(lenses))
        }
    }
}

impl DovetailLanguageServer {
    fn dot_completion(&self, uri: &Url, pos: Position) -> Result<Option<CompletionResponse>> {
        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(LogLevel::Debug, "dot_completion → None (no workspace root)");
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path: crate::common::span::FilePath = file_path.into();

        let context = self.state.context_for(uri);
        let tm = context.as_ref().map(|c| c.module.clone());
        let reg = context.as_ref().map(|c| c.registry.clone());
        let (tm, reg) = match (tm.as_ref(), reg.as_ref()) {
            (Some(t), Some(r)) => (t, r),
            _ => {
                self.log(
                    LogLevel::Debug,
                    "dot_completion → None (typed_module/registry not available)",
                );
                return Ok(None);
            }
        };

        // Find the expression type at the position before the dot.
        // The dot is at `pos.character`, so the expression is at `pos.character` (0-indexed).
        // Convert to 1-indexed for our span system.
        let expr_type = position::find_expression_type_at_position(
            tm,
            &file_path,
            pos.line + 1,
            pos.character, // character before the dot (0-indexed → the column of the last char)
        );

        // Get import scope for extension method lookup
        let scope_info = self.state.get_import_scope_for_file(uri);

        let receiver_type = match expr_type {
            Some(t) => t,
            None => {
                // Fall back to text-based analysis: check if the text before the dot
                // is an enum type, module, or package name.
                if let Some(ident) = self.get_identifier_before_dot(uri, pos) {
                    if let Some((ref scope, _, ref pkg_path)) = scope_info {
                        let items =
                            completion::qualified_dot_completion(&ident, reg, scope, pkg_path);
                        if !items.is_empty() {
                            return Ok(Some(CompletionResponse::Array(items)));
                        }
                    }
                }
                return Ok(None);
            }
        };

        let import_scope = match &scope_info {
            Some((scope, _, _)) => scope,
            None => {
                self.log(LogLevel::Debug, "dot_completion → None (no import scope)");
                return Ok(None);
            }
        };

        let items = completion::dot_completion(&receiver_type, reg, import_scope);
        self.log(
            LogLevel::Debug,
            &format!("dot_completion → {} items", items.len()),
        );
        Ok(Some(CompletionResponse::Array(items)))
    }

    /// Extract the identifier before the dot at the given cursor position.
    ///
    /// When the user types `Async.`, the cursor is right after the dot.
    /// This reads the document text and walks backwards to find the identifier.
    fn get_identifier_before_dot(&self, uri: &Url, pos: Position) -> Option<String> {
        let docs = self.state.documents.read().unwrap();
        let doc = docs.get(uri)?;
        let line = doc.content.lines().nth(pos.line as usize)?;

        // pos.character is 0-indexed cursor position right after the dot.
        // The dot is at pos.character - 1, identifier ends at pos.character - 2.
        let dot_col = pos.character as usize;
        if dot_col == 0 {
            return None;
        }

        let before_dot = line.get(..dot_col.saturating_sub(1))?;
        let ident: String = before_dot
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        if ident.is_empty() { None } else { Some(ident) }
    }

    fn scope_and_auto_import_completion(
        &self,
        uri: &Url,
        pos: Position,
    ) -> Result<Option<CompletionResponse>> {
        let workspace_root = self.state.workspace_root.read().unwrap().clone();
        let workspace_root = match &workspace_root {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "scope_completion → None (no workspace root)",
                );
                return Ok(None);
            }
        };

        let file_path = uri_to_file_path(uri, workspace_root);
        let file_path_arc: crate::common::span::FilePath = file_path.into();

        // Get the prefix being typed
        let prefix = get_prefix_at_position(&self.state, uri, pos);
        self.log(
            LogLevel::Debug,
            &format!("scope_completion: prefix={:?}", prefix),
        );

        // Collect visible locals
        let visible = {
            let tm = self.state.typed_module_for(uri);
            match tm.as_ref() {
                Some(tm) => scope::collect_visible_locals(
                    tm,
                    &file_path_arc,
                    pos.line + 1,
                    pos.character + 1,
                ),
                None => {
                    self.log(
                        LogLevel::Debug,
                        "scope_completion: typed_module not available, using empty locals",
                    );
                    scope::VisibleLocals {
                        params: Vec::new(),
                        locals: Vec::new(),
                    }
                }
            }
        };

        // Build import scope
        let scope_info = self.state.get_import_scope_for_file(uri);
        let (import_scope, source_file, pkg_path) = match scope_info {
            Some(info) => info,
            None => {
                self.log(LogLevel::Debug, "scope_completion → None (no import scope)");
                return Ok(None);
            }
        };

        let reg = self.state.registry_for(uri);
        let registry = match reg.as_ref() {
            Some(r) => r,
            None => {
                self.log(
                    LogLevel::Debug,
                    "scope_completion → None (registry not available)",
                );
                return Ok(None);
            }
        };

        // Scope completion
        let mut items =
            completion::scope_completion(&visible, registry, &import_scope, &pkg_path, &prefix);

        // Auto-import completion
        let mut visible_names = BTreeSet::new();
        for item in &items {
            visible_names.insert(item.label.clone());
        }

        let import_pos = completion::compute_import_insert_position(&source_file);
        let auto_items = if self.state.is_dependency(uri) {
            Vec::new()
        } else {
            completion::auto_import_completions(&prefix, registry, &visible_names, import_pos)
        };
        items.extend(auto_items);

        self.log(
            LogLevel::Debug,
            &format!("scope_completion → {} items", items.len()),
        );
        Ok(Some(CompletionResponse::Array(items)))
    }
}

/// Extract the identifier prefix at the cursor position by scanning backwards.
fn get_prefix_at_position(state: &WorldState, uri: &Url, pos: Position) -> String {
    let docs = state.documents.read().unwrap();
    let doc = match docs.get(uri) {
        Some(d) => d,
        None => return String::new(),
    };

    let lines: Vec<&str> = doc.content.lines().collect();
    let line_idx = pos.line as usize;
    if line_idx >= lines.len() {
        return String::new();
    }

    let line = lines[line_idx];
    let col = pos.character as usize;
    let before = if col <= line.len() {
        &line[..col]
    } else {
        line
    };

    // Scan backwards for identifier characters
    let prefix: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    prefix
}

/// Convert a URI to a file path relative to the workspace root.
fn uri_to_file_path(uri: &Url, workspace_root: &std::path::Path) -> String {
    match uri.to_file_path() {
        Ok(abs) => {
            let abs = state::physical_source_path(&abs);
            let canonical_root = state::physical_path(workspace_root);
            let workspace_root = canonical_root.as_path();
            let prelude = workspace_root.join(".dovetail/dependencies/prelude/src");
            if let Ok(relative) = abs.strip_prefix(prelude) {
                return format!("<prelude>/{}", relative.to_string_lossy());
            }
            let relative = abs.strip_prefix(workspace_root).unwrap_or(&abs);
            if relative.starts_with(std::path::Path::new(".dovetail").join("generated")) {
                // Generated binding spans use portable separators, while ordinary
                // source spans retain the platform spelling used by discovery.
                relative.components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                relative.to_string_lossy().into_owned()
            }
        }
        Err(()) => uri.path().to_string(),
    }
}

#[doc(hidden)]
pub fn create_test_service() -> (LspService<impl LanguageServer>, tower_lsp::ClientSocket) {
    LspService::build(|client| DovetailLanguageServer {
        client,
        state: Arc::new(WorldState::new()),
        log_level: LogLevel::Warn,
        progress_counter: std::sync::atomic::AtomicU32::new(0),
    })
    .custom_method(
        "window/workDoneProgress/cancel",
        DovetailLanguageServer::cancel_progress,
    )
    .finish()
}

/// Start the LSP server.
///
/// If `tcp` is true, listens on the given port; otherwise uses stdio.
pub fn run_server(tcp: bool, port: u16, verbose: bool) {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let log_level = LogLevel::resolve(verbose);
        let (service, socket) = LspService::build(|client| DovetailLanguageServer {
            client,
            state: Arc::new(WorldState::new()),
            log_level,
            progress_counter: std::sync::atomic::AtomicU32::new(0),
        })
        .custom_method(
            "window/workDoneProgress/cancel",
            DovetailLanguageServer::cancel_progress,
        )
        .finish();

        if tcp {
            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
                .await
                .unwrap_or_else(|e| panic!("failed to bind to port {port}: {e}"));
            eprintln!("Dovetail LSP server listening on port {port}");
            let (stream, _) = listener
                .accept()
                .await
                .expect("failed to accept connection");
            let (read, write) = tokio::io::split(stream);
            Server::new(read, write, socket).serve(service).await;
        } else {
            let stdin = tokio::io::stdin();
            let stdout = tokio::io::stdout();
            Server::new(stdin, stdout, socket).serve(service).await;
        }
    });
}

#[cfg(test)]
mod dependency_uri_tests {
    use super::*;

    #[test]
    fn emitted_prelude_uri_round_trips_to_compiler_source_path() {
        let root = std::path::Path::new("/workspace");
        let source = "<prelude>/types.dove";
        let uri = diagnostics::file_path_to_uri(root, source).unwrap();
        assert_eq!(uri_to_file_path(&uri, root), source);
    }

    #[test]
    fn generated_binding_uri_round_trips_to_portable_compiler_span() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let source = ".dovetail/generated/project/sqlite.raw.dove";
        let uri = diagnostics::file_path_to_uri(&root, source).unwrap();
        assert_eq!(uri_to_file_path(&uri, &root), source);
    }

    #[cfg(unix)]
    #[test]
    fn source_navigation_and_overlays_follow_workspace_and_project_symlinks() {
        use std::os::unix::fs::symlink;
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("workspace");
        std::fs::create_dir_all(root.join("actual/src")).unwrap();
        std::fs::write(root.join("Dovetail.toml"), format!(
            "compiler-version = {:?}\n[[project]]\nname = \"app\"\nroot_package = \"app\"\npackages = [\".\"]\n",
            env!("CARGO_PKG_VERSION")
        )).unwrap();
        let content =
            "package app\nfunction value(): Int32 = 1\nfunction main(): Int32 = value()\n";
        std::fs::write(root.join("actual/src/main.dove"), content).unwrap();
        symlink("actual", root.join("app")).unwrap();
        let linked_workspace = temporary.path().join("linked-workspace");
        symlink(&root, &linked_workspace).unwrap();
        let state = WorldState::new();
        *state.workspace_root.write().unwrap() = Some(linked_workspace.clone());
        let uri = Url::from_file_path(linked_workspace.join("app/src/main.dove")).unwrap();
        state.documents.write().unwrap().insert(
            uri.clone(),
            state::DocumentState {
                version: 1,
                content: content.into(),
            },
        );
        let diagnostics = state.load_and_check_workspace(None).unwrap();
        assert!(diagnostics.values().all(Vec::is_empty), "{diagnostics:?}");
        let context = state.context_for(&uri).unwrap();
        let file = uri_to_file_path(&uri, &linked_workspace);
        assert_eq!(file, "actual/src/main.dove");
        assert_eq!(
            state.content_overlays().get(&file).map(String::as_str),
            Some(content)
        );
        let node = position::find_node_at_position(&context.module, &file.into(), 3, 28).unwrap();
        let definition = navigation::goto_definition(
            &node,
            &context.module,
            &context.registry,
            &state::physical_path(&root),
        )
        .unwrap();
        assert_eq!(
            definition.uri.to_file_path().unwrap(),
            state::physical_path(&root).join("actual/src/main.dove")
        );
        assert_eq!(definition.range.start.line, 1);
    }
}
