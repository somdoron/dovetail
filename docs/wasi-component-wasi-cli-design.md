# WASI Component and WASI CLI Target Design

**Implementation status:** Done. The compiler emits WASI CLI components,
initializes globals before `main`, and runs components through the shared WASI
p3 host driver. The original p2 ABI proposal is superseded by the implemented
p3 integration described below.

## 1. Output and component pipeline

The compiler generates a WebAssembly core module and wraps it in a component
using `wit-component`:

1. Load the vendored WIT packages through `wit_parser::Resolve`.
2. Select the WASI CLI command world, incorporating application WIT imports and
   test exports when needed.
3. Embed component metadata with `embed_component_metadata`.
4. Encode the component with `ComponentEncoder`.

The implementation is in [component.rs](../dovetail/src/compiler/codegen/component.rs).
WIT parsing, metadata, and encoding failures become compiler errors.

The repository currently pins WASI p3 to `0.3.0-rc-2026-03-15` in
[p3.rs](../dovetail/src/p3.rs), which also defines the vendored WIT list and shared
host configuration. This is the repository's ABI pin, not a claim about the
latest upstream WASI release.

## 2. Core exports and execution

The [code generator](../dovetail/src/compiler/codegen/mod.rs) emits these core
exports for component encoding:

| Export | Purpose |
|--------|---------|
| `[async-lift-stackful]wasi:cli/run@<P3_VERSION>#run` | Stackful async entry wrapping Dovetail `main`. |
| `_initialize` | Initializes user globals before the component's entry runs. |
| `cabi_realloc` | Allocator for the canonical ABI's linear-memory boundary. |
| `memory` | Linear memory used at the WASI boundary. |
| `[async-lift-stackful]test-n<N>` | Individual test entry points in test components. |

The core `run` wrapper calls `main`, then reports the successful WASI result
through the `[task-return]run` import. Dovetail `main` returns `Unit`; the original
proposal to interpret an `Int32` return as a process status is obsolete. In test
components, the ordinary `run` wrapper does not run the individual tests; the
host calls their separate exports.

The core module does not use a Start section for user global initialization.
Component encoding wires `_initialize` into instantiation, before the host
invokes the component-level `wasi:cli/run` export. The host does not separately
call an exported component function named `cm32p2_initialize`.

The shared [p3 driver](../dovetail/src/p3.rs) configures the required Wasmtime
component features, instantiates asynchronously, and invokes `run` through the
concurrent API. It distinguishes setup failures, guest failures, and a returned
WASI error. The CLI and integration-test helpers use this same driver.

## 3. WASI imports

Imports follow the vendored p3 WIT and generated
[import definitions](../dovetail/src/compiler/codegen/p3_imports.rs), including
CLI environment and standard streams. Application WIT imports are included in
the generated component world. The old p2 `blocking-write-and-flush` example is
historical, not the current stdout ABI.

## 4. Global initialization

[globals.rs](../dovetail/src/compiler/codegen/globals.rs) implements global storage
and dependency ordering:

- User globals receive default storage values in the core Global section: zero
  for numeric values and nullable references for reference values.
- `_initialize` evaluates user initializers and stores their results, including
  primitive initializers. The original proposed optimization to place constant
  user initializers directly in the Global section is not required by the
  implemented design.
- Initializers run in dependency order. Dependency discovery includes reads
  reached through function calls, class-body initializers, and inherited
  constructor arguments, rather than only direct global references.

This supplies the original design's central guarantee: initialized global values
are available when `main` runs.

## 5. Validation and disposition of the original plan

The original work items—component encoding, CLI imports/exports, host execution,
and ordered initialization—are implemented. The `cm32p2_*` export spellings,
separate `run_post` hook, p2-only WIT loading, and prior-repository migration steps
are obsolete implementation proposals, not outstanding tasks.

Regression coverage:

- [p3_encoding.rs](../dovetail/tests/p3_encoding.rs) checks the component encoder and
  runtime contract for async imports, stackful/callback lifts, waitables, and
  task completion.
- [global_variables.rs](../dovetail/tests/global_variables.rs) compiles and runs
  global-initialization examples, including dependencies through function calls
  and class construction.
- [common test runner](../dovetail/tests/common/mod.rs) compiles Dovetail programs to
  components and executes them through the production p3 driver.

A compatible host must support the pinned p3 async ABI and configured Wasm
features. Compatibility with arbitrary p2-only hosts is not part of this design's
completion criteria.
