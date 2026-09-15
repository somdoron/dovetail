# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Naming

The language is Dovetail, formerly called Domain. The user may still say “Domain” or “the Domain language” out of habit; when referring to this language, compiler, or tooling, interpret that as Dovetail. Use Dovetail in new prose and code references, with `.dove` source files, `Dovetail.toml`, and the `dovetail` executable. Ordinary uses of “domain,” including domain-driven design and network domains, retain their meaning.

## Project Overview

Dovetail is a compiler for the Dovetail programming language — a high-level, business-logic focused language that compiles to WebAssembly (WASMGC, WASI CLI world). The compiler is written in Rust.

## Build Commands

```bash
cargo build              # Build the compiler
cargo test --workspace   # Run compiler and generator tests
cargo test <test_name>   # Run a single test by name
cargo clippy             # Lint
cargo fmt                # Format code
cargo run -- check       # Typecheck the workspace (Dovetail.toml)
cargo run -- build       # Build the workspace to WASM
cargo run -- test        # Run Dovetail test declarations
cargo run -- fmt --check # Check canonical Dovetail source formatting
```

**Important:** Always use `cargo run --` instead of the `dovetail` CLI. The installed `dovetail` binary may be outdated; `cargo run` ensures you test against the locally-built compiler.

The workspace root is at `/Cargo.toml` with the main compiler package in `dovetail/`.

When testing Dovetail language features (especially the wasi/IO library or multi-project workspace), always use `cargo run -- check` or `cargo run -- test` against the real workspace rather than writing inline Rust integration tests that duplicate Dovetail source.

## Compiler Bug Policy

When you encounter a compiler bug (crash, incorrect codegen, wrong type error, etc.): **stop, investigate the root cause in the Rust compiler code, and present the findings to the user.** Do NOT work around compiler bugs in Dovetail source code — they must be fixed at the source in the compiler.

Rust edition: **2024**.

## Commit and Staging Policy

**Never `git add` (stage) or `git commit` without an explicit instruction in the current message.** The user reviews work — including which files end up staged — before commits go in. Even if a logical chunk of work is finished, multiple related chunks have completed, or it would be a natural breakpoint:

- Do not run `git add` / `git add -A` / `git add -p` / any staging command
- Do not run `git commit` / `git commit -a` / `git commit -am`

Wait for the user to type something like "stage", "commit", "commit this", or otherwise plainly request staging/committing in the message you're currently responding to. Prior authorizations do not carry forward to new chunks of work — each commit or staging operation needs its own explicit ask. Edits to the working tree are fine without authorization; it's only the index/history that needs explicit user consent.

## Architecture

### Compilation Pipeline (per package, in dependency order)

```
Source Files → Lexer → Layout Filter → Parser → Typechecker → Codegen → WASM Component
```

1. **Lexer** — Hand-written. Produces raw tokens with spans (file, line, column). Emits `Newline` tokens.
2. **Layout Filter** — Inserts virtual `Begin`/`End`/`Sep` tokens based on indentation (offside rule, similar to Haskell). Strips `Newline`. Layout openers: `=`, `then`, `else`, `with`, `->`, `{`.
3. **Parser** — Hand-written with error recovery (skip to next statement/declaration on error). Produces untyped AST. One AST per file; combined AST per package feeds the typechecker.
4. **Typechecker** — Three phases: **Collect** (build registry + symbol table with FQNs) → **Inference** (type inference; **replace FQN with MangledName** — each symbol gets a MangledName with a reference back to FQN and a mangled string for codegen) → **Rules** (enforce subtyping, trait bounds, visibility). Public and internal in registry during inference; internal stripped before returning registry to dependents. Public class members collected. Typed AST carries MangledNames.
5. **Codegen** — Produces WASM component (WASMGC types, WASI CLI world). Only runs if zero errors in prior stages. Builds a type dependency graph to emit definitions in valid order. Linear memory used only at WASI boundary (bump allocator).

### Package System

- **Package = folder.** All source files in a package are type-checked together.
- **No circular dependencies.** Package dependency graph must be a DAG.
- **Compilation order** specified in `Dovetail.toml` manifest. Packages compiled in dependency order; each receives merged registry from prior packages.
- **Prelude** is always the first package compiled (ships with compiler, defines intrinsic types like `Array`, `String`).
- File package declarations must match directory structure under `src/`.
- **Caching** — Package-level cache of typechecker output (typed AST, registry, errors). Orchestrator merges each package’s registry and typed AST after typechecking for use by later packages. Incremental: reuse cache when package sources and dependency outputs unchanged.

### Key Design Principles

