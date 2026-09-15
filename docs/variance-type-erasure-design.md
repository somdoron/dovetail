# Selective Type Erasure for Variance Parameters

**Status:** Design
**Supersedes:** [reified-generics-types-only-research.md](reified-generics-types-only-research.md) (full erasure + type\_id approach -- rejected due to hot-path overhead)

---

## 1. Problem

Dovetail supports variance annotations on generic type parameters:

```dovetail
enum Option<out T> = None | Some(T)
enum Result<out T, out E> = Ok(T) | Err(E)
sealed abstract class Async<out T, out E>
```

The `out` (covariant) annotation means `Option<Int32> <: Option<Any>` whenever `Int32 <: Any`. The `in` (contravariant) annotation reverses the direction.

Today, variance is implemented by **monomorphization + variance casts**:

1. **Monomorphization:** Each concrete instantiation (`Option<Int32>`, `Option<String>`) becomes a separate WASM struct type with concrete field types.
2. **Variance casts:** At every coercion point where actual and expected types share the same generic FQN but differ in type arguments, the `variance_cast.rs` pass (~1200 lines) synthesizes runtime code that deconstructs the source value, boxes/unboxes payloads, and reconstructs a new value with the target type's WASM struct.

This approach has two critical problems:

- **Cross-instantiation subtyping breaks at WASM level.** `Option$Int32` and `Option$String` are unrelated WASM struct types. A `ref.test` on one never matches the other. This means sealed class hierarchies like `final class Succeed<out T> extends Async<T, Never>()` cannot be assigned to `Async<T, String>` despite the Dovetail type system allowing it -- WASM-GC has no subtype relationship between `Async$T$Never` and `Async$T$String`.

- **Variance cast pass is complex and fragile.** The deep deconstruct/reconstruct synthesis handles records, enums, classes, and functions, with special cases for recursive enums, boxed fields, and nested variance casts. It is the single most complex pass in the typechecker.

---

## 2. Design: Selective Erasure

### 2.1 Core idea

Erase **only variance-marked type parameters** (`out` and `in`) to `(ref any)` in WASM struct layouts. Invariant type parameters remain monomorphized with concrete WASM types.

This means all instantiations of a generic type that differ **only** in their variance parameters share the **same WASM struct type**. Variance casts between such types become true no-ops -- no copying, no runtime code.

### 2.2 What is erased

A type parameter is erased if and only if its declaration carries a variance annotation (`out` or `in`). An unannotated (invariant) parameter is monomorphized as today.

| Declaration | Erased params | Monomorphized params |
|-------------|---------------|----------------------|
| `Option<out T>` | `T` | -- |
| `Result<out T, out E>` | `T`, `E` | -- |
| `Pair<A, B>` | -- | `A`, `B` |
| `MutableRef<T, out E>` | `E` | `T` |

### 2.3 Erased mangled names

Today, `MangledName::for_generic_type(fqn, type_args)` produces names like `Option$Int32`. With selective erasure, variance type args are **dropped** from the mangled name entirely:

```
Option<Int32>  -> WASM type: Option          (T is covariant -> dropped)
Option<String> -> WASM type: Option          (same WASM type)
Option<Bool>   -> WASM type: Option          (same WASM type)

MutableRef<Int32, String> -> WASM type: MutableRef$Int32  (T concrete, E dropped)
MutableRef<Int32, Bool>   -> WASM type: MutableRef$Int32  (same WASM type)
MutableRef<Bool, String>  -> WASM type: MutableRef$Bool   (different -- T differs)
```

The typed AST uses erased mangled names after monomorphize. Only **variance** `type_params` are preserved on TypeDefs and only **variance** `type_args` are preserved on expressions (invariant params are already fully substituted in field types and baked into the mangled name). This enables codegen to know the actual types for cast-back-on-read via `resolve_concrete_field_type`, and supports the future type-ID extension.

### 2.4 Struct layout

For an erased type parameter position, the WASM struct field is `(ref null any)` (or `(ref any)` if non-nullable). For an invariant parameter, the field retains its concrete WASM type.

```
// Option<out T>:
struct $Option {
    // No type_id field -- not needed
    field 0: (ref any)    // T slot (erased)
}

// Pair<A, B> (invariant):
struct $Pair$Int32$String {
    field 0: i32          // A = Int32 (concrete)
    field 1: (ref $String) // B = String (concrete)
}

// MutableRef<T, out E>:
struct $MutableRef$Int32 {
    field 0: i32          // T = Int32 (concrete)
    field 1: (ref any)    // E (erased)
}
```

### 2.5 Box on write

When storing a value into an erased slot:

- **Primitive** (`Int32`, `Bool`, `Float64`, etc.): box it first using existing boxing machinery (`struct.new $Int32Box`), then store the box ref as `(ref any)`.
- **Reference type** (`String`, record, enum, class, etc.): store directly -- upcasts implicitly to `(ref any)` in WASM-GC.

