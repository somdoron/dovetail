# Full Type Erasure for Generic Types

**Status:** Implemented (closure portion uses always-erased arity-bucketed design — see §3.2 and §6)

**Trait terminology:** The historical trait-object names below now refer to
interface objects. Shared arrays/interfaces are implemented by the
[array/interface representation design](array-traitobject-demonomorphize-design.md);
the [trait/interface audit](trait-implementation-status.md) records superseded
representation details and current restrictions.

**Supersedes:**
- [variance-type-erasure-design.md](variance-type-erasure-design.md) — selective (variance-only) erasure
- [reified-generics-types-only-research.md](reified-generics-types-only-research.md) — full erasure with type\_id

**Future extension:** [type-id-extension.md](type-id-extension.md) — adds runtime type discrimination via type\_id. This document does **not** include the type\_id; reified generics are tracked as future work.

---

## 1. Motivation

### 1.1 Problems with monomorphized generic types

Today, each generic-type instantiation (`Option<Int32>`, `Option<String>`, …) gets its own WASM struct type. This has three consequences:

- **Cross-instantiation subtyping breaks at the WASM level.** Sealed class hierarchies with variance — `sealed abstract class Async<out T, out E>` with `final class Succeed<out T> extends Async<T, Never>` — cannot be assigned across instantiations even when the Dovetail type system allows it, because `Async$Int32$Never` and `Async$Int32$String` are unrelated WASM types.
- **The variance-cast pass is large and fragile.** `variance_cast.rs` (~2000 lines) synthesizes runtime deconstruct/reconstruct code for every coercion point where actual and expected types share a generic FQN but differ in type arguments. It's the most complex pass in the typechecker.
- **N WASM type definitions per generic.** Programs with many instantiations carry the cost in binary size, type graph, and cache size.

### 1.2 Why full erasure now

Selective erasure (the previous design) erases only `out`/`in` parameters and keeps invariant ones monomorphized. It works, but it leaves complexity in the codebase: two code paths through monomorphize, two flavors of mangled names per generic, special-casing for the invariant/variance split.

Full erasure is simpler: **one WASM struct type per generic definition, period.** Variance annotations stay in the typechecker (for assignability and variance-position checks), but no longer affect codegen.

The cost — `ref.cast` + unbox on every read of a type-parameter-typed field — is real, but in exchange we get:

- Cross-instantiation subtyping works automatically.
- `variance_cast.rs` deletes ~1800 lines (we keep only trait-object coercion and primitive→Any boxing).
- One TypeDef, one rec group, one vtable layout per generic def.
- A clean staging point for reified generics: adding a type\_id field is a strict extension.

### 1.3 Functions stay monomorphized

This design erases generic **types**. Generic **function declarations** remain monomorphized:

- Trait bounds (`fn show<T: Display>(x: T): String = x.display()`) resolve to concrete impl calls at specialization time. No witness tables, no runtime dispatch.
- Values of type `T` inside the function body have concrete WASM types (`i32`, `(ref $String)`, …) — no boxing for locals, params, or return values inside the function.
- `is`/`as`/match on type parameters resolves at compile time (`x is Int32` becomes `true` or `false` in the monomorphized body).
- Reads of erased fields cast back to the type the monomorphized function knows.

This means we keep `MangledName::for_function(fqn, &param_types)` style mangling for functions and globals. Only **TypeDef** mangled names change.

---

## 2. Core Design

### 2.1 What is erased

For generic **records, enums, and classes**: every type-parameter slot in the WASM struct layout becomes `(ref null any)` (`anyref`). All instantiations of a generic definition share **one** WASM struct type (or one rec group, for enums and class hierarchies).

For generic **function declarations**: not erased. Each specialization gets its own WASM function with concrete signature.

For generic **function-typed values** (closures): erased — see §3.2.

### 2.2 What is not erased

