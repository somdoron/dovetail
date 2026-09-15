# Multi-Project, Multi-Package, and Multi-File Design

This document designs support for **multiple projects**, **multiple packages per project**, and **multiple source files per package** in the Dovetail compiler. It covers the manifest format (`Dovetail.toml`), directory layout, CLI changes, orchestration, and implementation phases.

---

## 1. Overview and goals

- **One `Dovetail.toml`** at the workspace root (unlike Rust’s per-crate `Cargo.toml`). All projects are defined in this single manifest.
- **Projects** are the top-level unit. Each project is a folder with a `src/` directory. Projects can depend on other projects.
- **Packages** live inside a project. The project’s `root_package` defines the root FQN; packages are subpaths under that (e.g. `root_package = "com.example.app"` → package `com.example.app.utils` is the folder `src/utils/`).
- **Package declaration in source** must match the directory structure; the compiler verifies this.
- **Compilation order** is explicit: per project, a `packages` field defines the order of packages (by folder name or FQN). File order within a package is irrelevant.
- **CLI** gains an optional project name (compile one project or all). The driver orchestrates multi-project and multi-package: check each package in order, merge registry and typed module on success, stop on first package error; codegen only after all packages (and projects) are checked, from the merged TypedModule (via a merge function on TypedModule).
- **Imports** are the only way to bring external symbols into scope. The compiler never searches the registry directly for a bare name; it resolves names from **source-level imports** (and same-package declarations). Imports can be symbol-level or package-level; Dovetail supports the **`as`** keyword to rename imports and does **not** support wildcard imports. Every import is validated against the registry (symbol must exist and be public). One can also use **fully qualified names** in code (e.g. `com.example.app.utils.func(...)`) without importing.

**Implementation status:** Done.

---

## 2. Dovetail.toml schema and semantics

### 2.1 Location and scope

- **Single manifest:** `Dovetail.toml` lives at the **workspace root** (directory from which `dovetail` is typically invoked).
- **All projects** are declared in this file. There is no per-project manifest.

### 2.2 Projects

Each project is a table in the `[project]` or `[[project]]` section (exact TOML shape to be chosen; below uses a list of tables).

```toml
[[project]]
name = "myapp"
root_package = "com.example.myapp"
depends = ["mylib"]   # optional: list of project names this project depends on
packages = ["utils", "core", "main"]   # order of packages (folder names or FQNs, see below)
```

- **`name`** (required): Identifier for the project. Used in `depends` and on the CLI.
- **`root_package`** (required): The FQN prefix for all packages in this project. Every source file under this project’s `src/` belongs to a package whose path is `root_package` or a subpath (e.g. `com.example.myapp` or `com.example.myapp.utils`).
- **`depends`** (optional): List of project names that this project depends on. The dependency graph must be a DAG. Compilation order of projects is a topological sort of this graph (dependencies first).
- **`packages`** (required): Ordered list of packages in this project. Defines the order in which packages are typechecked and merged. Each entry can be:
  - **`"."`** — the root package (package at `src/`, FQN = `root_package`).
  - **Folder-relative name:** e.g. `"utils"` meaning the package under `src/utils/` → FQN `root_package.utils` (i.e. `com.example.myapp.utils`).
  - **FQN:** e.g. `"com.example.myapp.utils"` (must be under this project’s `root_package`). This allows explicit full names when desired.

The compiler resolves each `packages` entry to a canonical `PackagePath` (FQN). The root package is the package at `src/` (no extra path segment); in the manifest it is referred to as `"."`.

### 2.3 Project directory layout

- **Project folder:** Each project lives in a directory under the workspace root. The directory name must equal the project’s `name` in the manifest. For example, `name = "myapp"` means the project’s files are in `<workspace_root>/myapp/` (and that folder must contain a `src/` subdirectory).
- **Source directory:** Inside the project folder there must be a **`src`** directory. All Dovetail source files (`*.dove`) under `src/` belong to that project.
- **Package ↔ path:** Under `src/`, the path relative to `src/` corresponds to the package path under `root_package`:
  - `src/` → package `root_package` (e.g. `com.example.myapp`)
  - `src/utils/` → package `root_package.utils` (e.g. `com.example.myapp.utils`)
  - `src/utils/helpers/` → package `root_package.utils.helpers`
- **Package declaration check:** Every source file must start with `package <path>` and that path must equal the package derived from the file’s path under `src/`. The compiler reports an error if they differ.

### 2.4 Cross-project dependencies

