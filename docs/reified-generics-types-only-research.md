# Reified Generics (Types Only) — Research

**Goal:** Assess switching from monomorphized generic **types** (records, enums, classes) to a **single runtime representation** per generic definition. Primitives are boxed; type-parameter slots become `(ref any)`. Functions stay monomorphized. This removes the variance-cast pass but introduces two new runtime costs: (1) **cast-back on every read** of a type-parameter slot, and (2) a **type_id** (`i32`, first field) for `is`/`as`/type-annotated match from `Any`.

---

## 1. Current State

### 1.1 Monomorphization

Each instantiation gets its own WASM struct with concrete field types. `Option<Int32>` has an `i32` payload field; `Option<String>` has a `(ref $string)` payload field. Codegen maps each mangled name to a distinct WASM type index (`type_indices[mn]`). Field access (`struct.get`) returns a value of the right WASM type directly — no cast needed.

### 1.2 Variance cast pass

`elaborate_variance_casts` in `variance_cast.rs` (~1200 lines). After typechecking, walks the entire typed module. At every coercion point where actual and expected have the same generic FQN but different type args, it **synthesizes a runtime conversion**: for enums, a match that deconstructs and reconstructs with the target type; for records, extract all fields then `RecordCreate` with the target type. This happens at: function return, global initializer, `Let`, `Assign`, `GlobalAssign`, every function-call argument, `EnumCreate`/`RecordCreate` args, `RecordWith` overrides, match arms, closure bodies. The pass is broad, deep, fragile, and easy to get wrong.

### 1.3 `is` / `as` / type-annotated match (today)

All use `wasm_type_index_for_any_cast(target_type)` which for generic types returns `type_indices[mn]` — the **per-instantiation** type index. So `x is Option<Int32>` emits `RefTestNonNull` against the `Option$Int32` struct type, and `x is Option<String>` against `Option$String`. They are different WASM types, so we can distinguish them with a single `ref.test`.

### 1.4 Field access and match (today)

Field access: `struct.get` on the concrete struct type with the field index → returns the concrete WASM value type (e.g. `i32` for `Int32`, `(ref $string)` for `String`). No cast needed.

Enum match: `ref.test` against the per-instantiation variant struct type, then `ref.cast`, then `struct.get` for each payload field → returns the concrete WASM value type. No cast needed.

---

## 2. Proposed Model

### 2.1 Single representation

- **One WASM struct type per generic definition**, not per instantiation. All `Option<T>` share one struct; all `Result<T, E>` share one struct.
- **Type-parameter slots are `(ref any)`**: every record field, enum variant payload field, or class field whose type comes from a type parameter is stored as WASM-GC `(ref any)`. Not a typed ref — literally `anyref`.

### 2.2 Box on write

When constructing a generic value and storing into a type-parameter slot:

- **Primitive** (Int32, Bool, Float64, etc.): box it first (`struct.new $Int32Box` etc.), then store the box ref as `(ref any)`.
- **Reference type** (String, Record, Enum, Array, etc.): already a GC ref; store it directly as `(ref any)` (upcasts implicitly in WASM-GC).

We already have boxing machinery: `box_type_index_for`, `BoxToAny`, `StructNew`.

### 2.3 Cast back on every read

**This is the key cost.** The type-parameter slot holds `(ref any)`. When the user reads that slot — via **field access** or via **match** (record destructure, enum variant payload binding) — the value on the WASM stack is `(ref any)`. But the user expects a concrete type (e.g. `Int32`, `String`). So **every read** of a type-parameter slot must:

1. `struct.get` → gets `(ref any)`.
2. `ref.cast` to the expected concrete type (box struct for primitives, concrete struct for reference types).
3. If primitive: `struct.get` field 0 on the box to unbox.

The **expected type is known at compile time** from the static type in the typed AST (e.g. the field type is `Int32` because the expression is typed as `Option<Int32>`). So the compiler emits the right cast and unbox — no runtime type-info needed for this path.

**Examples:**