### 2.6 Cast back on read

When reading from an erased slot, the value on the WASM stack is `(ref any)`. To cast it back, codegen needs the **concrete type** that the slot actually holds. The mechanism:

**TypeDef field types preserve type parameters.** Variance-marked type parameters are **not** substituted in the TypeDef's field types — they remain as `Type::TypeParameter`. Only invariant type parameters are substituted with their concrete types. For example, `Option<out T>` with variant `Some { payload: T }` keeps the field type as `Type::TypeParameter("T")` in the erased TypeDef.

**`resolve_concrete_field_type` helper.** Codegen resolves the concrete type by substituting the preserved type parameters with the expression's concrete `type_args`:

```rust
fn resolve_concrete_field_type(
    field_type: &Type,                   // from TypeDef (may contain TypeParameter)
    type_params: &[TypeParamName],       // from TypeDef
    type_args: &[(Variance, Type)],      // from expression's type (concrete)
) -> Type {
    // Build substitution: TypeParamName -> concrete Type
    let sub: BTreeMap<TypeParamName, Type> = type_params.iter()
        .zip(type_args.iter())
        .map(|(name, (_, concrete))| (name.clone(), concrete.clone()))
        .collect();
    substitute(field_type, &sub)
}
```

This same helper works for all erased positions: record fields, enum payloads, class fields, and function params/returns. It also composes correctly for nested types — e.g., a field of type `Array<T>` resolves to `Array<Int32>` when `T = Int32`.

**Read sequence:**

1. `struct.get` -> gets `(ref any)`.
2. Call `resolve_concrete_field_type` to determine the expected concrete type.
3. `ref.cast` to the expected concrete type (box struct for primitives, concrete struct for reference types).
4. If primitive: `struct.get` field 0 on the box to unbox.

```wasm
;; Reading field 'value' from Option<Int32> (erased T)
;; resolve_concrete_field_type(TypeParameter("T"), [T], [(Covariant, Int32)]) = Int32
struct.get $Option, 0     ;; -> (ref any)
ref.cast (ref $Int32Box)  ;; -> (ref $Int32Box)
struct.get $Int32Box, 0   ;; -> i32
```

```wasm
;; Reading field 'value' from Option<String> (erased T)
;; resolve_concrete_field_type(TypeParameter("T"), [T], [(Covariant, String)]) = String
struct.get $Option, 0     ;; -> (ref any)
ref.cast (ref $String)    ;; -> (ref $String)
```

**For WASM struct layout emission**, `type_to_valtype` (in `codegen/mod.rs`) currently has `Type::TypeParameter => unreachable!()`. With erasure, this arm will match any `TypeParameter` to `anyref` — the same `ValType` as `Type::Any`. This single change makes all struct/enum/class layout emission work automatically for erased fields.

### 2.7 Closure / function types at erased positions

There are two cases for closures at erased positions:

**Case 1: Field type is just `TypeParameter("T")` and the concrete type happens to be a function type.**

This is the simple case. The field is `anyref` in WASM. On write, the closure reference upcasts to `anyref`. On read, `ref.cast` to the concrete closure struct type works because the actual closure matches the expected type. No wrapping needed.

**Case 2: Field type is a function type that _contains_ a variance type parameter (e.g., `(String) => T`, `(T) => Unit`).**

This is the complicated case. The function type must be **erased** — variance type parameters in the function signature are replaced with `Any`. The WASM struct field is typed as the erased function type, not as `anyref`.

The problem: without erasure, `Processor<String>` stores a closure `(Int32) => String` and `Processor<Int32>` stores `(Int32) => Int32`. These are different WASM function types. After a variance coercion (`Processor<String>` → `Processor<Any>`), the reader expects `(Int32) => Any` but the slot holds `(Int32) => String` — a `ref.cast` would fail.

The solution: **erase the function type itself** and generate **wrapper closures** on write and read.

```dovetail
class Processor<out T>
    val transform: (Int32) => T
```

TypeDef field type: `Function([Int32], TypeParameter("T"))`.
WASM field type: substitute TypeParameter → Any → `(ref $Closure_Int32_Any)`.

**On write** — wrap concrete closure to match erased function type:

```
// Storing a (Int32) => String closure into the erased (Int32) => Any slot:
//
// Generate wrapper closure that:
//   1. Takes Int32 param (unchanged)
//   2. Calls original closure → gets String result
//   3. Upcasts/boxes result to Any
//   4. Returns Any
//
// Store the wrapper (type $Closure_Int32_Any) in the struct field.
```

**On read** — wrap erased closure to match concrete function type:

```
// Reading from the erased slot, expecting (Int32) => String:
//
// Generate wrapper closure that:
//   1. Takes Int32 param (unchanged)
//   2. Calls erased closure → gets Any result
//   3. Downcasts/unboxes result to String (ref.cast)
//   4. Returns String
//
// Return the wrapper (type $Closure_Int32_String) to the caller.
```

