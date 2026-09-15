# Nominal Type Identity — Single Module-Wide Rec Group

**Status:** Implemented — `emit_type_section` emits one module-wide rec group; `type_graph.rs` is now a 60-line ordering helper
**Related:** [type-id-extension.md](type-id-extension.md) builds on this (its base-type checks and set sizing assume nominal emission); [full-erasure-design.md](full-erasure-design.md)

---

## 1. The problem — WASM-GC type identity is structural

WASM-GC uses **iso-recursive canonicalization**: a type's identity is the pair *(canonical closed rec group, index within the group)*. Two rec groups that are structurally identical — same shapes, same supertype references, same field mutability — canonicalize to the **same** group, making their corresponding members the **same** type. `ref.test` / `ref.cast` operate on canonical types, so they cannot distinguish two Dovetail types that happen to share a shape.

Codegen used to emit one rec group per type-graph SCC (a singleton `subtype` for a non-cyclic record, one `rec` per enum with its variants, one per class with its vtable). Any two same-shape nominal types in separate rec groups therefore **aliased**. This was a live bug in the specialized compiler, independent of erasure. The repros below both failed — `is` returned `true` — before §2 landed:

```dovetail
record Point = x: Int32; y: Int32
record Vec2 = x: Int32; y: Int32

let p: Any = Point { x = 1; y = 2 }
assert !(p is Vec2)          // FAILED before the fix: Point ≡ Vec2 structurally
```

```dovetail
enum Color = Red | Green
enum Switch = On | Off

let c: Any = Color.Red
assert !(c is Switch)        // FAILED before the fix: the two enum rec groups canonicalize equal
```

The same applies to same-shape classes (vtable and struct shapes both match) and to collisions between user types and compiler-internal structs (e.g., a single-`Int32`-field record vs. the primitive box struct used for `Any`). Within one enum, variants do *not* collide — they live in the same rec group at different indices, and index is part of identity.

The worst instance was internal: nine of the thirteen primitive box structs (`$Box$Unit`, `$Box$Bool`, `$Box$Char`, `$Box$Int8/16/32`, `$Box$Uint8/16/32`) are byte-identical `(struct (field i32))` singletons, so on `Any` a boxed `Bool` answered `true` to `is Int32` and `as Int32` handed back its payload. Only the `i64`, `f32` and `f64` boxes were separable at all.

Under [full erasure](full-erasure-design.md) the problem gets strictly worse: erased shapes are far more uniform (`struct { i32 }` bases, `anyref` payload fields), so unrelated generic types collide almost by default.

Codegen already contained a targeted workaround for one instance of this bug: [type_graph.rs](../dovetail/src/compiler/codegen/type_graph.rs) force-merged every class hierarchy into a single SCC precisely because "sibling classes with identical layouts in separate rec groups would be considered the same type, breaking `ref.test`/`ref.cast`". §2 is the generalization of that hack to all types — and it replaced it.

## 2. The fix — emit all types in one module-wide rec group

Within a **single** rec group, two members at different indices are **distinct** types even when structurally identical — index is part of identity. (Verified on wasmtime 45: `ref.test (ref $vec2)` on a `$point` value returns 0 when the two same-shape structs share a rec group, 1 when they sit in separate singleton groups.) So the fix is to emit every GC type the compiler generates — records, enum bases + variants, class structs + vtables, trait-object structs, primitive boxes, closure envs, specialized array types — into **one rec group** for the whole module:

- **Zero runtime cost.** No extra fields, no extra instructions, no per-object memory; `ref.test` stays a single instruction and becomes *precise*. Non-generic `is` / `as` / `match` codegen is untouched.
- **Codegen got simpler, not just different.** WASM only requires *declared supertypes to precede their subtypes* within a group; field references may point anywhere in the group, forward or backward. So mutual-recursion analysis is unnecessary: Tarjan's SCC, the forced-hierarchy edges and the vtable-signature edge tracking in [type_graph.rs](../dovetail/src/compiler/codegen/type_graph.rs) were all deleted, leaving a trivial parent-before-child sort over the class inheritance forest. This fix is a net code deletion (see §5).
- **It covers what a type\_id never can: arrays.** WASM arrays have no header field to carry an id, so two same-shape array types can *only* be separated by group membership. Today's fixed array set happens to have seven distinct element storages, and `String` is a struct wrapping its `(array (mut i8))` backing store rather than an array itself — so no array pair collides right now. But any future array type that shares a storage type with another has no other mechanism available. Boxes, closure envs, and compiler-internal structs also stop aliasing with user types for free.
- **Load-time cost is negligible** (measured, wasmtime 45): a single rec group with 100,000 struct types compiles in ~640 ms, scaling linearly (1k → 36 ms, 10k → 72 ms), and instantiates in under a millisecond. Real programs are orders of magnitude smaller.
- **Precedent:** Kotlin/Wasm ships exactly this strategy to get nominal semantics on structural WASM-GC.

### Which func types go in the group

A rec group may contain func types, and Dovetail's already does — `Func_N` (the closure signature `Closure_N` points at), class vtable slot signatures, and trait-object wrapper signatures all sit in cycles with the structs that reference them, so they have no choice. But a func type inside a group takes on that group's identity, and would then no longer match the canonical signature wasmtime expects at an import or export. So the group is bounded on both sides by the host-facing func types:

| Types | What | In the group |
|---|---|---|
| 0-5 | the two WASI imports; the `run`, `run_post`, `initialize`, `realloc` exports | no — emitted first |
| 6 .. `func_type_base` - 1 | every GC type (string types, boxes, mut-boxes, `$Uint128`, the fixed array set, `Closure_N`, `$Tuple_N`, all user types, closure envs) plus the func types tangled up with them and the internal string/alloc helper signatures | **yes — one `rec`** |
| `func_type_base` .. | user function types, then p3 and WIT import signatures | no — emitted last; they only reference group members backwards, which is legal |

Types 8-13 and 32 (the `string_eq`…`debug_print` and `pinned_alloc` signatures) are inside the group: they take or return GC refs, so they can never cross the host boundary, and a type *outside* a group may not reference a member of a group defined later anyway. Keeping them in place is also what lets every fixed `*_TYPE_INDEX` constant retain its value — the change renumbered nothing.

## 3. The multi-module question

Canonicalization is store-wide, so what happens if another module in the store produces the same types? Unification is **all-or-nothing per closed rec group**, which makes every outcome benign:

- If two modules' groups differ *anywhere*, **no** types unify — complete nominal separation between the modules. For `is` / `as` this is exactly right.
- If the groups are byte-identical, members unify **index-wise**: module A's `$Point` ≡ module B's `$Point`, `$Vec2` ≡ `$Vec2` — and A's `$Point` remains distinct from B's `$Vec2`. That is the *correct* nominal mapping, not a collision.

There is no input for which a wrong `ref.test` answer comes out. Independently, Dovetail's architecture never passes GC references across module boundaries — the WASI/component boundary is linear memory — so cross-module type tests do not occur at all today. If Dovetail ever adds shared-everything linking of separately compiled modules exchanging GC values, nominal identity needs a link-time story under *any* scheme (a shared type section here; globally coordinated id assignment under a type\_id design) — neither approach escapes that work.

## 4. Alternative considered — type\_id on every type

Since erasure needs the type\_id machinery anyway ([type-id-extension.md](type-id-extension.md) §2), extending it to non-generic types is tempting: one mechanism, one check shape everywhere, and ids are self-contained in values (plus useful someday for `typeof`-style reflection). Sketch: a shared root `$Object = struct { type_id: i32 }` that every nominal struct declares as supertype (declared-supertype membership is not structural, so `ref.test $Object` reliably means "carries an id"), then every check is object-test → id read → `i32.eq` / set / hierarchy-range compare.

It was rejected because the uniformity buys less than it appears to:

- **It does not simplify type emission.** Mutually recursive types must share a rec group for the WASM to validate at all, so the SCC machinery stays — unlike §2, which deletes it.
- **It cannot cover arrays** (no header field), so §2-style group thinking would still be needed there — two mechanisms after all.
- **It costs at runtime what §2 gets free:** +4 bytes on every object including primitive boxes, the +1 field-index shift applied to *all* nominal types (codegen churn across construction, field access, and vtable offsets), and an id compare added to every non-generic check.
- The erasure-scoped type\_id loses nothing: it still lands later, on top of §2, exactly as specified — and §2 is what makes its step-1 base check sound (type-id-extension.md §2.5) and its per-generic dense id sets possible (type-id-extension.md §2.4).

The division of labor is: **the rec group gives nominal identity to WASM types** (free, and the only fix that reaches arrays); **the type\_id discriminates values that intentionally share one WASM type** (erased generic instantiations — where no emission strategy can help). Each mechanism is used only where it is the only one that works.

Other non-options: unique empty "brand" supertypes (the brands themselves canonicalize equal); brands distinguished by supertype-chain depth (capped by engine subtyping-depth limits — wasmtime allows 63); a dummy self-referential field (`struct $A { ref null $A, … }` unfolds identically to `$B`'s under iso-recursion — still equal).

## 5. What landed

This fix stands alone and landed ahead of full erasure — it repairs `is` / `as` / typed-`match` in the specialized compiler. When full erasure and the type\_id extension arrive, they build on it unchanged.

`emit_type_section` now runs a single pass over `type_graph::emission_order`: pre-allocate an index for every slot (records, enum bases + variants, class vtable func types + vtable structs + class structs, trait-object wrappers + vtables + object structs), then build every `SubType`, then emit one `rec`. The two-armed cyclic/non-cyclic split is gone, along with `emit_class_with_vtable_rec_group` and `emit_trait_object_types` (the standalone duplicates of the in-group builders) and `scc_type_slot_count`.

As predicted, this was a net deletion. `type_graph.rs` lost Tarjan, the forced class-hierarchy merge, the vtable-signature edges and the within-SCC class topo-sort; what remains is `emission_order`, which returns key order with each class preceded by its ancestor chain — the only ordering WASM still requires, since a declared supertype must precede its subtype while field references may point anywhere in the group. That in turn left `TypeDef::type_dependencies`, `type_dependency_names` and `type_dependency_name` with no callers, and they were removed from the typechecker too.

**Tests.** [dovetail/tests/any_type.rs](../dovetail/tests/any_type.rs) gained same-shape record, enum, payload-carrying enum and unrelated-hierarchy class pairs; a single-`Int32`-field record vs. a boxed `Int32`; boxed `Bool`/`Char`/`Uint32` vs `Int32`; and `String` vs `Array<Int8>` (which passes either way today — `String` is a struct wrapping the backing array, not an array — kept as a guard for if that representation changes). Six of the seven fail on the pre-fix compiler. A structural test in `codegen::tests`, `all_gc_types_share_one_rec_group`, parses the emitted core module and asserts there is exactly one rec group, that it starts at index 6, and that no struct or array type sits outside it — so a later edit cannot silently split the group back apart.