```
// record Box<T> = value: T
let b: Box<Int32> = Box(42)
let x: Int32 = b.value
//   struct.get $Box, field 1  → (ref any)
//   ref.cast (ref $Int32Box)  → (ref $Int32Box)
//   struct.get $Int32Box, 0   → i32

let b2: Box<String> = Box("hi")
let s: String = b2.value
//   struct.get $Box, field 1  → (ref any)
//   ref.cast (ref $String)    → (ref $String)

// enum Option<out T> = None | Some(T)
match opt with
    case Some(x) =>
//   struct.get $SomeVariant, field 0  → (ref any)
//   ref.cast (ref $Int32Box)          → (ref $Int32Box)
//   struct.get $Int32Box, 0           → i32
//   (bind x as i32)
```

This applies on **every** access to a generic field or variant payload, regardless of whether the receiver came from `Any` or was a local variable typed `Option<Int32>`.

### 2.4 Variance cast: true no-op

With subtype sets (§3.4), variance casts become **true no-ops** — just pass the pointer. The type\_id is never modified.

At first glance, this seems wrong:

```
let opt: Option<Int32> = Some(42)     // type_id = TYPE_ID_OPTION_INT32
let opt2: Option<Any> = opt           // variance coercion — just pass the pointer
opt2 is Option<Any>                   // type_id is still TYPE_ID_OPTION_INT32...
```

But with subtype sets, `opt2 is Option<Any>` does **not** check `i32.eq TYPE_ID_OPTION_ANY`. Instead, it looks up `set_option_any[TYPE_ID_OPTION_INT32]` in a pre-computed byte array — and finds `1` (because `Int32 <: Any` and `T` is covariant). So the check passes correctly.

The type\_id records the **original instantiation** permanently. Subtype sets encode all the variance relationships at compile time. No copying, no mutation, no allocation at coercion points.

What remains:

- Trait object coercion (unchanged).
- Boxing primitives to Dovetail `Any` in non-generic contexts (unchanged).
- Variance position checking in the rules phase (unchanged — type safety).

### 2.5 Functions stay monomorphized

Each specialized function (e.g. `unwrap$Int32`, `unwrap$String`) knows the concrete `T` at compile time. It reads the `(ref any)` slot and emits the right `ref.cast` + unbox. No type-info parameter, no runtime dispatch. Functions are unchanged except that field access / match codegen now emits the cast-back sequence instead of a direct `struct.get`.

---

## 3. Type-Info for `is` / `as` / Type-Annotated Match

### 3.1 The problem

With single representation, all `Option<T>` share one WASM struct type. `ref.test` can tell us "this is an Option" but **not** "this is an Option<Int32> vs Option<String>". So `is`, `as`, and type-annotated match on generic types from `Any` break.

### 3.2 The design: type_id as first field

Since functions are monomorphized, at every construction site we know the **full concrete instantiation** (e.g. `Option<Int32>`, `Result<String, Int32>`). So we do not need a per-type-parameter array — we assign a single **type_id** (`i32`) to each instantiation and store it as the first field.

**Layout:**

```
// Option<T>:
struct $Option {
    field 0: i32          // type_id (e.g. TYPE_ID_OPTION_INT32)
    field 1: (ref any)    // payload (T slot)
}

// Result<T, E>:
struct $Result {
    field 0: i32          // type_id (e.g. TYPE_ID_RESULT_INT32_STRING)
    field 1: (ref any)    // ok payload (T slot)
    field 2: (ref any)    // err payload (E slot)
}

// Pair<A, B>:
struct $Pair {
    field 0: i32          // type_id (e.g. TYPE_ID_PAIR_INT32_BOOL)
    field 1: (ref any)    // first (A slot)
    field 2: (ref any)    // second (B slot)
}
```

**Construction:** When constructing e.g. `Some(42)` in a context where `T = Int32`:

1. Push the type_id constant for `Option<Int32>` (e.g. `i32.const TYPE_ID_OPTION_INT32`).
2. Box `42` → `(ref $Int32Box)`.
3. `struct.new $Option` with the type_id and the boxed ref.