**Contravariant parameters** work in reverse — the wrapping direction for params is opposite to returns:

```dovetail
class Consumer<in T>
    val consume: (T) => Unit
```

TypeDef field type: `Function([TypeParameter("T")], Unit)`.
WASM field type: `(ref $Closure_Any_Unit)`.

- **On write**: wrapper takes `Any`, downcasts to the concrete param type, calls original.
- **On read**: wrapper takes the concrete param type, upcasts to `Any`, calls erased.

**Implementation notes:**

- **Existing machinery:** `synthesize_function_variance_cast` in `variance_cast.rs` already generates wrapper closures for function variance casts — it captures the original, coerces params contravariantly and return covariantly. The erasure wrappers follow the same pattern.
- **Two wrappers per erased function field:** For every closure field at a variance position, we generate two wrappers:
  1. **Write wrapper** (concrete → erased): e.g., `(Int32) => String` → `(Int32) => Any` — boxes/upcasts the return.
  2. **Read wrapper** (erased → concrete): e.g., `(Int32) => Any` → `(Int32) => String` — downcasts/unboxes the return.
- **Where to generate:** These wrappers can be generated in the **variance cast phase** (which already has the infrastructure) or in the **monomorphize phase**. The variance cast phase is a natural fit since it already walks expressions and synthesizes function wrappers. It would be extended to detect function-typed fields at variance positions and insert wrap/unwrap closures at write/read sites.
- **Deterministic mangled names:** Each wrapper function gets a deterministic `MangledName` computed from the concrete and erased function types, e.g. `$closure_wrap(Int32$String→Int32)` for the write wrapper and `$closure_unwrap(Int32→Int32$String)` for the read wrapper. This means codegen can derive the wrapper's mangled name directly from the field's TypeDef type and the expression's concrete `type_args` — no searching or lookup tables needed. If the wrapper for that pair already exists, it is reused.
- **`FunctionSigTypeDef` for erased types:** Monomorphize must ensure the erased function type (e.g., `(Int32) => Any`) has a corresponding `FunctionSigTypeDef` and WASM closure struct.
- **Closure captures at erased positions:** If a closure captures a variable whose type involves a variance parameter, and that capture is stored in a closure struct, the capture field is `anyref` with cast-back-on-read (same as any other erased field).

### 2.8 Mutable class fields no longer need boxing

Today, mutable fields on generic classes with variance parameters are boxed (`field.boxed = true`) so that variance casts can share the box reference across copies -- mutating the box in one copy is visible in the other. This boxing exists solely to support the deep-copy variance cast mechanism.

With erasure, variance casts are **no-ops** -- there is no copying, so there is no need to share mutable state across copies. Mutable class fields at erased positions are stored directly as `anyref` (with primitive boxing as described in 2.5), not wrapped in an extra mutable box struct. This removes one layer of indirection and allocation.

### 2.9 Variance casts become no-ops

Since `Option<Int32>` and `Option<Any>` share the same WASM struct type, a variance coercion between them requires **no runtime code**. The reference is bit-for-bit identical. The `variance_cast.rs` pass can be simplified:

- **Records, enums, classes** where the only type-arg differences are in erased (variance) positions -> **skip entirely** (no-op). Just relabel the type in the typed AST.
- **Functions** with variance in return/param types that are themselves erased generic types -> also no-op at the WASM level.
- **Function variance casts** where the param/return types are structurally different (e.g., different concrete types in invariant positions) -> still needed, but rare.
- **Trait object coercion** and **boxing to Any** -> unchanged.

### 2.10 `is` / `as` / type-annotated patterns

With erasure, `ref.test` cannot distinguish `Option<Int32>` from `Option<String>` -- they share the same WASM struct type. Therefore:

- **Runtime behavior:** `is`, `as`, and type-annotated patterns on types with variance parameters check **only the base type identity** (e.g., "is this an Option?"), not the type arguments.
- **Type safety:** This is sound because variance allows the type system to substitute compatible types. If you hold an `Option<Int32>` and test `is Option<String>`, the test succeeds (both are `Option` at runtime), but subsequent field access will `ref.cast` to the expected type, which may trap if the value is incompatible. However, in well-typed programs this cannot happen -- the type checker ensures compatibility before the code reaches this point.

### 2.11 Static variables restriction

Static variables (module-level globals and class-level statics) on generic types with variance parameters must not reference erased type parameters in their types. With erasure, all instantiations share one WASM type, so there is only **one** WASM global per static declaration -- but semantically each instantiation expects its own independent global.

```dovetail
class Cache<out T>
    static var instance: Option<T> = None   // error: static variable type uses
                                            //    erased type parameter T
    static var count: Int32 = 0             // OK: type does not involve T
```

The compiler should reject static variable declarations whose types reference any variance-annotated type parameter. This is enforced in the rules phase alongside existing variance position checks.