- Projects in `depends` are compiled (and checked) before the depending project. Their **merged** registry (and typed module) is what the depending project’s packages see.
- No circular project dependencies. The compiler builds a DAG and reports an error on cycles.

---

## 3. Packages and multi-file

- **Package = unit of typechecking.** The typechecker runs once per package. Its inputs: the **package name** (FQN), built from the project’s `root_package` and the folder path under `src/`; all source files in that package (parsed); and the merged registry from previously checked packages (and projects).
- **File order within a package is irrelevant.** The compiler gathers all source files in the package directory (recursively or by convention; see below), parses them, and presents a single logical “module” to the typechecker (see implementation).
- **No circular package dependencies.** Within a project, order is given by `packages`. Across projects, a project only sees dependencies from projects it lists in `depends`; order is by project DAG then by each project’s `packages` order.

---

## 4. Imports

Imports determine how symbols from other packages are visible in a file. The compiler **never** resolves a bare identifier by searching the registry; it only resolves names that are (1) declared in the current package, (2) brought in by an import in this file, or (3) written as a fully qualified name (FQN) or as a qualified access on an imported package.

### 4.1 Where imports appear

- Imports appear at the top of a file, after the **package** declaration. Order of import lines is irrelevant. Each import is a single statement (one symbol or one package per import).

### 4.2 Import forms

- **Symbol import:** `import <PackagePath>.<Symbol>`  
  Brings a single type or function into scope under its symbol name. Example: `import com.example.utils.helper` → `helper` is in scope (and must refer to a public type or function in that package).
- **Symbol import with alias:** `import <PackagePath>.<Symbol> as <LocalName>`  
  Same as above, but the symbol is visible under `LocalName`. Example: `import com.example.utils.helper as h` → use `h(...)` or `h` as type.
- **Package import:** `import <PackagePath>` or `import <PackagePath> as <LocalName>`  
  Brings the **package** into scope so that members can be accessed with qualified syntax. If `as` is omitted, the local name is the last segment of the package path (e.g. `import com.example.utils` → local name `utils`). Then in code one writes `utils.func(...)` or `utils.SomeType`. If `as` is present, that name is used (e.g. `import com.example.utils as u` → `u.func(...)`).
- **No wildcard imports.** Dovetail does not support `import pkg.*` or similar. Every imported symbol or package is explicit.

### 4.3 Fully qualified names (FQN) in code

- A caller can use the **fully qualified name** of a symbol without importing it: e.g. `com.example.app.utils.func(1)`. The compiler resolves such names by looking up the FQN in the registry (the symbol must exist and be public in a dependency package). So:
  - **With import:** `import com.example.app.utils.func` then call `func(1)`.
  - **With package import:** `import com.example.app.utils` then call `utils.func(1)`.
  - **With FQN only:** no import; write `com.example.app.utils.func(1)`.

FQN use is validated against the registry the same way as imports: the referenced package and symbol must be present and public.

### 4.4 Resolution order

The compiler resolves function and type names in a fixed order.

**Bare name (no dot):** resolve in this order:
1. **Local in the file** — declarations in the current file (e.g. local variables, function parameters, types and functions declared in the same file).
2. **From imports** — symbols and package aliases brought in by this file’s imports (symbol import brings one name; package import brings `pkg` for `pkg.member`).
3. **From package** — declarations in the same package (other files in the package). No import needed.

**Name with a dot (qualified):** resolve in this order:
1. **From imports** — the first segment is a package alias from a package import (e.g. `utils.func` where `utils` was imported); then the rest is looked up in that package.
2. **From registry** — treat as a fully qualified name (FQN) and look up in the dependency registry (e.g. `com.example.utils.func`).

We never resolve a bare identifier by searching the registry; only the three steps above (for bare names) or the two steps above (for qualified names).

So “unknown name” errors are resolved by adding an import or using the FQN; when reporting an "unknown name" error, the compiler may suggest an import if a matching public symbol exists in the registry (e.g. "unknown name `foo`; add `import com.example.utils.foo`?").

### 4.5 Validation of imports

- Every import is **validated** against the **dependency registry** (the merged registry from previously checked packages and projects):
  The import path alone does not distinguish package from symbol—**check the registry for both**: if the full path is a registered package, treat as package import; if the path is (package + symbol) and that symbol is public in the registry, treat as symbol import. If both match (a package and a symbol share the same path), **prefer symbol**. However, see Section 8.8: the registry enforces that **no conflict** exists between package paths and symbol FQNs, so in practice this ambiguity should not arise. Emit diagnostics for unknown symbol, unknown package, or non-public symbol.