| Kind | Treatment |
|------|-----------|
| Non-generic types | Unchanged. |
| Generic function declarations | Monomorphized as today. |
| `Array<T>` where `T` is primitive | One WASM array type per primitive element type (`Array<Int32>` ≠ `Array<Float64>`). WASM-GC arrays must declare a concrete element type. |
| `Array<T>` where `T` is a reference type | One shared WASM array type with `anyref` elements. Cast-back-on-read for the specific reference type. |
| Tuples | Unchanged — one WASM struct per distinct tuple shape. |
| Trait objects | Unchanged — vtable dispatch. |
| Newtypes | Transparent in codegen, unchanged at the WASM level. |

### 2.3 Erased mangled names

Today, `MangledName::for_generic_type(fqn, type_args)` produces `Option$Int32`, `Option$String`, … . With full erasure, **TypeDef mangled names drop the type arguments entirely**:

```
Option<Int32>  -> WASM type: prelude.Option
Option<String> -> WASM type: prelude.Option         (same WASM type)
Result<Int32, String> -> WASM type: prelude.Result
Pair<A, B>     -> WASM type: pkg.Pair                (no per-instantiation suffix)
```

`MangledName::for_generic_type` is removed entirely from the TypeDef path. All TypeDefs use `MangledName::for_type(fqn)`. Function mangling (`MangledName::for_function(fqn, &param_types)` and friends) is unchanged.

### 2.4 TypeDefs preserve `TypeParameter` in field types

Today, monomorphize substitutes type parameters with concrete types and produces N TypeDefs per generic. New design: monomorphize produces **one TypeDef per generic definition**, and the field types **retain `Type::TypeParameter`** for slots that come from a type parameter.

```rust
// Source:
// enum Option<out T> = None | Some(T)

// Old: N entries (Option$Int32, Option$String, …) each with concrete payload types.
// New: one entry.
TypeDef::Enum(EnumTypeDef {
    fqn: "prelude.Option",
    mangled_name: "prelude.Option",        // no type args
    type_params: [T],                       // preserved
    variants: [
        EnumVariantDef { name: "None", payload_types: [] },
        EnumVariantDef { name: "Some", payload_types: [TypeParameter("T")] },
    ],
})
```

Expression types still carry concrete `type_args`:

```rust
// Some(42):
TypedExprKind::EnumCreate {
    fqn: "prelude.Option",
    variant_name: "Some",
    args: [<expr ty=Int32>],
    type_args: [(Covariant, Int32)],       // concrete — codegen reads this
}
// expr.ty = Type::GenericEnum {
//     fqn: "prelude.Option",
//     mangled_name: "prelude.Option",      // erased
//     type_args: [(Covariant, Int32)],
// }
```

This gives codegen everything it needs:
- `mangled_name` on the expression → look up the (shared) WASM type index.
- `type_args` on the expression → concrete type for cast-back-on-read and boxing-on-write.
- `type_params` and `TypeParameter` in field types on the TypeDef → know which slots are erased.

### 2.5 Struct layout

```
;; enum Option<out T> = None | Some(T)
struct $Option {}                              ;; base (empty)
struct $None   <: $Option {}                   ;; no payload
struct $Some   <: $Option { f0: anyref }       ;; T erased

;; record Pair<A, B>
struct $Pair { f0: anyref, f1: anyref }        ;; A, B both erased

;; class Box<T> { val value: T }
struct $Box { f0: ref $BoxVtable, f1: anyref } ;; T erased

;; sealed abstract class Async<out T, out E>
;; final class Succeed<out T> extends Async<T, Never>() { val value: T }
struct $Async   { f0: ref $AsyncVtable }
struct $Succeed <: $Async { f0: ref $SucceedVtable, f1: anyref }  ;; value: T erased
```

`type_to_valtype` in `codegen/mod.rs` changes `Type::TypeParameter(_) => unreachable!()` to return `anyref`. That single change makes struct layout emission work for erased fields.

### 2.6 Box on write

When storing into an erased slot:

- **Primitive** (`Int32`, `Bool`, `Float64`, …): box via existing `box_type_index_for` machinery (`$Int32Box`, `$BoolBox`, …), then store as `anyref`.
- **Reference type** (`String`, record, enum, class, function value, array, …): store directly — WASM-GC upcasts implicitly to `anyref`.

