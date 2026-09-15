use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};

use tower_lsp::lsp_types::{self, Url};

use crate::common::diagnostics::Diagnostics;
use crate::common::span::FilePath;
use crate::common::types::PackagePath;
use crate::discovery;
use crate::parser::ast::SourceFile;
use crate::typechecker::imports::{self, ImportScope};
use crate::typechecker::registry::Registry;
use crate::typechecker::types::TypedModule;

use super::diagnostics as lsp_diagnostics;
use super::symbols;

/// Tracks the state of a single open document.
pub struct DocumentState {
    pub version: i32,
    pub content: String,
}

/// Central state shared between the LSP server and its request handlers.
#[derive(Default)]
pub struct WorldState {
    pub workspace_root: RwLock<Option<PathBuf>>,
    pub documents: RwLock<HashMap<Url, DocumentState>>,
    /// URIs that had diagnostics published in the last check cycle.
    /// Used to clear stale diagnostics for files that no longer have errors.
    pub previously_published_files: RwLock<HashSet<Url>>,
    /// Semantic snapshots remain separate for distinct dependency closures.
    pub contexts: RwLock<Vec<AnalysisContext>>,
    cancellation: Mutex<Option<Arc<AtomicBool>>>,
    /// A network-capable analysis survives source edits so fetching can finish.
    dependency_analysis: Mutex<Option<Arc<AtomicBool>>>,
    pub publication_gate: tokio::sync::Mutex<()>,
    /// Handle to the pending debounced analysis task (if any).
    pub analysis_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Clone)]
pub struct AnalysisContext {
    pub identity: String,
    pub directory: PathBuf,
    pub generated_directory: PathBuf,
    pub module: Arc<TypedModule>,
    pub registry: Arc<Registry>,
    /// Physical source and generated directories mapped to graph ownership.
    pub declaration_owners: Vec<(PathBuf, String)>,
}

impl AnalysisContext {
    pub fn declaration_owner(&self, uri: &Url) -> Option<&str> {
        let path = physical_source_path(&uri.to_file_path().ok()?);
        self.declaration_owners
            .iter()
            .filter(|(directory, _)| path.starts_with(directory))
            .max_by_key(|(directory, _)| directory.components().count())
            .map(|(_, identity)| identity.as_str())
    }
}

fn declaration_owners(
    workspace: &crate::manifest::ResolvedWorkspace,
    project: &crate::manifest::ResolvedProject,
) -> Vec<(PathBuf, String)> {
    let mut pending = vec![project];
    let mut seen = HashSet::new();
    let mut owners = vec![(
        workspace
            .workspace_root
            .join(".dovetail/dependencies/prelude/src"),
        format!("bundled-prelude:{}", env!("CARGO_PKG_VERSION")),
    )];
    while let Some(project) = pending.pop() {
        let identity = project.identity();
        if !seen.insert(identity.clone()) {
            continue;
        }
        owners.push((project.project_dir.clone(), identity.clone()));
        owners.push((
            project.generated_sources_dir(&workspace.workspace_root),
            identity,
        ));
        pending.extend(
            project
                .depends
                .iter()
                .filter_map(|dependency| workspace.project(&dependency.0)),
        );
    }
    owners
}

struct DependencyAnalysisGuard<'a> {
    state: &'a WorldState,
    cancel: &'a Arc<AtomicBool>,
}

impl Drop for DependencyAnalysisGuard<'_> {
    fn drop(&mut self) {
        let mut active = self.state.dependency_analysis.lock().unwrap();
        if active
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, self.cancel))
        {
            *active = None;
        }
    }
}

pub(super) fn physical_path(path: &std::path::Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Discovery preserves a file's own name but resolves its containing package
/// directory. Match that spelling even when the editor opened a directory alias.
pub(super) fn physical_source_path(path: &std::path::Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => physical_path(parent).join(name),
        _ => physical_path(path),
    }
}

