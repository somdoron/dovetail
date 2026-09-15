# Tuple Multi-Value Codegen Design

This document designs a new **codegen representation for tuples**: instead of always heap-allocating a WASMGC struct, tuples are **expanded ("unboxed") into a sequence of WASM values** wherever a sequence of values can live — function parameters, function results (via the WASM multi-value proposal), local variables, and concrete record/class fields — and **kept as a struct ("boxed") only at the boundaries where a single slot is required** (array elements and type-erased `anyref` positions).

This generalizes and supersedes the earlier research note [tuple-scalar-erasure.md](tuple-scalar-erasure.md), turning its high-level sketch into an actionable design, and it provides the substrate for the `Uint128` value type proposed in the [crypto library design](crypto-library-design.md) §3 — a `Uint128` is, at the WASM level, the smallest interesting unboxed tuple (a pair of `i64`).

**In scope:** A hybrid unboxed/boxed tuple representation; the `type_to_valtypes` flattening primitive; multi-value function returns; flattened parameters and locals; concrete struct-field flattening; the box/unbox coercion boundaries; the relationship to `Uint128`; a phased implementation plan.

**Out of scope:** Changes to the surface language, typechecker semantics, or the tuple type system (see [tuples-design.md](tuples-design.md)); struct-of-arrays representations; SIMD; the `Uint128` arithmetic lowering itself (covered by the crypto doc).

**Related:** [tuples-design.md](tuples-design.md) (surface semantics, tuples-as-records in inference), [tuple-scalar-erasure.md](tuple-scalar-erasure.md) (original research note), [crypto-library-design.md](crypto-library-design.md) §3 (`Uint128`), [full-erasure-design.md](full-erasure-design.md) (anyref erasure of generic type params), [arrays-design.md](arrays-design.md), [records-design.md](records-design.md).

**Implementation status:** Functional implementation complete (phases 1–6).
Tuples flatten in locals, direct-call parameters, multi-value returns, field
access, destructuring, and `match`. Concrete tuple fields are spliced into record
and class structs, including **mutable class fields**, which are updated
leaf-by-leaf. Captured mutable values and single-slot boundaries use boxing where
needed. `Uint128` lowers to `[i64, i64]`, including parameters/returns, arrays,
and wide-arithmetic operations. The earlier two deferrals are implemented.

**Current representation:** Boxed tuples use a shared `$Tuple_N` with `anyref`
leaves, keyed by flattened width, rather than one struct per tuple shape.
Type-parameter leaves also flatten (`(Int32, T)` → `[i32, anyref]`). Codegen uses
`type_to_valtypes` for the flattened run and `single_val_type` for a single-slot
representation. `Uint128` has a dedicated boxed `$Uint128` representation.

**Historical text below:** The motivation, old helper names, per-shape tuple
structs, and proposals are retained to explain the design; these implementation
notes take precedence. Indirect-call boundaries follow their actual signature
and adapter: concrete class-vtable tuple signatures can flatten, while erased
interface/closure boundaries need box/unbox adaptation. Generic data layouts are
shared under [full erasure](full-erasure-design.md); they are not per-instantiation
class layouts.

**Evidence:** [flattening and boxed types](../dovetail/src/compiler/codegen/mod.rs),
[class fields](../dovetail/src/compiler/codegen/classes.rs),
[wide arithmetic](../dovetail/src/compiler/codegen/function_emitter/expressions.rs),
[tuple regressions](../dovetail/tests/tuples.rs), and
[Uint128 regressions](../dovetail/tests/uint128.rs).

**Separate follow-ups:** The Phase 6 benchmark comparison against manual
arithmetic decomposition is not established by this source audit. Composite
DWARF presentation from the original open questions is also not verified here.
Neither is claimed complete by the functional status above.

---

## 1. Motivation

Before this implementation, every tuple is a heap allocation. In the inference layer each distinct tuple shape is lowered to a synthetic record (`tuples-design.md` §6); codegen then treats it as an ordinary WASMGC struct: `struct.new` to construct, `struct.get` to read, a `(ref $TupleN)` everywhere it flows. For most code this is fine, but it is a problem for:

- **Numeric / cryptographic inner loops.** The `Uint128` type (crypto doc §3) is a `(i64, i64)` pair produced and consumed millions of times in field arithmetic (Poly1305, Curve25519, P-256, RSA). A heap allocation per intermediate is unacceptable — the crypto design explicitly requires `Uint128` to be "a multi-value stack type, **not** a heap-allocated GC struct."
- **Multiple return values.** Returning `(Int, Bool)` from a function should be a multi-value return, not an allocation, so that the common "return two things" pattern is zero-cost.
- **Small ephemeral pairs/triples** in hot paths generally.

The key observation is that **`Uint128` is not a special case** — it is a tuple `(Int, Int)` whose two halves happen to have custom arithmetic. If tuple codegen can keep an arbitrary tuple unboxed on the stack/in locals/across calls, then `Uint128` falls out of the same machinery: a width-2 value with bespoke `+`/`*`/`>>` lowering. Conversely, even a *narrow* `Uint128`-only implementation has to answer the same questions (how does a 2×`i64` value live in a parameter? a return? a record field? an array?), so building the general mechanism is the economical path, not a detour.

---

## 2. The core reframing: a type lowers to a *sequence* of values

The central change is to the fundamental lowering primitive. Today:

```
fn type_to_valtype(&self, ty: &Type) -> ValType        // one Dovetail type → one WASM value type
```