```wasm
;; Some(42) where T = Int32
i32.const 42
struct.new $Int32Box        ;; box
struct.new $Some            ;; store as anyref
```

```wasm
;; Some("hi") where T = String
;; (string already a ref)
struct.new $Some
```

### 2.7 Cast back on read

When reading an erased slot, the value on the WASM stack is `anyref`. The monomorphized function knows the concrete type that the slot holds and emits the matching cast:

```wasm
;; Reading Option<Int32>.value
struct.get $Some, 0          ;; -> anyref
ref.cast (ref $Int32Box)     ;; -> (ref $Int32Box)
struct.get $Int32Box, 0      ;; -> i32

;; Reading Option<String>.value
struct.get $Some, 0          ;; -> anyref
ref.cast (ref $String)       ;; -> (ref $String)
```

A helper `resolve_concrete_field_type(field_type, type_params, type_args) -> Type` substitutes the TypeDef's `TypeParameter`s with the expression's concrete `type_args`. Works uniformly for record fields, enum payloads, class fields, and nested generic types (`Array<T>`, `Option<T>`, …).

```rust
fn resolve_concrete_field_type(
    field_type: &Type,
    type_params: &[TypeParamName],
    type_args: &[(Variance, Type)],
) -> Type {
    let sub: BTreeMap<_, _> = type_params.iter()
        .zip(type_args.iter())
        .map(|(name, (_, ty))| (name.clone(), ty.clone()))
        .collect();
    substitute(field_type, &sub)
}
```

### 2.8 Arrays

WASM-GC arrays are parameterized by element type — there is no `anyarray` equivalent. We split by primitive vs. reference:

- **`Array<T>` for primitive `T`:** one WASM array type per primitive (`Array<Int32>`, `Array<Float64>`, …). Element type is the native WASM type (`i32`, `f64`, …). No erasure — these can't share.
- **`Array<T>` for reference `T`:** one shared WASM array type with `anyref` element type. Storing a reference upcasts implicitly; reading casts back to the concrete element type.

When an array appears at an erased slot (e.g., `record Container<T> = items: Array<T>`):

- The slot itself is `anyref` (the field type is `TypeParameter` indirectly via `Array<T>`).
- On read, cast back to the specific `Array<T>` WASM type (primitive-specialized or `Array<anyref>`-shared).
- On read of an element from the shared array, additionally cast back to the concrete reference type.

### 2.9 Tuples

Tuples are unchanged. Each distinct tuple shape (`(Int32, String)`, `(Bool, Float64)`, …) keeps its own WASM struct. Tuples have no variance, are typically small, and the current representation is already efficient.

A tuple field at an erased generic slot still becomes `anyref` and casts back to the specific tuple struct type on read.

---

## 3. Function Declarations vs. Function-Typed Values

This distinction is load-bearing.

### 3.1 Generic function declarations: monomorphized

Each specialization of a generic function gets its own WASM function with fully concrete signatures.

```dovetail
fn id<T>(x: T): T = x
```

emits:

- `id$Int32` : `(i32) -> i32` — body is `local.get 0; return`
- `id$String` : `(ref $String) -> (ref $String)`
- …

Inside the body, `T` is concrete (`Int32`, `String`, …), so:

- Locals, params, intermediate values of type `T` use the native WASM type — **no boxing for non-erased-slot uses**.
- Trait-bound calls (`x.display()` for `T: Display`) resolve to the concrete impl's `MangledName` at specialization time.
- `x is Int32` resolves to `true`/`false` at specialization time. Same for `as` and typed patterns.
- Cast-back on read of an erased field uses the function's known `T` as the target.

### 3.2 Function-typed values: always erased, arity-bucketed

Closures take erasure further than data types: rather than emit a WASM struct per concrete `Type::Function` signature with adapters bridging concrete↔erased slot crossings, **every closure of arity N shares one canonical WASM struct**, regardless of param/return types or where it's stored.

#### Canonical types

For each function arity N actually used in the program (discovered by walking the typed module), codegen pre-declares one rec group before user types:

```
rec
  Func_N    = (func (param anyref env, anyref ×N) (result anyref))
  Closure_N = (struct (field anyref env) (field (ref Func_N) funcref))
```

`type_to_valtype(Type::Function(params, ret))` returns `(ref Closure_N)` where `N = params.len()` — concrete or type-parameter-containing, doesn't matter. A `(Int32) => String` and a `(B) => String` and a `(T) => U` all lower identically.

#### Closure body — prologue and epilogue

The body's WASM signature is `Func_N` (all anyref). The compiler wraps the user's body with two thin layers:

- **Prologue**: for each declared param `P_i` at WASM index `i+1`, cast `anyref → P_i`. `ref.cast (ref Box_X) + struct.get 0` for primitives, `ref.cast (ref X)` for ref types, identity for `Any` / `TypeVariable` / `GenericParam`.
- **Epilogue**: box the declared return type back to anyref. `struct.new Box_X` for primitives, identity for ref types, `unreachable` for `Never`/`Error`.

```wasm
;; let f: Int32 => String = (x: Int32) => format(x)
;; lifted body: (anyref env, anyref x) -> anyref
local.get 1                  ;; x: anyref
ref.cast (ref $Int32Box)
struct.get $Int32Box, 0      ;; x: i32 (prologue done)
;; ... user body produces (ref $String) ...
;; (epilogue: ref types upcast to anyref implicitly — no instruction)
```

#### Call site — box args, cast return

```wasm
;; f(42), where f: Int32 => String
local.tee $f_local
struct.get Closure_1, 0      ;; env
i32.const 42
struct.new $Int32Box         ;; box arg
local.get $f_local
struct.get Closure_1, 1      ;; funcref
call_ref Func_1              ;; -> anyref
ref.cast (ref $String)       ;; cast return back to declared type
```

#### Why this is better than concrete-closure-types-plus-adapters

- **Variance just works.** `class A`, `class B extends A`, `f: A => String` passed where `B => String` is expected: both lower to `(ref Closure_1)`, the call site pushes a `B`, `f`'s prologue does `ref.cast (ref A)` which succeeds (B IS-A A). Contravariance is enforced by the runtime cast in the prologue, not by the type system at the WASM boundary.
- **No adapters.** A closure stored in a variant payload, record field, or local variable is the same WASM type. Reads and writes are upcasts/downcasts of `(ref Closure_N)`, never wrapping.
- **One TypeDef machinery deleted.** No `FunctionSigTypeDef`, no per-signature deduplication, no `function_sig_collector`, no `canonicalize_for_function_sig`. The SCC graph has no function-type edges.

#### Cost

Every closure invocation boxes primitive args and unboxes the primitive result if the static return is primitive. For closures over reference types, the costs are just `ref.cast`s — cheap in WASM-GC engines. Hot numerical kernels pay; business-logic / async / WASI workloads don't notice. Mitigation (future): small-integer interning for `BoxInt32`.

#### Function references and method references

`FunctionRef` and `MethodRef` produce `Closure_N` values via `RefTrampoline`s — small synthesized WASM functions with `Func_N` signature that cast the anyref params back to the target's declared types, call the target, and box the return. `MethodRef` captures `self` as the env (boxed for primitive self); `FunctionRef` uses `null` env.

### 3.3 Implication

We get the best of both:

- **Direct generic-function calls stay direct.** A monomorphized caller calling `id<Int32>(42)` emits a direct `(i32) -> i32` call. No boxing — `id` is a function declaration, not a closure value.
- **Function-typed values flow uniformly everywhere.** One `Closure_N` per arity; no boundary crossings, no adapters, runtime-correct variance.

---

## 4. Closures, Captures, and Methods

### 4.1 Closure captures of type-parameter values

A closure body that closes over a value of type `T` stores the capture in its env struct. The enclosing function is monomorphized, so the capture has its concrete WASM type — **no erasure inside the env struct itself**. The env struct is per-closure, with one field per capture, typed to the capture's concrete WASM type (or its mutable-box wrapper for mutable captures).