impl WorldState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel_analysis(&self) {
        if let Some(cancel) = self.cancellation.lock().unwrap().as_ref() {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    pub fn is_dependency(&self, uri: &Url) -> bool {
        let root = self.workspace_root.read().unwrap();
        root.as_ref()
            .zip(uri.to_file_path().ok())
            .is_some_and(|(root, path)| {
                physical_path(&path).starts_with(physical_path(root).join(".dovetail"))
            })
    }

    pub fn context_for(&self, uri: &Url) -> Option<AnalysisContext> {
        let path = physical_source_path(&uri.to_file_path().ok()?);
        let contexts = self.contexts.read().unwrap();
        let mut candidates: Vec<_> = contexts
            .iter()
            .filter(|c| path.starts_with(&c.directory) || path.starts_with(&c.generated_directory))
            .collect();
        if let Some(depth) = candidates
            .iter()
            .map(|c| c.directory.components().count())
            .max()
        {
            candidates.retain(|c| c.directory.components().count() == depth);
            // The same source can belong to alternate manifests with different closures.
            // Its URI alone cannot identify a revision; consumers keep their own context.
            return (candidates.len() == 1).then(|| candidates[0].clone());
        }
        let root = self.workspace_root.read().unwrap();
        if root
            .as_ref()
            .is_some_and(|root| path.starts_with(root.join(".dovetail/dependencies/prelude/src")))
        {
            contexts.first().cloned()
        } else {
            None
        }
    }

    pub fn begin_analysis(&self) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut current = self.cancellation.lock().unwrap();
        if let Some(previous) = current.replace(cancel.clone()) {
            previous.store(true, Ordering::SeqCst);
        }
        cancel
    }

