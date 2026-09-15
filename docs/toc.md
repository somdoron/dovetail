# Docs — Table of Contents

Design and reference documents for the Dovetail compiler. See [Backlog.md](Backlog.md) for the overall feature backlog.

---

## Design documents

| Document | Description | Implementation status |
|----------|-------------|------------------------|
| [array-traitobject-demonomorphize-design.md](array-traitobject-demonomorphize-design.md) | Shared array and interface-object representations; removes monomorphize type-discovery walk | **Done** (implementation notes supersede original details) |
| [arrays-design.md](arrays-design.md) | Arrays: WASM-GC representation, `Array<T>`, intrinsics, specialized | **Done** |
| [async-await-design.md](async-await-design.md) | Async/await, Awaitable trait, associated types and GATs in traits, desugaring | **Done** |
| [async-runtime-design.md](async-runtime-design.md) | Async runtime: Async&lt;T,E&gt;, Fiber, Promise, cooperative scheduler, WASI event loop | **Done** (core runtime in `standard-io`; higher-level combinators ongoing) |
| [classes-design.md](classes-design.md) | Classes: constructor, inheritance, traits, abstract, visibility, methods, properties, generics, codegen | **Done** |
| [closures-design.md](closures-design.md) | Closures, function types, first-class functions, capture/boxing, closure enum (FreeFunction \| MethodWithSelf) | **Done** |
| [debugging-backtraces-design.md](debugging-backtraces-design.md) | WASM debug info, Name Section, DWARF, panic messages, wasmtime backtraces | **Done** |
| [enums-design.md](enums-design.md) | Enums (discriminated unions), variants, generics, match | **Done** |
| [extension-methods-design.md](extension-methods-design.md) | Extension methods for any type (non-generic) | **Done** |
| [fs-library-design.md](fs-library-design.md) | High-level filesystem library `standard-io-fs`: pure `Path`, `Directory`/`File` handles, `Div` join operator; minimal `standard-text` (UTF-8) | **Done** (v1 shipped) |
| [full-erasure-design.md](full-erasure-design.md) | Full type erasure for generic types (one WASM struct per generic def, arity-bucketed closures); supersedes variance-only erasure and reified-generics-types-only research | **Done** |
| [generic-extension-methods-design.md](generic-extension-methods-design.md) | Generic extension methods (e.g. `extension <T> for Array<T>`) | **Done** |
| [generics-design.md](generics-design.md) | Generics: specialized only (monomorphized), codegen | **Done** (records, match; enums/classes later) |
| [io-library-design.md](io-library-design.md) | Low-level IO library: non-blocking WASI 2.0 wrapping (poll, streams, TCP/UDP, filesystem, clocks, random) for async framework authors | **Done** |
| [lsp-design.md](lsp-design.md) | LSP server and VS Code extension: diagnostics, completion, navigation, hover, inlay hints, test runner | **Done** |
| [match-expression-design.md](match-expression-design.md) | Match: literals, variable/wildcard, guards, exhaustiveness | **Done** |
| [modules-design.md](modules-design.md) | Modules (standalone and module-for-type), import rules | **Done** |
| [monomorphize-separation-design.md](monomorphize-separation-design.md) | Monomorphize phase separation: inference 1:1 with types, separate monomorphize pass, LSP support | **Done** (dedicated `compiler/monomorphize` phase) |
| [multi-project-multi-package-design.md](multi-project-multi-package-design.md) | Multi-project, multi-package, multi-file, manifest, imports | **Done** |
| [github-dependencies-design.md](github-dependencies-design.md) | Repository-based dependencies, aliases, locked revisions, local source storage, artifacts, and LSP navigation | **Done** (Git resolver, compiler pinning, and LSP integration) |
| [newtypes-design.md](newtypes-design.md) | Newtypes: zero-cost wrapper, construction, `value`, private newtypes | **Done** |
| [railway-early-return-design.md](railway-early-return-design.md) | Railway early return: EarlyReturn trait (OnFailure associated type), try / orReturn, desugaring to unwrap + return | **Done** |
| [records-design.md](records-design.md) | Records: definition, construction, `with`, pattern matching | **Done** |
| [resource-management-design.md](resource-management-design.md) | Resource management: Usable trait (Wrapped GAT), `use` prefix expression, bracket primitive, Resource type, sync/async | **Done** (`Usable`, `use` desugar, `resource.drop`, `Resource`) |
| [strings-design.md](strings-design.md) | String, Char, literals, interpolation, data section, intrinsics | **Done** |
| [testing-design.md](testing-design.md) | Testing: test declarations, attributes, unit/integration, `dovetail test` CLI | **Done** |
| [tuples-design.md](tuples-design.md) | Tuples: anonymous positional types, construction, `_0`/`_1`, destructuring, match | **Done** |
| [type-alias-design.md](type-alias-design.md) | Type aliases and generic type aliases: pure aliases, trait bounds, visibility | **Done** |
| [variance-any-design.md](variance-any-design.md) | Generic variance (+/-), Any type, boxing, `is`/`as`, type-annotated match | **Done** |
| [macros.md](macros.md) | Compile-time macros (Rhai scripts): manifest wiring, codegen of trait impls | **Partial** (only `derive` macros supported, e.g. `Equatable`, `JsonEncoder`/`JsonDecoder`) |
| [trait-design-appendix.md](trait-design-appendix.md) | Inheritance, disambiguation, defaults, associated types/GATs, coherence | **Done** within the current scope; [trait completion](trait-completion-design.md) supersedes older overload and projection restrictions |
| [wasi-component-wasi-cli-design.md](wasi-component-wasi-cli-design.md) | WASI p3 CLI components, shared host driver, dependency-ordered initialization before `main` | **Done** (original p2 ABI proposal superseded) |
| [http-library-production-readiness.md](http-library-production-readiness.md) | HTTP library (`standard.io.http`) production-readiness plan: functional end-to-end (validated against Rust via `standard-io-http-interop`); hardening tasks | **In progress** |
| [trait-completion-design.md](trait-completion-design.md) | Generic class/method defaults, ordinary and generic associated projections, inherited overloads | **Done** within the agreed scope; final reviews clean, 1,187 compiler tests and 1,608 workspace tests passed |
| [traits-design.md](traits-design.md) | Core traits, generic implementations, bounds, orphan rule, classes | **Done** within the documented scope; dynamic values use interfaces; see [audit](trait-implementation-status.md) and [completion](trait-completion-design.md) |
| [class-identity-design.md](class-identity-design.md) | `ClassIdentity.equals`/`hash` and `where T: class`; universal lazy hash slot, explicit trait implementations; removes `MutexId`/`WaiterId` while preserving effectful allocation | **Done** |
| [crypto-library-design.md](crypto-library-design.md) | Crypto primitives, X.509, trust anchors, async randomness; Uint128 | **Partial** (core implemented; streaming extension and specific validation checklist items remain) |
| [inference-layer-refactor-design.md](inference-layer-refactor-design.md) | Refactor of typechecker inference phase (lean context, free functions) | **Not started** |
| [interface-objects-design.md](interface-objects-design.md) | Interface values, declaration safety, intersections, upcasts, shared trait behavior, compiler rename, book | **Done** (phases 1–6) |
| [slice-design.md](slice-design.md) | `Slice<T>` in prelude: writable stack-based view (newtype over tuple), `a[\|i..j\|]` syntax, replaces `(array, offset, count)` triples | **Done** |
| [stream-design.md](stream-design.md) | Stream library: chunked pull `Stream<T,E>` as reified tree + cursor chains, Rendezvous/Take primitives, merge, WASI 0.2/0.3 edges; replaces `Selectable` | **Not started** |
| [tls-library-design.md](tls-library-design.md) | TLS 1.3 sans-I/O client/server, record layer, full handshake, PSK resumption | **Partial** (core implemented; retry/client-auth/RSA signing, traffic-key erasure, optional 0-RTT, and conformance work remain) |
| [tuple-extension-design.md](tuple-extension-design.md) | Type/value `~`, `Tuple` bound, `init`/`last`, inductive traits, enclosing bounds, parser `append` and `++` | **Done** (milestones 1–6 implemented and validated; documented limits remain) |
| [tuple-multivalue-codegen-design.md](tuple-multivalue-codegen-design.md) | Flattened tuple parameters/locals/returns and record/class fields; shared boxed boundaries; Uint128 wide arithmetic | **Done** (functional phases 1–6; benchmark/debugger follow-ups not verified) |
| [nominal-type-identity.md](nominal-type-identity.md) | Single module-wide rec group: fixes `is`/`as` aliasing of same-shape types (WASM-GC structural canonicalization); net code deletion, prerequisite for the type-ID extension | **Done** |
| [type-id-extension.md](type-id-extension.md) | Type-ID extension on top of [full erasure](full-erasure-design.md): runtime discrimination for generic records, enums, and classes via immutable type IDs and compile-time subtype sets; shared static subtyping and generic-aware match coverage | **Done** |
| [variance-type-erasure-design.md](variance-type-erasure-design.md) | Selective type erasure for variance parameters; single representation, no variance casts | **Superseded** by [full-erasure-design.md](full-erasure-design.md) |
| [interfaces-design.md](interfaces-design.md) | Interfaces as trait + safety check + boxability | **Superseded** by [interface-objects-design.md](interface-objects-design.md) |
| [trait-objects-removal-design.md](trait-objects-removal-design.md) | Gating fat pointers behind `interface` | **Superseded** by [interface-objects-design.md](interface-objects-design.md) |