The env is upcast to `anyref` when stored in the `Closure_N` struct's env slot, and downcast back to the per-closure env struct type in the closure body's prologue (before the per-param prologue runs).

Under always-erased closures (§3.2) there is no separate "wrapper closure when crossing erased slots" — the closure body's prologue/epilogue does all the cast-back/box work uniformly, regardless of where the closure value ends up being stored.

### 4.2 Class methods

Methods on generic classes follow the same rule: the method body is monomorphized per concrete `T`, but the class struct's field slots are erased. The first thing a method body does for a `T`-typed field access is cast-back-on-read.

```dovetail
class Box<T>(val value: T)
    fn get(): T = self.value
```

`Box$Int32.get` body:

```wasm
local.get $self                ;; -> (ref $Box)
struct.get $Box, 1             ;; -> anyref         (field 0 is vtable)
ref.cast (ref $Int32Box)       ;; -> (ref $Int32Box)
struct.get $Int32Box, 0        ;; -> i32
return
```

### 4.3 Vtables

One vtable layout per class hierarchy, shared across instantiations. Today: one per instantiation. The vtable struct stores the per-class method `funcref`s; method dispatch is unchanged. The functions pointed to by the vtable slots are still per-instantiation (e.g., `Box$Int32.get`, `Box$String.get`).

The vtable construction must use the right per-instantiation method references but emit them into a single shared vtable struct type. Each generic class instantiation builds its own vtable instance (one per `T`) but they all share the WASM struct type.

---

## 5. Removed Machinery

### 5.1 `variance_cast.rs` renamed to `coerce.rs`, slimmed down

The pass no longer does variance casts — under full erasure for data types and always-erased closures (§3.2), variance is enforced at the type-system level or via runtime prologue casts inside closure bodies, not via WASM-level coercion AST. The file was renamed to `dovetail/src/compiler/coerce.rs` and the public entry to `elaborate_coercions`.

What the pass actually does today:
- **Trait-object coercion** — synthesizes `TraitObjectCoerce` / `TemplateTraitObjectCoerce` at every assignment point where a value flows into a `Type::TraitObject` slot. Also registers `TraitObjectTypeDef` entries.
- **Primitive→`Any` boxing** — synthesizes `BoxToAny` when a primitive flows into a `Type::Any` slot.

What was deleted along the way:
- `needs_variance_cast` (always returned `false`), all 16 of its call sites, and the `coerce` identity stub it gated.
- `synthesize_record_variance_cast`, `synthesize_enum_variance_cast`, `synthesize_class_variance_cast`, `synthesize_function_variance_cast` — all gone.
- The `vc_helpers` / `vc_params` RefCell accumulators and the "Walk generated helper function bodies" loop — were write-only after the synthesizers' deletion.

### 5.2 Mutable class field boxing

`ClassFieldDef.boxed` was deleted along with its codegen branches in `classes.rs::build_class_subtype` and `function_emitter/expressions.rs` (ClassStructCreate field-push + `emit_class_hierarchy` field-push). The remaining `boxed` flags on `TypedExprKind::Let` / `VarRef` / `Assign` are now used **only** by the closure-mutable-capture mechanism (set from `capture.rs`); they are independent of class fields. The `MUT_BOX_*` WASM types stay — closure captures still need them.

### 5.3 `MangledName::for_generic_type` (TypeDef path)

Removed from all TypeDef-path call sites. Replaced with `MangledName::for_type(fqn)`. Function/global mangling unchanged.

### 5.4 `Type::Substituted` and the `typechecking_only` flag

Both deleted earlier (Feb–Mar 2026, commits `7f85854` and `fc0b036`). The match-static-vs-generic distinction lives in `is_assignable_for_pattern` (see [match-static-vs-generic-without-substituted.md](match-static-vs-generic-without-substituted.md)). Generic function/method bodies are still typechecked at declaration time via dedicated `infer_function_typecheck_only` / `infer_template_class_method` entry points — same semantics as before, no flag needed.

---

## 6. Match, `is`, and `as`

### 6.1 On type parameters inside generic functions