    pub fn begin_dependency_analysis(&self) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut current = self.cancellation.lock().unwrap();
        if let Some(previous) = current.replace(cancel.clone()) {
            previous.store(true, Ordering::SeqCst);
        }
        *self.dependency_analysis.lock().unwrap() = Some(cancel.clone());
        cancel
    }

    /// Source edits wait for an existing fetch instead of replacing it with an
    /// offline check that cannot finish materializing missing dependencies.
    pub fn begin_source_analysis(&self) -> Option<Arc<AtomicBool>> {
        let mut current = self.cancellation.lock().unwrap();
        let fetching = self.dependency_analysis.lock().unwrap();
        if fetching.as_ref().is_some_and(|fetching| {
            !fetching.load(Ordering::SeqCst)
                && current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, fetching))
        }) {
            return None;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(previous) = current.replace(cancel.clone()) {
            previous.store(true, Ordering::SeqCst);
        }
        Some(cancel)
    }

    pub fn is_current_analysis(&self, cancel: &Arc<AtomicBool>) -> bool {
        !cancel.load(Ordering::SeqCst)
            && self
                .cancellation
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, cancel))
    }

    pub fn typed_module_for(&self, uri: &Url) -> Option<Arc<TypedModule>> {
        self.context_for(uri).map(|c| c.module)
    }

    pub fn registry_for(&self, uri: &Url) -> Option<Arc<Registry>> {
        self.context_for(uri).map(|c| c.registry)
    }

    /// Write embedded prelude source files to `.dovetail/dependencies/prelude/src/`
    /// under the workspace root so that goto-definition can navigate into prelude types.
    pub fn emit_prelude_sources(&self) {
        let root = self.workspace_root.read().unwrap();
        let Some(root) = root.as_ref() else { return };

        let prelude_dir = root
            .join(".dovetail")
            .join("dependencies")
            .join("prelude")
            .join("src");
        if let Err(e) = std::fs::create_dir_all(&prelude_dir) {
            eprintln!("[dovetail-lsp WARN] failed to create prelude dir: {e}");
            return;
        }

        for (rel_path, contents) in crate::prelude_sources() {
            let dest = prelude_dir.join(rel_path);
            // Only write if content changed (avoid unnecessary disk writes)
            if std::fs::read_to_string(&dest).ok().as_deref() == Some(contents) {
                continue;
            }
            if let Err(e) = std::fs::write(&dest, contents) {
                eprintln!("[dovetail-lsp WARN] failed to write prelude file {rel_path}: {e}");
            }
        }
    }

    /// Build a map from workspace-relative paths to in-memory content
    /// for all currently open documents.
    pub fn content_overlays(&self) -> HashMap<String, String> {
        let root = self.workspace_root.read().unwrap().clone();
        let root = root.map(|root| physical_path(&root));
        let docs: Vec<_> = self
            .documents
            .read()
            .unwrap()
            .iter()
            .map(|(uri, state)| (uri.clone(), state.content.clone()))
            .collect();

        let mut overlays = HashMap::new();
        for (uri, content) in docs {
            if let Ok(abs) = uri.to_file_path() {
                let abs = physical_source_path(&abs);
                if root
                    .as_ref()
                    .is_some_and(|root| abs.starts_with(root.join(".dovetail")))
                {
                    continue;
                }
                let relative = match &root {
                    Some(r) => abs
                        .strip_prefix(r)
                        .unwrap_or(&abs)
                        .to_string_lossy()
                        .into_owned(),
                    None => abs.to_string_lossy().into_owned(),
                };
                overlays.insert(relative, content);
            }
        }
        overlays
    }

    /// Run a full workspace check and return (diagnostics_by_file, workspace_root).
    /// Returns None if no workspace root is set or manifest loading fails.
    /// Caches the merged TypedModule and Registry for navigation features.
    pub fn load_and_check_workspace(
        &self,
        on_progress: Option<Arc<crate::compiler::WorkspaceProgress>>,
    ) -> Option<HashMap<Url, Vec<lsp_types::Diagnostic>>> {
        self.analyze_workspace(on_progress, false, self.begin_dependency_analysis())
    }

    pub fn analyze_workspace(
        &self,
        on_progress: Option<Arc<crate::compiler::WorkspaceProgress>>,
        offline: bool,
        cancel: Arc<AtomicBool>,
    ) -> Option<HashMap<Url, Vec<lsp_types::Diagnostic>>> {
        let _finished = DependencyAnalysisGuard {
            state: self,
            cancel: &cancel,
        };
        let root = physical_path(&self.workspace_root.read().ok()?.clone()?);
        *self.workspace_root.write().ok()? = Some(root.clone());
        if !self.is_current_analysis(&cancel) {
            return None;
        }
        let dependency_progress = on_progress.clone().map(|callback| {
            Arc::new(move |message: &str| callback(0, 0, message))
                as Arc<dyn Fn(&str) + Send + Sync>
        });
        let options = crate::manifest::ResolveOptions {
            cancelled: Some(cancel.clone()),
            progress: dependency_progress,
            offline,
            ..Default::default()
        };
        let mut diagnostics = Diagnostics::new();
        let mut contexts = Vec::new();
        let workspaces = match crate::manifest::load_manifest_with_options(&root, &options) {
            Ok(workspace) => vec![workspace],
            Err(errors) => {
                for error in errors {
                    diagnostics.error(
                        crate::common::span::Span::point("Dovetail.toml".into(), 1, 1),
                        error.to_string(),
                    );
                }
                // Preserve unrelated valid local targets when one closure fails.
                let manifest = std::fs::read_to_string(root.join("Dovetail.toml"))
                    .ok()
                    .and_then(|s| s.parse::<toml::Value>().ok());
                manifest
                    .and_then(|m| m.get("project").and_then(|v| v.as_array()).cloned())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|p| p.get("name").and_then(|v| v.as_str()))
                    .filter_map(|name| {
                        crate::manifest::load_manifest_with_options(
                            &root,
                            &crate::manifest::ResolveOptions {
                                target: Some(name.into()),
                                ..options.clone()
                            },
                        )
                        .ok()
                    })
                    .collect()
            }
        };
        // Fetching may take time; use the latest document contents once it ends.
        let overlays = self.content_overlays();
        for workspace in workspaces {
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::build_workspace(
                    &workspace,
                    None,
                    crate::BuildMode::Check,
                    &overlays,
                    true,
                    on_progress.as_deref(),
                )
            }));
            let Ok(result) = result else {
                diagnostics.error(
                    crate::common::span::Span::point("Dovetail.toml".into(), 1, 1),
                    "workspace analysis failed",
                );
                continue;
            };
            diagnostics.extend_from(&result.diagnostics);
            for (key, result) in result.project_results {
                if let Some(project) = workspace.project(&key) {
                    if contexts
                        .iter()
                        .any(|c: &AnalysisContext| c.identity == project.identity())
                    {
                        continue;
                    }
                    contexts.push(AnalysisContext {
                        identity: project.identity(),
                        directory: project.project_dir.clone(),
                        generated_directory: project.generated_sources_dir(&root),
                        module: Arc::new(result.typed_module),
                        registry: Arc::new(result.registry),
                        declaration_owners: declaration_owners(&workspace, project),
                    });
                }
            }
        }
        // Publish snapshots only if this task is still current.
        let current = self.cancellation.lock().unwrap();
        if cancel.load(Ordering::SeqCst)
            || !current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel))
        {
            return None;
        }
        *self.contexts.write().unwrap() = contexts;
        Some(lsp_diagnostics::group_diagnostics_by_file(
            &diagnostics,
            &root,
        ))
    }

    /// Parse a single document and return its document symbols.
    pub fn get_document_symbols(&self, uri: &Url) -> Vec<lsp_types::DocumentSymbol> {
        // Try to get content from open documents first
        let content = {
            let docs = self.documents.read().unwrap();
            docs.get(uri).map(|d| d.content.clone())
        };

        // Fall back to reading from disk
        let content = match content {
            Some(c) => c,
            None => {
                let path = match uri.to_file_path() {
                    Ok(p) => p,
                    Err(()) => return vec![],
                };
                match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(_) => return vec![],
                }
            }
        };

        // Compute a file path relative to workspace root (or use the URI path)
        let file_path_str = {
            let root = self.workspace_root.read().unwrap();
            match (&*root, uri.to_file_path().ok()) {
                (Some(root), Some(abs)) => abs
                    .strip_prefix(root)
                    .unwrap_or(&abs)
                    .to_string_lossy()
                    .into_owned(),
                _ => uri.path().to_string(),
            }
        };

        let file_path: FilePath = file_path_str.into();
        let (source_file, _diagnostics) = discovery::parse_source(&content, file_path);
        symbols::source_file_to_document_symbols(&source_file)
    }

    /// Run a full workspace check and update the cached TypedModule + Registry,
    /// but do NOT return diagnostics. Used on `didChange` to keep completions fresh.
    pub fn check_workspace(&self) {
        self.analyze_workspace(None, true, self.begin_analysis());
    }

    /// Parse a file and build its import scope against the cached registry.
    ///
    /// Returns `(ImportScope, SourceFile, PackagePath)` or `None` if the registry
    /// is not yet available or the file can't be parsed.
    pub fn get_import_scope_for_file(
        &self,
        uri: &Url,
    ) -> Option<(ImportScope, SourceFile, PackagePath)> {
        // Get file content
        let content = {
            let docs = self.documents.read().unwrap();
            docs.get(uri).map(|d| d.content.clone())
        };
        let content = match content {
            Some(c) => c,
            None => {
                let path = uri.to_file_path().ok()?;
                std::fs::read_to_string(&path).ok()?
            }
        };

        // File path relative to workspace root
        let file_path_str = {
            let root = self.workspace_root.read().unwrap();
            match (&*root, uri.to_file_path().ok()) {
                (Some(root), Some(abs)) => abs
                    .strip_prefix(root)
                    .unwrap_or(&abs)
                    .to_string_lossy()
                    .into_owned(),
                _ => uri.path().to_string(),
            }
        };

        let file_path: FilePath = file_path_str.into();
        let (source_file, _parse_diags) = discovery::parse_source(&content, file_path);

        // Extract package path from parsed source
        let pkg_path = PackagePath(
            source_file
                .package
                .path
                .iter()
                .map(|s| s.value.clone())
                .collect(),
        );

        // Build import scope using cached registry
        let reg = self.registry_for(uri);
        let registry = match reg.as_ref() {
            Some(r) => r,
            None => return Some((ImportScope::new(), source_file, pkg_path)),
        };

        let mut diagnostics = Diagnostics::new();
        let files: Vec<&SourceFile> = vec![&source_file];
        let scopes = imports::build_import_scopes(&pkg_path, &files, registry, &mut diagnostics);

        let file_key: FilePath = source_file.package.span.file.clone();
        let scope = scopes
            .into_iter()
            .find_map(
                |(fp, scope)| {
                    if fp == file_key { Some(scope) } else { None }
                },
            )
            .unwrap_or_default();

        Some((scope, source_file, pkg_path))
    }
}