---

Trait completion retains explicit boundaries: associated-type defaults, automatic
static trait defaults on classes, GAT equality constraints, blanket implementations,
specialization, and trait variance remain outside the completed scope. Interface
safety rules still exclude generic methods and associated types. For current
generic-default, projection, inherited-overload, and generic `use` behavior,
[trait-completion-design.md](trait-completion-design.md) supersedes older
restriction statements in the appendix and interface design.

## Reference and improvement docs

| Document | Description | Implementation status |
|----------|-------------|------------------------|
| [cranelift-gc-research.md](cranelift-gc-research.md) | Research: Cranelift + tracing GC, stack maps, safepoints, JIT/AOT, VMContext | **Research** |
| [lsp-manual-testing.md](lsp-manual-testing.md) | Manual testing checklist for LSP features in VS Code | **Reference** |
| [match-static-vs-generic-without-substituted.md](match-static-vs-generic-without-substituted.md) | Match: static vs generic subject types, role of `Type::Substituted` | **Reference** |
| [reified-generics-types-only-research.md](reified-generics-types-only-research.md) | Reified generics for types only (single representation, box primitives); difficulty vs variance cast | **Superseded** by [full-erasure-design.md](full-erasure-design.md) |
| [tuple-scalar-erasure.md](tuple-scalar-erasure.md) | Research: tuple erasure to scalars at codegen, multi-value returns | **Superseded** by [tuple-multivalue-codegen-design.md](tuple-multivalue-codegen-design.md) |
| [type-theory-and-improvements.md](type-theory-and-improvements.md) | Type-system theory, current implementation, gaps, improvement guide | **Reference** (improvements tracked in Backlog) |
| [trait-implementation-status.md](trait-implementation-status.md) | Audit of every trait/interface design: implementation evidence and explicitly retired proposals | **Reference** (audit complete) |
| [Backlog.md](Backlog.md) | Feature backlog: done, in-progress, queue | — |

---

## Status legend

- **Done** — Design implemented end-to-end (or substantially so).
- **In progress** — Actively being implemented.
- **Partial** — Partially implemented or only foundation in place.
- **Not started** — Design approved; implementation queued.
- **Reference** — Living document; not a single “feature” to implement.
- **Research** — Exploration or future-work notes.
- **Superseded** — Replaced by a newer design; kept for historical context.