- **Shadowing:** As part of import validation, if an import introduces a name that **shadows** another import or a type/function from the current package, the compiler emits a **warning** and suggests using the **`as`** keyword to alias the import (e.g. "import shadows package type `Foo`; use `import other.Foo as OtherFoo`"). This keeps names unambiguous and makes the intended binding explicit.

Validation happens in the **Collect** phase (or a dedicated import-validation step before inference): when building the import scope for a file, resolve each import against the registry, check for shadowing, and attach diagnostics.

### 4.6 Same package

- Symbols defined in the **same package** (other files in the package) are in scope without import. So within a package, all declarations from all files are visible; imports are only for **other** packages.

### 4.7 Grammar and placement (to be aligned with grammar.md)

- Import declaration: `import` followed by a dotted path, optionally `as` and a local name. Grammar:
  `import_decl = "import" IDENT { "." IDENT } [ "as" IDENT ]`
  The parser treats the entire dotted path as an opaque sequence of segments. Whether the import is a **package import** or a **symbol import** is determined during **semantic analysis** (Collect phase), not at parse time — the typechecker checks the registry to distinguish whether the path refers to a package or a package+symbol (see Section 4.5 and 8.8). Exact tokenization and layout rules follow the rest of the language.

---

## 5. Compilation and check flow (orchestration)

### 5.1 Check package (primitive)

- **Input:** A package identity (project + package path), its parsed AST (from all files in that package), and the **accumulated merged registry** from all previously checked packages (and projects).
- **Output:** `TypeCheckerResult`: typed module for this package, this package’s registry (public only, after strip_internal), and diagnostics.
- **Behaviour:** Same as current typechecker: collect → infer → rules. If diagnostics contain errors, the caller (orchestrator) stops and does not continue to the next package.

### 5.2 Orchestrator (high level)

1. **Load `Dovetail.toml`** at workspace root. Resolve project list and dependency DAG. If `dovetail build` or `dovetail check` is given an optional **project name**, filter to that project (and its dependencies); otherwise include all projects.
2. **Topological order of projects** (dependencies first).
3. For each project in that order:
   - **Discover packages** according to `packages` (and optionally by scanning `src/` to validate that every folder with source is listed).
   - For each package in the order given by `packages`:
     - **Gather** all source files for that package (path ↔ package check; error if package declaration doesn’t match path).
     - **Parse** all files (lex → layout → parse). If any file has lex/parse errors, treat as package error and stop (no typecheck for this package; orchestrator stops for “this package has errors” policy).
     - **Check package:** call typecheck with combined package AST and current merged registry.
     - If **diagnostics have errors:** stop orchestration; report diagnostics; do not run codegen.
     - Otherwise **merge:** merge this package’s registry into the accumulated registry; merge this package’s typed module into the accumulated typed module (see below).
   - After all packages **of that project** have been checked successfully: **Codegen** runs for that project on the accumulated **TypedModule**. This TypedModule is the result of merging **all** packages: the project's own packages (merged via `TypedModule::merge`) **plus** all dependency projects' TypedModules (pulled in transitively). Each project is built **independently** — its WASM contains all code from its own packages and all dependency packages. There is **no linking**; everything is code-based; each project produces a **self-contained, standalone WASM** with its own `main` from that project's root package.

4. So for `dovetail build` (no project filter), we build multiple WASM modules—one per project—each self-contained with all its dependency code baked in.

**Policy:** “If a package has errors we stop.” So we do not continue to the next package or project. No codegen if any package failed.

### 5.3 Merging registries and typed modules

- **Registry:** Already supported: `Registry::merge(a, b)` (or `a.merge(&b)`) produces a new registry with all packages, types, and functions from both. The orchestrator keeps an accumulated registry and merges each package’s (stripped) registry into it after a successful check.
- **Typed module:** The typechecker returns one `TypedModule` per package. Add a **merge function on TypedModule** (e.g. `merge(&self, other: &TypedModule) -> TypedModule`). The orchestrator merges each package's TypedModule into one per project. **Important:** the merge must keep **all** functions (both public and internal) — internal functions are needed for codegen. Only `Registry` strips internals; `TypedModule` never does. Codegen is unchanged (still takes `&TypedModule`). One WASM per project; entry point `main` from the root package. No linking — each project's TypedModule includes all dependency code (see Section 5.2).

---

## 6. CLI changes

### 6.1 Commands (extended)