#[cfg(test)]
mod dependency_context_tests {
    use super::*;

    fn context(identity: &str, directory: PathBuf) -> AnalysisContext {
        AnalysisContext {
            identity: identity.into(),
            directory,
            generated_directory: PathBuf::from(format!(
                "/workspace/.dovetail/generated/{identity}"
            )),
            module: Arc::new(TypedModule::empty()),
            registry: Arc::new(Registry::new()),
            declaration_owners: vec![],
        }
    }

    #[test]
    fn ambiguous_dependency_uri_does_not_choose_an_arbitrary_manifest() {
        let state = WorldState::new();
        let shared = PathBuf::from("/workspace/.dovetail/deps/repo/commit/library");
        *state.contexts.write().unwrap() = vec![
            context("first-manifest", shared.clone()),
            context("second-manifest", shared.clone()),
            context("consumer", PathBuf::from("/workspace/app")),
        ];
        let uri = Url::from_file_path(shared.join("src/lib.dove")).unwrap();
        assert!(state.context_for(&uri).is_none());
        let consumer = Url::from_file_path("/workspace/app/src/main.dove").unwrap();
        assert_eq!(state.context_for(&consumer).unwrap().identity, "consumer");
        let generated =
            Url::from_file_path("/workspace/.dovetail/generated/first-manifest/bindings.dove")
                .unwrap();
        assert_eq!(
            state.context_for(&generated).unwrap().identity,
            "first-manifest"
        );
    }

