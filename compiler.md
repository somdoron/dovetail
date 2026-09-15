# Compiler Design

This document describes the design of the Dovetail compiler: pipeline, data structures, and key decisions. The language grammar is defined in [grammar.md](grammar.md); layout rules in [layout_rules.md](layout_rules.md).

---

## Design Principles

- **Packages.** A package is a folder; see [Packages](#packages) for dependency rules and compilation unit.
- **Fully-qualified names (FQN).** Every symbol has an FQN: package path + symbol name. Used for name resolution and diagnostics. In the **Inference** phase of the typechecker, FQN is replaced by **MangledName**: a MangledName holds a reference back to the FQN and a **mangled string** (used by codegen for WASM symbols, etc.).
- **Newtypes over raw strings and ints.** Use newtypes for identifiers, names, and domain-specific scalars so the type system catches misuse and the compiler avoids ad-hoc string/int handling.
- **Test-driven, iterative development.** For each feature we add support across all relevant compiler stages, add unit tests per stage, and add integration tests that run the full compiler. No feature is “done” without tests at every layer.

---

## Packages

- **Package = folder.** Each package is the unit of type-checking and codegen; all source files in the package are type-checked together and share one type environment.
- **No circular dependencies.** The dependency graph between packages must be acyclic. Circular references between packages are not allowed and are reported as errors (e.g. when building the package graph or resolving imports).
- **Compilation unit.** When we compile a package:
  - **Input:** The parsed AST from all source files in that package, plus the **output of the packages the package depends on** (e.g. typed interfaces, exported symbols, or compiled component fragments—whatever the next stage needs).
  - **Output:** Whatever the dependent packages need (e.g. typed AST or WASM for that package), so that packages that depend on this one can be compiled in turn.
- Packages are compiled in dependency order: a package is compiled only after all of its dependencies have been compiled.
- **v1: explicit order in `Dovetail.toml`.** For the first version of Dovetail, the user specifies the order of packages in a project manifest `Dovetail.toml`. The compiler uses that order as the compilation order (and still checks that there are no circular dependencies). Inferring order from imports or a dependency graph may be considered in a later version.
- **Prelude.** A prelude library, written in Dovetail, is the **first package** in every compilation. It is shipped with the Dovetail compiler and is always compiled before any user package, so user code can depend on it (e.g. for standard types and functions). Built-in types such as `Array` and `String` are **resolved** by defining them in the prelude with a special keyword `intrinsic`; see [Intrinsics](#intrinsics). User packages listed in `Dovetail.toml` follow after the prelude in the compilation order.

### Project layout and `Dovetail.toml` (see book Part 11)

The following is defined in the language book, [Part 11: Packages and Modules](book/11-packages.md); the compiler implements it.

- **Project manifest:** `Dovetail.toml` lives at the project (or workspace) root. Each project is a `[[project]]` entry with `name`, `type` (`library` or `application`), `root-package` (e.g. `com.example.api`), and `depends` (list of project names). The dependency list defines which packages are visible and implies compilation order (DAG). For v1, the compiler may accept an explicit package order instead of or in addition to `depends`.
- **Package ↔ directory:** The declared package in each file must **match the file’s location**. Under a project’s `src/`, the path mirrors the package: e.g. `root-package = "com.example.myapp"` and `src/utils/helpers.dove` → `package com.example.myapp.utils`. So package names map to folders by convention (dot-segments → path segments under `src/`).
- **Source discovery:** All source files under the project’s **`src/`** directory belong to packages under that project’s root package. The compiler discovers sources by walking that tree; each file’s package declaration must match its path.
- **Imports → packages:** Imports use the **qualified package path** (e.g. `import com.example.core.types.Id`). The compiler resolves them using the packages exposed by the current project and its **dependencies** from `Dovetail.toml`. So “registry from previous packages” is the merged public registry of all packages in the dependency set (those projects’ packages, built in dependency order).

---

## Compilation Pipeline

The pipeline below is **per package**. The compiler compiles packages in dependency order; each package’s input includes the outputs of its dependencies (see [Packages](#packages)).

```
┌──────────┐     ┌─────────────────┐     ┌────────┐     ┌─────────────┐     ┌──────────┐
│  Source  │ ──> │ Lexer           │ ──> │ Layout │ ──> │ Parser      │ ──> │   AST    │
│  Files   │     │ (by hand)       │     │ Filter │     │ (by hand)   │     │ (all     │
│ (in pkg) │     └─────────────────┘     └────────┘     └─────────────┘     │  files)  │
└──────────┘                                                               └────┬─────┘
                                                                                  │
       ┌─────────────────────────────────────────────────────────────────────────┘
       │  + outputs of dependency packages (e.g. exported types, interfaces)
       v
┌─────────────┐     ┌────────┐
│ Typechecker │ ──> │ Typed  │     ┌─────────────────┐
│ Collect →   │     │  AST   │ ──> │ Codegen         │ ──> WASM component
│ Infer →     │     │        │     │ (WASMGC, WASI)  │     (or output for
│ Rules       │     └────────┘     └────────┬────────┘     dependents)
└─────────────┘                            │
                                           └── Build type dependency graph for codegen
```

1. **Lexer** → Raw tokens (including `Newline`). Applied to each source file in the package.
2. **Layout filter** → Inserts virtual `Begin`, `End`, `Sep`; strips `Newline`. Parser sees only layout-aware tokens.
3. **Parser** → AST (untyped). One AST per file; the **input to the typechecker** is the combined parsed AST from all source files in the package.
4. **Typechecker** → Takes that combined AST plus the **outputs of the packages this package depends on** (e.g. exported type and function signatures). Produces typed AST (symbols resolved, types attached).
5. **Codegen** → Takes typed AST (and dependency outputs as needed). Produces WASM component (WASI CLI world, WASMGC) or whatever artifact dependent packages consume.

---

## Dependencies

| Crate | Role |
|-------|------|
| **wasmtime** | Runtime for Dovetail applications. |
| **wasm-encoder** | Building WASM binaries. |
| **wit-component** | WASM component model (interfaces, linking). |
| **wit-parser** | Parsing WIT for WASI and component boundaries. |
| **wasmparser** | Validation and inspection of WASM (e.g. in tests). |

---

## Pipeline Stages

### 1. Lexer (by hand)

- Hand-written lexer producing tokens as defined in the [lexical grammar](grammar.md#lexical-grammar).
- **Spans:** Each token carries a span (file, line, column; optionally end line/column) so that errors and diagnostics can report exact source locations (see [Error handling](#error-handling)).
- **Prefixed string literals:** an identifier immediately followed by a quote (`sql"..."`) is scanned as a single `PrefixedStringLiteral` token carrying a structured payload — literal text and interpolations, with the interpolations already tokenized. Unlike ordinary string interpolation (which the lexer expands into a `+`/`.format()` token soup), nothing is assembled here: what `$x` *means* is the registered builder's business. The sub-lexer for `${expr}` is seeded with the expression's real line and column, so interpolation spans are absolute — which is what lets a trait-bound failure point at the interpolation rather than at the literal.
- **Output:** Stream of raw tokens, including `Newline`. No layout interpretation here.

### 2. Layout filter

- Consumes raw tokens from the lexer and inserts virtual **Begin**, **End**, and **Sep** tokens according to the [layout rules](layout_rules.md).
- Tracks offside context and layout openers (`=`, `then`, `else`, `with`, `->`, `{`).
- **Output:** Token stream with layout tokens; `Newline` is not passed to the parser.

### 3. Parser (by hand)

- Hand-written parser for the [syntactic grammar](grammar.md#syntactic-grammar), consuming the layout-aware token stream.
- One parse unit = one file. The driver parses every source file in the package; the **combined AST from all files in the package** is the input to the typechecker (see [Packages](#packages)).
- **Error recovery:** On a syntax error, the parser tries to recover (e.g. skip to next statement or declaration) and continue parsing, so multiple errors can be reported in one run (see [Error handling](#error-handling)).
- **Prefixed string literals:** the payload's interpolations are sub-parsed into expressions, producing an `Expr::PrefixedLiteral` of text/value/spread parts.
- **Output:** **AST** — tree of declarations and expressions, no type information, no symbol resolution.

### 4. Typechecker

- **Input:** Parsed AST from all source files in the package, plus the **outputs of the packages this package depends on** (e.g. exported types and function signatures). No circular package dependencies (see [Packages](#packages)).
- Three phases, in order:

| Phase | Purpose | Output |
|-------|---------|--------|
| **Collect** | Build registry of the package: collect stub types and function signatures (including from imports). Build symbol table and FQNs. | Registry + symbol table |
| **Inference** | Infer types for expressions and declarations that don’t have explicit types. **Replace FQN with MangledName** for each symbol: MangledName stores a reference back to the FQN and a mangled string (for codegen). Also **lowers prefixed string literals**: the prefix is an ordinary name, resolved through the file's import scope and required to have been declared `@stringLiteral`, and the literal is rewritten into an ordinary `Builder.empty().literal(..).value(..).spread(..).build()` chain rooted at an `Expr::ResolvedTypeRef` (a receiver that names the resolved type by FQN, so an import alias resolves to the same builder). Everything after that — overload resolution, trait bounds, monomorphization, codegen — treats it as hand-written code, so no later stage knows the feature exists. | Types + MangledNames attached to AST nodes |
| **Rules** | Enforce type rules (subtyping, trait bounds, visibility, etc.). | Constraints checked |

**Visibility and the registry:** During Collect and Inference, the registry includes both **public** and **internal** symbols (so the typechecker can resolve and type-check everything visible within the package). We also collect **public class variables and methods** into the registry. **Before returning** the typechecker result (and before merging into the combined registry for dependents), we **remove internal** from the registry. The registry that is returned, cached, and passed to later packages therefore contains only **public** symbols (types, functions, and public class members). **Private** symbols never appear in the registry for other packages; they are used only within the package.

**Output:** **Typed AST** — same tree shape as AST, with types and resolved symbols attached. Symbols use **MangledName** (with a reference back to FQN and the mangled string); this is the input to codegen.

### 5. Codegen

- **Type dependency graph:** Build a dependency graph over types (and possibly functions) so that codegen can emit WASM definitions in a valid order (e.g. structs before functions that use them).
- **WASMGC:** Emit GC types and instructions for records, tuples, enums, classes, arrays, and strings. Prefer non-null WASMGC types where possible.
- **WASI integration:** Use linear memory only at the boundary (e.g. bump allocator); copy between Dovetail’s GC heap and linear memory when calling or returning from WASI.
- **Output:** WASM component targeting the WASI CLI world. Codegen runs only when there are **no errors** in the compilation so far (see [Error handling](#error-handling)).

---

## Error handling

- **Spans.** Every error is associated with a **span**: the source location (file, line, column, and optionally end position) where the error occurred. The lexer attaches spans to tokens; the parser and later stages preserve or derive spans for AST nodes and type information so that diagnostics can report the exact line and column for each error.
- **Accumulate errors.** The compiler does not stop at the first error. It collects **all** (or as many as practical) errors during a run so that the user can fix multiple issues in one pass.
- **Parser: recover from errors.** When the parser hits a syntax error, it **attempts to recover** (e.g. by skipping to a known good point such as the next statement or declaration) and continues parsing. This allows more errors to be discovered in the same file and avoids cascading false positives from a single mistake.
- **No codegen on errors.** If any errors were reported in earlier stages (lexer, layout, parser, typechecker), the compiler **does not proceed to codegen**. The pipeline stops after the phase that reported errors; no WASM is emitted. This keeps the compiler from generating code from invalid or partially resolved programs.

---

## Dovetail CLI

The Dovetail compiler is invoked via a CLI. For now it provides two commands:

| Command | Description |
|---------|-------------|
| **`build`** | Full compilation: lexer → layout filter → parser → typechecker → codegen. Produces the WASM component (and any artifacts for dependent packages). |
| **`check`** | Same as `build` but **stops after the typechecker**; codegen is **not** run. Use this to validate syntax and types without emitting WASM (e.g. in editors or CI). |

- Both commands use the same pipeline up to and including the typechecker. Errors are accumulated and reported the same way; neither command runs codegen if there are errors.
- A **`dovetail test`** command is planned for the future: it will compile and run `test` declarations (from the grammar). Additional CLI commands (e.g. `run`) may be added later.

### Entry point

The entry point for the final WASM component is a **main** function. **For now:** that function has signature `function main(): Unit` — it returns `Unit` and takes no arguments. The compiler uses this to export the component’s entry and to have the runtime invoke it. (Which package contains `main` may be specified by the project manifest or by convention; additional signatures for `main` may be supported later.)

---

## Caching and incremental compilation

Caching is done **at the package level**. The goal is to avoid re-running the typechecker (and earlier stages) for a package when its inputs have not changed.

### What is cached

For each package, we cache the **output of the typechecker** for that package:

- **Typed AST** — the result of inference and type-checking for that package’s AST.
- **Registry** — the symbol/type registry produced for **that package only** (public types, function signatures, and public class members; internal is stripped before the result is returned; see [Typechecker](#4-typechecker)).
- **Errors** — any errors reported while typechecking this package.

The **input** to the typechecker for a package is:

- Parsed AST from all source files in the package (from lexer → layout → parser).
- **Registry from previous packages** — the merged registry of all packages that this package depends on (prelude and any earlier packages in the compilation order).

### Orchestrator: merging after each package

After **every** package is typechecked, the **orchestrator** merges that package’s output with the accumulated state from previous packages:

- **Registry:** Merge the new package’s registry into the combined registry so far. The next package in the order will receive this merged registry as its “registry from previous packages.”
- **Typed AST:** Merge (or collect) the new package’s typed AST with the typed AST from previous packages, so that the full program’s typed AST is available for codegen or for later packages that need it.

So the flow is: typecheck package 1 → merge its registry and typed AST → typecheck package 2 with merged registry from package 1 → merge package 2’s registry and typed AST → … and so on. Each package’s typechecker only sees and produces its own scope; the orchestrator maintains the cumulative view.

### Incremental compilation

When recompiling (e.g. after a source change), the compiler can **reuse the cached typechecker output** for a package if:

- The package’s source files (and thus parsed AST) have not changed, and
- The outputs of all packages it depends on (merged registry, and any other inputs the typechecker uses) have not changed.

If a package or any of its dependencies has changed, that package and every package that depends on it must be typechecked again (and their caches invalidated or updated). Packages that do not depend on the changed package can keep using their cached output.

(Codegen can have its own caching or invalidation rules on top of this; the typechecker cache is the main enabler for incremental compilation.)

---

## Language and Runtime Design

### Intrinsics

Built-in types (e.g. `Array`, `String`) are **declared in the prelude** so that name resolution and type-checking work like any other type, but they are marked with a special keyword so the compiler implements them via WASM built-ins rather than as normal Dovetail types.

- **Declaration:** In the prelude, such types are defined with the right-hand side `intrinsics` (the special keyword). Example:
  ```dovetail
  type Array<T> = intrinsics
  ```
  The grammar treats this as a type alias (or similar) whose body is the keyword; the compiler recognizes it and does not generate Dovetail-level code for the type—instead it lowers uses to WASM intrinsics.
- **Resolution:** User code and the typechecker resolve `Array`, `String`, etc. via the prelude like any other type; the “intrinsic” marker is used in codegen (and possibly in collect) to dispatch to the built-in implementation.

### Primitives

- **Integers:** Signed (`Int8`, `Int16`, `Int32`, `Int64`, …) and unsigned (`Uint8`, `Uint16`, `Uint32`, `Uint64`, …) as defined in the language. Represented as primitive values in the IR/codegen; use specialized generics when they appear as type parameters.
- **Boolean:** `Bool` as defined in the language. Represented as a primitive value in the IR/codegen (e.g. WASM `i32` 0/1); use specialized generics when it appears as a type parameter. When used in an array (`Array<Bool>`), use a **packed** representation.
- **Floats:** `Float32` and `Float64` as defined in the language. Represented as primitive values in the IR/codegen (WASM `f32` / `f64`); use specialized generics when they appear as type parameters.
- **String and Char:** `String` (UTF-8 encoded) and `Char` (Unicode scalar). Represented in WASMGC as appropriate (e.g. packed arrays or GC types; see [WASI, WASM, and WASMGC](#wasi-wasm-and-wasmgc)).

### Arrays

Arrays are **special types**: declared in the prelude as an [intrinsic](#intrinsics) (e.g. `type Array<T> = intrinsic`), so they are resolved like any other type but implemented by the compiler via WASM built-ins.

- **Specialization:** Primitive-element arrays (`Array<Int32>`, `Array<Uint8>`, etc.) are specialized to use WASMGC packed/unpacked array types. Reference-element arrays (`Array<String>`, `Array<SomeRecord>`, etc.) are specialized: one WASM type per element type.
- **Implementation in WASM:** Array operations are not implemented as ordinary Dovetail code. They are lowered to **WASM built-ins** or **intrinsics**—i.e. the compiler emits the corresponding WASM GC (or other) instructions directly.

**Intrinsic categories:**
- **Access:** `ArrayGet`, `ArraySet`, `ArrayLength` — element read, write, and length query.
- **Construction:** `ArrayNewFixed` (from literal elements), `ArrayFill` (fill with a value).
- **Utility:** `ArrayClone` — creates an independent copy of the array.

**Literal syntax:** An array literal is written `[| 1, 2, 3 |]` (`[||]` when empty) and produces `Expr::ArrayLiteral`, lowered to `ArrayNewFixed`. The plain `[ ... ]` form is the *list* literal — see below.

**Indexing sugar:** The parser produces `Expr::Index` for `arr[i]`. The typechecker desugars: `arr[i]` → `IntrinsicCall { ArrayGet, [arr, i] }` and `arr[i] = v` → `IntrinsicCall { ArraySet, [arr, i, v] }`. No new codegen is needed.

### Lists

`List<out T>` is an ordinary prelude enum (`Nil | Cons(T, List<T>)`), not an intrinsic — it needs no codegen support of its own.

- **Literal syntax:** `[1, 2, 3]`, and `[]` for the empty list. Both parse to `Expr::ListLiteral`.
- **Cons:** `h :: t` is right-associative and parses to that same node, so `a :: b :: []` and `[a, b]` are indistinguishable after parsing and share one inference path. Keeping them together is what lets both take the element type from a join across all elements — desugaring `::` straight to `List.Cons` would instead bind the element type from the head and reject a widening tail (`dog :: animals`).
- **Inference:** `infer_list_literal` mirrors `infer_array_literal`'s lowest-common-type join, then folds the result into a right-nested chain of `EnumCreate` nodes. Unlike an empty array literal, `[]` needs no annotation: `List` is covariant, so `List<Never>` is assignable to every `List<T>`.
- **Patterns:** `[]`, `[a, b]` and `h :: t` are desugared in the **parser** into plain `Nil` / `Cons` variant patterns, so inference, exhaustiveness and codegen never see a list-specific pattern form.

### WASI, WASM, and WASMGC

- Every Dovetail application is a **valid WASM component**. The component targets the **WASI CLI world**. See [docs/wasi-component-wasi-cli-design.md](docs/wasi-component-wasi-cli-design.md) for the plan to implement the component output and the initialization export (e.g. `cm32p2_initialize`, used for global variable initialization).
- **WASMGC** is the primary execution model:
  - **Records, tuples, enums, classes, arrays, strings** are all represented as WASMGC types/values.
  - Prefer **non-null** WASMGC types where the type system guarantees non-null.
- **Linear memory** is used only for integration with WASI (e.g. passing buffers to syscalls). Use a **bump allocator** and copy from Dovetail’s GC representation into linear memory and back at the boundary.
- **Strings:** UTF-8 encoded. Use packed arrays or the appropriate GC array type as defined by the WASMGC design.
- **Packed arrays** for `Int8`, `Int16`, `Uint8`, `Uint16`, `Bool`, and for string storage where applicable.
- **wasmtime** is the reference runtime for Dovetail.

### Generics

- **Reified generics:** Type parameters are present at runtime (type info available when needed).
- **Specialization:** Use **specialized** generics when any type parameter is a primitive.
- **Shared code:** Generics are **specialized only**: one instantiation per type-argument list.
- Type information is available at runtime for reflection, debugging, and generic dispatch where required.

### Closures

- Dovetail supports **closures**. Design of closure representation (e.g. fat pointers, env + function, or GC-managed closure objects) and capture rules will be detailed in a later section or a dedicated doc.

---

## Development Process

- **Feature-by-feature:** For each language or compiler feature, implement it across all relevant stages (lexer, layout, parser, typechecker, codegen) so that the pipeline stays coherent.
- **Unit tests:** Each stage has unit tests (lexer on raw input, parser on token sequences, typechecker on AST fragments, codegen on typed AST fragments).
- **Integration tests:** For every feature, add at least one integration test that runs the full compiler on one or more source files and checks the resulting component or runtime behavior.
- **Regression tests:** Add tests for every bug fix; prefer small, focused tests that pin the intended behavior.

---

## Open / Future Work

- **Diagnostics:** Formatting (multi-line errors, suggestions), severity (error vs warning), and source snippets.
- **Closure representation:** Concrete layout and codegen for closures.
- **Codegen:** Detailed design for the type dependency graph, GC type mapping, and WASI adapter layer. Codegen-level caching/invalidation (e.g. when to re-emit WASM) to be defined.

---

## Gaps / to be defined

The following are not yet specified in this document; they can be filled in as the implementation is designed.

| Topic | Open question |
|-------|----------------|
| **Cache storage** | Where the per-package typechecker cache is stored (e.g. `.dovetail/cache/`, `target/`), and how invalidation is detected (mtime, content hash). |
| **Errors in earlier stages** | When lexer or parser reports errors for a package, do we still run the typechecker on that package (e.g. on partial AST) or skip it? Do we typecheck other packages in the same run? |