No allocation for the type-info — it is just an `i32` constant.

### 3.3 Type_id assignment

At codegen, assign a unique `i32` constant to every concrete instantiation that appears in the program:

- `Option<Int32>` → 0
- `Option<String>` → 1
- `Result<Int32, String>` → 2
- `Option<Option<Int32>>` → 3 (handles nested generics)
- etc.

The type_id identifies the **whole instantiation**, not individual type parameters.

### 3.4 Subtype sets (compile-time pre-computation)

A simple `i32.eq` on the type\_id does not handle variance correctly. If `x` is `Option<Dog>` (type\_id = 3) and we check `x is Option<Animal>`, `i32.eq TYPE_ID_OPTION_ANIMAL` returns false — even though `Dog <: Animal` and `T` is covariant.

Instead, for each expected type at an `is`/`as`/type-annotated match site, the compiler **pre-computes the set of type\_ids** that satisfy the variance-aware subtype check. This uses the existing `is_assignable` logic in the typechecker.

**Example:** `x is Option<Animal>` where `T` is covariant.

All `Option` instantiations in the program: `Option<Int32>` (id=0), `Option<String>` (id=1), `Option<Animal>` (id=2), `Option<Dog>` (id=3). Covariant `T`, expected = `Animal`. Valid: all ids where actual `T <: Animal`:

- `Int32 <: Animal` ✗ → id 0 excluded
- `String <: Animal` ✗ → id 1 excluded
- `Animal <: Animal` ✓ → id 2 included
- `Dog <: Animal` ✓ → id 3 included
- **Valid set = {2, 3}**

For invariant parameters, only exact equality. For contravariant (`in`), the direction is reversed (`actual_T :> expected_T`). For multiple type parameters, all must pass (conjunction).

Because functions are monomorphized, the expected type at each `is`/`as` site is always fully concrete. The actual type\_id is unknown at compile time, but the set of all possible type\_ids is known (every instantiation in the program). So the subtype set is complete.

**Storage:** Sets are stored as WASM-GC byte arrays — one byte per type\_id, value 0 or 1. Each unique expected type at an `is`/`as` site produces one global array. Sites checking the same expected type share the array.

```wasm
;; Subtype set for "is Option<Animal>" (4 Option instantiations in program)
(global $set_option_animal (ref $i8_array)
    (array.new_fixed $i8_array 4
        i32.const 0    ;; id 0 (Option<Int32>)    → no
        i32.const 0    ;; id 1 (Option<String>)   → no
        i32.const 1    ;; id 2 (Option<Animal>)   → yes
        i32.const 1    ;; id 3 (Option<Dog>)      → yes
    )
)
```

### 3.5 `is` with generic target

`x is Option<Animal>` when `x: Any`:

1. `ref.test $Option` — is it an Option at all? If no → false.
2. `ref.cast $Option` → `struct.get` field 0 → type\_id.
3. `array.get_u $set_option_animal, type_id` → 0 or 1.

```wasm
;; Step 1: ref.test $Option already done
;; Steps 2+3:
global.get $set_option_animal     ;; (ref $i8_array)
local.get $x
ref.cast (ref $Option)
struct.get $Option 0              ;; type_id → i32
array.get_u $i8_array             ;; set[type_id] → 0 or 1
```

For invariant generic types (no variance params), the subtype set degenerates to a single `1` entry, equivalent to `i32.eq`.

### 3.6 `as` with generic target

Same test as `is`; if the set lookup returns 0 → panic. If 1 → the value is now known to be compatible. Subsequent field access uses cast-back-on-read (§2.3).

### 3.7 Type-annotated match

`match a with case o: Option<Animal> => ...` when `a: Any`:

1. `ref.test $Option` — skip arm if false.
2. `struct.get` field 0 → type\_id, `array.get_u` from the subtype set → skip arm if 0.
3. Bind `o` as `Option<Animal>`. Any field access on `o` uses cast-back-on-read.