- **Newtypes over raw strings/ints** — Use newtypes for identifiers and domain-specific scalars.
- **Accumulate errors** — Compiler collects all errors in a run, does not stop at first error.
- **Feature-by-feature development** — Each feature implemented across all pipeline stages with unit tests per stage and integration tests for the full pipeline.
- **FQN then MangledName** — Every symbol has a fully-qualified name (FQN) for resolution and diagnostics. In the Inference phase, FQN is replaced by MangledName (reference back to FQN + mangled string) for use in codegen.
- **`type_params` not `type_args`** — Always use `type_params` for naming variables/parameters that hold type parameter lists. Never use `type_args`.

## Dovetail Language Style

- **camelCase only** — All Dovetail-language identifiers (functions, variables, parameters, properties) use camelCase. Never use snake_case. Examples: `encodeUrlSafe`, `parseValidated`, `hexChar`, `findInsertPos`.

- **camelCase file names** — `.dove` source files are camelCase too, never snake_case. A file holding one type's module takes that type's casing (`Order.dove`, `StringBuilder.dove`); everything else is camelCase (`types.dove`, `orderTypes.dove`, `placeOrderTest.dove`). Some existing stdlib files still use snake_case (`resource_test.dove`, `net_intrinsics.dove`); new files should not.

- **Full names, not abbreviations** — Prefer `isAtLeast`, `greaterOrEqual`, `currentStream`, `bytesWritten` over `geq`, `gte`, `curStrm`, `bw`. Identifiers should read like English.

- **`make` for constructors** — Static constructor functions on records/classes are named `make` (e.g. `Logger.make`, `LogSpan.make`, `MemorySink.make`). Avoid `new` or `create`. (Some existing modules in `standard-collection` still use `new`; new code should prefer `make`.)

- **Ordinary discards need no binding** — Dovetail blocks automatically discard intermediate non-`Unit` expression values; only the last expression contributes to the block's type. Write `c.tick()` on a line by itself, not `let _ = c.tick()`. For builder-style helpers (e.g. functions that take a `StringBuilder` and call `.append()`), declare the helper to return the builder itself and let its last expression be the returning call — see `standard-json/src/Json.dove` and `standard-io-log/src/Formatter.dove` for the idiom. Exception: discarded `Result`, `Async`, and `Resource` values warn. Handle or use them, or write `let _ = expression` to acknowledge intentional discard. This does not execute an `Async` or acquire a `Resource`.

### Testing

- **Integration tests** live in `dovetail/tests/`, split by topic (e.g. `basics.rs`, `bool_panic_assert.rs`, `main_function.rs`). Shared helpers are in `tests/common/mod.rs`.
- **Integration tests use `assert`** — since `main` must return `Unit`, test expected values using the Dovetail `assert` expression (e.g. `assert x == 5`) rather than trying to return values from `main`.
- **Dovetail source in tests** must use `r#"..."#` raw string literals with the source aligned to the left (column 1), not inline `\n` escape sequences:
  ```rust
  common::compile_and_run(r#"
  package a

  function main(): Unit = assert 1 + 2 == 3
  "#)
  .expect("1 + 2 == 3");
  ```

## Key Specifications

- [compiler.md](compiler.md) — Compiler architecture, pipeline stages, caching, and runtime design
- [grammar.md](grammar.md) — Complete lexical and syntactic grammar (CFG notation)
- [layout_rules.md](layout_rules.md) — Indentation/offside rule implementation guide with data structures
- [docs/type-theory-and-improvements.md](docs/type-theory-and-improvements.md) — Type-system theory, current implementation, gaps, and improvement guide (for engineers)
- [book/](book/) — Language reference (see [book/toc.md](book/toc.md); parts on getting started, basics, control flow, functions, type system, generics, traits, classes, packages, testing)

## CLI and Entry Point

- **`dovetail build`** — Full pipeline through codegen; produces WASM component.
- **`dovetail fmt`** — Format local workspace sources and tests; `--check` checks without writing. See [docs/formatting.md](docs/formatting.md).
- **`dovetail check`** — Same pipeline but stops after typechecker (no codegen). Use for validation (e.g. editors, CI).
- **Entry point** — A `function main(): Unit` (no arguments, returns Unit) is the component entry. `dovetail test` runs test declarations.

## Dovetail Language Key Features

- Expression-based, layout-sensitive (indentation, no braces), strongly typed with inference
- Records, enums (discriminated unions), classes with inheritance, traits
- Generics with type parameters; specialized only (monomorphized, one instantiation per type-argument list)
- No exceptions — uses `Result`/`Option`
- Async/await, closures, pattern matching
- Intrinsic types (`Array`, `String`) declared in prelude, lowered to WASM built-ins

## Planned Dependencies

| Crate | Purpose |
|-------|---------|
| wasmtime | Runtime for Dovetail applications |
| wasm-encoder | Building WASM binaries |
| wit-component | WASM component model and interfaces |
| wit-parser | Parsing WIT for WASI boundaries |
| wasmparser | WASM validation and inspection (tests) |