| Command | Description |
|---------|-------------|
| `dovetail init <PROJECT>` | Create a new workspace: write a basic `Dovetail.toml` with one project and create the project directory structure (`<PROJECT>/src/`). Fails if `Dovetail.toml` or the project directory already exists. |
| `dovetail projects add <PROJECT>` | Add a project to the existing workspace: append the project to `Dovetail.toml` and create the project directory structure (`<PROJECT>/src/`). Fails if `Dovetail.toml` is missing or the project already exists. |
| `dovetail build [PROJECT]` | Compile. If `PROJECT` is given, build that project (and its dependencies). Otherwise build all projects. Reads `Dovetail.toml` at current directory (workspace root). |
| `dovetail check [PROJECT]` | Same as build but stop after typechecking; no codegen. Optional `PROJECT` as above. |

Without a `Dovetail.toml`, `dovetail build` / `dovetail check` can fall back to "single file" mode (current behaviour) for backward compatibility, or require `Dovetail.toml` and fail with a clear error. To be decided in implementation.

### 6.2 Workspace root

- **Workspace root** is the directory containing `Dovetail.toml`. CLI should accept a flag or env to override (e.g. `--manifest-path` or `DOVETAIL_WORKSPACE_ROOT`). Default: current directory.

### 6.3 Scaffolding: `init` and `projects add`

- **`dovetail init <PROJECT>`** — Bootstrap a new workspace from scratch. Run from the directory that will become the workspace root. Creates:
  - **`Dovetail.toml`** with a single `[[project]]`: `name = "<PROJECT>"`, `root_package` = same as name (or a sanitized form, e.g. `com.example.<project>`), `packages = ["."]` for the root package.
  - **`<PROJECT>/`** directory and **`<PROJECT>/src/`** (empty). Optionally add a placeholder source file under `src/` if desired (e.g. `main.dove` with package decl and `function main(): Unit = ()`).
  - Fails if `Dovetail.toml` already exists or if `<PROJECT>/` already exists, to avoid overwriting.

- **`dovetail projects add <PROJECT>`** — Add another project to an existing workspace. Run from the workspace root (where `Dovetail.toml` lives). Creates:
  - **Append to `Dovetail.toml`** a new `[[project]]` with `name = "<PROJECT>"`, `root_package` = same as name (or sanitized), `depends = []`, `packages = ["."]` for the root package.
  - **`<PROJECT>/`** and **`<PROJECT>/src/`** directory structure.
  - Fails if `Dovetail.toml` is not found (not a workspace root) or if a project with that name already exists in the manifest or on disk.

---

## 7. Implementation phases

### Phase 1: Manifest and project layout (no multi-file yet)

- **1.1** Add `toml` crate dependency. Implement a `Dovetail.toml` parser. Schema: list of projects with `name`, `root_package`, `depends`, `packages`. No need to support "compile" yet; focus on parsing and validating.
- **1.2** Resolve project DAG from `depends`; topological sort; report cycle errors.
- **1.3** For each project, validate that the project directory (`<workspace_root>/<name>/`) and its `src/` subdirectory exist. Resolve `packages` entries to canonical `PackagePath` values (using `root_package` and folder names). Validate that each package path has a corresponding directory under `src/`.

**Deliverable:** Library and/or CLI that can load `Dovetail.toml`, resolve projects and packages, and validate layout (both project dirs and package dirs). No change yet to single-file `build`/`check`.

### Phase 2: Multi-file package and "check package" (single project)

This phase adds multi-file and multi-package support within a **single project** (no cross-project dependencies yet).

- **2.1** **File paths in diagnostics:** Replace `FileId(u32)` with a **file path** (relative to workspace root, e.g. `myapp/src/utils/io.dove`) in `Span`. Update `Span`, `Diagnostics`, and diagnostic rendering to display the file path so that multi-file errors point to the correct source file.
- **2.2** **Multi-file input for typechecker:** Introduce `PackageAst { package_path: PackagePath, files: Vec<SourceFile> }`. The typechecker takes this combined AST. Collect, infer, and rules all accept `&PackageAst`. Every file's `package` declaration must match `package_path`; reject files whose declaration doesn't match (diagnostic + skip).
- **2.3** **Package declaration verification:** When gathering files for a package, compute the expected `PackagePath` from the file's path under `src/` (see Section 8.1). Compare against the `package` declaration in the parsed `SourceFile`. Report a clear error if they differ.
- **2.4** **Discovery:** Given a package path and project root, list all `*.dove` files under the corresponding `src/` directory. Parse each (lex → layout → parse); verify package declaration (2.3); combine into one `PackageAst` for typecheck.
- **2.5** **`check_package(package_ast, accumulated_registry)`** in the library: returns `TypeCheckerResult` (typed module, registry, diagnostics). Tests and the orchestrator can call this in a loop, merging registries between packages.
- **2.6** **Merge typed modules:** Add a **merge function on TypedModule** (e.g. `merge(&self, other: &TypedModule) -> TypedModule`). After each successful check, merge the package's TypedModule into the accumulated TypedModule for that project. Merge keeps **all** functions (public + internal). No new types; codegen continues to take `&TypedModule`.
- **2.7** **Single-project orchestrator (for testing):** Implement a test-level orchestrator that processes one project: iterate `packages` in order, for each: discover files → parse → check_package → on error stop; on success merge registry and TypedModule. After all packages pass, run codegen on the merged TypedModule. This validates the full single-project pipeline end-to-end.

