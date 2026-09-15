# Interfaces Design

> **Superseded** by [interface-objects-design.md](interface-objects-design.md), which consolidates this document with [trait-objects-removal-design.md](trait-objects-removal-design.md) and carries the implementation plan.

This document designs **interfaces** in Dovetail. The design is deliberately small: **an interface *is* a trait** — same grammar, same AST, same implementation machinery, same rules for who may implement it. An interface adds exactly two things to a trait:

1. an **object-safety check** ("trait-safe"), run at the interface declaration; and
2. **permission to appear in type position** — an interface value is a fat pointer (an **interface object**); a plain trait cannot be used this way.

Everything else — any type may implement it, `implement` blocks, class `implements`, bounds, generics, `extends`, default methods, the orphan rule, name resolution — is **identical to traits** and is specified in [traits-design](traits-design.md). This document specifies only the delta.

> **One-sentence rule:** *Write `interface` instead of `trait` when you want to use the type as a runtime value; the compiler then checks its methods are dispatchable.*

It aligns with [traits-design](traits-design.md), [trait-design-appendix](trait-design-appendix.md), and [grammar](../grammar.md). The migration that gates dynamic dispatch behind `interface` is in [trait-objects-removal-design](trait-objects-removal-design.md).

**Status:** Obsolete / superseded in full by [interface-objects-design.md](interface-objects-design.md), now implemented. The text below is historical, including its migration, representation, and optional future proposals; none are outstanding requirements. See the [audit](trait-implementation-status.md).

---

## 1. The Model

`trait` and `interface` are the **same construct**. The compiler parses, collects, infers, and resolves them identically; an `interface` is a `trait` that additionally:

- **passes the object-safety check** (§3), enforced at its declaration; and
- **may be written in type position**, where it denotes a fat-pointer **interface object** (§4) with dynamic dispatch.

A plain `trait` is **bound-only**: full type-class power (associated types, generic methods, free `Self`), resolved by static dispatch at monomorphized call sites, and it may **not** appear in type position. An `interface` may be used **both** as a bound *and* as a runtime value.

This is the explicit-opt-in answer to Rust's object safety: Rust checks object-safety implicitly the moment you write `dyn Trait`, so errors land far from the declaration. Dovetail runs the check because you *declared* `interface` — intent is explicit and the error is local.

### 1.1 What is identical to a trait

Per [traits-design](traits-design.md), with **no difference** for interfaces:

- **Who may implement:** any type — records, enums, newtypes, primitives, intrinsics, classes. Via `implement I for T` blocks ([traits-design §2, §3, §5, §6](traits-design.md)) or `class C implements I` ([traits-design §8.x / class section](traits-design.md)).
- **Bounds:** `where T: I` — monomorphized, static dispatch ([traits-design §9](traits-design.md)).
- **Generics:** generic interfaces (`interface Iterator<T>`), generic impl blocks, bounds on blocks.
- **`extends`, default methods, name resolution / disambiguation, orphan rule:** [trait-design-appendix §1–§4](trait-design-appendix.md), with the one `extends` refinement in §5 below.

"Implementing an interface is free" means literally this: an implementer does nothing different than for a trait. There is no separate interface-implementation syntax or mechanism.

### 1.2 What is *not* simplified

This model does **not** simplify inference or codegen. The interface object reuses the entire trait-object representation — fat pointer `(data, type_info, vtable)`, wrappers, the single calling convention, de-monomorphized vtables ([traits-design §8.3–8.5](traits-design.md)). The benefit is **user-facing only**: one concept, a trivial `trait`-vs-`interface` decision, and zero new rules for implementers.

---

## 2. Syntax

Identical to `trait_decl` ([traits-design §1.2, §4](traits-design.md)) with the keyword `interface`:

```
interface_decl = [ doc_comment ] "interface" IDENT [ type_params ]
                 [ "extends" interface_list ] [ where_clause ] "=" trait_body
```

The body grammar (`trait_body`, `trait_method`, default methods, properties) is **shared with traits** — there is no separate interface grammar. Whether a body is legal for an interface is decided by the §3 *check*, not by a different grammar.