### 2.12 Compiler warning for erased parameter discrimination

The compiler should emit a warning when the user writes an `is`/`as`/type-annotated pattern where the target type has variance parameters with specific (non-`Any`) type arguments, since these are not checked at runtime:

```dovetail
function check(x: Any): Bool =
    x is Option<Int32>  // Warning: covariant type parameter T is not checked at runtime.
                        //   This test only verifies that x is an Option.
```

The warning is suppressed when the erased params are `Any` (the user is explicitly testing only the base type).

### 2.13 Class hierarchies with variance

This design solves the sealed-class cross-instantiation problem:

```dovetail
sealed abstract class Async<out T, out E>
final class Succeed<out T> extends Async<T, Never>()
    val value: T
final class FailCause<out E> extends Async<Never, E>()
    val cause: Cause<E>
final class MakePromise extends Async<PromiseId, Never>()
```

With erasure:
- `Async` has one WASM struct type: `$Async` (both `T` and `E` erased)
- `Succeed` has one WASM struct type: `$Succeed` (subtypes `$Async`)
- `FailCause` has one WASM struct type: `$FailCause` (subtypes `$Async`)
- `MakePromise` has one WASM struct type: `$MakePromise` (subtypes `$Async`)

The WASM subtype hierarchy matches the Dovetail class hierarchy exactly. `ref.test $Succeed` on an `Async` reference works correctly. Assigning `Succeed<Int32>` to `Async<Int32, String>` is a WASM no-op -- same `$Succeed` struct, same `$Async` supertype.

---

## 3. Changes by Compiler Phase

### 3.1 Key design decision: create erased types directly in monomorphize

Instead of creating fully concrete TypeDefs and then erasing them in a post-pass, the monomorphize phase creates **erased TypeDefs directly** when instantiating generic templates with variance parameters.

When monomorphize encounters a template like `Option<out T>` and needs to create a concrete TypeDef for `Option<Int32>`:

- **Without erasure (old):** substitute `T -> Int32`, create `Option$Int32` with field type `Int32`
- **With erasure (new):** do **not** substitute variance-marked params in field types — keep them as `TypeParameter("T")`. Compute the erased mangled name by dropping variance params (e.g., `Option` instead of `Option$Int32`). Codegen maps `TypeParameter` to `anyref` via `type_to_valtype`.

Only **variance** `type_params` are preserved on the TypeDef (invariant params are dropped — they are already substituted in field types and in the mangled name). Expressions retain only **variance** `type_args` (the concrete types for the erased positions). This keeps the information needed for cast-back-on-read and the future type-ID extension (see [selective-erasure-type-id-extension.md](selective-erasure-type-id-extension.md)).

**What the monomorphize output looks like:**

```
// TypeDef for Option (erased -- one entry, not N):
TypeDef::Enum {
    fqn: prelude.Option,
    mangled_name: "prelude.Option",       // variance params dropped from name
    type_params: [T],              // only variance params kept
    variants: [
        None { payload_types: [] },
        Some { payload_types: [TypeParameter("T")] },  // type param preserved, NOT Any
    ],
}

// Expression in a function body:
TypedExprKind::EnumCreate {
    fqn: prelude.Option,
    variant_name: "Some",
    args: [<expr with ty=Int32>],
    type_args: [Int32],            // only variance type args kept
}
// expr.ty = Type::GenericEnum {
//     fqn: prelude.Option,
//     mangled_name: "prelude.Option",        // erased mangled name (variance params dropped)
//     type_args: [(Covariant, Int32)],       // only variance type args
// }
```

This gives codegen everything it needs:
- `mangled_name` -> look up the WASM type index (erased, shared across instantiations)
- `type_args` on expressions (variance only) -> concrete types for `resolve_concrete_field_type` on reads and boxing on writes
- `type_params` on TypeDef (variance only) + `TypeParameter` in field types -> know which fields are erased; `resolve_concrete_field_type(field_type, type_params, type_args)` is a direct 1:1 zip to recover concrete types (future: which slots get type-IDs)
- WASM layout: any `TypeParameter` in a field type → `anyref`

### 3.2 Typechecker -- minimal changes

The typechecker continues to work with fully concrete types. Inference, subtyping, and assignability are unchanged. Changes:

- **Warning pass (new):** After inference, check `is`/`as`/type-annotated patterns where the target type is a generic with variance params and the type args are specific. Emit a warning.
- **Static variable restriction (new):** In the rules phase, reject static variable declarations on generic types whose types reference any variance-annotated type parameter.

### 3.3 Monomorphize -- erased type creation

This is where the core erasure logic lives. Changes to existing monomorphize functions:

**`collect_new_type_defs` / `collect_type_defs_from_type`:**

When instantiating a concrete TypeDef from a template:

1. Look up the template's `type_param_variances` from the registry
2. Build the substitution map: for **invariant** params, substitute with the concrete type; for **variance-marked** params, **do not substitute** — leave them as `Type::TypeParameter` in field types
3. Compute the erased mangled name (variance args dropped from the name)
4. Create the TypeDef with the erased mangled name but field types that still contain type parameters for erased positions
5. Set `type_params` to only the **variance** params (drop invariant params from the list)

```rust
// In collect_type_defs_from_type, GenericRecord branch:
let variances = registry.lookup_type_param_variances(fqn);

// Substitution for field types: only substitute invariant params
let field_sub: BTreeMap<TypeParamName, Type> = tmpl.type_params.iter()
    .zip(type_args.iter().zip(variances.iter()))
    .filter(|(_, (_, variance))| **variance == Variance::Invariant)
    .map(|(tp, ((_, concrete_ty), _))| (tp.clone(), concrete_ty.clone()))
    .collect();
// Apply field_sub to template field types — variance params remain as TypeParameter

let erased_mn = compute_erased_mangled_name(fqn, type_args, &variances);

// Keep only variance type_params on the TypeDef
let variance_type_params: Vec<TypeParamName> = tmpl.type_params.iter()
    .zip(variances.iter())
    .filter(|(_, v)| **v != Variance::Invariant)
    .map(|(tp, _)| tp.clone())
    .collect();
// Create TypeDef with erased mangled name, partially-substituted field types,
// and only variance type_params
```

**`instantiate_generic_classes`:**

Same approach for classes. When creating a concrete `ClassTypeDef`, only invariant params are substituted in field types; variance-marked fields stay as `Type::TypeParameter`. The erased mangled name is used. Parent class resolution uses erased names. Only variance `type_params` are kept.

**Type rewriting in expressions:**

When `substitute_types_in_expr` / `substitute_types_in_function` rewrites types in function bodies, the `mangled_name` on `Type::GenericRecord/Enum/Class` becomes the erased version, and `type_args` are filtered to only variance params with their concrete types:

```rust
// After substitution:
Type::GenericEnum {
    fqn: prelude.Option,
    mangled_name: MangledName("prelude.Option"),       // erased (variance params dropped)
    type_args: vec![(Covariant, Type::Int32)],         // only variance type args
}
```

This ensures `type_indices[mangled_name]` finds the right (shared, erased) WASM type, while codegen can read `type_args` to know the actual type for casts. The `type_args` directly correspond 1:1 to the TypeDef's `type_params` (both contain only variance params), making `resolve_concrete_field_type` a straightforward zip.

**`MangledName` computation for erased types:**

A helper function computes the erased mangled name by dropping variance params:

```rust
fn compute_erased_mangled_name(
    fqn: &Fqn,
    type_args: &[(Variance, Type)],
    variances: &[Variance],
) -> MangledName {
    let invariant_args: Vec<&Type> = type_args.iter()
        .zip(variances.iter())
        .filter(|(_, v)| **v == Variance::Invariant)
        .map(|((_, ty), _)| ty)
        .collect();
    if invariant_args.is_empty() {
        MangledName::for_type(fqn)  // no type args in name at all
    } else {
        MangledName::for_generic_type(fqn, &invariant_args)
    }
}
```

### 3.4 Variance cast pass -- simplification

With erased mangled names, `needs_variance_cast` can early-exit: when actual and expected types share the same erased mangled name (which they always do when only variance params differ), the cast is a **no-op**. No deep deconstruct/reconstruct synthesis needed.

What remains:
- Trait object coercion (unchanged).
- Boxing primitives to `Any` (unchanged).
- **Function-typed fields at variance positions** require wrapper closures on write and read (see §2.7). The variance cast pass (or monomorphize) generates these wrappers — concrete→erased on write, erased→concrete on read — because different instantiations produce closures with different WASM function signatures that must be normalized to the erased signature.
- **Standalone function variance casts** (e.g., `(Int32) => Result<Int32, Never>` assigned to `(Int32) => Result<Int32, String>`) — with erasure, `Result<Int32, Never>` and `Result<Int32, String>` share the same WASM type, so the return types match and this cast also becomes a **no-op**. The only function casts that remain are those with structural differences in invariant positions.

### 3.5 Codegen -- construction

When constructing a generic value (`RecordCreate`, `EnumCreate`, `ClassStructCreate`):

- For each field, check if its TypeDef field type contains a `TypeParameter` (i.e., the WASM field is `anyref`):
  - The arg expression already has a concrete type — no need for `resolve_concrete_field_type`
  - If the arg is a primitive -> box it (`struct.new $PrimitiveBox`), store as `(ref any)`
  - If the arg is a reference type -> store directly (implicit upcast to `(ref any)`)
- For fields whose TypeDef types are fully concrete (no `TypeParameter`) -> unchanged.

### 3.6 Codegen -- field access

When accessing a field at an erased position (TypeDef field type contains `TypeParameter`):