**Deliverable:** Ability to check and build a single project with multiple packages and multiple files per package. Package declarations verified against paths. Diagnostics include file paths. Cross-package calls within the same project work (same-package declarations + existing resolution).

### Phase 3: Imports

- **3.1** **Imports (parser and AST):** Add import declarations to the grammar and parser (`import IDENT { "." IDENT } [ "as" IDENT ]`). AST: each `SourceFile` gains a list of `ImportDecl` nodes (opaque dotted path + optional alias). No semantic handling yet.
- **3.2** **Registry conflict validation:** When registering a package or a symbol (type/function) in the registry, check that the new entry does not conflict with an existing entry of the other kind. Specifically: a new package path must not match an existing symbol's FQN path, and a new symbol's FQN must not match an existing package path. Emit an error on conflict (see Section 8.8).
- **3.3** **Imports (collect and validation):** In Collect, build a per-file **import scope**: for each import, resolve against the dependency registry. The import path alone does not distinguish package from symbol — check the registry for both (see Section 4.5; prefer symbol on ambiguity, though Section 8.8 prevents this). Emit diagnostics for unknown symbol, unknown package, or non-public symbol. Check for shadowing (warn + suggest `as`). Export this scope for inference.
- **3.4** **Imports (inference resolution):** In Inference, resolve bare identifiers and qualified names only from (1) current package declarations, (2) this file's import scope (symbol imports and package aliases for `pkg.name`), (3) FQN lookups when the user writes a full path. Refactor existing `lookup_function_by_symbol` / `lookup_type_by_name` so that resolution is import- and FQN-driven only. **Note:** keep a registry-search helper for **error messages** — when reporting "unknown name `foo`", search the registry for a matching public symbol and suggest an import (e.g. "add `import com.example.utils.foo`?").

**Deliverable:** Imports parsed, validated, and used for name resolution. No bare-name registry search (only for error suggestions). Cross-package symbols require explicit imports or FQN.

### Phase 4: Multi-project orchestrator and CLI

- **4.1** **Multi-project orchestrator:** Given workspace root (and optional project filter): load manifest, compute project order (topological sort of DAG), for each project in order: process packages per Phase 2; on error stop. Each project's TypedModule is the merge of its own packages **plus** all dependency projects' TypedModules (pulled in transitively for codegen). Each project's accumulated registry starts with the merged registries of its dependency projects.
- **4.2** **Codegen:** No change to codegen signature: it still accepts `&TypedModule`. Per project, that TypedModule is self-contained (all dependency code included). Produces **one WASM module per project**. Entry point: `main` from the **root package** of that project. No linking — each project's WASM is standalone.
- **4.3** **CLI:** `dovetail check [PROJECT]`, `dovetail build [PROJECT]` using the orchestrator. Optional project name: if present, restrict to that project (and its transitive dependencies).

**Deliverable:** End-to-end `dovetail build` and `dovetail check` with multi-project, multi-package, multi-file; optional project filter; one standalone WASM per project.

### Phase 5: CLI scaffolding

- **5.1** **`dovetail init <PROJECT>`:** Create a new workspace: write `Dovetail.toml` with one project, create `<PROJECT>/src/`, optionally add a placeholder `main.dove`. Fail if `Dovetail.toml` or project dir already exists.
- **5.2** **`dovetail projects add <PROJECT>`:** Add a project to an existing workspace: append to `Dovetail.toml`, create `<PROJECT>/src/`. Fail if `Dovetail.toml` is missing or project already exists.

**Deliverable:** Scaffolding commands for workspace and project creation.

### Phase 6: Caching and incremental (optional / later)