```dovetail
interface Iterator<T> =
    function next(self): Option<T>

interface Drawable =
    function draw(self, canvas: Canvas): Unit
    function boundingBox(self): Rect
```

---

## 3. The Object-Safety Check ("Trait-Safe")

Run during the **Rules** phase on every `interface` declaration. Each rule, if violated, errors **at the offending method on the interface** (not at a use site).

| Rule | Why it can't be in a vtable | Error |
|---|---|---|
| Every method takes `self` (no static/associated functions in the dispatch set) | A vtable dispatches on a receiver; nothing to dispatch on otherwise. | `interface methods must take self; put static functions on a trait` |
| No method type parameters (no generic methods) | A monomorphizing AOT compiler has no JIT (C#) or erasure (Java) to instantiate a generic method reached through an erased receiver. See [trait-objects-removal-design §3](trait-objects-removal-design.md). | `interface methods cannot be generic` |
| No associated types / GATs ([appendix §5](trait-design-appendix.md)) | Would have to be recovered from an erased receiver. | `interfaces cannot declare associated types; use a trait` |
| `Self` only as the receiver or the return type | `Self` elsewhere (`other: Self`) needs both values to be the *same* concrete type — unknowable behind a fat pointer (the `Equatable` problem). | `Self may appear only as the receiver or return type in an interface method` |

**`Self`-return is allowed and reinterpreted (§6).** This is *more* permissive than Rust trait objects and enables fluent/builder patterns dynamically.

**Consequence:** a trait that would fail this check can never be an interface — `Equatable`, `Comparable`, `From<T>`, `Awaitable<T>`, `EarlyReturn<T>` stay traits (bound-only). A trait that *would* pass is still not boxable until you change its keyword to `interface` — the opt-in is explicit.

---

## 4. Interface Objects (Type Position)

When an interface name appears in **type position**, it denotes an **interface object**: a fat pointer whose concrete type is hidden behind a vtable. No `dyn` keyword.

```dovetail
function render(shapes: Array<Drawable>): Unit = ...   // heterogeneous Circle, Square, ...
function hashFor(cs: CipherSuite): HashAlgorithm = ... // runtime-chosen concrete type
```

- **Generic interfaces** apply their type args: `Iterator<Int32>`.
- **Intersection:** `Drawable and Serializable` denotes an interface object implementing both; `(A and B) <: A`, `<: B` ([traits-design §8.1, §8.5](traits-design.md)). `and` is used only in interface-object types and `extends`/`implements` lists.
- **Coercion:** a value implementing the interface is **implicitly** coerced to the interface object where one is expected (argument, return, field, assignment); intersection requires implementing all components.

Because interfaces are implementable by **any** type, an interface object may wrap a record, enum, primitive, intrinsic, or class. This is what lets `Iterable<T>` (interface) be boxed even though `Array<T>` — its implementer — is an intrinsic, not a class.

**Representation:** unchanged from trait objects ([traits-design §8.3–8.5](traits-design.md)) — `(data, type_info, vtable)`, one calling convention `(type_info_array, self, ...) -> Ret`, wrappers for generic/non-generic impl sources. The runtime value is the **interface object**; the vtable struct is the **interface vtable** (`$IfaceVtable`).

### 4.1 Optional: argument-position bound sugar

A future ergonomic option (orthogonal to this design): treat a bare interface in **argument** position as sugar for a generic bound — `foo(xs: Iterable<T>)` ⟶ `foo<C>(xs: C) where C: Iterable<T>` — giving static dispatch and skipping the box when the concrete type is known at the call site, while return/field positions remain real interface objects. This mirrors Rust's `impl Trait` (static, arg) vs `dyn Trait` (dynamic). **Not required** by this model (any-type interface objects already make `foo(xs: Iterable<T>)` work as a box); listed as a possible later optimization.

---

## 5. `extends`

`extends` works as for traits ([appendix §1](trait-design-appendix.md)). The one refinement: an **interface may extend only interfaces** — a `B` interface object must dispatch its supers' methods through the vtable, so every super must itself be trait-safe and boxable.

- `interface B extends A and C` — `A`, `C` must be interfaces; `B <: A`, `B <: C`; a `B` interface object coerces to an `A` interface object.
- A **trait may extend an interface** (a trait is the superset; it adds type-class power atop the object-safe contract). `interface X extends SomeTrait` is an error: `an interface may extend only interfaces`.

(Alternative considered: let the safety check run transitively over a plain-trait super. Rejected for simplicity — "interfaces extend interfaces" is the clearer rule.)

---

## 6. `Self` Semantics (Dual)

- **Bound context** (`T: I`, monomorphized): `Self` = the concrete type. `clone(self): Self` returns the concrete type, static dispatch.
- **Interface-object context** (dynamic): a `Self`-typed **return** is observed as the **interface type**; the vtable wrapper for concrete type `C` returns a `C`, re-coerced to an `I` interface object at the boundary.

`Self` outside receiver/return is rejected by §3, so no other case arises.

---

## 7. Choosing `trait` vs `interface`

| You need | Use |
|---|---|
| A value whose concrete type is hidden behind a vtable: heterogeneous collections, runtime-chosen returns, plugin fields, iterator abstraction | **interface** |
| Type-class power (`equals(self, other: Self)`, `from(value: T): Self`, associated types, generic methods); static dispatch is enough | **trait** |
| A contract you might use *either* way | **interface** — it is usable as both a bound and a value, at the cost of the §3 restrictions |

And the other tools, unchanged ([traits-design](traits-design.md)): **enum** for a closed set you own; **abstract class** for shared state + single is-a hierarchy. Interfaces own stateless capability, multiply implemented, dynamically dispatched, over any type.

---

## 8. Migration

Gate fat pointers behind `interface`: re-declare the traits used dynamically today, leave bound-only traits as `trait`. (All listed traits already pass §3.) See [trait-objects-removal-design §5](trait-objects-removal-design.md).

- **Become `interface`:** `Iterator<T>` ([dovetail/prelude/src/Iterator.dove](../dovetail/prelude/src/Iterator.dove)); `HashAlgorithm`, `AeadAlgorithm`, `KeyExchange` (`standard-crypto`); `LogSink` (`standard-io-log`); plus any `standard-tls` returns (`hashFor`, `aeadFor`).
- **Stay `trait`:** `Iterable<T>` (used only as a bound / `for`-loop; *may* become an interface if you ever want to box it, since any-type impl makes that valid), `Equatable`, `Comparable`, `From<T>`, `Awaitable<T>`, `EarlyReturn<T>`, …

---

## 9. Phasing

1. **Add the keyword + check** — `interface` declaration (shares `trait` parsing/collect/infer); the §3 trait-safety check in the Rules phase. Interface objects reuse trait-object types/coercion/codegen. Object-safe traits remain boxable for now.
2. **Gate type position** — only an `interface` may appear in type position; a plain trait there is an error pointing to `interface`. `extends` refinement (§5).
3. **Migrate the standard library** (§8).
4. **(Optional)** argument-position bound sugar (§4.1).

---

## Grammar Summary

- **Interface:** `interface` IDENT [ type_params ] [ `extends` interface_list ] [ where_clause ] `=` trait_body — body grammar shared with `trait`.
- **Type position:** interface name (with type args) = interface object; `I1 and I2` = intersection interface object. Plain trait names are not allowed in type position.
- **Implements / bounds:** as for traits ([traits-design](traits-design.md)).

---

## References

- [traits-design.md](traits-design.md) — Traits: the shared construct, all implementation forms, bounds, and the fat-pointer/vtable mechanics interfaces reuse.
- [trait-design-appendix.md](trait-design-appendix.md) — Extends, name resolution, default methods, associated types, object safety.
- [trait-objects-removal-design.md](trait-objects-removal-design.md) — Gating fat pointers behind `interface`; migration.
- [grammar.md](../grammar.md) — Grammar.