Resolved at compile time during monomorphization. `T is Int32` inside `foo<T>` becomes `true` in `foo$Int32` and `false` in `foo$String`. No runtime cost. Same for `as` (resolves to identity-or-panic at compile time) and typed patterns.

### 6.2 On generic instantiations from `Any`

`ref.test` checks only the base type (the erased WASM struct). `x is Option<Int32>` and `x is Option<String>` are indistinguishable at runtime — both succeed if `x` is any `Option`.

This is the deliberate trade-off. The compiler emits a **warning** at sites where the target type has type parameters with specific (non-`Any`) type arguments:

```
warning: type arguments are not checked at runtime
  --> file.dn:12:9
   |
12 |     x is Option<Int32>
   |     ^^^^^^^^^^^^^^^^^^ this checks only that x is an Option;
   |                        the type parameter T is erased
```

The warning is suppressed when type arguments are `Any` (the user is explicitly testing only the base type).

For `as` with mismatched concrete type arguments: the runtime check passes (base type matches), but subsequent field reads `ref.cast` to the expected type and trap if the actual stored value doesn't match. In well-typed programs this can't happen.

### 6.3 Future: reified generics

When type discrimination on `Any` is needed, the [type\_id extension](type-id-extension.md) adds an `i32` first field and compile-time subtype sets. All the information needed (`type_params` on TypeDefs, concrete `type_args` on expressions) is preserved by this design.

---

## 7. Static Globals and Module Generics

Static variables on generic types (module-level globals tied to a generic class, generic-class statics, generic-module statics) must not reference any type parameter in their types:

```dovetail
class Cache<T>
    static var instance: Option<T> = None   // ERROR: type involves type parameter T
    static var count: Int32 = 0              // OK
```

With one shared WASM struct per generic def, there is only one WASM global per static declaration, but semantically each instantiation expects its own independent storage. Reject these in the rules phase. Same restriction on generic modules.

---

## 8. The Motivating Example: Sealed Hierarchies

```dovetail
sealed abstract class Async<out T, out E>
final class Succeed<out T> extends Async<T, Never>()
    val value: T
final class FailCause<out E> extends Async<Never, E>()
    val cause: Cause<E>
final class MakePromise extends Async<PromiseId, Never>()
```

Today: `Async$Int32$Never`, `Async$Int32$String`, `Succeed$Int32`, … are all unrelated WASM types. Assigning `Succeed<Int32>` to `Async<Int32, String>` requires runtime synthesis.

With erasure:
- `$Async`, `$Succeed`, `$FailCause`, `$MakePromise` — one WASM struct each.
- `$Succeed <: $Async`, `$FailCause <: $Async`, `$MakePromise <: $Async` in the WASM subtype hierarchy.
- `let a: Async<Int32, String> = succeedValue` — pure no-op at the WASM level.
- `ref.test $Succeed` on an `Async` reference works correctly.

This is the immediate, concrete reason for the change. Standard-io's `Async` becomes natural to express; it's currently encoded as an enum because the class form doesn't survive monomorphization.

---

## 9. Implementation Phases

Prerequisite: [monomorphize-separation-design.md](monomorphize-separation-design.md) should land first. It cleanly separates inference (per-package, generic-aware) from monomorphize (final-pass, concrete), and is independently valuable.

### Phase 1: Erased TypeDef creation in monomorphize

- Replace per-instantiation TypeDef creation with one canonical erased TypeDef per generic def.
- Field types retain `Type::TypeParameter` for type-parameter slots.
- TypeDef mangled name uses `MangledName::for_type(fqn)`.
- `type_params` preserved on TypeDefs.
- Deduplicate: multiple instantiations map to one TypeDef entry.

Affected: `collect_type_defs_from_type` (records, enums), `instantiate_generic_classes`, `substitute.rs::apply_type_substitution`, `discover_generic_class_instances`.

After this phase, the pipeline is broken — codegen hits `unreachable!()` on `Type::TypeParameter`.

### Phase 2: Codegen for erased fields

