# Traits Design: Appendix

**Status:** Implemented within the [audited scope](trait-implementation-status.md).
Unsupported original proposals are obsolete, as noted below; they are not pending
requirements for trait completion.

This document describes **trait extensions and edge cases** that are not fully covered in [traits-design](traits-design.md): trait inheritance (extends), name resolution and disambiguation, multiple traits with the same method name, default implementations, and associated types (including GATs). It aligns with [traits-design](traits-design.md), [async-await-design](async-await-design.md), and [railway-early-return-design](railway-early-return-design.md).

**In scope:** Trait extending another trait (syntax and semantics); same-name method rules when extending (same vs different signature, default implementation); ambiguous names between module, named extension, and implement block (priority, ambiguous error, calling a trait method specifically); multiple traits with same method name; trait with default method/property implementation; trait generic associated type (GAT) and non-generic associated type.

**Out of scope:** Variance; other future trait extensions.

---

## 1. Trait Extending Another Trait

### 1.1 Syntax

A trait may **extend** one or more traits using explicit `extends` syntax:

```dovetail
trait A =
  function foo(self): Int32

trait B extends A =
  function bar(self): String
```

- **Semantics (as implemented):** `B`'s member set is the **flattened** union of its own members and all supers' (transitively). `implement B for T` must implement that full flattened set **inline** — a separate `implement A for T` does not contribute to `B`'s completeness, and the two impls are independent ("A is on its own"; they may coexist).
- **Satisfaction:** implementing `B` makes `T` satisfy `A` everywhere — bounds (`T: A`) and, when `A` is an interface, coercions (`let a: A = t`) — dispatching `A`'s members to the `B` block's inline members. A **direct** `implement A for T` always wins in `A`-contexts; two distinct sub-trait providers with no direct impl are an ambiguity error (implement `A` directly to disambiguate).
- **Contract inheritance:** satisfying `B` also satisfies `A`. Value subtyping applies when these are interfaces. A value of a type that implements `B` may be used where `A` is required (including interface objects: a `B`-object coerces to an `A`-object; see [interface-objects-design §6](interface-objects-design.md)).

### 1.2 Same-Name Methods When Extending

When `trait B extends A` and `B` declares a method with the **same name** as a method in `A`, the following rules apply.

**Signature** is defined as: same method name, same parameter types (and order), and same return type. "Different signature" means at least the parameter list differs (we do not allow overloading by return type only).

| Situation | Allowed? | Treatment |
|-----------|----------|-----------|
| **Exact same signature** (same params, same return type) | | |
| — B only *declares* the method (no body) | **No** | Error: redundant; the method is already required by `A`. |
| — B *implements* the method (has a body) | **Yes** | Treated as a **default implementation** (override): for types that implement `B`, this implementation is used when the method is called. |
| **Different params** (same name, different parameter list) | **No** | Overloading through `extends` is unsupported; the original proposal to retain both is obsolete. |
| **Same params, different return type only** | **No** | Error: same effective signature for disambiguation; we do not allow overloading by return type only. |

So:

- Redeclaring the same signature without providing an implementation is an error (no added requirement).
- Providing an implementation for the same signature is allowed and acts as the default/override for implementors of `B`.
- Different parameter lists under the same inherited name form distinct overloads. Dispatch preserves the selected declaration through implementation, generic, class, and interface calls.
- Same parameters with a different return type is still a conflict and is not allowed.

---

## 2. Ambiguous Names: Module, Named Extension, Implement Block

### 2.1 Resolution Priority

When a call of the form **`receiver.name(args...)`** (or equivalent) could resolve to a **module** function, a **named extension** method, or a **trait** method from an implement block, resolution uses a fixed priority:

1. **Module** — first: look for a matching function in the module (e.g. in scope or receiver's module).
2. **Named extension** — second: if no module match, look for a matching named extension.
3. **Trait impl** — last: if no named extension match, look for a matching method from an implement block.

*(Implemented — for both instance methods and static functions.)*

The first category that yields a single matching candidate wins. Cross-source ambiguity (e.g. one from module and one from trait) is **not** reported: priority disambiguates.

### 2.2 Ambiguity Within a Source

- **Two (or more) trait impl blocks** provide a method with the same name and a matching signature for the receiver → **error** (ambiguous). The programmer must call explicitly.
- **Two (or more) named extensions** provide a method with the same name and a matching signature → **error** (ambiguous). The programmer must call explicitly. *(Within a category, a NON-generic block is more specific than a generic one and wins without ambiguity — `extension E for Box<Int32>` beats `extension F<T> for Box<T>` on a `Box<Int32>` receiver; this does not permit overlapping implementations of the same trait; coherence rejects those.)*

### 2.3 Explicit Call Syntax

To call a specific implementation when there is ambiguity, or to force a trait or extension method when priority would have chosen another source, use:

- **Named extension:** **`NamedExtension.foo(self, args...)`**
- **Trait method:** **`TraitName.bar(self, args...)`**

Here `NamedExtension` is the name of the extension (e.g. the module or type the extension is defined on), and `TraitName` is the trait whose implementation should be used. `self` is the receiver; `args...` are the remaining arguments.

---

## 3. Multiple Traits with Same Method Name

When a type implements **two or more traits** that each define a method with the **same name** (and a signature that matches the call), that is the same situation as "two trait impl blocks have ambiguous methods" in **§2.2**: the compiler reports an **error** (ambiguous). The programmer must call explicitly using **§2.3**: **`TraitName.method(self, args...)`** to select which trait's implementation to use. No separate resolution rule or priority between traits; explicit disambiguation is required.

---

## 4. Trait with Default Method/Property Implementation

A trait may provide a **default implementation** for a method or a **default value** for a property directly in the trait body. Implementors (implement blocks or classes that implement the trait) may **omit** that member; the trait's default is then used.

- **Methods:** A trait method may have a body in the trait (e.g. `function bar(self): Int32 = 42`). If an impl block or class does not define that method, the trait's default implementation is used.
- **Properties:** A trait property may have a default (e.g. `property x(self): Int32 = 0`). If an impl block or class does not define that property, the trait's default is used.

**Current scope:** Defaults work for generic and non-generic implementation
blocks and classes, including methods with their own type parameters. Bodies are
checked once in the trait's scope against its declared bounds, then specialized
for calls. Explicit matching methods take precedence; calls inside instance
defaults preserve virtual dispatch.

Classes must define static trait members explicitly. Conflicting defaults for
the same signature require an explicit member; repeated paths to the same
default application share it. Default bodies may not be `intrinsic`. Associated
outputs in default bodies retain their implementation-dependent identity.
See [trait completion](trait-completion-design.md) and the
[advanced book examples](../website/content/book/26-advanced-generics.md#267-generic-trait-defaults).

---

## 5. Trait Associated Type and Generic Associated Type (GAT)

Traits may declare **associated types**: types that are chosen per implementation rather than as parameters of the trait. An associated type may be **generic** (a **generic associated type**, GAT), with its own type parameters, so the trait can express "this context but with a different value type" without higher-kinded types. This section specifies declaration, definition in impls, and use; it is the spec for implementing associated types as part of the trait system. Concrete uses appear in [async-await-design](async-await-design.md) (Awaitable, Rebind&lt;U&gt;) and [railway-early-return-design](railway-early-return-design.md) (EarlyReturn, OnFailure).

### 5.1 Declaration in the Trait

In the trait body, alongside methods and properties, a trait may declare one or more **associated types**:

- **Non-generic:** `type` *IDENT* — every implementation must define it.
- **Generic (GAT):** `type Name<T, ...>` — every implementation must define the GAT.

**Grammar:** `trait_assoc_type = "type" IDENT [ type_params ]`, as in
[grammar.md](../grammar.md). Associated-type defaults in trait declarations were
proposed but never implemented; that proposal is obsolete.

**Examples:**

```dovetail
trait EarlyReturn<T> =
  type OnFailure
  function unwrap(self): Result<T, OnFailure>

trait Awaitable<T> =
  type Rebind<U>
  function succeed(x: T): Self
  function map<U>(self, f: T => U): Rebind<U>
  function andThen<U>(self, f: T => Rebind<U>): Rebind<U>
```

`Self` keeps its usual meaning (the implementing type). The trait declares the associated type (or GAT); each implementation defines it.

### 5.2 Definition in the Impl

In an **implement** block, the implementer **defines** each associated type. The syntax is the same as a type alias: `type` *IDENT* [ type_params ] `=` *type*.

**Non-generic associated type:**

```dovetail
implement <T, E> EarlyReturn<T> for Result<T, E> =
  type OnFailure = Result<Never, E>
  function unwrap(self): Result<T, Result<Never, E>> = ...
```

**Generic associated type (GAT):**

```dovetail
implement <T, E> Awaitable<T> for Async<T, E> =
  type Rebind<U> = Async<U, E>
  function succeed(x: T): Async<T, E> = ...
  function map<U>(self, f: T => U): Async<U, E> = ...
  function andThen<U>(self, f: T => Async<U, E>): Async<U, E> = ...
```

Every impl must define each associated type and GAT that the trait declares.
The original proposal to place these definitions inside a class body is obsolete;
use an implementation block targeting the class. No inference from the implementing type's shape is required; the compiler uses the defined right-hand side when type-checking method signatures and calls.

### 5.3 Use and Projection

Where a type is known to implement a trait that has an associated type (or GAT), the type checker **projects** that type: e.g. for a value of type `Async<T, E>`, the trait's `Rebind<U>` is the impl's definition `Async<U, E>`. At call sites the compiler therefore has a concrete type for the associated type. Method signatures in the trait and impl may refer to the associated type by name; when checking call sites, the projected concrete type is used.

For a parameter bounded by `Producer`, `P.Output` names its ordinary associated
output. `W.Wrapped<T>` names a generic associated output through a `Wrapper`
bound. Both remain symbolic until an equality binding or concrete implementation
allows normalization. Unknown outputs cannot be assigned arbitrary capabilities.
Ambiguous members and incorrect associated argument counts are errors.

A generic `use` continuation must return the resource's symbolic
`R.Wrapped<U, E2>` result; it cannot assume that every resource chooses the same
container. See [Generic Resource Helpers](../website/content/book/26-advanced-generics.md#generic-resource-helpers).
Associated-type defaults and GAT equality bindings remain unsupported.

### 5.4 Relation to Type Aliases

The syntax for defining an associated type in an impl is the same as Dovetail's type alias (`type Name = ...` or `type Name<U> = ...`). Only the context differs: the name is declared in the trait and defined in the impl (including a block targeting a class).

### 5.5 References to Other Design Docs

- **[async-await-design](async-await-design.md)** — Awaitable&lt;T&gt; and GAT Rebind&lt;U&gt;; async/await and same-Awaitable-type rule.
- **[railway-early-return-design](railway-early-return-design.md)** — EarlyReturn&lt;T&gt; and associated type OnFailure; try / orReturn and desugaring.

---

## 6. Additional Edge Cases (from Rust and Scala)

This section summarizes trait-related edge cases and rules in **Rust** and **Scala** that may inform Dovetail. For each, we note whether the main [traits-design](traits-design.md) or this appendix already covers it, or whether the original suggestion has been retired from the current scope.

### 6.1 Object Safety (Rust)

**In Rust:** A trait is *object safe* only if it can be used as a trait object (`dyn Trait`). Rules include: no generic associated types in the trait object (or they must be concretized); methods must have a receiver that allows vtable dispatch (`&self`, `&mut self`, `Box<Self>`, etc.) and must not use `Self` except in the receiver; no static methods that aren’t explicitly excluded from dispatch; supertraits must be object safe; `Self: Sized` must not be required.

**In Dovetail (implemented):** The `interface` keyword opts into value types and
declaration-time safety checks. Plain traits cannot be value types. The complete
rules are in [interface-objects-design §4](interface-objects-design.md#4-the-object-safety-check-trait-safe).
The earlier recommendation to add an object-safety checklist is complete.

### 6.2 Coherence and Overlapping Impls (Rust)

**In Rust:** The **orphan rule** restricts where a trait can be implemented (local trait or local type). **Coherence** forbids two impl blocks from applying to the same type: if `implement Trait for A` and `implement Trait for B` could both apply to some type (e.g. blanket `implement Trait for T where T: Foo` and `implement Trait for T where T: Bar` when a type implements both Foo and Bar), the compiler errors. Negative bounds (`T: !Trait`) can be used to prove two blanket impls don’t overlap.

**In Dovetail (implemented):** The orphan rule is in [traits-design](traits-design.md). Two implement blocks for the same trait may not overlap: if their for-types (and trait type args) can both apply to some type, the compiler reports an error naming both locations and a witness type (e.g. `overlapping implementations of trait 'Show': this block and the one at file:12 can both apply to 'Box<Int32>'`). The rule is conservative Rust-style — `where`-bounds never disprove overlap. Sibling instantiations with disjoint shapes (`Tr for List<Int32>` and `Tr for List<String>`, or `<T> Tr for Pair<T, Int32>` and `<T> Tr for Pair<T, String>`) are legal. Negative bounds are outside the current scope; the earlier suggestion is retired.

### 6.3 Linearization and Multiple Trait Order (Scala)

**In Scala:** With multiple inheritance of traits (`class D extends B with C`), Scala uses **linearization**: a deterministic left-to-right order (with right taking precedence for overrides). That avoids the diamond problem: one linear order decides which method implementation is used.

**In Dovetail:** We have `trait B extends A and C` (multiple parents supported) and §1.2 defines same-name rules. For classes we have `implements TraitA and TraitB`; [traits-design](traits-design.md) doesn’t define an order between TraitA and TraitB for method resolution. Our §2 and §3 say: if both traits define the same method name, it’s ambiguous and the programmer must call explicitly (`TraitName.method(self, args...)`). So we avoid defining a linearization order by requiring explicit disambiguation. **No change needed** unless we want a defined order for `implements A and B` when names don’t conflict (e.g. for default implementations or super calls).

### 6.4 Defaults and Specialization (Rust)

**In Rust:** Default method implementations in a trait let impls omit those methods. **Specialization** (unstable) would allow one impl to override another when it is “more specific”; overlapping impls would be allowed when one clearly wins.

**In Dovetail:** §1.2 (subtrait override) and §4 (default method/property in trait) cover defaults. We do not have full specialization (competing impls for the same trait and type). **Recommendation:** Leave specialization out of scope unless we later need “more specific impl overrides more general” for the same type.

### 6.5 Self Types (Scala)

**In Scala:** A **self type** (`trait A { this: B => }`) requires that any concrete type mixing in `A` also mixes in `B`. It’s a dependency between traits without inheritance; used in the “cake pattern” for modular composition.

**In Dovetail:** Our `trait B extends A` expresses “B requires A” and subtyping. Self types would add “A requires B” without B being a supertrait of A (e.g. trait A can call B’s methods because any implementor of A must also implement B). **Recommendation:** Out of scope for now; `extends` covers the common case. Self types could be revisited if we need trait composition without subtyping.

### 6.6 Summary Table

| Topic | Rust / Scala | Dovetail status |
|-------|----------------|----------------|
| Object safety | Rust: rules for `dyn Trait` | Implemented: explicit `interface` declaration checks. |
| Orphan rule | Rust: local trait or local type | In [traits-design](traits-design.md). |
| Overlapping impls | Rust: forbidden; coherence | Implemented: no overlap; bounds do not prove disjointness. |
| Negative bounds | Rust: `T: !Trait` for disjointness | Retired from current scope. |
| Linearization | Scala: right-to-left order | We require explicit disambiguation (§2, §3); no order defined. |
| Default impl in trait | Rust / Scala | §4 (and §1.2 for subtrait override). |
| Specialization | Rust: unstable | Out of scope. |
| Self types | Scala: `this: B =>` | Out of scope. |

---

## References

- [traits-design.md](traits-design.md) — Core traits: definition, impl blocks, trait objects, bounds.
- [async-await-design.md](async-await-design.md) — Associated types and GATs; Awaitable.
- [railway-early-return-design.md](railway-early-return-design.md) — EarlyReturn and associated type OnFailure.
- [grammar.md](../grammar.md) — Grammar.