- Per-package cache: store typed module + registry per package; invalidate when source or dependency outputs change. Orchestrator skips check for packages with valid cache. (See compiler.md caching section.)

---

## 8. Detailed implementation notes

### 8.1 Package path from file path

- Project root = e.g. `./myapp/`, `root_package` = `com.example.myapp`.
- File path: `myapp/src/utils/io.dove`.
- Relative to `src/`: `utils/io.dove` → path segments `["utils"]` → package path `com.example.myapp.utils`.
- So: expected package = `root_package` + segments from `path_relative_to_src` (dropping the file name). Implement as a small function `file_path_to_package_path(project_root, root_package, file_path) -> PackagePath`.

### 8.2 Package declaration verification

- When parsing a file, the parser yields `SourceFile { package: PackageDecl { path }, declarations }`. Compare `path` (as `PackagePath`) to the expected package from the file path. If they differ, add a diagnostic and still feed the file into the pipeline (or skip typecheck for this file and treat as package error). Prefer one clear error per file.

### 8.3 Combining multiple files for typecheck

- Use a **combined AST**: package name (`PackagePath`) + list of `SourceFile`. Typechecker signature: `typecheck(package_ast: &PackageAst, registry: &Registry) -> TypeCheckerResult` where `PackageAst { package_path: PackagePath, files: Vec<SourceFile> }`. Every file's `package` decl must match `package_path`. Collect phase: iterate all files and collect into one registry. Infer phase: iterate all files and infer; all share the same package_path and merged registry. Rules: run over all inferred modules. Output: one `TypedModule` per package. File identity is tracked via the **file path** (relative to workspace root) stored in each `Span`, so diagnostics point to the correct source file.

### 8.4 TypedModule merge and codegen