- `type_to_valtype`: `Type::TypeParameter` → `anyref`.
- `resolve_concrete_field_type` helper.
- `RecordCreate`, `EnumCreate`, `ClassStructCreate`: box-on-write at erased positions.
- `FieldAccess`, enum-pattern payload binding, record destructuring: cast-back + unbox-on-read.
- `RecordWith` override: same box/cast as create.
- Class methods: cast-back on `self.field` access for erased fields.

After this phase, basic generic types compile, but the variance-cast pass is still active and generates broken IR (operating on the now-shared erased types).

### Phase 3: Strip variance casts

- `needs_variance_cast` returns `false` for `GenericRecord`/`GenericEnum`/`GenericClass` pairs that share an erased mangled name (i.e., all of them).
- Delete `synthesize_record_variance_cast` / `synthesize_enum_variance_cast`.
- Delete class variance-cast synthesis.
- Remove mutable-field boxing in class instantiation.
- Tests pass at this point for non-closure code.

### Phase 4: Always-erased closures (supersedes the original "function-type adapters" plan)

The original Phase 4 plan generated per-(concrete, erased) adapter closures at slot-crossing boundaries. That approach was abandoned in favor of the simpler, more uniform always-erased-closures design described in §3.2 — which also incidentally fixes function-param contravariance for class subtyping (e.g., `f: A => String` passed where `B => String` is expected, B extending A).

Concrete work:
- Add `discover_closure_arities` to walk the typed module and collect every `Type::Function` arity used.
- Pre-declare `Func_N`/`Closure_N` rec groups in the type section before user types.
- `type_to_valtype(Type::Function)` returns `(ref Closure_N)`.
- Closure body lifted to `Func_N` signature with prologue (cast each anyref param → declared type) and epilogue (box return → anyref).
- `ClosureCall` boxes args; casts the anyref result back to the declared return type.
- `RefTrampoline` (for `FunctionRef` / `MethodRef`) uses `Func_N` signature with a cast-call-box body.
- Delete `FunctionSigTypeDef`, `function_sig_collector.rs`, `canonicalize_for_function_sig`, `MangledName::for_function_type`, `closure_sig_indices`, `emit_function_sig_types`, `build_function_sig_subtypes`, and the corresponding SCC arms.
- Delete `synthesize_function_variance_cast` and the Function arm of `coerce`.

### Phase 5: Diagnostics, restrictions, and cleanup

- ✅ Rename `variance_cast.rs` → `coerce.rs`; delete `needs_variance_cast`, `coerce`,
  `vc_helpers`/`vc_params`, the helper-walk loop. What stays is trait-object coercion and
  primitive→`Any` boxing — see §5.1.
- ✅ Delete `ClassFieldDef.boxed` and its codegen branches; mut-box machinery stays for
  closure captures only. See §5.2.
- ✅ Rules-phase rejection of statics on generic classes AND generic modules that reference
  type parameters in their declared type (`dovetail/src/compiler/typechecker/rules/generic_static_rules.rs`).
- ✅ `Type::Substituted` and the `typechecking_only` flag — already removed in Feb/Mar
  2026 (commits `7f85854`, `fc0b036`). See §5.4.
- ⏭ Deferred: warning for `is`/`as`/typed patterns with non-`Any` type arguments on generic
  targets. Runtime semantics are correct (base-type check + downstream `ref.cast` traps);
  only diagnostic precision is missing. Low priority — can be picked up later.

### Phase 6: Validation

- ✅ Convert standard-io `Async` enum to a sealed class hierarchy (17 final classes
  + abstract parent in `standard-io/src/types.dove`). Update Async/Promise/Fiber/Queue/
  Awaitable/Runtime accordingly.
- ✅ Full workspace `dovetail test` (706 tests) passes.
- ✅ Integration tests covering (all in `dovetail/tests/classes.rs`):
  - Cross-instantiation class assignment (`test_sealed_class_cross_instantiation_variance_assignment`)
    — flipped from expects-failure to passing.
  - Pattern matching on erased enums (`test_phase6_nested_enum_matching`).
  - `is`/`as` with and without type arguments (`test_phase6_is_as_with_type_args`).
  - Function-typed fields at erased positions (`test_phase6_function_typed_erased_field`).
  - Recursive generic types `Tree<T>` (`test_phase6_recursive_generic_tree`).
  - Nested generics `Option<Option<T>>` (`test_phase6_nested_generics_double_box`).
