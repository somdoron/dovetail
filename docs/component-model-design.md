# Component Model Design

Dovetail's interop story: **the WIT interface is Dovetail's FFI.** Instead of raw C FFI or host functions, external capabilities are WASM components (WASI 0.2 component model). The compiler consumes a component's WIT interface, projects it into a generated low-level Dovetail bindings package, emits generic canonical-ABI lift/lower code, and composes the component into the final output — which stays a portable component whose imports are `wasi:*` only (`wasmtime run --dir=... app.wasm`). SQLite is the first consumer: `sqlite3.c` (upstream WASI build) plus a thin Rust shim, shipped as **`standard-sqlite/artifacts/sqlite.wasm`** alongside the Dovetail library.

**In scope:** importing WIT interfaces from component dependencies (`[[project.component]]`), generated virtual bindings, the generic canonical-ABI engine, build-time composition, `standard-sqlite`.

**Out of scope (future):** exporting Dovetail-defined worlds; WIT-only imports satisfied by a custom host; async WIT (`future`/`stream`, WASI 0.3); GC canonical ABI (component-model#525 — would eliminate the marshaling linear memory but not the GC↔linear copy); a component registry; a `use`-binding language feature for scoped disposal.

**Implementation status:** Implemented (compiler + `standard-sqlite`).

---

## 1. Layering

| Layer | Artifact | Job |
|---|---|---|
| Component (e.g. sqlite3.c + Rust shim) | `.wasm` component | the capability; exports a WIT interface, imports `wasi:*` only |
| Generated bindings (virtual) | injected package (e.g. `sqlite.raw`) | policy-free 1:1 projection of the WIT; canonical-ABI marshaling |
| Wrapper library | normal Dovetail package (e.g. `standard-sqlite`) | all API design: `Result`, typed values, lifecycles |

The generated layer contains **zero policy**: resources are `newtype Handle = Int32` with module functions and an explicit `drop`; raw scalar codes stay raw. Everything opinionated lives in the wrapper, which iterates without touching the compiler.

## 2. Manifest surface

```toml
[[project.component]]
path = "artifacts/sqlite.wasm" # component file relative to the project dir
package = "sqlite.raw"       # Dovetail package for the generated bindings
# interface = "raw"          # only needed when the component exports several
```

The interface WIT is **extracted from the component binary** (components are self-describing) — no separate `.wit` files ship. Validation: a project-contained `path` is required; imports of the component must be `wasi:*` only. A project's *component closure* is its own components plus its transitive dependencies' (deduplicated); the closure drives bindgen and composition for whichever project runs codegen.

The compiler embeds no library components. SQLite shim sources live under `components/sqlite-shim/`; `build.sh` writes the checked-in artifact to `standard-sqlite/artifacts/sqlite.wasm`. Git dependencies fetch it with the owning library, pinning the wrapper, macros, and component to one commit.

## 3. Type mapping (bindgen, `compiler/witgen/`)

| WIT | Generated Dovetail |
|---|---|
| record / variant / enum | record / enum (payloads 1:1) |
| option / result / string / list\<T\> / tuple | native `Option` / `Result` / `String` / `Array<T>` / tuple |
| flags | `newtype X = Uint32` + constant functions |
| resource | `newtype X = Int32`; methods take `self`; constructor → `make`; `[resource-drop]` → `drop(self)` |
| u8..s64, f32/f64, bool, char | matching primitives |

kebab-case → camelCase (types PascalCase). Freestanding functions group in a module named after the interface. Function bodies are `panic` stubs (typed `Never`) — ordinary `TypedFunction`s that codegen recognizes via a `WitImportTable` (MangledName → WIT import, rebuilt per project by matching the virtual span file `<wit>/<pkg>.dove` + `source_name`) and replaces with generated bodies. **Zero typechecker changes.**

**v1 restrictions** (clean bindgen errors): interfaces must be self-contained (no foreign `use`); no async types; flags ≤ 32; ≤ 16 flattened params; no multi-named results; no empty records.

**Error variants must not own handles.** Every error type in the tree today is a payload-less code — verified across `dovetail/wit/*.wit`, `components/sqlite-shim/wit`, and the test fixtures — and the runtime relies on it: a typed failure is DISCARDED wherever an interrupt supersedes it (`interruptFiber`'s raced-cancel branch, `handleFailure`'s `UninterruptibleFrame` arm, every mask pop). An imported interface whose error variant carried an `own<...>` handle would have that handle dropped unowned on those paths — a leak, and under p3, where indices are reused after a drop, a later close landing on somebody else's handle. Adding such an import means giving typed errors a finalizer of their own (an `IoError` method the runtime can call) so those sites can release instead of discard; it is not a per-site fix.

## 4. Codegen

- **Import space:** fixed WASI imports at `0..118`; WIT imports at `118..118+N` (order: interface declaration order, WIT function order, then `[resource-drop]` per resource); runtime/user functions shift — always use the `func_*()` accessors on `Codegen`.
- **Engine** (`function_emitter/wit_marshaling.rs`): recursive lower/lift walking the WIT type and Dovetail type in lockstep, driven by wit-parser's `wasm_signature`/`push_flat`/`SizeAlign` (never hand-roll flattening). Lowering spills flat args into slot locals (variant cases write a discriminant plus join-converted payload slots); results lift from a single flat value or through a bump-allocated retptr. Strings/lists copy through the existing linear-memory helpers; `Option`/`Result` map WIT case order to Dovetail declaration order by name (Dovetail declares `Some` before `None`).
- **World:** `wasi:cli` command world plus one `WorldItem::Interface` per imported interface (programmatic `Resolve` manipulation in `codegen/component.rs`).
- **Composition:** after codegen, `wac_graph::plug` plugs each component's exports into the app's imports (`codegen::compose`); dependency `wasi:*` imports pass through. Build, test, and run modes all produce fully-linked artifacts, so the runner/test-runner needed no changes.

## 5. The copy at the boundary

Crossing the canonical ABI copies data (GC heap ↔ the component's linear memory). This is inherent to a GC↔linear boundary and unaffected by the future GC canonical ABI (which helps GC↔GC and would only remove Dovetail's marshaling scratch memory). Row reads cost one crossing per cell; if profiling ever shows this matters, add a batched accessor to the shim's WIT alongside the 1:1 base — not a redesign.

## 6. SQLite specifics

- Shim WIT (`dovetail:sqlite-raw/raw`): 1:1 with the C API — `database`/`statement` resources, raw `s32` result codes, `bind-*`/`column-*` per type, drop = `close_v2`/`finalize`. No callbacks (`exec`, hooks, custom functions) — punted; custom SQL functions would need reverse (Dovetail→shim) exports.
- Upstream `SQLITE_WASI` build: no WAL, no mmap, no loadable extensions; dot-file locking. **Constraint: one process per database file** — cross-process concurrent access is not safely coordinated.
- `standard-sqlite` owns the API, in the house idiom: `Connection.open`/`conn.statement` return `Resource<_, SqliteError>` (scoped with `use`, released on scope exit); engine-touching operations (`execute`/`query`/`queryOne`/`step`, `transaction(fn)`) are `Async`-shaped and lift the sync internals via `Async.thunk` — exactly like `standard.io.fs` over sync WASI calls. The async contract allows a truly non-blocking implementation later without an API change; pure row/cell reads and parameter binding stay sync.

## 7. Testing

- `dovetail/tests/wit_imports.rs` — bindgen/world/determinism/error cases (compile-level).
- `dovetail/tests/wit_engine.rs` — runtime fixture (`tests/fixtures/testiface.wit`) implemented host-side via wasmtime `bindgen!`; every type shape round-trips through the engine, including resources and flat-join variants.
- `standard-sqlite/test/` — Dovetail `test` declarations against the real composed component via `dovetail test`.
