# Interface Objects Design (Consolidated)

This document designs **interfaces** in Dovetail and the **removal of trait objects** in one place, and carries the single implementation plan for both plus the completion of the trait system. It consolidates and supersedes [interfaces-design](interfaces-design.md) and [trait-objects-removal-design](trait-objects-removal-design.md). It aligns with [traits-design](traits-design.md), [trait-design-appendix](trait-design-appendix.md), and [grammar](../grammar.md).

**One-sentence rule:** *Write `interface` instead of `trait` when you want to use the type as a runtime value; the compiler then checks its methods are dispatchable.*

**Status:** Phases 1–5 implemented — keyword + declaration-time check, stdlib migration, type-position gate, intersection `and` with static subset upcasts, `extends` (flattened members, nested super vtables), default method/property bodies, explicit `TraitName.method(self, …)` disambiguation, resolution priority per [appendix §2.1](trait-design-appendix.md), the coherence (no-overlap) rule, and the mechanical `TraitObject` → `InterfaceObject` rename (wasm keys are now `$IfaceObj$…`). Phase 6 (book) implemented in Part 6 and Part 8.

---

## 1. The Model

`trait` and `interface` are the **same construct**. The compiler parses, collects, infers, and resolves them identically; an `interface` is a `trait` that additionally:

1. **passes the object-safety check** (§4), enforced at its declaration; and
2. **may be written in type position**, where it denotes a fat-pointer **interface object** (§5) with dynamic dispatch.

A plain `trait` is **bound-only**: full type-class power (associated types, generic methods, free `Self`), resolved by static dispatch at monomorphized call sites, and it may **not** appear in type position. An `interface` may be used **both** as a bound *and* as a runtime value.

### 1.1 What is identical to a trait

Per [traits-design](traits-design.md), with **no difference** for interfaces:

- **Who may implement:** any type — records, enums, newtypes, primitives, intrinsics, classes. Via `implement I for T` blocks or `class C implements I`.
- **Bounds:** `where T: I` — monomorphized, static dispatch.
- **Generics:** generic interfaces (`interface Iterator<T>`), generic impl blocks, bounds on blocks.
- **`extends`, default methods, name resolution / disambiguation, orphan rule:** [trait-design-appendix §1–§4](trait-design-appendix.md), with the one `extends` refinement in §6 below.

"Implementing an interface is free" means literally this: an implementer does nothing different than for a trait. There is no separate interface-implementation syntax or mechanism.

### 1.2 What is *not* simplified