- ✅ Sealed-class hierarchy smoke + companion module + zero-typeparam-subclass + record-field-child tests pass.
- ✅ 4 compiler fixes landed while validating Phase 6:
  - `infer/types.rs` — non-generic Class assignable to GenericClass (zero-typeparam child → generic parent).
  - `codegen/type_graph.rs` — force class-hierarchy edges at graph construction so Tarjan groups them into one SCC with topologically-correct surrounding emission.
  - `codegen/mod.rs` (vtable emission) — skip vtable instances whose monomorphized methods don't exist (unreachable under full erasure).
  - `monomorphize/mod.rs` (`create_concrete_class_methods`) — stop auto-monomorphizing
    companion-module functions per class instance. They're now monomorphized on-demand
    when actually called. This avoided creating `Async.run<Any, Any>` etc. for type-arg
    combinations that violate the function's `where E: Display` bound.
- `Async`/`Selectable`/`Frame` enums still exist as enums where appropriate; only the
  generic-and-variance-heavy `Async<T, E>` became a sealed-class hierarchy.

---

## 10. Trade-offs

### 10.1 What we gain

- **Cross-instantiation subtyping works.** Sealed hierarchies with type parameters compile correctly.
- **Variance casts deleted** — ~1500 lines removed from `variance_cast.rs`.
- **One TypeDef per generic def** — smaller binaries, smaller cache, simpler dependency graphs.
- **`Type::Substituted` removed** — fewer strip points throughout the pipeline.
- **One vtable layout per class hierarchy.**
- **Clean staging for reified generics** — type\_id is a strict additive extension.

### 10.2 What we pay

- **`ref.cast` + unbox on every read** of an erased field. Hot-path cost. Mitigated by monomorphized functions knowing the concrete type — no runtime type-info needed for the cast.
- **Primitive boxing on writes** to erased slots. Extra allocation, more GC pressure.
- **Reduced runtime type discrimination** on `Any`. `is`/`as` on generic instantiations check base type only. Compiler warning; reified generics restore precision later.
- **Closure body prologue/epilogue** on every closure invocation (§3.2). Primitive args/returns box and unbox at the call boundary; reference types pay just a `ref.cast`. Significant for hot numerical kernels using closures-over-primitives; negligible for business logic / async / WASI workloads. No per-write/read adapter allocation — the prologue lives once in the body, not once per slot crossing.

### 10.3 What stays the same

- Generic function declarations are monomorphized.
- Trait-bound resolution is compile-time.
- Locals/params inside generic functions use native WASM types — no boxing for in-body uses.
- `is`/`as`/match on type parameters inside generic functions resolves at compile time.
- Arrays of primitives stay specialized per element type.
- Tuples unchanged.
- Trait objects unchanged.
- Type inference, assignability, variance-position checks — unchanged.

---

## 11. Open Items

- **Recursive generic types**: `class Node<T>(val next: Node<T>)`. The `next` field's TypeDef type is `GenericClass { mangled_name: $Node, type_args: [TypeParameter("T")] }`. `type_to_valtype` returns the erased `$Node` struct directly — **no anyref, no cast-back** for the recursive reference. Confirm this works through `resolve_concrete_field_type` (which would substitute `T` and return `GenericClass { type_args: [Int32] }`, still mapping to `$Node` via the erased mangled name).
- **Field index drift across monomorphize vs. codegen for vtables.** Verify the vtable field ordering is consistent when only one vtable layout exists per hierarchy.
- **Cross-package cache invariants.** With erased TypeDefs, a downstream package sees a single TypeDef for `Option`. Existing cache invalidation should already handle this, but verify nothing keys on per-instantiation TypeDef counts.
- **LSP impact.** Hover on `Option<Int32>` vs `Option<String>` should still distinguish them via expression `type_args`, but go-to-definition lands on a single TypeDef. Acceptable — this matches user mental model.