- `struct.get` returns `(ref any)`
- Call `resolve_concrete_field_type(field_type, type_params, type_args)` to determine the expected concrete type
- Emit `ref.cast` to the expected concrete type
- If primitive -> `struct.get` field 0 to unbox

For invariant/concrete fields -> unchanged (direct `struct.get`).

### 3.7 Codegen -- `is` / `as` / type-annotated match

Since the `mangled_name` on types is already erased, `wasm_type_index_for_any_cast` naturally returns the shared type index. No special logic needed -- `ref.test` checks the base type identity.

### 3.8 Codegen -- match (enum variants, record destructuring)

Enum variant matching: all instantiations share the same variant WASM type (erased mangled name) -> `ref.test` works across instantiations. Payload bindings at erased positions: `struct.get` -> `ref.cast` (using `resolve_concrete_field_type` with the expression's `type_args`) -> unbox if primitive.

Record destructuring: same pattern -- erased fields get `ref.cast` + unbox.

---

## 4. Detailed Examples

### 4.1 Option -- fully erased

```dovetail
enum Option<out T> = None | Some(T)

let x: Option<Int32> = Some(42)
let y: Option<Any> = x            // no-op (same WASM type)

match x with
    case Some(v) => v + 1          // struct.get -> ref.cast $Int32Box -> struct.get -> i32
    case None => 0
```

WASM struct layout:
```
struct $Option {}                           // base (empty)
struct $None   <: $Option {}                // no payload
struct $Some   <: $Option { f0: anyref }    // erased T
```

Construction of `Some(42)`:
```wasm
i32.const 42
struct.new $Int32Box    ;; box the primitive
struct.new $Some        ;; store boxed ref as anyref
```

Reading the payload:
```wasm
ref.cast $Some              ;; narrow to Some variant
struct.get $Some, 0         ;; -> anyref
ref.cast (ref $Int32Box)    ;; -> (ref $Int32Box)
struct.get $Int32Box, 0     ;; -> i32
```

### 4.2 Sealed class hierarchy -- cross-instantiation

```dovetail
sealed abstract class Async<out T, out E>
final class Succeed<out T> extends Async<T, Never>()
    val value: T
```

WASM struct layout:
```
struct $Async   { f0: ref $AsyncVtable }
struct $Succeed <: $Async { f0: ref $SucceedVtable, f1: anyref }  // value: T (erased)
```

```dovetail
let s: Succeed<Int32> = Succeed(42)
let a: Async<Int32, String> = s     // WASM no-op! Same $Succeed struct, $Succeed <: $Async.
```

Before erasure, this assignment would fail at the WASM level because `Async$Int32$Never` and `Async$Int32$String` were unrelated types. With erasure, there is only one `$Async` type and `$Succeed` is always its subtype.

### 4.3 Mixed variance and invariant

```dovetail
record MutableRef<T, out E> = ref: T, error: E
```

WASM struct layout (per invariant param `T`):
```
struct $MutableRef$Int32  { f0: i32, f1: anyref }    // T=Int32 concrete, E erased
struct $MutableRef$String { f0: ref $String, f1: anyref }  // T=String concrete, E erased
```

```dovetail
let m: MutableRef<Int32, String> = MutableRef(42, "err")
let m2: MutableRef<Int32, Any> = m   // no-op (same $MutableRef$Int32 WASM type)
```

---

## 5. What Doesn't Change

- **Functions remain monomorphized.** Each specialized function knows the concrete types at compile time and emits the right cast-back/box sequences. No type-info parameters.
- **Arrays remain monomorphized.** `Array<T>` is always invariant, so arrays keep their efficient unboxed layout.
- **Trait objects** -- unchanged.
- **Primitive boxing to `Any`** in non-generic contexts -- unchanged.
- **Variance position checking** in the rules phase -- unchanged.
- **Subtyping / assignability checking** in the typechecker -- unchanged.
- **Type inference** -- unchanged.

---

## 6. Trade-offs

### 6.1 What we gain

- **Cross-instantiation variance works.** The primary motivation. Sealed class hierarchies with specialized type parameters compile correctly. `Async<Int32, Never>` is assignment-compatible with `Async<Int32, String>` at the WASM level.
- **Variance casts become no-ops.** No runtime copying for record/enum/class variance coercions. The ~1200-line deep synthesis in `variance_cast.rs` for records/enums/classes can be replaced with type relabeling.
- **Fewer WASM type definitions.** All instantiations differing only in variance params share one WASM type. Reduces binary size for programs with many instantiations.
- **Simpler class hierarchy encoding.** One rec group per class hierarchy regardless of type parameter instantiations.

### 6.2 What we pay

- **Cast back on every read** of an erased field: `ref.cast` (+ unbox for primitives). This is a hot-path cost, but only for fields at variance type parameter positions, not all fields.
- **Boxing primitives** in erased slots: extra allocation on construction, more GC pressure. Reference types pay nothing extra.
- **Reduced runtime type discrimination:** `is`/`as` on types with variance params cannot distinguish instantiations. This is sound but less precise than monomorphized `ref.test`.
- **No type\_id:** Unlike the rejected full-erasure approach, we don't embed type IDs. This means no runtime type-parameter checking. This is a deliberate choice -- WASM-GC's `ref.test` cannot help with phantom/erased types anyway, so runtime variance checking would require a fully custom codegen implementation regardless.

### 6.3 Comparison with full erasure (previous research)

| Aspect | Full erasure (rejected) | Selective erasure (this design) |
|--------|------------------------|--------------------------------|
| Erased params | All type params | Only variance (`out`/`in`) params |
| Invariant fields | `(ref any)` + cast on read | Concrete WASM type (no cast) |
| type\_id field | Yes (i32 per struct) | No |
| `is`/`as` precision | Can distinguish instantiations via type\_id | Cannot distinguish erased params; warns user |
| Variance cast | Shallow copy (new type\_id) | True no-op (same WASM struct) |
| Hot-path cost | Cast on every generic field read | Cast only on variance-param field reads |
| Complexity | Medium | Medium |

The selective approach is strictly better on the hot path: invariant fields retain direct access, and variance casts are free instead of requiring shallow copies.

### 6.4 Future extension: type-ID and subtype sets

If runtime type-parameter checking becomes necessary, a type\_id (`i32`, first field) combined with compile-time pre-computed subtype sets can restore full runtime discrimination for erased parameters -- including variance-aware checks. See [selective-erasure-type-id-extension.md](selective-erasure-type-id-extension.md) for the full design. This extension is compatible with the selective erasure layout and can be added incrementally. The preserved `type_params` on TypeDefs and `type_args` on expressions (from this design) provide all the information needed for this extension without retroactive changes.

---

## 7. Implementation Plan

Erasure is integrated into the monomorphize phase (see [monomorphize-separation-design.md](monomorphize-separation-design.md)), not a separate post-pass.

### Phase 1: Erased type creation in monomorphize

**Goal:** Monomorphize produces erased TypeDefs and erased mangled names. TypeDef field types keep `TypeParameter` for variance params.

**1.1** Add `compute_erased_mangled_name` helper to `monomorphize/mod.rs`:
  - Takes `fqn`, `type_args`, `variances`; drops variance args from the mangled name.
  - Uses `MangledName::for_type(fqn)` when all params are variance, `MangledName::for_generic_type(fqn, &invariant_args)` otherwise.

**1.2** Modify `collect_type_defs_from_type` — `GenericRecord` branch (line ~1504):
  - Look up variances from registry.
  - Build substitution with only invariant params; leave variance params as `TypeParameter`.
  - Use `compute_erased_mangled_name` instead of the incoming `mangled_name`.
  - Set `type_params` to only variance params (not `vec![]`).
  - Deduplicate: check `existing`/`new_types` by erased mangled name (multiple instantiations map to one TypeDef).

**1.3** Modify `collect_type_defs_from_type` — `GenericEnum` branch (line ~1542):
  - Same changes as 1.2 for enums.

**1.4** Modify `instantiate_generic_classes` (line ~423):
  - Modify substitution in `substitute_types_in_class_type_def` call: only substitute invariant params.
  - Use `compute_erased_mangled_name` for the concrete class mangled name.
  - Set `type_params` on the resulting `ClassTypeDef` to only variance params.
  - Update parent class resolution to use erased mangled names.
  - Deduplicate: skip ClassTypeDef creation if erased mangled name already exists.

**1.5** Modify type rewriting in expressions (`substitute.rs` — `apply_type_substitution`):
  - For `Type::GenericRecord/Enum/Class`: compute erased mangled name, filter `type_args` to only variance params.
  - This affects all expression types throughout the typed AST after monomorphize.

**1.6** Modify `discover_generic_class_instances` (line ~866):
  - When discovering instances from `Type::GenericClass` in expressions, use erased mangled names as keys so multiple instantiations that share the same erased name don't produce duplicate work.

**Tests will NOT pass** after this phase. Codegen will hit `unreachable!()` on `Type::TypeParameter` in `type_to_valtype`. Phase 2 is required to restore a working pipeline.

### Phase 2: Codegen — construction, field access, and matching

**Goal:** Codegen handles `TypeParameter` in TypeDef field types. Box/cast at erased positions.

**2.1** `type_to_valtype` in `codegen/mod.rs` (line ~1236):
  - Change `Type::TypeParameter(_, _) => unreachable!()` to return `anyref`.

**2.2** Add `resolve_concrete_field_type` helper to `codegen/mod.rs`:
  - Takes `field_type`, `type_params` (from TypeDef), `type_args` (from expression's type).
  - Builds substitution map, applies to field_type, returns concrete type.

**2.3** Record construction in `codegen/function_emitter/expressions.rs` — `RecordCreate`:
  - For each field, check if TypeDef field type contains `TypeParameter`.
  - If erased and arg is primitive → emit `struct.new $PrimitiveBox` before `struct.new`.
  - If erased and arg is reference → store directly (implicit upcast to `anyref`).

**2.4** Enum construction in `codegen/function_emitter/expressions.rs` — `EnumCreate`:
  - Same boxing logic for erased variant payload positions.

**2.5** Class construction in `codegen/function_emitter/expressions.rs` — `ClassStructCreate`:
  - Same boxing logic for erased class field positions.

**2.6** Field access in `codegen/function_emitter/expressions.rs` — `FieldAccess`:
  - After `struct.get`, if the TypeDef field type contains `TypeParameter`:
    - Call `resolve_concrete_field_type` to get the concrete type.
    - Emit `ref.cast` to the concrete type.
    - If primitive → emit `struct.get` on box to unbox.

**2.7** Enum match in `codegen/function_emitter/match_expression.rs`:
  - When binding payload variables from erased positions:
    - After `struct.get`, emit `ref.cast` + unbox using `resolve_concrete_field_type`.

**2.8** Record destructuring in `codegen/function_emitter/match_expression.rs`:
  - Same cast-back logic for erased record fields.

**2.9** Struct layout emission (`codegen/records.rs`, `codegen/enums.rs`, `codegen/classes.rs`):
  - These already call `type_to_valtype` per field — the change in 2.1 makes them automatically emit `anyref` for `TypeParameter` fields. Verify this works.

**Tests will NOT pass** after this phase alone. The variance cast pass will generate broken synthesized code — it looks up erased TypeDefs (where field types are `TypeParameter`) and creates expressions with those non-concrete types. The `field.boxed` logic also conflicts with the new erasure handling. Phase 3 is required.

### Phase 3: Simplify variance casts and remove mutable field boxing

**Goal:** Remove the deconstruct/reconstruct variance cast machinery for types that now share erased WASM types. Remove mutable field boxing. This phase is required before tests pass.

**3.1** Simplify `needs_variance_cast` in `variance_cast.rs` (line ~714):
  - For `GenericRecord`, `GenericEnum`, `GenericClass`: if both sides have the same `mangled_name` (erased), return `false` — no cast needed, just relabel the type.

**3.2** Remove `synthesize_record_variance_cast` and `synthesize_enum_variance_cast` for erased types:
  - These deep deconstruct/reconstruct functions become dead code once `needs_variance_cast` returns false for erased types.
  - Keep `synthesize_class_variance_cast` only for classes with invariant param differences (if any).
  - Keep `synthesize_function_variance_cast` — still needed for standalone function casts with structural invariant differences.

**3.3** Remove mutable field boxing in `instantiate_generic_classes` (line ~519):
  - Remove the `needs_boxing` / `field.boxed = true` logic for classes with variance params (see §2.8).
  - Remove corresponding codegen logic that creates and reads through box structs for `boxed` fields.

**3.4** Add function closure wrappers for erased function-typed fields (see §2.7):
  - Generate write wrappers (concrete→erased) and read wrappers (erased→concrete) for function-typed fields at variance positions.
  - Use deterministic mangled names for wrapper functions.
  - Ensure erased `FunctionSigTypeDef`s are created in monomorphize.

**Tests SHOULD pass** after this phase. This is the first point where the full pipeline is consistent: monomorphize produces erased types, codegen handles them, and variance casts are no-ops for erased types.

### Phase 4: Warnings and restrictions

**Goal:** Add compile-time diagnostics for erasure edge cases.

**4.1** Compiler warning for erased type discrimination:
  - In the typechecker rules/warning pass, detect `is`/`as`/type-annotated patterns where the target type has variance params with specific (non-`Any`) type args.
  - Emit a warning that these are not checked at runtime.

**4.2** Static variable restriction:
  - In the typechecker rules phase, reject static variable declarations on generic types whose types reference any variance-annotated type parameter.

**Tests SHOULD pass.** New diagnostics only; existing code should not trigger the static variable restriction.

### Phase 5: Validation

**Goal:** Prove the design works end-to-end on the sealed class use case.

**5.1** Convert the `Async` enum in standard-io to a sealed abstract class hierarchy with specialized type params (see §2.13).

**5.2** Update all `Async` construction and match sites in standard-io (`Runtime.dove`, `Fiber.dove`, `Promise.dove`, `Awaitable.dove`).

**5.3** Run `dovetail test` on the full standard-io workspace — verify cross-instantiation assignments and pattern matching work.

**5.4** Add integration tests in `dovetail/tests/` for:
  - Cross-instantiation assignment (`Succeed<Int32>` → `Async<Int32, String>`).
  - Pattern matching on erased types.
  - `is`/`as` on types with variance params.
  - Mixed variance and invariant type params.
  - Function-typed fields at variance positions.
