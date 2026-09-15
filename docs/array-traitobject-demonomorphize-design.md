# De-monomorphizing Arrays & Trait Objects — Eliminating the Monomorphize Type Walk

**Status:** Implemented. See the [trait/interface audit](trait-implementation-status.md).

The historical name “trait object” below now means **interface object**. Compiler
identifiers use `InterfaceObject` and `$IfaceObj$`; the implementation notes take
precedence over the old plan and its open questions. Packed arrays were retained;
vtable signatures preserve concrete types, so the proposed universal arity-only
slot bucketing is obsolete. The type-discovery walk is removed. Runtime type-ID
recovery remains a separate design, not unfinished work here.

> **Implementation notes (deviations from the design below):**
> - **Arrays are fixed constant type indices, not a `for_array_type` remap.** The 8 array types are
>   predeclared at fixed indices (`$Array$i8` reuses the String backing array at 6; `i16/i32/i64/f32/f64/u128/ref`
>   at 53–59) via `emit_array_types`, and `array_type_index` is a direct `match` — `MangledName::for_array_type`
>   is left as the per-element *type* mangle (still needed for function-name mangling).
> - **`Uint128` gets its own `$Array$u128`** (`(array (mut (ref $Uint128)))`), not folded into `$Array$ref`,
>   so reads only unbox (no `ref.cast`). `$Array$ref` stores **non-null** `(ref any)`; the abstract erased-array
>   slot is **non-null** `(ref array)` (matching the existing non-null erased-slot convention).
> - **Trait-object vtable slots erase only the trait's generic params (and `Self`), not everything** — built
>   from the trait method's *raw* signatures (concrete param/return types stay concrete). The wrapper bridges the
>   erased↔concrete ABI; the dispatch site boxes erased args / casts back erased returns symmetrically.
> - **Per-instantiation vtable globals:** because a generic impl class's `mangled_name()` is the erased canonical,
>   the per-trait trait-object key would collide across instantiations (e.g. `ArrayIterator<Int32>` vs
>   `<String>`). Vtable globals + wrappers are keyed on the new `Type::instance_key()` (includes type args).
> - **Walk deleted; trait objects register at coercion sites only.** `register_trait_object_type_def` no longer
>   skips generic type-args (registration is instantiation-independent), and `coerce_to_trait_object` registers
>   up front — so traits coerced *only* in generic contexts (e.g. `ArrayIterator<T> as Iterator<T>`) are covered.

---

**Continuation of:** [full-erasure-design.md](full-erasure-design.md) (§2.8 Arrays, §3.2 closures) and
[tuple-multivalue-codegen-design.md](tuple-multivalue-codegen-design.md) (the "discover-once, emit shared type" pattern).

**Backlog items addressed:**
- *stop monomorphize trait objects*
- *stop monomorphize arrays… one array per all reference types*
- *what is still monomorphize today?*

---

## 1. Motivation

After full type erasure for records/enums/classes and always-erased arity-bucketed closures,
the monomorphize layer no longer monomorphizes generic *types* — there is one canonical erased
`TypeDef` per generic definition. Functions (and class/trait/extension methods, which are
functions) are still monomorphized per type-argument list, and that is correct and intended.

But two things are still *type-monomorphized*, and both are discovered by a full reachability
walk over every function body, global, and existing TypeDef:

| What | Where | How discovered |
|------|-------|----------------|
| **Array types** — one concrete `$Array$<elem>` per concrete element type, references included | [`collect_type_defs_from_type`, Array arm](../dovetail/src/compiler/monomorphize/mod.rs#L1168) | walk |
| **Trait-object types** — one `TraitObjectTypeDef` (vtable + struct) per concrete trait instantiation | [same fn, TraitObject arm](../dovetail/src/compiler/monomorphize/mod.rs#L1236) | walk |

The walk itself is [`collect_new_type_defs`](../dovetail/src/compiler/monomorphize/mod.rs#L1107):
it crawls `module.functions`, `module.globals`, `module.tests`, every existing `TypeDef`, and
recurses through every expression and pattern. After all the erasure work already landed, **these
two kinds of TypeDef are the only things this walk still creates** — everything else (tuples →
shared `$Tuple_N`, closures → shared `Closure_N`, generic records/enums/classes → single
canonical erased def) is reached only to recurse into nested types.

The walk is pure overhead for the array case. The set of array types we actually need is a
*fixed, tiny, closed set* plus one shared reference array — knowable without looking at the
program at all. Unconditionally predeclaring the 6 primitive array types and one reference array
type is strictly cheaper than crawling every expression to discover which subset is used.

Note this is *stronger* than the closure/tuple precedent, not the same as it: `Closure_N` and
`$Tuple_N` still run a discovery walk (`discover_closure_arities`, `discover_tuple_widths`) because
their arity/width axis is open-ended — any N is possible, so the set in use must be learned by
looking. Arrays have no such axis; the set is genuinely fixed, so they skip discovery entirely.

**Goal:** make array and trait-object WASM types knowable without walking the code, then delete
the walk.

---

## 2. Current state (what is still monomorphized)

### 2.1 Arrays — monomorphized per concrete element type

`single_val_type` ([mod.rs:1121](../dovetail/src/compiler/codegen/mod.rs#L1121)):

```rust
Type::Array(elem) if elem.contains_type_parameter() => /* (ref any) — erased */,
Type::Array(elem) => /* (ref Concrete(array_type_index(elem))) */,
```

`element_storage_type` ([mod.rs:2185](../dovetail/src/compiler/codegen/mod.rs#L2185)) gives every
element type its own storage: packed `i8`/`i16` for small ints, native `i32`/`i64`/`f32`/`f64`
for the rest, and a **concrete `(ref $Foo)`** for every reference element type. So `Array<Logger>`
and `Array<String>` each get a distinct WASM array type. Each one must be discovered by the walk
and emitted into the type graph.

The element type is always concrete inside a monomorphized function body, so array
get/set/new/fill all read it directly off the receiver's `Type::Array(elem)`
([intrinsics.rs:29–112](../dovetail/src/compiler/codegen/function_emitter/intrinsics.rs#L29-L112))
— there is no element-level boxing today.

### 2.2 Trait objects — monomorphized per instantiation

`single_val_type` erases `TraitObject` to `(ref any)` when any type arg contains a type parameter,
otherwise returns the concrete per-instantiation struct. The per-instantiation vtable wrapper
funcref signatures are built from the **substituted concrete** param/return types
([emit_trait_object_types:2263–2283](../dovetail/src/compiler/codegen/mod.rs#L2263-L2283)):

```rust
let mut params = vec![anyref];          // receiver
for pt in param_types { params.extend(self.type_to_valtypes(pt)); }  // concrete!
let result = self.type_to_valtypes(return_type).into_vec();          // concrete!
```

Because the funcref signatures embed concrete types, `Showable<Int32>` and `Showable<Int64>`
produce *different* vtable func types and therefore different WASM trait-object types — which is
exactly why they're monomorphized and why a type-param instantiation has to fall back to `anyref`.

Note: class vtables already solved this. Virtual class methods are registered with an **erased
vtable-slot func type** shared across instantiations
([generate:1711–1715](../dovetail/src/compiler/codegen/mod.rs#L1711-L1715)). Trait-object vtables
have not yet adopted that treatment.

### 2.3 `is_erased_slot` ([mod.rs:1413](../dovetail/src/compiler/codegen/mod.rs#L1413))

Answers "is this stored as an erased `anyref` slot → box on write, `ref.cast`+unbox on read?":

```rust
Type::TypeVariable(_, _) | Type::GenericParam(_, _, _) => true,
Type::Array(elem) => elem.contains_type_parameter(),
Type::TraitObject { trait_type_args, .. } => trait_type_args.iter().any(|t| t.contains_type_parameter()),
Type::Newtype(_, inner) => Self::is_erased_slot(inner),
Type::GenericNewtype { concrete_inner_type, .. } => Self::is_erased_slot(concrete_inner_type),
_ => false,
```

Today erasure is an **all-or-nothing property of the whole array** (only when the element is a
type parameter). There is no notion of an array whose *elements* are erased — because reference
arrays are concrete per element type.

---

## 3. Design

Two independent changes that together let us delete the walk:

- **§3.1 Arrays** stay monomorphized, but only along a *closed, predeclared axis* — one type per
  primitive (primitives can't share storage in WASM-GC) — while *reference* arrays stop being
  monomorphized and collapse to a single shared array. The fully-generic slot gets a real array
  *base type* instead of being erased to bare `any`.
- **§3.2 Trait objects** stop being monomorphized: one WASM type per trait, layout independent of
  type args. The enabling mechanism is erasing the vtable slot signatures (the class-vtable
  treatment) — so here de-monomorphization is *achieved by* more erasure.

### 3.1 Arrays: fixed primitive set + one shared reference array

WASM-GC arrays must declare a concrete element storage type and there is **no `anyarray`** that
all arrays subtype. Element types of *mutable* arrays are **invariant** for subtyping, so
`(array (mut (ref $Foo)))` is **not** a subtype of any common reference-array type. Two
consequences drive the whole design:

1. Primitive arrays each genuinely need their own type (`(array (mut i32))` ≠ `(array (mut f64))`)
   — but there is a *fixed, closed set* of them.
2. Reference arrays cannot share a type *unless* they all literally use the same element storage.
   So all reference arrays collapse to **one** `(array (mut (ref null any)))`, with cast-back on
   element read.

#### 3.1.1 The predeclared array types

Predeclared unconditionally at codegen start. The *emission* step resembles
[`emit_closure_arity_types`](../dovetail/src/compiler/codegen/mod.rs#L3812) /
[`emit_tuple_width_types`](../dovetail/src/compiler/codegen/mod.rs#L3887), but with one decisive
difference: **arrays run no discovery walk.** Closures and tuples must first crawl the whole module
([`discover_closure_arities`](../dovetail/src/compiler/codegen/mod.rs#L3611) /
[`discover_tuple_widths`](../dovetail/src/compiler/codegen/mod.rs#L3625)) because their arity/width
axis is open-ended — any N is possible, so the set actually used can only be learned by looking. The
array set has no such axis: it is fixed at 7 (6 primitive storages + one reference array), so there
is nothing to discover. Just emit the fixed set.

| Dovetail element type | WASM array type | Element storage |
|---|---|---|
| `Int8`, `Uint8` | `$Array$i8` | packed `i8` |
| `Int16`, `Uint16` | `$Array$i16` | packed `i16` |
| `Int32`, `Uint32`, `Bool`, `Char`, `Unit` | `$Array$i32` | `i32` |
| `Int64`, `Uint64` | `$Array$i64` | `i64` |
| `Float32` | `$Array$f32` | `f32` |
| `Float64` | `$Array$f64` | `f64` |
| **any reference type** (String, record, enum, class, tuple, closure, trait object, `Array<…>`, `Any`) | `$Array$ref` | `(ref null any)` |

That is **7 array types total**, for the entire program, always emitted. (`Bool`/`Char`/`Unit`
already share `i32` storage today, so collapsing them onto `$Array$i32` is consistent with current
behavior.)

> Open knob: whether `Int8`/`Uint8` should keep packed `i8` (saves 4–8× memory for byte buffers,
> which the IO library relies on) or fold into a smaller set. Recommendation: **keep the packed
> primitive arrays** — they're cheap (fixed count) and byte arrays are hot. The savings from this
> work come from killing the *walk* and the *per-reference-type proliferation*, not from dropping
> packed primitives.

#### 3.1.2 The array base type for the fully-erased slot

When `Array<T>` lands in an erased slot (e.g. `record Container<T> = items: Array<T>`), it
currently lowers to bare `(ref any)`. Replace that with WASM-GC's built-in **abstract `array`
heap type**:

```rust
// single_val_type, fully-generic array slot
Type::Array(elem) if elem.contains_type_parameter() =>
    (ref null array)   // wasm_encoder::HeapType::Abstract { shared:false, ty: AbstractHeapType::Array }
```

This is the "base type that doesn't fall down to simple `any`" from the requirements. It works as
a common storage slot because **every** concrete array type — each primitive `$Array$iN` *and*
`$Array$ref` — is a subtype of the abstract `array`. Benefits over `any`:

- More precise static type; catches "stored a non-array into an array slot" at validation time.
- `array.len` is defined on `(ref null array)`, so reading the length of an erased array slot
  needs **no** cast.
- Element get/set still requires a `ref.cast` to the concrete `$Array$iN` / `$Array$ref` the
  monomorphized reader knows — same as any other erased read, just to a more specific target.

(A *declared* base array type can't serve here: with mutable invariant elements it could only
generalize the reference arrays, never the primitive ones. The built-in abstract `array` is the
only common supertype across both, so we use it.)

#### 3.1.3 Element-level erasure for `$Array$ref`

This is the new concept `is_erased_slot` must learn: an array can be concrete while its *elements*
are erased. For `$Array$ref`:

- **Write** (`ArraySet`, `ArrayNew`, `ArrayNewFixed`, `ArrayFill`): the value is a reference;
  it upcasts to `anyref` implicitly — **no instruction needed**. A primitive can never reach
  `$Array$ref` (primitives route to `$Array$iN`). A *tuple* element must be reboxed into its
  `(ref $Tuple_N)` first (already done via `emit_rebox_tuple`), then upcast.
- **Read** (`ArrayGet`): the element comes off as `anyref`; emit `ref.cast` to the concrete
  element type the receiver's `Type::Array(elem)` names, then unbox if it's a boxed tuple
  (`emit_unbox_tuple`). This reuses the existing `cast_back_from_erased` logic
  ([expressions.rs:1831](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L1831)),
  applied at the array-element boundary.

Primitive arrays (`$Array$iN`) keep today's behavior verbatim: `ArrayGetS`/`ArrayGetU` for packed,
direct `ArrayGet`/`ArraySet` for the rest, no cast.

The single chokepoint is `array_type_index(elem)`
([mod.rs:1404](../dovetail/src/compiler/codegen/mod.rs#L1404)): route every reference element type
to `$Array$ref` and every primitive to its `$Array$iN`. Every array intrinsic already funnels
through it, so element get/set/new/fill/clone/concat all pick up the change centrally; the only
added logic is the cast-on-read / rebox-on-write guarded by "is this a `$Array$ref`?".

#### 3.1.4 Mangled names

`MangledName::for_array_type` ([types.rs:156](../dovetail/src/common/types.rs#L156)) currently keys
on the full element type. Change the mapping so all reference element types produce one canonical
name (`$Array$ref`) and primitives map to their fixed names. This keeps `type_indices` lookups
working with a finite key set and removes per-element-type entries from the type graph.

### 3.2 Trait objects: one type per trait, never erased

A trait object is always laid out as `(anyref data, ref $vtable)`
([emit_trait_object_types:2319](../dovetail/src/compiler/codegen/mod.rs#L2319)). The struct layout
does **not** depend on the type arguments — only the *vtable funcref signatures* do, because they
currently embed substituted concrete types. Erase those signatures (exactly as class vtables
already are) and the entire trait-object representation becomes independent of type args:

- **Vtable slot func type** becomes the erased shape `(anyref receiver, anyref×k) -> anyref`
  (one shape per *method arity*, analogous to closure `Func_N`), instead of the per-instantiation
  concrete signature at [emit_trait_object_types:2269–2274](../dovetail/src/compiler/codegen/mod.rs#L2269-L2274).
- The **wrapper function** gains a prologue/epilogue that casts the erased `anyref` params back to
  the impl method's concrete types and boxes the concrete return to `anyref` — the same
  cast-back/box machinery closures and erased fields already use.

Consequences:

1. **One WASM trait-object type per trait** (keyed by trait FQN), regardless of type args.
   `Showable<Int32>` and `Showable<Int64>` share it.
2. `single_val_type(TraitObject{..})` returns that concrete type **even when type args contain a
   type parameter** — the type-param guard at
   [mod.rs:1158–1167](../dovetail/src/compiler/codegen/mod.rs#L1158) is deleted. Trait objects are
   **no longer erased**.
3. `is_erased_slot`'s `TraitObject` arm is removed (returns `false`).
4. Trait objects come into existence only at explicit coercion points, which
   `coerce.rs`/`elaborate_coercions` already discovers and registers (`TraitObjectCoerce`
   registers `TraitObjectTypeDef` entries — see full-erasure §"trait-object coercion"). So the
   monomorphize-walk TraitObject arm is redundant and gets deleted; emission is driven off the
   registry + coercion sites, keyed per trait.

> Trade-off: the data pointer is already `anyref` today, so trait-object dispatch already pays one
> `ref.cast` on `self`. Erasing the method args/return adds cast-back/box at the wrapper boundary
> — paid once per dispatch, identical in spirit to closures. In exchange: no per-instantiation
> trait-object types, no walk, and `as`/trait-object coercion in generic contexts stops falling
> off a cliff to bare `anyref`.

### 3.3 Delete the walk

With §3.1 (arrays predeclared, no discovery) and §3.2 (trait objects per-trait, discovered at
coercion sites in `coerce.rs`), `collect_type_defs_from_type` no longer *creates* anything — both
its creating arms are gone. Verify nothing else depends on its recursion creating TypeDefs (under
full erasure, nested generic instantiations resolve to canonical erased defs already present), then
delete `collect_new_type_defs`, `collect_type_defs_from_type`, `collect_type_defs_from_expr`, and
`collect_type_defs_from_pattern` ([mod.rs:1107–1357](../dovetail/src/compiler/monomorphize/mod.rs#L1107-L1357)),
and drop the call from `monomorphize`.

What remains in the monomorphize layer is then exactly what the name promises: **function**
monomorphization (template functions, impl/ext/trait methods, generic-class methods).

> Caveat to verify before deletion: generic-class method instantiation is still driven by
> [`discover_generic_class_instances`](../dovetail/src/compiler/monomorphize/mod.rs#L702), a
> *type* walk that finds `GenericClass` instances to know which method sets to stamp out. That
> walk stays (it feeds function monomorphization, not type monomorphization). Only the
> *TypeDef-collection* walk goes away.

---

## 4. Changes to the two erasure predicates

### `single_val_type`

```rust
// arrays: closed set, never bare any
Type::Array(elem) if elem.contains_type_parameter()
    => (ref null array),                       // abstract array base (was: ref any)
Type::Array(elem)
    => (ref Concrete(array_type_index(elem))), // $Array$iN for primitives, $Array$ref for refs

// trait objects: one per trait, never erased — DROP the type-param→any guard
Type::TraitObject { mangled_name, .. }
    => (ref Concrete(trait_object_type_indices[per-trait key])),
```

### `is_erased_slot`

```rust
Type::TypeVariable(_, _) | Type::GenericParam(_, _, _) => true,
Type::Array(elem) => elem.contains_type_parameter(),   // unchanged: the *slot* is erased only when fully generic
// Type::TraitObject => REMOVED  (now concrete)
Type::Newtype(_, inner) => Self::is_erased_slot(inner),
Type::GenericNewtype { concrete_inner_type, .. } => Self::is_erased_slot(concrete_inner_type),
_ => false,
```

Note the asymmetry that is now explicit and intentional:

- **Whole-array erasure** (`Array<T>`, T a type param) → the slot is `(ref null array)`; handled by
  `is_erased_slot` as today, just with a more precise base type as the cast source.
- **Element erasure** (`$Array$ref`) is *not* an `is_erased_slot` property — the array reference is
  concrete. It is handled locally at the array get/set intrinsics (§3.1.3), the same way tuple
  box/unbox at array elements is handled locally today.

---

## 5. Codegen change points

| Concern | Location | Change |
|---|---|---|
| Predeclare 7 array types | new `emit_array_types`, called beside `emit_tuple_width_types`/`emit_closure_arity_types` | emit fixed set unconditionally; populate `type_indices` for the 7 names |
| Element→array-type routing | [`array_type_index`](../dovetail/src/compiler/codegen/mod.rs#L1404) | primitives → `$Array$iN`; references → `$Array$ref` |
| Element storage | [`element_storage_type`](../dovetail/src/compiler/codegen/mod.rs#L2185) | reference branch returns `(ref null any)`; primitive branches unchanged |
| Fully-generic array slot | [`single_val_type`](../dovetail/src/compiler/codegen/mod.rs#L1148) | `(ref null array)` instead of `(ref any)` |
| Element read cast | [`ArrayGet`](../dovetail/src/compiler/codegen/function_emitter/intrinsics.rs#L29) | if `$Array$ref`: `ref.cast` to concrete elem (+ unbox tuple) after `ArrayGet` |
| Element write upcast | [`ArraySet`/`ArrayNew`/`ArrayFill`/`ArrayNewFixed`](../dovetail/src/compiler/codegen/function_emitter/intrinsics.rs#L51) | reference values upcast implicitly; tuples rebox (already done) |
| Trait-object vtable sig | [`emit_trait_object_types`](../dovetail/src/compiler/codegen/mod.rs#L2263) / `build_trait_object_subtypes` | erased `(anyref…)->anyref` slot func types + wrapper cast-back/box prologue |
| Trait-object index keying | `trait_object_type_indices` | key per trait FQN, not per instantiation |
| Remove monomorphize walk | [`collect_new_type_defs` & friends](../dovetail/src/compiler/monomorphize/mod.rs#L1107) | delete after both consumers gone |

---

## 6. Edge cases

- **`Array<Array<T>>`** — the inner array is a reference, so the outer array is `$Array$ref` holding
  `anyref`; reading an element casts back to `(ref null array)` (or directly to the concrete inner
  `$Array$iN`/`$Array$ref` the reader knows). Nested-array element storage
  ([element_storage_type:2210](../dovetail/src/compiler/codegen/mod.rs#L2210)) folds into the
  reference branch.
- **`Array<(A,B)>`** — element is a boxed `(ref $Tuple_N)`, a reference → `$Array$ref`. Write reboxes
  the flattened tuple (existing `emit_rebox_tuple`), read casts to `(ref $Tuple_N)` then explodes
  (existing `emit_unbox_tuple`). Same pattern as today, now through the cast-on-read path.
- **`Array<closure>`** — closures are `(ref Closure_N)`, a reference → `$Array$ref`. Cast back to
  `(ref Closure_N)` on read.
- **`Array<Any>`** — `Any` is a reference → `$Array$ref`; element read casts to `(ref any)`, i.e. a
  no-op cast / trivial. Fine.
- **`is`/`as` on arrays** — `ref.test` against `(ref array)` only proves "it's some array", and
  against `$Array$ref` proves "array of references"; element type is not recoverable at runtime
  (consistent with full-erasure §"`is`/`as` on erased types"). Document that `x is Array<Int32>`
  vs `x is Array<Int64>` are distinguishable (different primitive arrays) but
  `x is Array<Logger>` vs `x is Array<String>` are **not** (both `$Array$ref`).
- **Empty arrays / `ArrayEmpty`** — unchanged; just routes through the new `array_type_index`.

---

## 7. Implementation phases

1. **Predeclare array types** — add `emit_array_types`, retarget `array_type_index` /
   `element_storage_type` / `MangledName::for_array_type` to the closed set. References get
   `$Array$ref`. At this point the walk still runs but discovers nothing new for arrays.
2. **Element erasure for `$Array$ref`** — cast-on-read / rebox-on-write at the array intrinsics;
   tests for reference, nested-array, tuple, closure, and `Any` elements.
3. **Abstract array base slot** — `single_val_type` fully-generic array → `(ref null array)`;
   update erased read sites to cast from the array base; verify `array.len` on erased slots.
4. **Trait objects per-trait** — erased vtable slot signatures + wrapper prologue; key
   `trait_object_type_indices` per trait; drop the type-param→`any` guard and the
   `is_erased_slot` TraitObject arm; drive emission from coercion sites.
5. **Delete the walk** — remove the Array and TraitObject arms, confirm `collect_new_type_defs`
   creates nothing, delete it and its helpers and the call in `monomorphize`.

Each phase keeps the suite green on its own; run the real workspace via `cargo run -- test` (IO /
byte-array heavy) in addition to `cargo test`.

---

## 8. Open questions

1. **Keep packed `i8`/`i16` arrays?** Recommendation yes (byte buffers are hot; cost is a fixed
   couple of types). Confirm no code relies on a uniform element width.
2. **Vtable slot bucketing for trait objects** — bucket erased slot func types by arity (like
   `Func_N`) and share across all traits, or one set per trait? Arity-bucketing is fewer types and
   matches closures; per-trait is simpler to map. Lean arity-bucketed for consistency.
3. **Does any non-array/non-trait-object path rely on `collect_new_type_defs`' recursion** to
   surface a nested user TypeDef that isn't otherwise present post-erasure? Must be proven before
   deletion (phase 5) — likely no, since canonical erased defs are emitted at typecheck time.
4. **`reified-generics` / `type-id`** ([type-id-extension.md](type-id-extension.md)) — element
   erasure on `$Array$ref` loses the element type at runtime. If reified generics land, a `type_id`
   on boxed elements (or on the array) would restore `x is Array<Logger>`. Out of scope here;
   noted as a strict extension.