This is called from function-signature emission ([codegen/mod.rs:1910-1925](../dovetail/src/compiler/codegen/mod.rs#L1910-L1925)), struct-field declaration ([codegen/records.rs:10-31](../dovetail/src/compiler/codegen/records.rs#L10-L31)), local allocation, and array element storage ([codegen/mod.rs:1984-1990](../dovetail/src/compiler/codegen/mod.rs#L1984-L1990)).

The new primitive lowers a type to an **ordered sequence** of WASM value types:

```
fn type_to_valtypes(&self, ty: &Type) -> SmallVec<[ValType; 2]>   // flattened "unboxed" lowering
```

with the rule:

- **Tuple** `(T1, …, Tn)` → concat of `type_to_valtypes(Ti)` (flattens **transitively**: `((Int, Bool), Int)` → `[i32, i32, i32]`).
- **`Uint128`** → `[i64, i64]` (lo, hi).
- Everything else → exactly one `ValType`, identical to today.
- **`Unit`** → see §7; initially kept as width-1 `i32` to bound scope.

The existing single-value `type_to_valtype` is **retained** — it is the **boxed** lowering used at the single-slot boundaries (§4): for an `anyref` position it returns `anyref`; for an array element / a tuple stored in a struct-as-a-whole it returns the concrete `(ref $TupleN)`. So the two functions coexist and encode the two representations.

---

## 3. Two representations, one type

A tuple value has two representations at runtime; the typed AST and `Type::Tuple` are unchanged.

| Representation | What it is | Where it lives |
|---|---|---|
| **Unboxed (flattened)** | N WASM values, in order, on the stack / in N consecutive locals / as N params / as N multi-value results | Hot path: direct-call params, function returns, locals, concrete record & class fields, destructuring |
| **Boxed** | A `(ref $TupleN)` WASMGC struct — exactly today's representation | Single-slot boundaries: array elements, `anyref`-erased positions, closure environments, enum payloads, `Any` |

The synthetic tuple `RecordTypeDef` produced in monomorphize ([monomorphize/mod.rs:1177-1203](../dovetail/src/compiler/monomorphize/mod.rs#L1177-L1203)) **does not go away** — it becomes the *boxed form*, allocated only when a tuple crosses into a single slot. The win is that the boxed form is no longer the *only* form: in the hot path tuples never allocate.

This mirrors how LLVM/Rust treat small aggregates: passed flattened in registers for calls, given a memory layout when stored.

---

## 4. The boxing boundary (why we can't flatten everything)

WASM lets a group-of-values exist transiently only on the **value stack**, in **parameters**, in **results** (multi-value), and in **locals**. The moment a tuple needs to occupy *exactly one slot*, the flattened form has nowhere to go and we must box. There are exactly two such boundaries:

### 4.1 Array elements

A WASMGC array has a single element storage type ([codegen/mod.rs:1984-1990](../dovetail/src/compiler/codegen/mod.rs#L1984-L1990)). `Array<(Int, Float64)>` cannot be "an array of two values" — and the element halves may even have different WASM types (`i64`/`f64`), so there is no single flat array that works. **Tuples in arrays are always boxed:** the array element type stays `(ref $TupleN)`, `array.set`/`array.new` box, and reads unbox on use (or stay boxed until used).

A struct-of-arrays representation (`Array<(A,B)>` → `{ as: Array<A>, bs: Array<B> }`) was considered and rejected: it changes `len`, indexing, identity, and breaks `Array<T>` uniformity under generics. Boxing is the pragmatic answer.

### 4.2 Type-erased (`anyref`) positions

Dovetail erases generic type parameters and several indirect-call surfaces to `anyref` / `eqref` (see [full-erasure-design.md](full-erasure-design.md)). Today `type_to_valtype` already maps a tuple-containing-a-type-parameter to `anyref` ([codegen/mod.rs:1094-1131](../dovetail/src/compiler/codegen/mod.rs#L1094-L1131)), and the closure arity types use all-`anyref` params/results ([codegen/mod.rs:3559-3573](../dovetail/src/compiler/codegen/mod.rs#L3559-L3573)). A flattened tuple cannot occupy a single `anyref` slot, so it must be boxed when it crosses into:

- a **generic type-parameter slot** (e.g. `Option<(Int, Bool)>` payload, `List<(Int, Bool)>`);
- a **closure** parameter, result, or captured environment field;
- a **trait vtable** `Self`-typed or erased slot;
- an **enum payload** field;
- the **`Any`** type.

At all of these, the boxed `(ref $TupleN)` is a subtype of `anyref`, so storing it is "free" (no instruction) and reading it back is a `ref.cast` to the concrete tuple struct — identical to how reference types already cross the boundary.

### 4.3 Consequence: indirect calls stay boxed

Because closures and vtables are already `anyref`-based, **any tuple flowing through an indirect call is boxed**. This deliberately bounds the blast radius of the flattened ABI to **direct calls of statically-known functions**. A function `f(t: (Int, Bool))` that is also taken as a first-class value needs a small adapter (unbox the tuple, then call the flattened `f`) at closure-creation time — the same adapter machinery that already reconciles scalar params with the all-`anyref` closure ABI (see §9.4).

---

## 5. Position-by-position

| Position | Representation | Mechanism / change |
|---|---|---|
| Direct-call **parameters** | **flattened** → N params | Each tuple param expands to N WASM params; call sites push N values in order. Arity changes; types are static so this is mechanical. |
| Function **return** | **flattened** → multi-value results | `vec![type_to_valtype(ret)]` becomes `type_to_valtypes(ret)`; encoder already accepts a `Vec<ValType>` of results. |
| **Locals** / `let` bindings | **flattened** → N locals | A binding maps to a *run* of locals + a width, not a single local (see §6). |
| Concrete **record / class fields** | **flattened** into the struct | A tuple field becomes N consecutive struct fields; field access uses a Dovetail-field → wasm-field-range map (see §8). |
| **Newtype** over a tuple/`Uint128` | **flattened** (transparent) | Newtypes are already transparent in codegen; `newtype Uint128 = (Int, Int)`-style wrappers inherit the pair representation for free. |
| **Array** element | **boxed** | §4.1 — irreducible. Box on store, unbox on read. |
| **Closure** param / env, **vtable** slot | **boxed** | §4.2/§4.3 — reuse existing anyref adapter. |
| **Enum** payload, **`Any`** | **boxed** | §4.2 — `(ref $TupleN)` as anyref subtype; `ref.cast` to read back. |

---

## 6. The local / binding model

This is the largest mechanical change. The function emitter currently assumes **one binding = one local of one `ValType`** (`VarName → u32`). Under flattening a binding occupies an **ordered run of locals**:

```
VarName → (base_index: u32, valtypes: SmallVec<[ValType; 2]>)   // width = valtypes.len()
```

- **Allocation:** `define_local` for a tuple-typed binding allocates `width` consecutive locals.
- **Load (`local.get`):** pushes all `width` locals in order, leaving the flattened value on the stack.
- **Store (`local.set`):** pops `width` values and writes them into the run; because the stack is LIFO, the run is filled in **reverse** index order (last element first) or via a small temp-local dance.
- **Width-1 bindings** (all scalars, refs) behave exactly as today — no regression for non-tuple code.

`Unit` width is a decision point (§7). Pattern destructuring (`let (x, y) = t`) becomes trivial in the unboxed case: the tuple is already N values, so each sub-binding simply takes its slice of locals — no `struct.get` ([cf. expressions.rs:1221-1269](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L1221-L1269)).

### 6.1 Field access on a flattened expression

A subtlety from the research note: `f()._1` where `f` returns a flattened multi-value tuple puts N values on the stack at once, but we only want one. The general rule: **when a flattened tuple expression is consumed by an element access, spill the run to temp locals, then `local.get` only the requested element(s).** For the common case where the tuple is already in a local run, `._i` is a single `local.get` of `base + offset(i)` — strictly cheaper than today's `struct.get`.

---

## 7. `Unit` and edge cases

- **`Unit` width.** Cleanest is width-0 (a tuple element of type `Unit` contributes nothing), but `Unit` is `i32` today across the whole codebase. To bound scope, **keep `Unit` as width-1 `i32` initially**; revisit a width-0 `Unit` as a separate change. (A width-0 `Unit` interacts with function returns, every `;`-discarded expression, and match — too broad to fold in here.)
- **Tuple arity.** Tuples are always ≥2 elements (`tuples-design.md` §2); no 0/1-arity tuples. `(x)` is a parenthesized expression.
- **Nested tuples** flatten transitively (§2).
- **Equality / hashing / printing.** When these route through generic or trait machinery they hit the `anyref` boundary and box anyway, so they require no special unboxed handling. Structural element-wise equality on an unboxed tuple compares the flattened values directly.
- **Debug info.** A flattened tuple local maps to several WASM locals; the DWARF/name-section view of a tuple variable becomes a composite over multiple locals. Minor; can degrade gracefully (show the run).

---

## 8. Concrete struct-field flattening

When a tuple appears as a **concrete** record or class field (not erased to `anyref`), the tuple's elements are spliced into the containing struct as consecutive fields. For `record R = a: Int, t: (Int, Bool)` the WASM struct becomes `{ a: i32, _t0: i32, _t1: i32 }`.

- `build_record_subtype` ([records.rs:10-31](../dovetail/src/compiler/codegen/records.rs#L10-L31)) flattens each field via `type_to_valtypes` instead of `type_to_valtype`.
- A **Dovetail-field-index → wasm-field-range** map is needed so that `r.t` (whole tuple) pushes the N `struct.get`s and `r.t._0` is a single `struct.get` at the right wasm index. This map replaces the current 1:1 field-index assumption in `RecordCreate`/`FieldAccess` ([expressions.rs:429-459](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L429-L459), [:518-593](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L518-L593)).
- Dovetail records are immutable (`build_record_subtype` emits `mutable: false`), so there are no in-struct aliasing concerns from splicing.
- A field whose type is a **generic parameter** instantiated to a tuple is **boxed**, not flattened (it lives in an `anyref`/erased slot) — only statically-concrete tuple fields are spliced.

---

## 9. Box / unbox coercions

The representational seams in §4 require a single, well-defined pair of operations:

- **box(tuple):** materialize the flattened run into a `(ref $TupleN)` via `struct.new` (the elements are already on the stack/in locals).
- **unbox(tuple):** explode a `(ref $TupleN)` into the flattened run via N `struct.get`s (after a `ref.cast` if arriving as `anyref`).

### 9.1 Where coercions are inserted

| Crossing | Coercion |
|---|---|
| Flattened value → array store (`array.set`/`array.new`) | box |
| Array read → use as flattened | unbox |
| Flattened arg → generic/closure/vtable param (anyref) | box |
| anyref result/field/payload → use as flattened | `ref.cast` + unbox |
| Flattened value → enum payload / `Any` | box |
| Tuple-param function taken as a closure value | adapter: unbox then call flattened (§9.4) |

### 9.2 Implicit-in-codegen vs. explicit AST node

Today boxing across the `anyref` boundary is done **implicitly in codegen** at specific seams (small scalars via `ref.i31`, reference types via subtyping); there is **no explicit coercion node in the typed AST**. Two options for tuples:

1. **Implicit codegen helper** — a `coerce(value, source_repr, target_slot)` invoked at each seam, deciding box/unbox/`ref.cast` from the static source and target types. Smallest change; matches current style.
2. **Explicit `Box`/`Unbox` typed-AST nodes** inserted by a pass after inference. More work, but makes the boundary auditable and keeps the emitter dumb.

**Recommendation:** start with the implicit codegen helper (option 1) for consistency with existing boxing; promote to explicit nodes only if the seams prove hard to reason about.

### 9.3 The `i64`-in-`anyref` gap

Small scalars box into `anyref` via `i31ref`, but **`i64`/`f64` boxing into `anyref` is a known gap** in the current codegen. This matters directly: a `Uint128` (two `i64`) or any `i64`-containing tuple that needs to box would hit it. The **boxed tuple struct sidesteps the gap** — an `i64` lives fine as a *struct field*; it is only `i31`-style direct boxing of a bare `i64` that is unimplemented. This must be verified before relying on `Array<Uint128>` working on day one.

### 9.4 Closure/vtable adapter

A function with a flattened tuple parameter has an arity/type that does not match the all-`anyref` closure/vtable signature. When such a function is taken as a first-class value, a thin adapter wraps it: receive the boxed tuple as `anyref`, `ref.cast` + unbox to the flattened run, then call the real (flattened) function. This is the same adapter strategy that already reconciles scalar params with the closure ABI; tuples extend it rather than introducing a new mechanism.

---

## 10. WASM multi-value support

- Multi-value results are part of the WASM core spec and **enabled by default in Wasmtime** (and all major runtimes); no `Config` change is required beyond what is already set (`wasm_gc`, `wasm_function_references`, `wasm_component_model` — see [runner.rs](../dovetail/src/runner.rs)).
- `wasm-encoder` already accepts a `Vec<ValType>` for results in `types.ty().function(params, results)` — **zero encoder changes**.
- **Component boundary is unaffected.** Dovetail functions returning tuples are *internal core-wasm* functions, not component exports; the canonical ABI (which flattens-then-spills to linear memory at the WASI boundary) never sees a Dovetail tuple return. `main(): Unit` returns nothing relevant. So internal multi-value returns and the component model do not interact.

---

## 11. Relationship to `Uint128`

`Uint128` (crypto doc §3) is the motivating consumer and the simplest instance of this design:

- Its WASM representation `[i64, i64]` (lo, hi) is exactly a width-2 flattened tuple.
- As a **local / param / return**, it is two `i64` on the stack — never allocates. This is what the crypto inner loops require.
- As an **array element** (`Array<Uint128>`) or in an **erased generic slot**, it boxes — gated by the `i64`-in-`anyref` analysis in §9.3 (the boxed struct holds two `i64` fields, sidestepping bare-`i64` `i31` boxing).
- Its arithmetic (`i64.add128`, `i64.sub128`, `i64.mul128`, `i64.mul_wide`) is **not** part of this design — it is custom lowering layered on top of the pair representation, specified in the crypto doc. This design only guarantees the pair can live unboxed everywhere a tuple can.

Two sequencing options, which **converge**:

- **General-first:** build tuple flattening + multi-value + box/unbox here, then `Uint128` is a width-2 value with custom arithmetic — efficient as field, param, and return automatically.
- **`Uint128`-first (narrow):** special-case a 2×`i64` stack value — but the moment it must be a record field, param, return, or array element, it rebuilds slices of this same machinery. The narrow path does not stay narrow.

This is the argument for doing the general tuple work: it is the substrate that makes `Uint128` cheap *everywhere it appears*, not just inside a single function.

---

## 12. Affected compiler components

| Concern | Location | Change |
|---|---|---|
| Type → value-type lowering | [codegen/mod.rs:1085-1177](../dovetail/src/compiler/codegen/mod.rs#L1085-L1177) | Add `type_to_valtypes` (flattening); keep `type_to_valtype` for boxed/single-slot use |
| Function signature emission | [codegen/mod.rs:1910-1925](../dovetail/src/compiler/codegen/mod.rs#L1910-L1925) | Flatten params; multi-value results via `type_to_valtypes(return)` |
| Local / binding model | [function_emitter/mod.rs](../dovetail/src/compiler/codegen/function_emitter/mod.rs) | `VarName → (base, valtypes)`; runs of locals; ordered get/set (§6) |
| Tuple construction / access | [expressions.rs:429-459](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L429-L459), [:518-593](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L518-L593) | Unboxed: build/consume runs; field access = `local.get`/`struct.get` at mapped offset |
| Destructuring / match | [expressions.rs:1221-1269](../dovetail/src/compiler/codegen/function_emitter/expressions.rs#L1221-L1269), match_expression.rs | Unboxed: slice the run; boxed: unbox first |
| Record/class struct layout | [codegen/records.rs:10-31](../dovetail/src/compiler/codegen/records.rs#L10-L31) | Splice tuple fields; Dovetail-field → wasm-field-range map (§8) |
| Array element storage | [codegen/mod.rs:1984-1990](../dovetail/src/compiler/codegen/mod.rs#L1984-L1990) | Unchanged (always boxed) |
| Box/unbox coercions | new (codegen helper) | Insert at the seams in §9.1 |
| Monomorphize tuple `RecordTypeDef` | [monomorphize/mod.rs:1177-1203](../dovetail/src/compiler/monomorphize/mod.rs#L1177-L1203) | Unchanged — still emitted, now as the *boxed* form |

The typechecker and inference layer are **unchanged**: `Type::Tuple` stays as-is, and the tuples-as-records lowering (`tuples-design.md` §6) still produces the boxed struct. This is purely a codegen-representation change.

---

## 13. Original questions and their disposition

1. **`Unit` width:** Retained as width-1 `i32`; width-0 is not required.
2. **Coercion mechanism:** Implemented through codegen boxing/unboxing helpers.
3. **`i64`/`f64` in `anyref`:** Primitive boxing is implemented; tuple and `Uint128`
   array/erased-boundary regressions cover the relevant paths.
4. **Debug-info fidelity:** Presenting a flattened tuple as one composite debugger
   variable was exploratory; completion is not established by this audit.
5. **Indirect-call adapters:** Implemented; tuple regressions cover function values,
   closure captures, virtual calls, and interface calls. The original claim that
   every vtable boundary must box is superseded by signature-specific lowering.

---

## 14. Original implementation plan (functional phases complete)

Phased so each step is independently testable. The highest-value, lowest-risk slices come first; the data-structure-flattening and boxing-boundary work comes last.

| Phase | Scope | Risk |
|---|---|---|
| **1 — Flattening primitive** | Add `type_to_valtypes`; keep `type_to_valtype`. No behavior change yet (still struct everywhere). Unit tests on the flattening of nested tuples / `Uint128`. | Low |
| **2 — Multi-value returns** | Return position only: `type_to_valtypes(return)` for results; callers consume N values. Highest value, smallest surface. | Low–Med |
| **3 — Flattened params + locals** | Flatten direct-call params; the binding/local run model (§6); destructuring as slicing; `._i` as `local.get`. | Med |
| **4 — Concrete struct-field flattening** | Splice tuple fields into record/class structs; field-range map (§8). | Med |
| **5 — Box/unbox boundaries** | Coercion helper (§9); array store/read, enum payload, `Any`, generic/closure/vtable crossings; the closure adapter (§9.4); resolve the `i64`-in-anyref question (§9.3). | High |
| **6 — `Uint128` on top** | Layer wide-arith lowering (crypto doc §3) onto the width-2 pair; `Array<Uint128>`; benchmarks vs. the manual-decomposition baseline. | Med |

**Dependencies:** Phase 1 is the base for all. Phases 2 and 3 are independent of each other and can land in either order. Phase 4 depends on 1/3. Phase 5 depends on 1/3/4 and is the gate for arrays/generics of tuples and for `Uint128` (Phase 6).

---

## 15. Summary

| Topic | Design |
|---|---|
| **Idea** | Tuples lower to a *sequence* of WASM values (unboxed), not always a struct. |
| **Primitive** | `type_to_valtypes(&Type) -> SmallVec<ValType>`; tuples flatten transitively; `single_val_type` supplies boxed single slots. |
| **Unboxed where** | Direct-call params, returns (multi-value), locals, concrete record/class fields, destructuring. |
| **Boxed where** | Array elements, `anyref`-erased positions (generics, closures, vtables, enum payloads, `Any`). |
| **Coercions** | box (`struct.new`) / unbox (`ref.cast` + `struct.get`) inserted at the seams; implicit codegen helper to start. |
| **Returns** | WASM multi-value; on by default in Wasmtime; encoder already supports it; component boundary unaffected. |
| **Typechecker** | Unchanged — purely a codegen-representation change. |
| **`Uint128`** | The smallest unboxed tuple (`[i64, i64]`); this design is its substrate; arithmetic lowering lives in the crypto doc. |