- **No new types.** Add a **merge function on TypedModule** (e.g. `merge(&self, other: &TypedModule) -> TypedModule`) so that the orchestrator can combine each package's TypedModule into one per project. The merge must keep **all** functions (public + internal) — only `Registry` strips internals; `TypedModule` never does, since codegen needs all function bodies. Codegen is **unchanged**: `generate_wasm(typed_module: &TypedModule)` — it still takes a single `TypedModule`, which per project is the result of merging all that project's packages (plus all dependency projects' packages). One WASM per project; entry point `main` from the root package. No linking — each project's WASM is self-contained.

### 8.5 Prelude

- The existing design (compiler.md) says the prelude is the first “package” in every compilation. In the new model, prelude can be treated as a synthetic project (or a synthetic package) that is always first in the dependency order: its registry is merged first, then project dependencies, then each project’s packages. So “accumulated registry” starts with prelude (if present), then projects in topological order, then packages in `packages` order within each project.

### 8.6 Imports (implementation)

- **Per-file import scope:** Each file has its own list of imports. The typechecker builds an import scope per file (or merges into one per-package scope with file attribution for diagnostics). For inference, when resolving a name in a given file, use that file's import scope plus the package's declarations.
- **Collect:** When processing imports, iterate over each file's imports. For each import path, the syntax alone does not distinguish package vs symbol: **check the registry for both** (is this path a package? is it package + symbol with that symbol public?). Resolve accordingly; if missing or not public, add diagnostic. Build a map: file path → (symbol name or package alias → FQN / package path). This map is the import scope passed to inference.
- **Inference:** For a call or type reference: if the name is a single identifier, look up in (1) current package (collect's package registry), (2) this file's import scope (symbol imports and package aliases). For a qualified name `a.b.c`, resolve `a` (and possibly `a.b`) from import scope as a package, then look up the rest in the registry; or if the whole thing is a known FQN (e.g. `com.example.utils.func`), resolve FQN from registry. Remove the current `lookup_function_by_symbol` and `lookup_type_by_name` resolution that searches the registry for a bare symbol name across all packages. **Note:** keep these helpers (or equivalent) available for **error messages only** — when reporting "unknown name `foo`", the compiler can search the registry for a matching public symbol and suggest an import (e.g. "unknown name `foo`; add `import com.example.utils.foo`?").
- **FQN in code:** When the AST has a multi-segment name that matches a package path + symbol (e.g. `com.example.app.utils.func`), treat as FQN: look up in registry by (PackagePath, SymbolName). If found and public, resolve to that; otherwise error.

### 8.8 Registry conflict validation (package vs symbol)

The registry must enforce that **no conflict** exists between package paths and symbol FQN paths. Specifically:

- When **registering a new package** (e.g. `com.example.app.utils`), check that no symbol exists whose FQN matches that package path (e.g. there is no function or type named `com.example.app.utils` — which would mean a symbol named `utils` in package `com.example.app`). If a conflict is found, emit an error.
- When **registering a new symbol** (type or function) with FQN `pkg.name`, check that no package in the registry has a path equal to `pkg.name` (e.g. registering symbol `utils` in package `com.example.app` should fail if `com.example.app.utils` is already a registered package). If a conflict is found, emit an error.

This invariant means that import resolution (Section 4.5) never encounters a true ambiguity between package and symbol paths. The "prefer symbol" fallback in Section 4.5 is a safety net but should never be triggered in practice.

### 8.9 File paths in spans (replacing FileId)

The current `Span` type uses `FileId(u32)` to identify the source file. For multi-file support, replace `FileId` with a **file path** stored as a string (relative to the workspace root, e.g. `myapp/src/utils/io.dove`). This allows diagnostics to directly display the file path without needing a separate `FileId → path` mapping table. Update `Span`, all diagnostic rendering, and all places that construct spans (lexer, parser) to use the relative file path.

### 8.10 AST for qualified access (dot)

The same syntactic form — an expression or name followed by a dot and a segment (e.g. `a.b`, `a.b.c`) — is used for several different language features:

- **Package-qualified names:** `utils.func`, `com.example.utils.foo` (import/registry resolution).
- **Field access:** `point.x`, `record.field` (fields of records).
- **Method / member access:** `obj.method(...)`, `obj.property` (functions and fields of classes).
- **Enum variants:** `MyEnum.Variant` (constructors or pattern matching).

The **AST should represent dot in a generic way**: a single “qualified access” or “dot” node (e.g. `receiver` + `segment`, or a chain of segments) without committing at parse time to package vs field vs method vs variant. The **typechecker** (or a dedicated resolution pass) then decides the meaning using the resolution order (imports then registry for package-like chains) and the **type of the receiver** when it is an expression: if the receiver has a record/class type, resolve the segment as a field or method; if it is an enum type, as a variant; if the receiver is a package alias or the chain looks like a package path, resolve via imports and registry. So one AST shape (e.g. `QualifiedAccess { base, segments }` or left-associative `Dot(expr, ident)`) suffices; the semantics are determined during typechecking. This keeps the grammar and AST simple and avoids duplicating the dot syntax for each use case.

---

## 9. Acceptance tests

- **Manifest parsing:** Given a valid `Dovetail.toml` with two projects where one depends on the other, parsing and DAG resolution yield the correct order. Given a cycle in `depends`, the compiler reports an error.
- **Package-path resolution:** For a project with `root_package = "com.example.app"` and `packages = ["utils", "main"]`, the compiler resolves to `com.example.app.utils` and `com.example.app` (or `com.example.app.main` if `main` is a folder under `src/main/`). Root package is `com.example.app` for `src/`; `utils` is `src/utils/`.
- **Package declaration vs path:** A file at `src/utils/io.dove` that declares `package com.example.app.utils` passes; `package com.example.app.other` fails with a clear error. A file at `src/utils/io.dove` with `package com.example.app` fails.
- **Multi-file package:** Two files in the same package, both declaring the same package path; one defines `function main(): Unit = ()`, the other defines `function helper(): Int32 = 42`. Check package succeeds; merged TypedModule (from merge function) contains both; build produces WASM that runs and main is the entry.
- **Cross-package reference (import):** Package `utils` defines `public function id(x: Int32): Int32 = x`. Package `main` has `import com.example.myapp.utils.id` and `function main(): Unit = assert id(1) == 1`. Check order: utils then main; main’s import is validated against registry; build succeeds.
- **Cross-package (package import):** Same setup; in `main`, `import com.example.myapp.utils` then `function main(): Unit = assert utils.id(1) == 1`. Resolve `utils` from import scope, then `utils.id` from registry; succeeds.
- **Cross-package (FQN):** In `main`, no import; `function main(): Unit = assert com.example.myapp.utils.id(1) == 1`. FQN is resolved from registry; succeeds.
- **Import validation:** A file has `import com.example.utils.nonexistent`. If `nonexistent` is not in the registry for that package, the compiler reports “symbol `nonexistent` not found in package …” (or similar). Same for importing a non-public symbol: error.
- **No registry search for bare name:** A file uses `helper(1)` with no import and no `helper` in the current package. Compiler reports “unknown name `helper`” (or “cannot find `helper` in scope”), and does **not** search the registry for a matching symbol.
- **Import with `as`:** `import com.example.utils.helper as h`; code uses `h(1)`. Resolution uses local name `h` from import scope; succeeds.
- **Error stops orchestration:** Package `a` has a type error; package `b` is next. Orchestrator runs check for `a`, gets errors, and does not run check for `b`. No codegen. Diagnostics show only (or primarily) errors from `a`.
- **CLI project filter:** With two projects `lib` and `app` (app depends on lib), `dovetail check app` runs check for `lib` then `app`. `dovetail check lib` runs check only for `lib`. `dovetail build` (no project name) builds both projects and produces **one WASM per project**, each with its own `main` from that project's root package.

---

## 10. Edge cases and considerations

- **Empty package:** A package listed in `packages` with no source files (or only empty files). Decide: error “package has no source files” or allow (no declarations). Prefer error for clarity.
- **Duplicate package path across projects:** Two projects must not expose the same package FQN. If they do (e.g. both have `root_package = "app"`), resolution of `app.foo` is ambiguous. Rule: project names and root packages should be globally unique in the workspace, or the compiler errors when merging registries on duplicate package path.
- **File in wrong directory:** A file under `src/utils/` that declares `package com.example.myapp.core`. Verification step fails; report “package declaration `com.example.myapp.core` does not match path (expected `com.example.myapp.utils`).”
- **Missing package in `packages`:** If `src/extra/` exists with source files but `packages` does not list `extra` (or `com.example.myapp.extra`), either error “package not listed in manifest” or implicitly add. Prefer error so order is explicit.
- **Extra package in `packages`:** `packages` lists `nonexistent` but `src/nonexistent/` does not exist. Error: “package `nonexistent` has no source directory.”
- **Prelude and first package:** The first “package” in the merged registry is the prelude. When there is no prelude yet, the first real package gets an empty (or minimal) registry. Ensure prelude is clearly defined (e.g. path and content) so that “check package” for the first user package has the right dependency registry.
- **Which package contains `main`:** Each project's WASM has exactly one entry point: `main` from the **root package** of that project. If the root package has no `main` (or multiple), codegen or rules phase should error for that project.
- **Build output:** **One self-contained WASM per project.** No linking; everything is code-based. Each project produces one standalone WASM module that includes all code from its own packages and all dependency projects' packages. Entry point: `main` from that project's root package. When the user runs `dovetail build`, we build every project and emit one WASM per project.
- **Import edge cases:**
  - **Duplicate import:** Same symbol imported twice (with or without different `as`). Prefer error or “already in scope” to avoid ambiguity.
  - **Name clash:** Symbol import `foo` and package import with local name `foo`. Error: name already in scope.
  - **Package import then bare name:** After `import com.example.utils as u`, use of bare `utils` (without `u`) should be “unknown name” (we don’t search registry for a package named `utils`).
  - **FQN of internal symbol:** Code uses full FQN for a symbol that is internal (not public) in that package. Validation fails: symbol not visible.
  - **Import from current package:** `import com.example.myapp.utils` when current file is in package `com.example.myapp` — allowed (same project). Import from a package that hasn’t been compiled yet (wrong order) is a dependency-order issue; with explicit `packages` order, any imported package must be earlier in the order, so it’s already in the registry.

---

## 11. Summary

| Concept | Design choice |
|--------|----------------|
| Manifest | One `Dovetail.toml` at workspace root; defines all projects. |
| Project | Folder with `src/`; has `root_package`, `depends`, `packages` (order). |
| Package | Path under `src/`; FQN = `root_package` + path segments. |
| Package declaration | Must match path; compiler verifies. |
| **Imports** | Source-level only; no wildcards; `as` supported. Symbol import, package import, FQN in code. Resolve only from imports + same package + FQN; never search registry for bare names. Validate every import against registry (public, exists). |
| Check package | Input: package AST (multi-file) + accumulated registry. Output: typed module, registry, diagnostics. |
| Orchestrator | Projects in DAG order; packages in `packages` order; check → merge or stop on error; codegen only after all pass. |
| CLI | `init <PROJECT>`, `projects add <PROJECT>`, `build [PROJECT]`, `check [PROJECT]`. |
| Build output | One **self-contained** WASM per project; entry point `main` from each project's root package. No linking — each project's WASM includes all dependency code. Per project: accumulated registry and one TypedModule (by merging own packages + dependency packages); codegen takes that TypedModule, no signature change. |

This design is intended to be both high-level (for understanding the model) and detailed enough to implement in phases, with clear acceptance tests and edge cases called out.