This model does **not** simplify inference or codegen. The interface object reuses the entire trait-object representation — the two-field fat pointer `(data: (ref any), vtable: (ref $Vtable))`, wrappers, the single calling convention `(anyref self, erased params…)`, de-monomorphized vtables ([traits-design §8.3–8.5](traits-design.md); note that document's `type_info` field was never built — the implementation erases via `anyref` alone). The benefit is **user-facing only**: one concept, a trivial `trait`-vs-`interface` decision, and zero new rules for implementers.

---

## 2. Why Remove Trait Objects

Trait objects make "can this be a runtime value?" an **implicit, use-site** property of a full-power trait — you discover at the use site whether the trait is object-safe (today: three per-method errors raised where the call happens). The `interface` keyword turns that into an **explicit, declaration-time** opt-in: the object-safety check runs because you wrote `interface`, and the error lands on the declaration. Same capability, same codegen; only the gate moves.

Because an interface **is** a trait (same construct, any type may implement it), this loses **no capability**: every value-level thing a trait object can do, an interface object can do, including dynamic dispatch over non-class types.

| Before | After |
|---|---|
| Any object-safe `trait` in type position → trait object | Only an `interface` in type position → interface object; a plain trait there is an error |
| Object-safety discovered as a **use-site** error, per method call | Object-safety checked at the **interface declaration**, with a local error (§4) |
| `Display and Equatable` (trait intersection) in type position — designed, never implemented | `I1 and I2` (interface intersection, §5.1) — implemented **only** for interfaces; trait intersections never exist |
| To box a type, nothing extra — any object-safe trait works | To box a type, declare its contract `interface` (explicit opt-in) |

**Almost no capability is dropped.** The traits that can no longer appear in type position are those that were never object-safe (`Self`-in-params, generic methods — `Equatable`, `From<T>`, `Awaitable<T>`, …); calling those methods through a trait object was already an error. They remain usable as **bounds**. The one deliberate drop: a trait *declaring* an associated type could previously be boxed and have its assoc-type-free methods called; an interface may not declare associated types at all, so such a trait is now bound-only.

### 2.1 Why generic methods can't be dynamic

A monomorphizing AOT compiler has neither a JIT (C#, which instantiates generic virtual methods at runtime) nor type erasure (Java, one boxed impl). A generic method reached through a vtable would need every `(impl, T)` pair known at compile time, but the call site holds an erased receiver, so the `T` set is not enumerable — the same wall NativeAOT/IL2CPP hit. Hence interface methods are non-generic, enforced at the declaration (§4). (`Self`-return is *not* in this category — it is allowed and reinterpreted as the interface type, §7.)

---

## 3. Syntax

Identical to `trait_decl` with the keyword `interface`:

```
interface_decl = [ doc_comment ] "interface" IDENT [ type_params ]
                 [ "extends" interface_list ] [ where_clause ] "=" trait_body
```

The body grammar (`trait_body`, `trait_method`, default methods, properties) is **shared with traits** — there is no separate interface grammar. Whether a body is legal for an interface is decided by the §4 *check*, not by a different grammar.

```dovetail
interface Iterator<T> =
    function next(self): Option<T>

interface Drawable =
    function draw(self, canvas: Canvas): Unit
    function boundingBox(self): Rect
```

---

## 4. The Object-Safety Check ("Trait-Safe")

Run during the **Rules** phase on every `interface` declaration. Each rule, if violated, errors **at the offending method on the interface** (not at a use site).

| Rule | Why it can't be in a vtable | Error |
|---|---|---|
| Every method takes `self` (no static/associated functions in the dispatch set) | A vtable dispatches on a receiver; nothing to dispatch on otherwise. | `interface methods must take self; put static functions on a trait` |
| No method type parameters (no generic methods) | See §2.1. | `interface methods cannot be generic` |
| No associated types / GATs ([appendix §5](trait-design-appendix.md)) | Would have to be recovered from an erased receiver. | `interfaces cannot declare associated types; use a trait` |
| `Self` only as the receiver or the return type | `Self` elsewhere (`other: Self`) needs both values to be the *same* concrete type — unknowable behind a fat pointer (the `Equatable` problem). | `Self may appear only as the receiver or return type in an interface method` |

**A bare `Self` return is allowed and reinterpreted (§7)** — the vtable wrapper re-boxes the concrete return into the declaring interface's object at the boundary; on an intersection receiver the result is the declaring **component's** type. `Self` **nested** in a return (`Option<Self>`, …) cannot be re-boxed and is rejected at the declaration. This is *more* permissive than Rust trait objects and enables fluent/builder patterns dynamically.

Once `extends` exists (§6), the check runs transitively over the interface's supers' method sets.

**Consequence:** a trait that would fail this check can never be an interface — `Equatable`, `Comparable`, `From<T>`, `Awaitable<T>`, `EarlyReturn<T>` stay traits (bound-only). A trait that *would* pass is still not boxable until you change its keyword to `interface` — the opt-in is explicit.

---

## 5. Interface Objects (Type Position)

When an interface name appears in **type position**, it denotes an **interface object**: a fat pointer whose concrete type is hidden behind a vtable. No `dyn` keyword. A plain trait name in type position is an **error** directing the user to `interface`.

```dovetail
function render(shapes: Array<Drawable>): Unit = ...   // heterogeneous Circle, Square, ...
function hashFor(cs: CipherSuite): HashAlgorithm = ... // runtime-chosen concrete type
```

- **Generic interfaces** apply their type args: `Iterator<Int32>`.
- **Coercion:** a value implementing the interface is **implicitly** coerced to the interface object where one is expected (argument, return, field, assignment). No explicit cast syntax.

Because interfaces are implementable by **any** type, an interface object may wrap a record, enum, primitive, intrinsic, or class.

**Representation:** unchanged from trait objects — a two-field fat pointer `(data: (ref any), vtable: (ref $Vtable$I))`, one struct + vtable per interface (`$IfaceObj$pkg.I`, de-monomorphized: type args erased in the slot signatures), one calling convention `(anyref self, erased params…) -> Ret`, wrappers for generic/non-generic impl sources (impl blocks and classes).

**Current `Any` limitation:** assigning an interface-object value to `Any` preserves the fat-pointer value. A concrete type test such as `erased is Rec` therefore returns false even if that interface wraps a `Rec`; assigning the `Rec` directly to `Any` preserves its concrete identity. Casting from `Any` to an interface object and testing an interface target are unsupported. Transparent recovery of the wrapped concrete identity would be a separate representation/semantics change, rather than a consequence of ordinary interface coercion.

**Sibling generic applications:** boxing different applications of the same interface uses `$inst$`-tagged wrapper and vtable-global keys so their implementations remain distinct. Sharing those wrappers across applications would require preserving that dispatch distinction explicitly.

### 5.1 Intersection: the `and` type

`I1 and I2` in type position denotes an interface object implementing **both** interfaces. `and` is used only in interface-object types and `extends`/`implements` lists.

- **Components must be interfaces.** Because intersection is implemented *after* the type-position gate (§10 phase 4), trait intersections never exist — the components are validated by the same resolution path that gates single names.
- **Subtyping:** `(A and B) <: A` and `(A and B) <: B` — an intersection value can be used where a single component is expected.
- **Coercion:** a value is coerced to `A and B` only if it implements all components.
- **Representation (as implemented — sub-vtable design):** the intersection's fat pointer keeps the same two-field shape `(data, vtable)`; its vtable struct `$Vtable$A&B` holds one immutable ref **per component vtable** (sorted by FQN). Component vtables and wrapper functions are reused verbatim — no new wrappers. A method call through an intersection costs one extra `struct.get` (set vtable → component vtable). **Upcasting `(A and B) → A` (or any strict subset) is fully static:** extract the data field plus the needed component-vtable refs and `struct.new` the target fat pointer — every index is known at compile time (`InterfaceObjectUpcast` node). Coercion globals init with nested `struct.new` const-exprs, reusing the per-component wrapper functions.
- **Method resolution:** a method/property on an intersection receiver must be declared by exactly **one** component; several → `ambiguous method` error (explicit disambiguation is a Phase-5 feature); none → the concrete-type fallback pipeline.
- **What else works:** `as` from an intersection to a component upcasts; an interface-object value satisfies a generic bound `T: I` when `I` is one of its components (the bound-dispatched call is rewritten to dynamic dispatch at monomorphize); `if`/`match` branches mixing an intersection with a subset get per-branch upcasts; a bound type parameter (`T where T: I`) coerces implicitly to `I`.
- **Deliberate gaps:**  an interface's OWN member always wins over a named extension declared `for` the interface type (on interface-object receivers, the vtable member is the interface's contract; the §2.1 priority applies to concrete receivers);  a trait sharing a name with a record/enum cannot use the explicit `TraitName.method(...)` escape hatch (the type claims the name — rename or alias the trait to disambiguate); interface-object types in **variance-checked generic argument positions** must match exactly (`List<A and B>` is not a `List<A>`, `Option<Concrete>` is not an `Option<I>` — per-element fat-pointer conversion cannot be reified; coerce values before building the container/enum, e.g. `let a: I = value; Some(a)`; branch unification will not launder mismatched generic args either); a bound `T: I` rejects an intersection instantiation when `I` declares a bare-`Self`-returning member (dynamic dispatch narrows `Self` to the component, which cannot honor `T`); methods from `implement X for I` (an impl block whose for-type is an interface) resolve on `I` receivers but not on intersections containing `I` — upcast first (`(v as I).method()`); no least-upper-bound between different interface-object types (branch unification falls back to one branch's type; annotate when needed); interface-object types inside **function types** must match exactly — function values have no coercion adapter, so `(A and B) => Unit` does not accept an `(A) => Unit` value or vice versa; `implement … for` and `extension … for` an intersection are rejected (declare against a single interface).

### 5.2 Obsolete proposal: argument-position bound sugar

This optional proposal is retired from the current scope. Argument-position
interfaces remain interface values; use an explicit generic bound for static
polymorphism. The paragraph below records the original alternative.


A future ergonomic option (orthogonal to this design): treat a bare interface in **argument** position as sugar for a generic bound — `foo(xs: Iterable<T>)` ⟶ `foo<C>(xs: C) where C: Iterable<T>` — giving static dispatch and skipping the box when the concrete type is known at the call site, while return/field positions remain real interface objects. This mirrors Rust's `impl Trait` (static, arg) vs `dyn Trait` (dynamic). **Not required** by this model; listed as a possible later optimization.

---

## 6. `extends` (as implemented)

`trait B extends A and C` / `interface B extends A` — supers are named types, `and`-separated, generic supers allowed (`trait B<T> extends A<T>`). The one refinement: an **interface may extend only interfaces** (`an interface may extend only interfaces; 'X' is a trait`); a trait may extend traits or interfaces.

**Model — flattened members, inline implementation required.** Collect physically flattens every super's members (transitively, substituted into the extender's type params) into the extender's signature, each tagged with its **origin** (the ultimately-declaring trait). Consequences:

- `implement B for T` must implement B's FULL flattened set inline. A separate `implement A for T` contributes nothing to B's completeness and may coexist independently ("A is on its own").
- **B satisfies A everywhere**: implementing B makes T satisfy the bound `T: A` and coerce `let a: A = t`, with A's members dispatched to the B block's inline members. Tie-break: a **direct** `implement A for T` always wins in A-contexts; with no direct impl, a **unique** sub-trait provider routes; several distinct providers are an error (`ambiguous implementations of trait 'A' for type 'T': provided by both 'B' and 'C'; implement 'A' directly to disambiguate`).
- Appendix §1.2 same-name rules run during flattening: redeclaring an inherited signature without a body is an error; **with a body it is a default-implementation override** (the member keeps the inherited origin/vtable slot, the override's body wins for implementors that omit it); different params = distinct members in the design, but rejected in v1 because dispatch is name-keyed (appendix §1.2); same params + different return = error (also across two supers). Diamonds dedupe by origin. Cycles are detected (`'extends' cycle detected: 'A' -> 'B' -> 'A'`).
- Classes: `class C implements B` checks the flattened set (class methods are name-based, so inherited members need no special casing) and satisfies `T: A` via the closure.

**Representation — nested super vtables.** `$Vtable$B` holds one immutable non-null ref per **direct super's** vtable (declaration order) before B's own member slots (slot indices offset by the super count; zero supers = byte-identical to the historical layout). Slot identity for an inherited member always comes from the origin trait's **raw** signature. Consequences:

- **Upcast `B`-object → `A`-object is fully static**: extract the data field plus the nested `$Vtable$A` ref (a chain of `struct.get`s for transitive supers) — the `InterfaceObjectUpcast` node covers subset upcasts and extends-upcasts uniformly, including from intersections (`(B and X) → A`).
- A call to an inherited member on a `B`-object navigates the super refs to the origin's vtable, then does today's slot dispatch. The node is keyed by the **origin** component.
- Concrete → `B` coercion builds the nested vtables bottom-up in one constant expression (groups in DFS post-order; the same provider block backs the whole tree). When a different trait's block provides a super's slots, the coercion group key carries a `$via$<provider>` tag so its wrapper names never collide with a direct coercion's.
- Standalone per-super vtable globals are emitted alongside (as for intersection components) so bare-`Self`-returning inherited members re-box into their origin's interface object. When the concrete type ALSO implements the super directly, the direct impl's vtable owns that (type, super) global deterministically — a re-boxed super object is a direct-super context, so the direct impl wins there (consistent with the tie-break above, and independent of coercion order).

Why nested rather than flattening super slots into `$Vtable$B`: all GC types live in one recursion group, where structurally-identical function types at different indices are **distinct** iso-recursive types — funcrefs extracted from `$Vtable$B` could not populate `$Vtable$A`'s fields. A nested ref extracts with the exact type.

---

## 7. `Self` Semantics (Dual)

- **Bound context** (`T: I`, monomorphized): `Self` = the concrete type. `clone(self): Self` returns the concrete type, static dispatch.
- **Interface-object context** (dynamic): a `Self`-typed **return** is observed as the **interface type**; the vtable wrapper for concrete type `C` returns a `C`, re-coerced to an `I` interface object at the boundary.

`Self` outside receiver/return is rejected by §4, so no other case arises.

---

## 8. Choosing `trait` vs `interface`

| You need | Use |
|---|---|
| A value whose concrete type is hidden behind a vtable: heterogeneous collections, runtime-chosen returns, plugin fields, iterator abstraction | **interface** |
| Type-class power (`equals(self, other: Self)`, `from(value: T): Self`, associated types, generic methods); static dispatch is enough | **trait** |
| A contract you might use *either* way | **interface** — it is usable as both a bound and a value, at the cost of the §4 restrictions |

And the other tools, unchanged: **enum** for a closed set you own; **abstract class** for shared state + single is-a hierarchy. Interfaces own stateless capability, multiply implemented, dynamically dispatched, over any type.

---

## 9. Historical Implementation State (before phases 1–6)

This is the pre-implementation snapshot, retained to explain the migration.
It is obsolete as a status report; phases 1–6 below are complete. See the
[requirement audit](trait-implementation-status.md) for implementation evidence
and explicitly retired proposals.

- **Trait objects end-to-end** (single trait): fat pointer, vtables, wrappers, implicit coercion, codegen, LSP support — spread across ~30 files in all pipeline phases, including `desugar_for` (for-loops box `Iterator`) and `desugar_await`.
- **Object safety at the use site:** three per-method errors in `typechecker/infer/function_expressions.rs` (generic method, `Self`-typed non-self parameter, associated-type reference); `Self`-return already coerces back to the object type. This is exactly the §4 rule set, minus the "every method takes self" rule — it just runs in the wrong place.
- **Intersection `and`:** parsed (`TypeExpr::Intersection`) but rejected in all three lowering sites ("not yet supported"); no `Type` variant, no codegen.
- **Not implemented:** trait `extends` (no field on `TraitDecl`), default method/property bodies in traits, explicit `TraitName.method(self, …)` instance disambiguation (statics through traits exist), overlap/coherence check.
- **Associated types / GATs:** implemented (parser, collect, infer, registry).

**Consequence for approach:** keep the machinery, gate it. Rebuilding the fat pointer buys zero capability and risks regressions in code that doesn't look trait-object-related (for-loop and await desugaring). And removal cannot go first — deleting trait objects before `interface` exists breaks the workspace (`Iterator` boxing, crypto/log/tls returns). The only order that keeps `cargo run -- check` green at every step is **add → migrate → gate**.

---

## 10. Completed Implementation Plan

One plan covering: removing trait objects + use-site object safety, adding `interface`, completing [traits-design](traits-design.md) / [trait-design-appendix](trait-design-appendix.md), and updating the book. Phases 1–3 are one contiguous chunk (each keeps the workspace compiling, but 3 without 1–2 breaks everything). Phase 4 and phases 5.3/5.4 are independent of each other; 5.1 → 5.2 must stay ordered; phase 6 trails 4 and 5.

### Phase 1 — `interface` keyword + declaration-time safety check ✅ (implemented)

- Lexer: `interface` keyword. Parser: reuse `parse_trait_decl` body grammar; add `is_interface: bool` (or a `TraitKind`) to `TraitDecl`.
- Collect/infer: identical to trait; registry records interface-ness.
- Rules phase: the §4 check on every `interface` declaration. Lift the existing use-site logic to declaration level; add the "every method takes `self`" rule. Keep the use-site checks for now — plain traits remain boxable during this phase.
- Tests: declaration-level errors land on the offending method; an interface works as both a bound and an object.

### Phase 2 — stdlib migration (mechanical) ✅ (implemented)

Re-declare the dynamically-used traits as `interface` (all pass §4 today):

- `Iterator<T>` — prelude (for-loop desugar boxes it)
- `HashAlgorithm`, `AeadAlgorithm`, `KeyExchange` — `standard-crypto`
- `LogSink` — `standard-io-log`
- `standard-tls` returns (`hashFor`, `aeadFor`, …)

Leave bound-only traits (`Iterable<T>`, `Equatable`, `Comparable`, `From<T>`, `Awaitable<T>`, `EarlyReturn<T>`, …) as `trait`. Workspace stays green because traits are still boxable.

### Phase 3 — close the gate (trait-object removal) ✅ (implemented)

- Type-position resolution (collect + infer): trait name → interface-object type **only if** declared `interface`; a plain trait there is an error suggesting `interface`.
- Delete the three use-site object-safety errors — unreachable once only checked interfaces are boxable.
- Update error text; reframe [traits-design §8](traits-design.md) as interface objects.

### Phase 4 — intersection `and` (interface-only by construction) ✅ (implemented)

- Un-reject `TypeExpr::Intersection`; extend the trait-object `Type` variant to carry a sorted interface set (rather than adding a parallel variant).
- Assignability/subtyping per §5.1; coercion requires implementing all components.
- Codegen: combined vtable per (impl type, interface set); upcast coercion re-boxes with the component's vtable (§5.1 decision).

### Phase 5 — complete traits-design and the appendix ✅ (implemented)

All five items landed (2026-09):

1. **`extends`** — flattened-member model with origin tracking, nested super vtables, direct-impl-wins tie-break (§6). Note the decided refinement vs the appendix's original wording: the flattened set must be implemented **inline**; a separate super impl does not feed the sub-trait's completeness.
2. **Default methods/properties** — bodies parse in `trait_body`; checked ONCE in the trait's own scope with `Self` as a trait-bounded type variable, stored as templates (`{trait}$$default${member}`), materialized per implementor at monomorphize under the standard impl-member names. Impl blocks and classes, generic or non-generic, may omit defaulted members. Generic methods may have default bodies, while remaining ineligible for interface-object dispatch. Classes must define static trait members explicitly. A class must also define a member explicitly when different default declarations, or different generic applications of one default declaration, would supply the same signature; the same default application reached through multiple supers is shared. §1.2 override rules implemented (a redeclaration with a body overrides the super's default; the impl's own definition beats every default).
3. **Explicit disambiguation** — `TraitName.method(receiver, …)` and `NamedExtension.method(receiver, …)` on concrete, type-param, and interface-object receivers (an ambiguous intersection member dispatches via `A.foo(v)`); trait statics select the unique implementing type. Resolution priority aligned to [appendix §2.1](trait-design-appendix.md): module → named extension → trait impl, for both instance methods and statics. Cross-trait / cross-extension / cross-bound ambiguities are reported with the trait names and the explicit-call escape hatch.
4. **Coherence** — a rules-phase check over the merged registry: two implement blocks whose for-types (and trait args) unify are an overlap error naming both locations and a witness type; where-bounds never disprove overlap. Sibling instantiations (`Tr for List<Int32>` / `Tr for List<String>`) are legal — their members mangle with the full for-type segment, and shaped generic blocks (`<T> Tr for Pair<T, Int32>`) carry their shape in the segment (this fixed a pre-existing silent name collision between shaped sibling blocks).
5. Mechanical rename `TraitObject` → `InterfaceObject` in the compiler; wasm name prefix `$TraitObj$` → `$IfaceObj$`.

**Sugar through bounds:** `for` resolves a unique `Iterable<T>` application through direct or inherited bounds, including `Self` in default bodies. `try` similarly resolves `EarlyReturn<T>` when the bound determines its `OnFailure` associated type. Abstract `use` remains unsupported: `Usable<T, E>` alone does not determine the generic associated constructor `Wrapped<U, E2>`, so the compiler cannot check the continuation's wrapper. It reports that limitation at the `use` expression rather than claiming the bound is absent. Concrete `use` operands remain supported.

### Phase 6 — book update ✅ (implemented)

Interfaces are documented within the existing type system chapter; chapter
numbers remain unchanged.

- [Part 6, §6.11](../website/content/book/06-type-system.md#611-interfaces-and-interface-types)
  covers declarations, interface values, implicit coercion, heterogeneous lists,
  runtime-selected returns, dependency fields, generic interfaces and bounds,
  intersections and upcasts, `Self` returns, declaration restrictions, `Any`
  boundaries, and the trait-versus-interface decision table.
- [Part 8, §8.5–8.7](../website/content/book/08-traits.md#85-trait-and-interface-inheritance)
  covers shared inheritance, default methods and properties, implementation
  precedence, coherence, and explicit member disambiguation. Its introduction
  distinguishes traits from interface value types, and its property examples
  use receiver syntax.
- The [TOC](../website/content/book/toc.md), [introduction](../website/content/book/01-getting-started.md), and
  architecture discussions of ports in [Part 18](../website/content/book/18-project-structure.md#186-ports-and-adapters)
  and [Part 20](../website/content/book/20-application-layer.md) link to the type system section.

Validation: all 11 new or corrected runnable examples compile and execute with
the current compiler; the added section link targets exist.

---

## Grammar Summary

- **Interface:** `interface` IDENT [ type_params ] [ `extends` interface_list ] [ where_clause ] `=` trait_body — body grammar shared with `trait`.
- **Type position:** interface name (with type args) = interface object; `I1 and I2` = intersection interface object. Plain trait names are not allowed in type position.
- **Implements / bounds:** as for traits ([traits-design](traits-design.md)).

---

## References

- [traits-design.md](traits-design.md) — Traits: the shared construct, all implementation forms, bounds, and the fat-pointer/vtable mechanics interfaces reuse (§8).
- [trait-design-appendix.md](trait-design-appendix.md) — Extends, name resolution, default methods, associated types, object safety background (§6.1).
- [interfaces-design.md](interfaces-design.md), [trait-objects-removal-design.md](trait-objects-removal-design.md) — **Superseded** by this document.
- [grammar.md](../grammar.md) — Grammar.