### 3.8 Non-generic targets

`x is String`, `x as Int32`, `case s: String =>` — keep current behaviour. `ref.test` / `ref.cast` against the existing type index (or box type index for primitives). No type\_id needed.

### 3.9 Casting from Any after field access

`record Box = value: Any`; then `b.value as Option<Animal>`. `b.value` has type `Any`; `as Option<Animal>` uses the same subtype set mechanism as §3.6. No special handling needed.

---

## 4. What Changes in the Compiler

### 4.1 Type definitions and type indices

- **Today:** one `RecordTypeDef`/`EnumTypeDef` per instantiation, keyed by mangled name. Codegen emits one WASM struct per def.
- **Proposed:** one **canonical** type def per generic definition. Type-parameter fields become `(ref any)`; first field is `i32` (type_id). All `Option<T>` map to the same WASM type index. Non-generic types unchanged.

### 4.2 Codegen: type emission

- **Records:** `build_record_subtype` for a generic record emits: field 0 = `i32` (type_id), then one `(ref any)` per type-parameter field, then concrete fields for non-type-parameter fields.
- **Enums:** one rec group per generic enum. Variant payload fields that are type-parameter slots become `(ref any)`. The type_id can live in the base enum type or in each variant.

### 4.3 Codegen: construction

- `RecordCreate` / `EnumCreate` for generic types: push the type_id constant (`i32.const`), box primitives, then `struct.new` the canonical type.
- Field index shifts by +1 (field 0 is now the type_id).

### 4.4 Codegen: field access

- `FieldAccess` on a generic record: `struct.get` (field index + 1 for type-param fields), then `ref.cast` to expected type, then unbox if primitive. The expected type is from the static type in the AST.
- Non-type-parameter fields (whose type does not involve a type parameter): stored at their concrete WASM type, no cast needed.

### 4.5 Codegen: match

- Enum variant match: `ref.test`/`ref.cast` to the canonical variant type (shared across instantiations), then for each payload binding that is a type-parameter slot: `struct.get` → `ref.cast` → unbox if needed.
- Record pattern match: same — `struct.get` each field, cast-back if it is a type-parameter slot.

### 4.6 Codegen: `is` / `as` / type-annotated match

- For generic targets: `ref.test` the canonical struct type, then `struct.get` field 0 and subtype set lookup (`array.get_u`). See §3.4–§3.7.
- For non-generic targets: unchanged.

### 4.7 Codegen: subtype set emission

1. Assign a unique `i32` type\_id to each concrete instantiation. Maintain `type_id_map: HashMap<MangledName, i32>`.
2. Collect all `is`/`as`/type-annotated match sites targeting generic types.
3. For each unique expected type, compute the valid type\_id set using `is_assignable` per type parameter (respecting variance direction).
4. Emit one WASM-GC `(array i8)` global per unique expected type.

### 4.8 Variance cast pass

- **Remove** the generic record/enum branches entirely. Variance casts become true no-ops — the type\_id stays unchanged and subtype sets handle variance (§2.4). The pass only needs to relabel types in the AST, not emit any runtime code.
- Keep trait-object coercion and primitive→Any boxing.

### 4.9 Inference / typed AST

- Still infer and store concrete types (`Option<Int32>`) in the AST for type-checking and for codegen (knowing what cast to emit on read, what type_id constant to use).
- May stop emitting per-instantiation `RecordTypeDef`/`EnumTypeDef` for layout (only need the canonical def), but keep the mangled-name → type_id mapping.

---

## 5. Trade-offs

### 5.1 What we gain

- **Remove the variance-cast pass for generics**: from ~1200 lines of deep match synthesis (deconstruct variants, box/unbox payloads, reconstruct) to true no-ops. The pass only relabels types in the AST. No runtime code emitted for generic variance coercions.
- **Simpler mental model**: one struct per generic, variance coercion is free.
- **Smaller WASM output** for programs with many instantiations (one type def instead of N).
- **Full runtime type discrimination** despite type erasure: subtype sets enable `is`/`as`/match to correctly distinguish instantiations with variance awareness.