    #[test]
    fn superseded_queued_analysis_cannot_replace_newer_snapshots() {
        let state = WorldState::new();
        let root = tempfile::tempdir().unwrap();
        *state.workspace_root.write().unwrap() = Some(root.path().into());
        *state.contexts.write().unwrap() = vec![context("current", root.path().join("app"))];
        let old = state.begin_analysis();
        let new = state.begin_analysis();
        assert!(!state.is_current_analysis(&old));
        assert!(state.analyze_workspace(None, true, old).is_none());
        assert_eq!(state.contexts.read().unwrap()[0].identity, "current");
        assert!(state.is_current_analysis(&new));
    }

    #[test]
    fn prelude_fallback_only_applies_to_emitted_bundled_sources() {
        let state = WorldState::new();
        *state.workspace_root.write().unwrap() = Some(PathBuf::from("/workspace"));
        *state.contexts.write().unwrap() = vec![context("app", PathBuf::from("/workspace/app"))];
        let bundled =
            Url::from_file_path("/workspace/.dovetail/dependencies/prelude/src/types.dove")
                .unwrap();
        assert!(state.context_for(&bundled).is_some());
        let unrelated = Url::from_file_path("/elsewhere/prelude/src/types.dove").unwrap();
        assert!(state.context_for(&unrelated).is_none());
    }

    #[test]
    fn source_edits_wait_for_fetch_but_manifest_changes_supersede_it() {
        let state = WorldState::new();
        let fetching = state.begin_dependency_analysis();
        assert!(state.begin_source_analysis().is_none());
        assert!(state.is_current_analysis(&fetching));
        let replacement = state.begin_dependency_analysis();
        assert!(!state.is_current_analysis(&fetching));
        drop(DependencyAnalysisGuard {
            state: &state,
            cancel: &fetching,
        });
        assert!(state.begin_source_analysis().is_none());
        drop(DependencyAnalysisGuard {
            state: &state,
            cancel: &replacement,
        });
        assert!(state.begin_source_analysis().is_some());
    }

    #[test]
    fn physical_declaration_location_does_not_equate_distinct_graph_owners() {
        let shared = PathBuf::from("/workspace/.dovetail/deps/repo/commit/library");
        let mut first = context("first", shared.clone());
        let mut second = context("second", shared.clone());
        first
            .declaration_owners
            .push((shared.clone(), "manifest-a#library".into()));
        second
            .declaration_owners
            .push((shared.clone(), "manifest-b#library".into()));
        let uri = Url::from_file_path(shared.join("src/lib.dove")).unwrap();
        assert_ne!(
            first.declaration_owner(&uri),
            second.declaration_owner(&uri)
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_file_symlink_retains_its_declaring_project_owner() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let app = root.join("app");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(root.join("shared.dove"), "package app").unwrap();
        let source = app.join("src/main.dove");
        std::os::unix::fs::symlink("../../shared.dove", &source).unwrap();
        let mut owner = context("app-owner", app.clone());
        owner.declaration_owners.push((app, "app-owner".into()));
        let uri = Url::from_file_path(source).unwrap();
        assert_eq!(owner.declaration_owner(&uri), Some("app-owner"));
        let state = WorldState::new();
        state.contexts.write().unwrap().push(owner);
        assert_eq!(state.context_for(&uri).unwrap().identity, "app-owner");
    }
}
