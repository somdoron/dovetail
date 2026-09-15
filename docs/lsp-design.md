# Dovetail LSP Server and VS Code Extension Design

This document designs the **Language Server Protocol (LSP) server** for Dovetail and its **VS Code extension**. The LSP server runs as `dovetail lsp-server` and provides IDE intelligence: diagnostics, completion, navigation, hover, inlay hints, and more. The VS Code extension provides syntax highlighting via a TextMate grammar, layout-aware auto-indentation, and launches the LSP server.

**In scope:** LSP server architecture; VS Code extension with TextMate grammar and semantic tokens; incremental workspace checking; completion (dot-access, scope-aware, auto-import); navigation (go-to-definition, go-to-type-definition, go-to-implementation, find-references); document and workspace symbols; hover with doc comments; signature help; inlay hints (inferred types, parameter names); call hierarchy; code actions (auto-import, add type annotation, organize imports, run tests); test runner integration (run single test, file tests, project tests, workspace tests); diagnostics on save; layout-aware auto-indent; `dovetail lsp-server` CLI with stdio and TCP transport.

**Out of scope:** Rename symbol; code folding; selection range; snippet completions; formatting (`dovetail fmt`); debugging/DAP integration.

**Placeholders (future):** Formatting (`textDocument/formatting`); unused-symbol fade-out (diagnostic tag `Unnecessary`).

**Implementation status:** Not started.

**Prerequisites:** Working `dovetail check` pipeline (lex → parse → collect → infer → rules) with error accumulation.

---

## 1. Overview

### 1.1 Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│  VS Code Extension (TypeScript)                                         │
│  ┌───────────────────────┐  ┌──────────────────────────────────────┐    │
│  │ TextMate Grammar       │  │ Language Client                      │    │
│  │ (.dove syntax)       │  │  - Spawns `dovetail lsp-server`        │    │
│  │ + layout auto-indent   │  │  - stdio or TCP transport            │    │
│  └───────────────────────┘  │  - forwards LSP messages              │    │
│                              └──────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────┘
                                        │ LSP (JSON-RPC)
                                        ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  dovetail lsp-server  (Rust, tower-lsp)                                   │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │ Transport Layer (stdio / TCP)                                      │  │
│  │  - tower-lsp Service                                               │  │
│  │  - JSON-RPC framing                                                │  │
│  ├───────────────────────────────────────────────────────────────────┤  │
│  │ Request Router                                                     │  │
│  │  - textDocument/completion, hover, definition, references, etc.    │  │
│  │  - workspace/symbol                                                │  │
│  │  - Debounced didChange handler                                     │  │
│  ├───────────────────────────────────────────────────────────────────┤  │
│  │ World State                                                        │  │
│  │  - In-memory document store (open files)                           │  │
│  │  - Per-package analysis cache                                      │  │
│  │  - Workspace snapshot (ResolvedWorkspace)                          │  │
│  │  - Symbol index                                                    │  │
│  ├───────────────────────────────────────────────────────────────────┤  │
│  │ Analysis Engine                                                    │  │
│  │  - Package-level incremental check                                 │  │
│  │  - Pipeline: Lex → Parse → Collect → Infer (keystroke)             │  │
│  │  - Pipeline: Lex → Parse → Collect → Infer → Rules (save)         │  │
│  │  - Registry merge for cross-package queries                        │  │
│  ├───────────────────────────────────────────────────────────────────┤  │
│  │ Compiler Library (dovetail crate)                                    │  │
│  │  - Lexer, LayoutFilter, Parser, Typechecker                        │  │
│  │  - Registry, TypedModule, Diagnostics                              │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Design Principles

- **Package-level incrementality:** On change, re-check only the package containing the modified file. Dependent packages are re-checked on save, not on keystroke.
- **Two-tier pipeline:** On keystroke (debounced), run through inference only to update symbols for IDE features. On save, run the full pipeline including rules to produce diagnostics.
- **In-memory documents:** Open files are served from an in-memory buffer. Non-open files are read from disk. The compiler pipeline is adapted to accept source content from either source.
- **Resilient analysis:** Continue collect and inference even when the parser produces error nodes. Insert "error/unknown" types for unresolvable expressions so the typed AST always covers the full file.
- **Leverage existing infrastructure:** The LSP server reuses the compiler's `Lexer`, `LayoutFilter`, `Parser`, `typechecker::collect`, `typechecker::infer`, `typechecker::rules`, `Registry`, `TypedModule`, `Diagnostics`, and `Span` types directly.

---

## 2. CLI Integration

### 2.1 Command Definition

Add `LspServer` to the existing `Commands` enum in `main.rs`:

```rust
/// Start the LSP server
LspServer {
    /// Use TCP transport instead of stdio
    #[arg(long)]
    tcp: bool,

    /// TCP port (only used with --tcp)
    #[arg(long, default_value = "9257")]
    port: u16,
},
```

Usage:

```bash
dovetail lsp-server           # stdio transport (default, used by VS Code)
dovetail lsp-server --tcp     # TCP transport on port 9257
dovetail lsp-server --tcp --port 8080  # TCP transport on custom port
```

### 2.2 Transport

- **stdio (default):** The LSP server reads JSON-RPC messages from stdin and writes responses to stdout. This is the standard VS Code ↔ LSP transport. Diagnostic/log output goes to stderr (never stdout).
- **TCP:** The LSP server listens on the specified port and accepts a single client connection. Useful for debugging the LSP server itself (attach a debugger to the running process, then connect VS Code to the TCP port).

Both transports use the same `tower-lsp` service; only the I/O layer differs.

---

## 3. LSP Server Architecture

### 3.1 Crate Structure

The LSP server lives in a new module within the `dovetail` crate:

```
dovetail/src/
  lsp/
    mod.rs              -- Module root, server setup, transport
    state.rs            -- WorldState, document store, analysis cache
    handlers.rs         -- LSP request/notification handlers
    analysis.rs         -- Incremental analysis engine
    completion.rs       -- Completion logic (dot, scope, auto-import)
    navigation.rs       -- Go-to-definition, references, implementation
    symbols.rs          -- Document/workspace symbol providers
    hover.rs            -- Hover with doc comments and type info
    signature_help.rs   -- Signature help on `(`
    inlay_hints.rs      -- Inferred types, parameter names
    semantic_tokens.rs  -- Semantic token provider (types vs values)
    code_actions.rs     -- Auto-import, add type annotation, organize imports
    call_hierarchy.rs   -- Incoming/outgoing call hierarchy
    diagnostics.rs      -- Diagnostic conversion (Span → LSP Range)
    util.rs             -- Shared LSP ↔ compiler conversions
```

### 3.2 Dependencies

Add to `dovetail/Cargo.toml`:

```toml
tower-lsp = "0.20"
tokio = { version = "1", features = ["full"] }
dashmap = "6"              # concurrent document store
serde_json = "1"
```

### 3.3 Server Setup

```rust
use tower_lsp::{LspService, Server};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

struct DovetailLanguageServer {
    client: Client,
    state: Arc<WorldState>,
}

#[tower_lsp::async_trait]
impl LanguageServer for DovetailLanguageServer {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> { ... }
    async fn initialized(&self, _: InitializedParams) { ... }
    async fn shutdown(&self) -> Result<()> { ... }

    // Notifications
    async fn did_open(&self, params: DidOpenTextDocumentParams) { ... }
    async fn did_change(&self, params: DidChangeTextDocumentParams) { ... }
    async fn did_save(&self, params: DidSaveTextDocumentParams) { ... }
    async fn did_close(&self, params: DidCloseTextDocumentParams) { ... }

    // Requests (see §4–§12 for each)
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> { ... }
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> { ... }
    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> { ... }
    // ... etc.
}
```

### 3.4 Server Capabilities

Declared in `initialize` response:

```rust
ServerCapabilities {
    text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
        open_close: Some(true),
        change: Some(TextDocumentSyncKind::INCREMENTAL),
        save: Some(SaveOptions { include_text: Some(true) }.into()),
        ..Default::default()
    })),
    completion_provider: Some(CompletionOptions {
        trigger_characters: Some(vec![".".into(), ":".into()]),
        resolve_provider: Some(true),
        ..Default::default()
    }),
    hover_provider: Some(HoverProviderCapability::Simple(true)),
    signature_help_provider: Some(SignatureHelpOptions {
        trigger_characters: Some(vec!["(".into(), ",".into()]),
        ..Default::default()
    }),
    definition_provider: Some(OneOf::Left(true)),
    type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
    implementation_provider: Some(ImplementationProviderCapability::Simple(true)),
    references_provider: Some(OneOf::Left(true)),
    document_symbol_provider: Some(OneOf::Left(true)),
    workspace_symbol_provider: Some(OneOf::Left(true)),
    code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
    inlay_hint_provider: Some(OneOf::Left(true)),
    code_lens_provider: Some(CodeLensOptions {
        resolve_provider: Some(false),
    }),
    call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
    semantic_tokens_provider: Some(
        SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
            legend: semantic_token_legend(),
            full: Some(SemanticTokensFullOptions::Bool(true)),
            range: Some(true),
            ..Default::default()
        }),
    ),
    ..Default::default()
}
```

---

## 4. World State

The LSP server maintains a single `WorldState` that is the source of truth for all IDE features.

### 4.1 Data Model

```rust
struct WorldState {
    /// Workspace root directory (from InitializeParams)
    workspace_root: PathBuf,

    /// Resolved workspace manifest (Dovetail.toml)
    workspace: RwLock<ResolvedWorkspace>,

    /// Open documents: URI → (version, source text)
    /// Updated on didOpen/didChange/didClose.
    documents: DashMap<Url, DocumentState>,

    /// Per-package analysis results, keyed by PackagePath.
    /// Updated incrementally on change/save.
    package_cache: RwLock<BTreeMap<PackagePath, PackageAnalysis>>,

    /// Merged workspace registry (all packages combined).
    /// Rebuilt after any package re-analysis.
    workspace_registry: RwLock<Registry>,

    /// Workspace-wide symbol index for workspace/symbol search.
    symbol_index: RwLock<SymbolIndex>,

    /// Debounce timer handle for didChange re-analysis.
    analysis_debounce: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

struct DocumentState {
    version: i32,
    content: String,
    /// Which package this file belongs to.
    package_path: PackagePath,
}

struct PackageAnalysis {
    /// This package's own registry (public + internal symbols).
    registry: Registry,
    /// Typed AST for this package.
    typed_module: TypedModule,
    /// Diagnostics from the most recent full check (save).
    diagnostics: Diagnostics,
    /// Source file ASTs (for navigation within untyped constructs).
    source_files: Vec<SourceFile>,
    /// Hash of source content that produced this analysis (for staleness check).
    content_hash: u64,
}
```

### 4.2 Document Lifecycle

| Event | Action |
|-------|--------|
| `didOpen` | Insert into `documents`. Trigger analysis if no cached result. |
| `didChange` | Update `documents` content. Debounce → re-analyze package (through inference only). |
| `didSave` | Re-analyze package (full pipeline including rules). Re-analyze dependent packages. Publish diagnostics. |
| `didClose` | Remove from `documents`. Keep `package_cache` (still valid from disk). |

### 4.3 Source Content Resolution

The analysis engine needs source content for all files in a package, not just the one being edited. The resolution order:

1. **In-memory:** If the file is in `documents` (open in editor), use that content.
2. **Disk:** Otherwise, read from disk.

This requires adapting the `discover_and_parse_package` function (or creating a parallel version) to accept an optional content overlay map:

```rust
fn analyze_package(
    package: &ResolvedPackage,
    workspace_root: &Path,
    content_overlays: &HashMap<FilePath, String>,
    dependency_registry: &Registry,
) -> PackageAnalysis { ... }
```

---

## 5. Incremental Analysis Engine

### 5.1 Pipeline Modes

The analysis engine supports two modes, corresponding to keystroke and save events:

**Keystroke mode (debounced, ~300ms):**
```
Source Files → Lex → Layout Filter → Parse (with recovery) → Collect → Infer → stop
```
- Only re-analyzes the package containing the changed file.
- Produces: updated `Registry` (symbols) and `TypedModule` (typed AST through inference).
- Does NOT run the Rules phase or produce diagnostics.
- Updates `package_cache` and `workspace_registry` for IDE queries.

**Save mode:**
```
Source Files → Lex → Layout Filter → Parse (with recovery) → Collect → Infer → Rules → diagnostics
```
- Re-analyzes the changed package AND downstream dependent packages.
- Produces: full diagnostics published to the editor.
- Updates `package_cache`, `workspace_registry`, and publishes diagnostics.

### 5.2 Dependency Tracking

The analysis engine uses the `ResolvedWorkspace` to determine:
- **Which package** a file belongs to (by matching file path to project/package source directories).
- **Which packages depend on** the changed package (reverse dependency lookup).

On save, only packages whose dependency's `content_hash` has changed are re-analyzed.

### 5.3 Error-Resilient Pipeline

To provide IDE features even in the presence of errors, the pipeline must:

1. **Parser error recovery** (already implemented): On parse error, skip to the next declaration boundary and continue. The AST contains all successfully parsed declarations.

2. **Continue collect past parse errors:** The collect phase registers all successfully parsed declarations into the registry, even if some declarations failed to parse.

3. **Continue inference past errors:** When inference encounters an unresolvable reference or type error, it assigns `Type::Error` (a new type variant, or reuses `Type::Never`) and continues. The typed AST covers the full file, with error nodes marked.

4. **Rules phase tolerates error types:** The rules phase skips constraint checking for expressions with error types.

### 5.4 Debouncing

On `didChange`, the server debounces re-analysis to avoid running the pipeline on every keystroke:

```rust
async fn did_change(&self, params: DidChangeTextDocumentParams) {
    // 1. Apply incremental text edits to document store
    self.state.apply_edits(&params);

    // 2. Cancel any pending debounced analysis
    self.state.cancel_pending_analysis();

    // 3. Schedule new analysis after 300ms
    self.state.schedule_analysis(params.text_document.uri, Duration::from_millis(300));
}
```

### 5.5 Initial Workspace Load

On `initialize`, the server:

1. Loads `Dovetail.toml` via `manifest::load_manifest()`.
2. Runs the full pipeline (through rules) for all packages in dependency order.
3. Populates `package_cache`, `workspace_registry`, and `symbol_index`.
4. Publishes any diagnostics found.

This is a potentially slow operation for large workspaces. The server should send `window/workDoneProgress` notifications to show progress.

---

## 6. Diagnostics

### 6.1 Diagnostic Publishing

Diagnostics are published only on save (not on keystroke). On save, the server runs the full pipeline and publishes diagnostics for all affected packages:

```rust
async fn did_save(&self, params: DidSaveTextDocumentParams) {
    let affected_packages = self.state.analyze_on_save(&params.text_document.uri).await;

    for (package_path, analysis) in &affected_packages {
        // Group diagnostics by file
        let by_file: HashMap<&FilePath, Vec<&Diagnostic>> = group_by_file(&analysis.diagnostics);

        for (file, diags) in by_file {
            let uri = file_path_to_uri(&self.state.workspace_root, file);
            let lsp_diags: Vec<lsp_types::Diagnostic> = diags.iter()
                .map(|d| convert_diagnostic(d))
                .collect();
            self.client.publish_diagnostics(uri, lsp_diags, None).await;
        }
    }
}
```

### 6.2 Span → LSP Range Conversion

The compiler's `Span` uses 1-indexed lines and columns. LSP uses 0-indexed. Both use UTF-16 code unit offsets for columns (LSP default).

```rust
fn span_to_range(span: &Span) -> Range {
    Range {
        start: Position {
            line: span.line - 1,
            character: span.column - 1,
        },
        end: Position {
            line: span.end_line - 1,
            character: span.end_column - 1,
        },
    }
}

fn convert_diagnostic(diag: &dovetail::common::diagnostics::Diagnostic) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: span_to_range(&diag.span),
        severity: Some(match diag.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        source: Some("dovetail".to_string()),
        message: diag.message.clone(),
        ..Default::default()
    }
}
```

### 6.3 Clearing Diagnostics

When a file's errors are resolved, the server publishes an empty diagnostic list to clear stale diagnostics:

```rust
self.client.publish_diagnostics(uri, vec![], None).await;
```

---

## 7. Completion

### 7.1 Dot Completion (Feature 5a)

Triggered by `.` after an expression. The server:

1. Identifies the expression before the dot using the typed AST.
2. Looks up the type of that expression.
3. Queries the registry for all methods, properties, and extension methods available on that type.
4. Returns `CompletionItem` entries with:
   - `label`: method/property name
   - `kind`: `Method` or `Property`
   - `detail`: type signature (e.g., `(key: K): Option<V>`)
   - `documentation`: doc comment if available
   - `insertText`: method name (with `($1)` snippet for methods with parameters)

**Type-specific completions:**
- **Record types:** field names + module methods
- **Enum types:** module methods (including constructors like `.None`, `.Some(value)`)
- **Class types:** class methods + inherited methods
- **Trait objects:** trait methods
- **Primitive types:** extension methods and module methods (e.g., `Int32.toInt64()`)
- **Array type:** `get`, `set`, `length`, `map`, `filter`, etc.

### 7.2 Scope-Aware Completion (Feature 5b)

Triggered by typing an identifier (no dot). The server:

1. Determines the cursor position in the AST.
2. Collects all symbols visible at that position:
   - **Local variables** in scope (from enclosing `let` bindings, function parameters, pattern bindings)
   - **Functions** from the same package (same-package visibility)
   - **Imported symbols** from the file's import statements
   - **Prelude symbols** (always in scope)
3. Returns `CompletionItem` entries with appropriate `kind` (`Variable`, `Function`, `Class`, `Enum`, `Interface` for trait, etc.)

**Scope resolution strategy:**

The server walks the typed AST to find the innermost scope containing the cursor position. It collects:
- Function parameters
- `let` bindings that precede the cursor
- `for` loop variables
- `match` pattern bindings
- `mutable` variable bindings

Then adds package-level symbols:
- Functions from same package
- Types from same package
- Globals from same package

Then import-level symbols:
- Explicitly imported symbols
- Prelude symbols

### 7.3 Auto-Import Completion (Feature 6)

When typing an unqualified symbol name, the server also searches the full workspace registry for matching symbols that are NOT currently imported:

1. Search `workspace_registry` for public symbols whose name matches the typed prefix.
2. For each match, create a `CompletionItem` with:
   - `labelDetails.description`: source package path (e.g., `standard.collection`)
   - `additionalTextEdits`: insert the `import` statement at the top of the file (after existing imports)
   - `sortText`: prefix with `~` to sort after in-scope completions
3. The import statement is auto-inserted when the user accepts the completion.

**Import insertion logic:**
- Find the last `import` line in the file.
- Insert the new `import` on the next line.
- If no imports exist, insert after the `package` declaration line with a blank line separator.
- Format: `import package.path.SymbolName`

### 7.4 Auto-Import Code Action (Feature 6)

When the typechecker produces an "unknown symbol" error, the server offers a quick fix:

1. Search `workspace_registry` for public symbols matching the error's symbol name.
2. For each match, create a `CodeAction` with:
   - `title`: `Import package.path.SymbolName`
   - `kind`: `quickfix`
   - `edit`: insert the import statement
   - `diagnostics`: link to the triggering diagnostic

---

## 8. Navigation

### 8.1 Go to Definition (Feature 4)

`textDocument/definition` — resolve the symbol under the cursor to its declaration site.

**Resolution strategy:**

1. Find the AST node at the cursor position in the typed AST.
2. Based on the node type:
   - **Variable reference:** Look up the `let` binding or parameter in the local scope. Return the binding's span.
   - **Function call:** The typed AST contains the resolved `MangledName` → look up in `TypedModule.functions` → return the function's `span`.
   - **Type reference:** Look up in the registry → return the type's declaration `span`.
   - **Import path:** Resolve the FQN against the registry → return the declaration span.
   - **Field access (`expr.field`):** Resolve the receiver type → look up the field in the record/class definition → return the field's span.
   - **Method call (`expr.method()`):** Resolve the receiver type → look up the method in the registry → return the method's span.
   - **Enum variant (`Type.Variant`):** Look up the variant in the enum type signature → return the variant's span.

3. Convert the declaration's `Span` (which contains `FilePath` relative to workspace root) to a `Location` URI.

### 8.2 Go to Type Definition (Feature — additional)

`textDocument/typeDefinition` — from a variable or expression, jump to the definition of its type.

1. Find the expression at the cursor position.
2. Get its type from the typed AST (`TypedExpr.ty`).
3. Extract the type's FQN (for `Record`, `Enum`, `Class`, `Newtype`, `TraitObject` types).
4. Look up the type declaration in the registry → return its span.

For primitive types (`Int32`, `Bool`, `String`, etc.), return `None` (no source declaration to navigate to).

### 8.3 Go to Implementation (Feature — additional)

`textDocument/implementation` — from a trait, find all types that implement it.

1. Identify the trait under the cursor (from a type reference or trait name).
2. Search the registry's `trait_impls` for all implementations of that trait.
3. Return a list of `Location`s pointing to each `implement` declaration.

For abstract class methods, find all concrete overrides in subclasses.

### 8.4 Find References (Feature 10)

`textDocument/references` — find all uses of a symbol across the workspace.

**Strategy:**

1. Identify the symbol under the cursor and its FQN/MangledName.
2. Search across all packages' typed ASTs for references to that symbol:
   - **Functions:** Walk all `TypedExpr` nodes looking for function calls whose resolved `MangledName` matches.
   - **Types:** Walk all type annotations and expressions for references to the type's `Fqn`.
   - **Variables:** Walk the enclosing function's body for variable references with matching `VarName`.
   - **Fields:** Walk all field access expressions on the matching record/class type.
3. If `includeDeclaration` is true, also include the declaration site.
4. Return `Location` list.

**Performance consideration:** For workspace-wide references, this requires walking all typed ASTs. The `symbol_index` (§4.1) pre-indexes symbol references to avoid full AST walks on every query:

```rust
struct SymbolIndex {
    /// MangledName → list of reference locations
    references: BTreeMap<MangledName, Vec<Span>>,
    /// FQN → declaration span
    declarations: BTreeMap<Fqn, Span>,
}
```

The index is rebuilt incrementally when a package is re-analyzed.

---

## 9. Symbols

### 9.1 Document Symbols (Feature 3 — document)

`textDocument/documentSymbol` — returns the structure of the current file for the outline view and breadcrumb bar.

Returns a hierarchical `DocumentSymbol` tree:

```
File
├── function main(): Unit              (SymbolKind::Function)
├── record User                        (SymbolKind::Struct)
│   ├── name: String                   (SymbolKind::Field)
│   └── age: Int32                     (SymbolKind::Field)
├── enum Color                         (SymbolKind::Enum)
│   ├── Red                            (SymbolKind::EnumMember)
│   ├── Green                          (SymbolKind::EnumMember)
│   └── Blue                           (SymbolKind::EnumMember)
├── trait Printable                    (SymbolKind::Interface)
│   └── function print(self): String   (SymbolKind::Method)
├── class Animal                       (SymbolKind::Class)
│   ├── name: String                   (SymbolKind::Property)
│   └── function speak(self): String   (SymbolKind::Method)
├── module StringUtils                 (SymbolKind::Module)
│   └── function capitalize(s): String (SymbolKind::Function)
├── test "addition works"              (SymbolKind::Event)
└── global maxRetries: Int32           (SymbolKind::Constant)
```

Built from the parser AST (`SourceFile.declarations`) — does NOT require typechecking. Available even when the file has type errors.

### 9.2 Workspace Symbols (Feature 3 — workspace)

`workspace/symbol` — fuzzy search across all symbols in the workspace.

Uses the `SymbolIndex` to search by name. Returns `SymbolInformation` entries with:
- `name`: symbol name
- `kind`: same mapping as document symbols
- `location`: declaration span
- `containerName`: package path

Supports fuzzy matching (case-insensitive substring or camelCase initials).

---

## 10. Hover

### 10.1 Hover Content (Feature 9)

`textDocument/hover` — show type information and doc comments on hover.

**Content format:**

```markdown
```dovetail
function get(self, key: K): Option<V>
`` `

An immutable hash map lookup. Returns `Some(value)` if the key exists,
`None` otherwise.
```

**For different node types:**

| Node | Hover Content |
|------|--------------|
| Function call | Function signature + doc comment |
| Variable | `let variableName: Type` or `mutable variableName: Type` |
| Parameter | `paramName: Type` |
| Type name | Type definition header + doc comment |
| Field access | `fieldName: Type` from the record/class definition |
| Method call | Method signature + doc comment |
| Enum variant | Variant definition with payload types |
| Import path | Resolved FQN + kind (function/type/module) |
| Literal | Literal type (e.g., `Int32`, `String`, `Bool`) |

**Doc comment extraction:**

Doc comments (`///`) are attached to declarations during lexing (`attach_doc_comments`). The doc comment text is available on the parser AST's declaration nodes and propagated to the registry's type/function signatures.

The hover handler renders doc comments as Markdown, preserving the original formatting.

---

## 11. Signature Help

### 11.1 Trigger and Behavior (Feature 11)

`textDocument/signatureHelp` — triggered on `(` after a function name or `,` between arguments.

1. Parse the text before the cursor to identify the function being called.
2. Look up the function in the typed AST or registry.
3. Handle overloaded functions: return multiple `SignatureInformation` entries, with `activeSignature` set to the best match based on arguments typed so far.
4. Set `activeParameter` based on the comma count before the cursor.

**Response format:**

```rust
SignatureHelp {
    signatures: vec![
        SignatureInformation {
            label: "get(self, key: K): Option<V>".to_string(),
            documentation: Some(doc_comment),
            parameters: Some(vec![
                ParameterInformation {
                    label: ParameterLabel::Simple("key: K".to_string()),
                    documentation: None,
                },
            ]),
            active_parameter: Some(0),
        }
    ],
    active_signature: Some(0),
    active_parameter: Some(0),
}
```

### 11.2 Context Tracking

To determine `activeParameter`, the server counts unmatched commas and open parens before the cursor position. Nested function calls are handled by tracking parenthesis depth.

---

## 12. Inlay Hints

### 12.1 Variable Type Hints (Feature 13)

`textDocument/inlayHint` — show inferred types for `let` bindings without explicit type annotations.

```dovetail
let x = computeValue()     →    let x: Result<Int32, Error> = computeValue()
                                       ^^^^^^^^^^^^^^^^^^ inlay hint
```

**Filtering (reduce noise):**

Skip inlay hints when the type is obvious from context:
- **Literal assignments:** `let x = 5` — type `Int32` is obvious
- **Constructor calls:** `let user = User("Alice", 30)` — type `User` is obvious
- **Explicit type annotation already present:** `let x: Int32 = foo()`
- **Boolean literals:** `let flag = true`
- **String literals:** `let name = "hello"`
- **Array literals with explicit type:** `let arr = Array.fill(10, 0)`

Show inlay hints when the type is NOT obvious:
- **Function call results:** `let result = processData(input)` → `: ProcessedData`
- **Method call results:** `let length = myList.length` → `: Int32`
- **Complex expressions:** `let combined = if cond then a else b` → `: SomeType`
- **Match expressions:** `let value = match x with ...` → `: MatchResultType`

### 12.2 Parameter Name Hints (Feature 13)

Show parameter names at call sites when they're not obvious:

```dovetail
createUser("Alice", 30, true)
           ^^^^    ^^  ^^^^
           name:   age: active:
```

**Filtering (reduce noise):**

Skip parameter name hints when:
- The argument is a variable whose name matches the parameter name: `createUser(name, age, active)` — no hints needed
- The function has only one parameter and the argument is a literal
- The argument is a named argument: `createUser(name = "Alice")` — already explicit
- The parameter name matches a common single-letter convention (`x`, `y`, `i`, `n`) and the function has only one parameter

### 12.3 Inlay Hint Positioning

```rust
InlayHint {
    position: Position { line, character },
    label: InlayHintLabelPart { value: ": Int32" },
    kind: Some(InlayHintKind::TYPE),       // for type hints
    // or
    kind: Some(InlayHintKind::PARAMETER),  // for parameter hints
    padding_left: Some(false),
    padding_right: Some(true),
    ..Default::default()
}
```

---

## 13. Semantic Tokens

### 13.1 Token Types (Feature 1)

The LSP provides semantic tokens to distinguish types from values beyond what the TextMate grammar can do.

**Semantic token types:**

| Token Type | Used For |
|-----------|----------|
| `type` | Type names in type annotations and expressions (`Int32`, `Option`, `User`) |
| `class` | Class type names (`Animal`, `Shape`) |
| `interface` | Trait names (`Hashable`, `Equatable`) |
| `enum` | Enum type names (`Color`, `Result`) |
| `enumMember` | Enum variant names (`Some`, `None`, `Red`) |
| `typeParameter` | Generic type parameter names (`T`, `K`, `V`) |
| `function` | Free function names in calls and definitions |
| `method` | Method names in dot-call expressions |
| `property` | Property names in field access |
| `variable` | Local variable references |
| `parameter` | Function parameter references |
| `keyword` | (handled by TextMate, but can be augmented) |
| `string` | (handled by TextMate) |
| `number` | (handled by TextMate) |
| `comment` | (handled by TextMate) |

**Semantic token modifiers:**

| Modifier | Used For |
|----------|----------|
| `declaration` | Definition site (vs usage site) |
| `definition` | Same as declaration |
| `readonly` | Immutable `let` bindings |
| `modification` | `mutable` variable usage on the left side of `=` |
| `documentation` | Doc comment tokens |

### 13.2 Token Generation

The semantic token provider walks the typed AST and emits tokens for all identifiers with their resolved semantic meaning:

```rust
fn provide_semantic_tokens(typed_module: &TypedModule, file: &FilePath) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    // Walk all typed expressions in functions defined in this file
    for (_, func) in &typed_module.functions {
        if &*func.span.file == file {
            walk_typed_expr(&func.body, &mut tokens);
        }
    }
    // Walk type definitions for type parameter highlights
    for (_, type_def) in &typed_module.types {
        // ...
    }
    tokens
}
```

The token data is encoded as deltas (line delta, start character delta, length, token type, token modifiers) per the LSP semantic tokens specification.

---

## 14. Code Actions

### 14.1 Auto-Import (Feature 6)

See §7.4. When an "unknown symbol" diagnostic is present, offer quick-fix code actions to import matching symbols.

### 14.2 Add Type Annotation (Feature 12)

Offered on `let` bindings without explicit type annotations where the type has been inferred:

```dovetail
let x = computeValue()
```

Code action: "Add type annotation" → transforms to:

```dovetail
let x: Result<Int32, Error> = computeValue()
```

**Implementation:**
1. Find `let` binding at cursor position in the typed AST.
2. Get the inferred type from the typed expression.
3. Format the type as a Dovetail type expression string.
4. Insert `: TypeExpr` after the binding name, before the `=`.

### 14.3 Organize Imports (Feature — additional)

Code action available at the file level (or when cursor is in the import section):

1. **Sort** imports alphabetically by full path.
2. **Group** imports by top-level package (with blank lines between groups).
3. **Remove unused** imports (imports whose symbol is never referenced in the file).

```dovetail
// Before:
import standard.prelude.Option
import myapp.utils.Helper
import standard.collection.Map
import standard.prelude.Result

// After:
import myapp.utils.Helper

import standard.collection.Map
import standard.prelude.Option
import standard.prelude.Result
```

---

## 15. Call Hierarchy

### 15.1 Prepare Call Hierarchy

`textDocument/prepareCallHierarchy` — identify the function at the cursor position. Returns a `CallHierarchyItem` with the function's name, kind, URI, range, and selection range.

### 15.2 Incoming Calls

`callHierarchy/incomingCalls` — find all functions that call the target function.

Uses the `SymbolIndex` (references map) to find all call sites of the target function's `MangledName`, then groups by calling function to produce `CallHierarchyIncomingCall` entries.

### 15.3 Outgoing Calls

`callHierarchy/outgoingCalls` — find all functions called by the target function.

Walks the target function's typed AST body, collecting all function call expressions and their resolved `MangledName`s. Groups unique callees into `CallHierarchyOutgoingCall` entries.

---

## 16. Test Runner Integration

Dovetail tests are declared with the `test` keyword and run via `dovetail test`. The LSP and VS Code extension provide integrated test running through code lenses and commands.

### 16.1 Code Lens: Run Single Test

A **Run Test** code lens appears above each `test` declaration:

```dovetail
▶ Run Test                        ◀ code lens
test "addition works" =
    assert 1 + 2 == 3
```

When clicked, the extension:

1. Identifies the test's fully-qualified test name (FQTN) from the typed AST.
2. Determines which project the test belongs to.
3. Runs `dovetail test --filter "<test-name>" <project>` in an integrated terminal.
4. Parses the output and shows pass/fail status inline (decorations on the test declaration).

**Implementation:**

The LSP server provides code lenses via `textDocument/codeLens`:

```rust
CodeLens {
    range: span_to_range(&test.span),
    command: Some(Command {
        title: "▶ Run Test".to_string(),
        command: "dovetail.runTest".to_string(),
        arguments: Some(vec![
            json!(project_name),
            json!(test_fqtn),
        ]),
    }),
    data: None,
}
```

The VS Code extension registers the `dovetail.runTest` command, which spawns the test runner in a terminal.

### 16.2 Command: Run All Tests in File

Available from the command palette and the editor context menu (right-click):

**Command:** `dovetail.runTestsInFile`

1. Collects all `test` declarations in the current file from the parser AST.
2. Determines the project containing the file.
3. Runs `dovetail test --file "<relative-file-path>" <project>` in an integrated terminal.

The LSP also provides a file-level code lens at the top of files that contain tests:

```dovetail
▶ Run All Tests (5 tests)         ◀ code lens (line 1)
package myapp

test "first test" = ...
```

### 16.3 Command: Run All Tests in Project

Available from the command palette:

**Command:** `dovetail.runTestsInProject`

1. If the workspace has multiple projects, prompts the user to select one (Quick Pick).
2. Runs `dovetail test <project>` in an integrated terminal.

Also available as a code lens on `Dovetail.toml` (one per project):

```toml
▶ Run Tests: myapp               ◀ code lens
[[project]]
name = "myapp"
```

### 16.4 Command: Run All Tests in Workspace

Available from the command palette:

**Command:** `dovetail.runTestsInWorkspace`

Runs `dovetail test` (no project filter) in an integrated terminal, executing all tests across all projects.

### 16.5 Test Output and Results

Test output is displayed in a VS Code integrated terminal. The extension parses the terminal output to provide:

- **Test results view:** Pass/fail counts displayed in the status bar.
- **Inline decorations:** Green checkmark (✓) or red cross (✗) next to each test declaration after a run, using VS Code's `TextEditorDecorationType`.
- **Problem matcher:** A problem matcher in `package.json` parses test failure output to create clickable diagnostic entries in the Problems panel.

**Problem matcher configuration:**

```json
{
  "problemMatchers": [{
    "name": "dovetail-test",
    "owner": "dovetail",
    "pattern": {
      "regexp": "^\\s*FAIL:\\s+(.+?)\\s+-\\s+(.+)$",
      "message": 2,
      "code": 1
    }
  }]
}
```

### 16.6 VS Code Testing API (Future Enhancement)

For richer integration, the extension can adopt VS Code's native Testing API (`vscode.TestController`):

- Discover tests from the LSP (via a custom request `dovetail/discoverTests`).
- Display tests in the Test Explorer sidebar.
- Run/debug individual tests or groups.
- Show inline pass/fail results with gutter icons.

This is a future enhancement over the initial code lens + terminal approach.

---

## 17. VS Code Extension

### 17.1 Extension Structure

```
vscode-dovetail/
  package.json           -- Extension manifest
  tsconfig.json
  src/
    extension.ts         -- Activation, language client setup
  syntaxes/
    dovetail.tmLanguage.json  -- TextMate grammar
  language-configuration.json  -- Brackets, comments, auto-indent
```

### 17.2 Extension Manifest (`package.json`)

```json
{
  "name": "dovetail-lang",
  "displayName": "Dovetail Language",
  "description": "Dovetail language support: syntax highlighting, LSP, and auto-indent",
  "version": "0.1.0",
  "publisher": "dovetail-lang",
  "engines": { "vscode": "^1.85.0" },
  "categories": ["Programming Languages"],
  "activationEvents": [
    "onLanguage:dovetail",
    "workspaceContains:Dovetail.toml"
  ],
  "main": "./out/extension.js",
  "contributes": {
    "languages": [{
      "id": "dovetail",
      "aliases": ["Dovetail"],
      "extensions": [".dove"],
      "configuration": "./language-configuration.json"
    }],
    "grammars": [{
      "language": "dovetail",
      "scopeName": "source.dovetail",
      "path": "./syntaxes/dovetail.tmLanguage.json"
    }],
    "commands": [
      {
        "command": "dovetail.runTest",
        "title": "Dovetail: Run Test"
      },
      {
        "command": "dovetail.runTestsInFile",
        "title": "Dovetail: Run All Tests in File"
      },
      {
        "command": "dovetail.runTestsInProject",
        "title": "Dovetail: Run All Tests in Project"
      },
      {
        "command": "dovetail.runTestsInWorkspace",
        "title": "Dovetail: Run All Tests in Workspace"
      }
    ],
    "menus": {
      "editor/context": [
        {
          "command": "dovetail.runTestsInFile",
          "when": "editorLangId == dovetail",
          "group": "dovetail@1"
        }
      ]
    },
    "configuration": {
      "title": "Dovetail",
      "properties": {
        "dovetail.serverPath": {
          "type": "string",
          "default": "dovetail",
          "description": "Path to the dovetail binary"
        },
        "dovetail.serverConnection": {
          "type": "string",
          "enum": ["stdio", "tcp"],
          "default": "stdio",
          "description": "How to connect to the LSP server"
        },
        "dovetail.serverPort": {
          "type": "number",
          "default": 9257,
          "description": "TCP port for the LSP server (when using tcp connection)"
        },
        "dovetail.inlayHints.typeHints": {
          "type": "boolean",
          "default": true,
          "description": "Show type hints for inferred variables"
        },
        "dovetail.inlayHints.parameterHints": {
          "type": "boolean",
          "default": true,
          "description": "Show parameter name hints at call sites"
        }
      }
    }
  }
}
```

### 17.3 Language Client (`extension.ts`)

```typescript
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
  StreamInfo,
} from 'vscode-languageclient/node';
import * as net from 'net';

let client: LanguageClient;

export function activate(context: vscode.ExtensionContext) {
  const config = vscode.workspace.getConfiguration('dovetail');
  const connection = config.get<string>('serverConnection', 'stdio');

  let serverOptions: ServerOptions;

  if (connection === 'tcp') {
    const port = config.get<number>('serverPort', 9257);
    serverOptions = () => {
      const socket = net.connect({ port });
      const result: StreamInfo = {
        writer: socket,
        reader: socket,
      };
      return Promise.resolve(result);
    };
  } else {
    const serverPath = config.get<string>('serverPath', 'dovetail');
    serverOptions = {
      command: serverPath,
      args: ['lsp-server'],
      transport: TransportKind.stdio,
    };
  }

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'dovetail' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.dove'),
    },
  };

  client = new LanguageClient(
    'dovetail-language-server',
    'Dovetail Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
```

### 17.4 TextMate Grammar (`dovetail.tmLanguage.json`)

The TextMate grammar provides baseline syntax highlighting before the LSP's semantic tokens are available. It covers:

**Scopes:**

| Pattern | Scope |
|---------|-------|
| `package`, `import`, `function`, `let`, `mutable`, `if`, `then`, `else`, `match`, `case`, `with`, `for`, `in`, `while`, `do`, `return`, `record`, `enum`, `class`, `trait`, `implement`, `extension`, `module`, `newtype`, `public`, `private`, `internal`, `protected`, `abstract`, `final`, `async`, `coroutine`, `where`, `as`, `test`, `true`, `false`, `assert`, `panic`, `extends`, `begin`, `end` | `keyword.control.dovetail` / `keyword.other.dovetail` / `storage.type.dovetail` |
| `///` doc comments | `comment.block.documentation.dovetail` |
| `//` line comments | `comment.line.double-slash.dovetail` |
| `"..."` strings | `string.quoted.double.dovetail` |
| `'...'` char literals | `string.quoted.single.dovetail` |
| `$name`, `${expr}` in strings | `variable.other.interpolation.dovetail` |
| `"""..."""` multi-line strings | `string.quoted.triple.dovetail` |
| Integer and float literals | `constant.numeric.dovetail` |
| `true`, `false` | `constant.language.boolean.dovetail` |
| Operators (`+`, `-`, `*`, `/`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `\|\|`, `!`, `&`, `\|`, `^`, `~`, `<<`, `>>`, `%`) | `keyword.operator.dovetail` |
| `->`, `=>` | `keyword.operator.arrow.dovetail` |
| Type names (uppercase start) | `entity.name.type.dovetail` (heuristic; semantic tokens provide accurate classification) |
| Function definitions | `entity.name.function.dovetail` |

### 17.5 Language Configuration (`language-configuration.json`)

```json
{
  "comments": {
    "lineComment": "//"
  },
  "brackets": [
    ["(", ")"],
    ["[", "]"],
    ["{", "}"]
  ],
  "autoClosingPairs": [
    { "open": "(", "close": ")" },
    { "open": "[", "close": "]" },
    { "open": "{", "close": "}" },
    { "open": "\"", "close": "\"", "notIn": ["string", "comment"] },
    { "open": "'", "close": "'", "notIn": ["string", "comment"] }
  ],
  "surroundingPairs": [
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
    ["\"", "\""],
    ["'", "'"]
  ],
  "indentationRules": {
    "increaseIndentPattern": "^.*\\b(=|then|else|do|with|->)\\s*$",
    "decreaseIndentPattern": "^\\s*(else|$)"
  },
  "onEnterRules": [
    {
      "beforeText": "^.*\\b(=|then|else|do|with)\\s*$",
      "action": { "indent": "indent" }
    },
    {
      "beforeText": "^.*->\\s*$",
      "action": { "indent": "indent" }
    },
    {
      "beforeText": "^.*=>\\s*$",
      "action": { "indent": "indent" }
    }
  ],
  "wordPattern": "[a-zA-Z_][a-zA-Z0-9_]*"
}
```

### 17.6 Layout-Aware Auto-Indent (Feature 2)

The language configuration's `onEnterRules` handle automatic indentation after layout openers. When the user presses Enter after a layout opener (`=`, `then`, `else`, `do`, `with`, `->`, `=>`), the next line is automatically indented by one level (4 spaces).

**Layout openers and their indent behavior:**

| Layout Opener | Example | Result |
|--------------|---------|--------|
| `=` (definition) | `function foo() =⏎` | Indent next line |
| `then` | `if x > 0 then⏎` | Indent next line |
| `else` | `else⏎` | Indent next line |
| `do` | `while x > 0 do⏎` | Indent next line |
| `with` | `match x with⏎` | Indent next line |
| `->` (lambda) | `(x) ->⏎` | Indent next line |
| `=>` (case arm) | `case Some(x) =>⏎` | Indent next line |

The `increaseIndentPattern` regex matches lines ending with a layout opener (possibly followed by whitespace). VS Code uses this to determine when to increase the indent level on the next line.

**Deindent behavior:**

When the user starts a new statement at the same indentation level as the block opener, no special action is needed — they simply type at the current indent level. When they want to close a block, they manually decrease indentation (Shift+Tab or backspace).

---

## 18. Compiler Adaptations

Several changes to the existing compiler infrastructure are needed to support the LSP:

### 18.1 Content Overlay Support

`discovery::discover_and_parse_package` currently reads all files from disk. For LSP, we need a version that accepts content overlays:

```rust
pub fn discover_and_parse_package_with_overlays(
    package_path: &PackagePath,
    source_dir: &Path,
    workspace_root: &Path,
    content_overlays: &HashMap<FilePath, String>,
    diagnostics: &mut Diagnostics,
) -> Option<PackageAst> {
    // Same as discover_and_parse_package, but:
    // - For each .dove file, check content_overlays first
    // - If found, use the overlay content instead of reading from disk
    // - If not found, read from disk as usual
}
```

### 18.2 Splittable Typechecker Phases

The `typecheck` function currently runs all three phases (Collect → Infer → Rules) as a unit. For LSP, we need to call them independently:

```rust
/// Run Collect + Infer only (for keystroke analysis).
pub fn typecheck_through_inference(
    package_ast: &PackageAst,
    registry: &Registry,
) -> TypeCheckerResult {
    // Same as typecheck(), but skip:
    //   - rules::check_rules()
    //   - desugar_for, desugar_try, desugar_await
    //   - capture::analyze_captures
    //   - variance_cast::elaborate_variance_casts
}
```

Alternatively, add a `mode` parameter to `typecheck`:

```rust
pub enum TypecheckMode {
    /// Full pipeline: Collect → Infer → Rules + desugaring
    Full,
    /// Symbols only: Collect → Infer (no rules, no desugaring)
    SymbolsOnly,
}

pub fn typecheck(
    package_ast: &PackageAst,
    registry: &Registry,
    mode: TypecheckMode,
) -> TypeCheckerResult { ... }
```

### 18.3 Error-Resilient Collect and Infer

The collect and infer phases must not bail out when the parser produces error nodes:

- **Collect:** Skip declarations that failed to parse (they won't be in the AST). Continue registering all successfully parsed declarations.
- **Infer:** When encountering an unresolved reference, record an error diagnostic and assign an "error" type to the expression. Continue inference for the rest of the function body. The typed AST is complete, with some nodes having error types.

This may require a new `Type::Error` variant (or reuse `Type::Never`) to represent unresolvable types in the typed AST without blocking further inference.

### 18.4 Span and Doc Comment Preservation

Ensure spans and doc comments survive all pipeline stages so the LSP can use them:

- **Doc comments on registry entries:** The registry's `FunctionSignature`, `RecordTypeSignature`, `EnumTypeSignature`, `TraitSignature`, `ClassTypeSignature` should carry an optional `doc_comment: Option<String>` field. (Check if this is already the case; if not, add it during collect.)
- **Declaration spans in registry:** Ensure all registry entries carry the full `Span` of the declaration (not just the name span). This is needed for document symbols and go-to-definition.
- **Parameter name spans:** For signature help and inlay hints, parameter names need spans in the typed AST.

---

## 19. Implementation Plan

The implementation is split into phases, each delivering incrementally useful functionality.

### Phase 1: Foundation (VS Code extension + basic LSP)

1. **VS Code extension scaffolding** — `package.json`, language configuration, TextMate grammar.
2. **LSP server skeleton** — `dovetail lsp-server` command, `tower-lsp` setup, stdio and TCP transport.
3. **Workspace initialization** — Load `Dovetail.toml`, compile all packages, populate world state.
4. **Diagnostics on save** — Full pipeline on `didSave`, publish diagnostics.
5. **Document symbols** — Outline view from parser AST.

Deliverable: Syntax highlighting, auto-indent on Enter, error diagnostics on save, file outline.

### Phase 2: Navigation

6. **Go to Definition** — Resolve symbols in typed AST to declaration spans.
7. **Go to Type Definition** — From expression type to type declaration.
8. **Hover** — Type info + doc comments.
9. **Workspace symbols** — Fuzzy search across symbol index.

Deliverable: Full navigation within and across files, hover documentation.

### Phase 3: Completion

10. **Scope-aware completion** — Local variables, functions, types in scope.
11. **Dot completion** — Methods and properties on the receiver type.
12. **Auto-import completion** — Search workspace registry, insert import on accept.
13. **Auto-import code action** — Quick fix on "unknown symbol" errors.

Deliverable: Full completion support with auto-import.

### Phase 4: Incremental Analysis + Advanced Features

14. **Incremental analysis engine** — Package-level caching, debounced keystroke analysis (through inference), content overlays.
15. **Semantic tokens** — Types vs values distinction.
16. **Inlay hints** — Inferred types and parameter names with noise filtering.
17. **Signature help** — Parameter info on `(` and `,`.

Deliverable: Responsive editing experience with real-time symbol updates.

### Phase 5: References and Code Intelligence

18. **Find references** — Symbol index, workspace-wide search.
19. **Go to Implementation** — Trait → implementing types.
20. **Call hierarchy** — Incoming and outgoing calls.
21. **Organize imports** — Sort, group, remove unused.
22. **Add type annotation** — Code action on `let` bindings.
23. **Test runner integration** — Code lens on tests, run test/file/project/workspace commands, terminal output parsing.

Deliverable: Complete code intelligence suite with integrated test running.

### Phase 6: Polish

24. **Error-resilient analysis** — Harden collect/infer to handle all error patterns gracefully.
25. **Performance tuning** — Profile and optimize hot paths (large workspaces, many packages).
26. **Formatting placeholder** — Stub `textDocument/formatting` capability.
27. **Unused symbol fade-out placeholder** — Stub `Unnecessary` diagnostic tag.

---

## 20. Position Resolution Utilities

Many LSP features require finding the AST node at a given cursor position. This is a critical shared utility.

### 20.1 Position-to-Node Resolver

```rust
enum NodeAtPosition {
    /// A function call expression
    FunctionCall {
        callee: MangledName,
        args: Vec<TypedExpr>,
        span: Span,
    },
    /// A method call (dot-access + call)
    MethodCall {
        receiver_type: Type,
        method: MangledName,
        span: Span,
    },
    /// A field access expression
    FieldAccess {
        receiver_type: Type,
        field_name: String,
        span: Span,
    },
    /// A variable reference
    Variable {
        name: VarName,
        ty: Type,
        span: Span,
    },
    /// A parameter reference
    Parameter {
        name: String,
        ty: Type,
        span: Span,
    },
    /// A type annotation
    TypeReference {
        fqn: Fqn,
        span: Span,
    },
    /// A let binding (cursor on the binding name)
    LetBinding {
        name: VarName,
        ty: Type,
        has_annotation: bool,
        span: Span,
    },
    /// A function definition (cursor on the function name)
    FunctionDef {
        name: MangledName,
        span: Span,
    },
    /// An import path
    Import {
        fqn: Fqn,
        span: Span,
    },
    /// An enum variant
    EnumVariant {
        enum_fqn: Fqn,
        variant_name: String,
        span: Span,
    },
    /// Inside a string literal or comment (no intelligence)
    Inert,
}

fn find_node_at_position(
    typed_module: &TypedModule,
    source_files: &[SourceFile],
    file: &FilePath,
    position: Position,
) -> Option<NodeAtPosition> {
    // 1. Convert LSP Position (0-indexed) to compiler position (1-indexed)
    // 2. Walk typed expressions in functions defined in this file
    // 3. Find the innermost node whose span contains the position
    // 4. Classify the node
}
```

### 20.2 Scope Resolver

For completion, determine all symbols visible at a given position:

```rust
struct VisibleScope {
    /// Local variables (name → type), ordered by declaration
    locals: Vec<(VarName, Type)>,
    /// Function parameters
    params: Vec<(String, Type)>,
    /// Same-package functions
    package_functions: Vec<FunctionSignature>,
    /// Same-package types
    package_types: Vec<(Fqn, SymbolKind)>,
    /// Imported symbols
    imports: Vec<(Fqn, SymbolKind)>,
    /// Prelude symbols
    prelude: Vec<(Fqn, SymbolKind)>,
}

fn resolve_scope_at_position(
    typed_module: &TypedModule,
    registry: &Registry,
    source_file: &SourceFile,
    file: &FilePath,
    position: Position,
) -> VisibleScope { ... }
```

---

## 21. File-to-Package Resolution

The LSP receives file URIs from the editor. It needs to determine which package a file belongs to.

### 21.1 Resolution Algorithm

```rust
fn resolve_file_to_package(
    workspace: &ResolvedWorkspace,
    file_path: &Path,
) -> Option<(ProjectName, PackagePath)> {
    // 1. Make file_path relative to workspace root
    // 2. For each project in the workspace:
    //    a. For each package in the project:
    //       - Check if the file is in the package's source_dir
    //       - If yes, return (project.name, package.path)
    // 3. Check test directories similarly
    // 4. Return None if file doesn't belong to any known package
}
```

This resolution is cached in `DocumentState.package_path` when a file is opened, so subsequent operations don't need to re-resolve.

---

## 22. Threading and Concurrency

### 22.1 Async Architecture

The LSP server uses `tokio` as its async runtime (required by `tower-lsp`). The architecture:

- **Main thread:** Runs the `tower-lsp` event loop, dispatches requests/notifications.
- **Analysis task:** Spawned on a background `tokio::task::spawn_blocking` thread for CPU-intensive compiler work (lex, parse, typecheck). Results are communicated back via channels.
- **World state:** Protected by `RwLock` for concurrent read access (hover, completion) and exclusive write access (analysis results).

### 22.2 Request Cancellation

LSP supports `$/cancelRequest`. When the client cancels a request (e.g., the user moves the cursor before completion finishes), the server should abort the in-flight computation. `tower-lsp` handles this automatically for async handlers.

### 22.3 Debounce Strategy

```
didChange event
    │
    ▼
Cancel previous debounce timer (if any)
    │
    ▼
Start new 300ms timer
    │
    ├─── (more didChange events arrive) → restart timer
    │
    ▼ (timer fires)
    │
Spawn analysis task (keystroke mode: through inference only)
    │
    ▼
Update package_cache + workspace_registry
    │
    ▼
(No diagnostics published)
```

---

## 23. Error Handling

### 23.1 Graceful Degradation

The LSP server should never crash. When an analysis fails unexpectedly:

1. Log the error to stderr.
2. Keep using the last successful analysis results.
3. Return empty/null results for LSP requests rather than errors.
4. Report the issue via `window/showMessage` (for fatal errors) or `window/logMessage` (for warnings).

### 23.2 Workspace Recovery

If `Dovetail.toml` is malformed or missing:
- The LSP server enters a "degraded mode" with no workspace analysis.
- TextMate grammar still provides syntax highlighting.
- File-level parsing (document symbols) still works.
- Type-aware features (completion, hover, go-to-definition) are unavailable until the manifest is fixed.

When the user saves a corrected `Dovetail.toml`, the server re-loads the workspace and performs a full analysis.

---

## 24. Logging

### 24.1 Logging Strategy

All log output goes to stderr (never stdout, which is reserved for LSP JSON-RPC). Logging levels:

| Level | Content |
|-------|---------|
| Error | Analysis crashes, manifest parse failures, transport errors |
| Warn | Degraded features, stale cache used, file not found |
| Info | Workspace loaded, package analyzed, server started/stopped |
| Debug | Individual request handling, debounce events, cache hits/misses |
| Trace | Full request/response JSON, token-level analysis |

The log level is configurable via environment variable (`DOVETAIL_LSP_LOG=debug`) or CLI flag.

---

## 25. Testing Strategy

### 25.1 Unit Tests

Each LSP module has unit tests with mock world state:
- **Completion:** Given a typed module and cursor position, verify completion items.
- **Navigation:** Given a typed module and cursor position, verify the resolved location.
- **Diagnostics:** Given source with errors, verify LSP diagnostic output.
- **Inlay hints:** Given a typed module, verify hint positions and labels.

### 25.2 Integration Tests

End-to-end tests that spin up the LSP server and send JSON-RPC messages:
- Open a file, verify diagnostics.
- Trigger completion at a position, verify items.
- Go to definition, verify target location.
- Edit a file, save, verify updated diagnostics.

Use the `tower-lsp` test utilities or spawn the server process and communicate via stdio.

### 25.3 VS Code Extension Tests

- Grammar tests: Verify TextMate scopes for sample Dovetail code.
- Auto-indent tests: Verify indentation behavior after layout openers.
- Extension activation: Verify the language client starts and connects.