### 5.2 What we pay

- **Cast back on every read** of a type-parameter slot: `ref.cast` (+ unbox for primitives). This is a runtime cost on **every** field access and **every** match binding for generic payloads — the **hot path**. Field access is far more frequent than variance coercion, so this likely dominates.
- **Boxing primitives** in generic slots: extra allocation on construction (one box struct per primitive value stored in a generic), plus more **GC pressure** from the box objects. Reference types pay nothing extra.
- **Type\_id field**: one extra `i32` per generic value (field 0). Negligible memory overhead.
- **Subtype set storage**: one byte array per unique expected type at `is`/`as` sites. Small and bounded by the number of distinct `is`/`as` expected types.
- **`is`/`as` two-step check**: base-type `ref.test` + `array.get_u` lookup. Still O(1) but slightly more work than a single `ref.test`.
- **Field index shift**: all field accesses shift by +1 (type\_id is field 0). Must be handled consistently in codegen for field access, match, `with`, construction.

### 5.3 Performance assessment

**This approach is likely a net runtime performance loss** on the hot path. We make variance coercion free (true no-op via subtype sets) and gain full runtime type discrimination, but the **common** operations (field access, construction) become more expensive. The cast-back on every read and the boxing on every construction are hot-path costs.

**The real win is compiler complexity and correctness**: variance-cast pass eliminated for generics, cross-instantiation subtyping works correctly (sealed class hierarchies with type params), fewer WASM type definitions, one struct per generic. This is an engineering/maintainability win with some runtime cost.

### 5.4 Comparison

| Aspect | Monomorphized (today) | Single representation (proposed) |
|--------|----------------------|----------------------------------|
| WASM types per generic | One per instantiation | One per definition |
| Field access cost | Direct struct.get | struct.get + ref.cast (+ unbox) |
| Construction cost | Direct struct.new | Box primitives + store type\_id + struct.new |
| Variance cast | Deep deconstruct/reconstruct + boxing | True no-op (subtype sets encode variance) |
| `is`/`as` on generics | ref.test against per-instantiation type | ref.test + subtype set lookup (array.get\_u) |
| Code complexity | variance\_cast.rs (~1200 lines, deep) | Subtype set emission + cast-back at every read site |

---

## 6. Difficulty and Scope

| Area | Effort | Notes |
|------|--------|-------|
| Canonical type def + type indices | Medium | One def per generic; map all instantiations to same index. Field 0 = i32 type\_id. |
| Type\_id assignment | Low | Assign one i32 constant per instantiation at codegen. Push it at construction. |
| Subtype set emission | Medium | Collect `is`/`as` sites, compute valid type\_id sets using `is_assignable`, emit WASM-GC byte arrays. |
| Box on write | Low | Reuse existing boxing (`box_type_index_for`, `StructNew`). |
| Cast back on read (field access) | Medium | Every `FieldAccess` on a generic record: shift index, emit ref.cast + unbox. |
| Cast back on read (match) | Medium | Every enum variant payload / record destructure binding for type-param slots: emit ref.cast + unbox after struct.get. |
| `is` / `as` / type-annotated match | Medium | For generic targets: ref.test + subtype set lookup. Non-generic unchanged. |
| Remove variance cast for generics | Low | Remove deep match synthesis entirely. Pass only relabels types in AST. |
| Arrays (`Array<T>`) | None | Arrays stay specialized (one WASM array type per `T`). Arrays are invariant (no variance), so no variance cast needed. |

**Overall: Medium effort.** The compiler gets significantly simpler (variance-cast pass eliminated for generics, cross-instantiation subtyping works), and full runtime type discrimination is preserved via subtype sets. The cost is hot-path overhead (cast-back on every field access, boxing on construction). This is primarily a compiler-engineering and correctness win, with runtime cost. The selective-erasure approach (`variance-type-erasure-design.md`) avoids the hot-path cost for invariant type parameters.
