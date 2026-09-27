# Traits Design (Consolidated)

**Status:** Implemented core; superseded proposals are explicitly retired in the
[trait/interface audit](trait-implementation-status.md). Section 8 preserves the
obsolete object design for historical context only.

This document describes **traits** in Dovetail in one place: non-generic and generic traits, all forms of implementation (impl blocks and class `implements`), interfaces (dynamic dispatch), and trait bounds. It consolidates and supersedes the previous traits-design and generic-traits-design documents. It aligns with the [traits book](../website/content/book/08-traits.md) and [grammar](../grammar.md).

**Generics context:** Functions are monomorphized; generic data representations
follow [full erasure](full-erasure-design.md). Bounds are checked at compile time.
A concrete receiver uses static dispatch; a generic argument that is itself an
interface value retains interface dispatch. Plain trait names cannot be value types.


**In scope:** Non-generic traits; non-generic and generic impl blocks; generic traits; class `implements` (non-generic and generic classes); orphan rule; built-in traits; trait bounds on monomorphized functions and records; interface integration (specified in [interface-objects-design](interface-objects-design.md)).

**Out of scope:** Variance on trait type parameters, and future extensions.

---

## 1. Non-Generic Traits

### 1.1 Overview

- **Traits** define a set of methods and properties that implementing types must provide. They are Dovetail's approach to ad-hoc polymorphism.
- **Two ways to implement a trait:**
  1. **Inline on a class:** `class C(...) implements TraitA and TraitB = ...` — the class body defines required members, subject to the [default rules](trait-design-appendix.md#4-trait-with-default-methodproperty-implementation).
  2. **Implement block:** `implement Trait for Type = ...` — any type (record, enum, class, newtype, primitive, intrinsic) can implement a trait in a separate block.
- **Orphan rule:** A trait may be implemented for a type only if the implementation lives in a package that **owns the trait** or **owns the type**.
- **Built-in traits:** See the prelude for the current contracts; `Iterable` and `Iterator` are interfaces. Names such as `Display` below illustrate user-defined contracts.

### 1.2 Syntax

A **non-generic** trait has no type parameters: `trait Name =` with a body of required methods and properties.

**Grammar:**

```
trait_decl    = [ doc_comment ] "trait" IDENT [ type_params ]
                [ "extends" named_type { "and" named_type } ] [ "=" trait_body ]
trait_body    = BEGIN { trait_member SEP } trait_member [ SEP ] END
trait_member  = trait_method | trait_property | trait_assoc_type
```

For non-generic traits we use `trait Name =` with no type parameters. The type **`Self`** in a trait refers to the type that implements the trait.

**Examples:**

```dovetail
trait Equatable =
    function equals(self, other: Self): Bool

trait Display =
    function format(self): String
```

Traits also declare **properties** (`property name(self): Type`), associated types,
and default member bodies. See the [grammar](../grammar.md) and [appendix](trait-design-appendix.md).

---

## 2. Non-Generic Implement Block

Any type can implement a trait using a top-level **implement** declaration. For non-generic traits and concrete types, no type_params on the implement.

**Grammar:**

```
implement_decl = [ doc_comment ] "implement" [ type_params ]
                  trait_type "for" type [ "as" IDENT ] [ where_clause ] "=" impl_body
impl_body      = BEGIN { method_decl SEP } method_decl [ SEP ] END
```

**Examples:**

```dovetail
implement Display for Point =
    function format(self): String = "(${self.x}, ${self.y})"

implement Display for Color =
    function format(self): String = match self with ...
```

Implementations must provide every required method and property not supplied by a default, with compatible signatures (`Self` substituted by the implementing type).

**Which types can implement traits:** Records, enums, classes, newtypes, primitives, and intrinsics via implement block; **classes** may also use the `implements` clause on the class declaration (see §8 for trait objects and classes).

---

## 3. Generic Implement Block for Non-Generic Traits

Implement a **non-generic** trait for a **generic type** by giving the implement block its own type parameters.

**Example:**

```dovetail
implement <T> Display for Array<T> =
    function format(self): String = ...
```

One block applies to all instantiations (e.g. `Array<Int32>`, `Array<String>`). The block’s type parameters are in scope in the `for` type and in the impl body. Bounds on the block are allowed (see §6): e.g. `implement <T> Equatable for Array<T> where T: Equatable = ...`.

---

## 4. Generic Traits

A **generic trait** has plain type parameters on the trait. A trailing trait-level
`where` clause is not supported; put supported constraints on methods and implementations.

**Examples:**

```dovetail
trait From<T> =
    function from(value: T): Self

interface Iterator<T> =
    function next(self): Option<T>
```

`Self` still refers to the implementing type. A generic trait may also have **generic methods** (methods with their own `type_params`). **Trait-level and method-level type parameters must not shadow each other:** a method’s type parameter may not have the same name as a trait type parameter (compiler error). Resolution: the typechecker collects the trait’s type parameter names; when checking each trait method, it ensures the method’s type parameter names are not in that set. Method type parameters are in scope for that method’s signature and default body.

**Example (valid):** `function map<U>(self, f: T => U): U where U: From<T>` with `U` distinct from trait param `T`.
**Example (invalid):** `function bad<T>(self): T` inside `trait From<T>` — shadows trait `T`.

---

## 5. Non-Generic Implement Block for Generic Traits

Implement a **generic** trait by applying the trait with concrete (or block) type arguments. No type params on the implement block when the implementing type and trait application are concrete.

**Example:**

```dovetail
implement From<Int32> for String =
    function from(value: Int32): String = ...
```

The number and order of trait type arguments must match the trait’s declaration.

---

## 6. Generic Implement Block for Generic Traits

Implement a generic trait for a generic type by using type parameters on the implement block and applying the trait with those params.

**Examples:**

```dovetail
implement <T> From<Array<T>> for List<T> =
    function from(value: Array<T>): List<T> = ...

implement <T> Equatable for Array<T> where T: Equatable =
    function equals(self, other: Self): Bool = ...
```

Bounds on the block (e.g. `T: Equatable`) are enforced at use sites. Bounds on **method** type parameters (in trait or impl) use a `where` clause and constrain only how that method is called.

**Coherence:** implementation heads for the same trait application must not overlap.
Disjoint shaped instantiations are legal; ordinary user-trait `where` bounds
do not establish disjointness. The compiler's built-in `Tuple` constraint is
a narrow structural exception: `T ~ U where T: Tuple` has arity at least
three and is disjoint from the exact pair head `(A, B)`. It overlaps a
concrete triple or larger tuple head when the complete trait applications
unify. Selection does not depend on registration order. See the
[tuple extension design](tuple-extension-design.md#72-matching-and-coherence).
The orphan rule also applies. See also §7 for bounds on methods.

**Summary of cases:**

| Trait       | Implementing type   | Implement block   | Example |
|------------|---------------------|-------------------|---------|
| Non-generic | Concrete            | No type params    | `implement Display for Point = ...` |
| Non-generic | Generic             | Generic block     | `implement <T> Display for Array<T> = ...` |
| Non-generic | Generic             | Generic + bounds  | `implement <T> Display for Array<T> where T: Equatable = ...` |
| Generic     | Concrete            | No block params   | `implement From<Int32> for String = ...` |
| Generic     | Generic             | Generic block     | `implement <T> From<Array<T>> for List<T> = ...` |
| Generic     | Generic             | Generic + bounds  | `implement <T> Equatable for Array<T> where T: Equatable = ...` |

---

## 7. Bounds on Implement Blocks and Methods

- **Implement block:** `implement <T> Equatable for Array<T> where T: Equatable = ...` — bounds on block type params; enforced when the impl is used (e.g. `Array<U>` only has this impl when `U: Equatable`). Use a `where` clause, such as `where T: Equatable and Display`. Inline `<T: Bound>` is not supported here.
- **Methods:** A **non-generic** trait can have **generic methods**, e.g. `function formatWith<F>(self, formatter: F): String where F: Display`. Implementations must provide the same method signatures (compatible type params and bounds). Bounds on method type parameters do not affect whether the **type** implements the trait; they only constrain how that method can be called. The impl must honor the trait method contract; it cannot add stronger requirements on callers.

---

## 8. Trait Objects (superseded: Interface Objects)

> **Superseded.** This section predates the `interface` keyword. Only a declared `interface` may appear in type position; a plain trait there is an error. The implemented representation is a two-field fat pointer `(data, vtable)` — the `type_info` field below was never built — with nested super-vtable refs for `extends` and per-component sub-vtables for intersections. See [interface-objects-design](interface-objects-design.md) for the authoritative design; the subsections below are kept for the original vtable rationale.

Trait objects provide **dynamic dispatch**: a value whose type is “any type that implements trait(s)” is represented at runtime by a fat pointer (data + type-info + vtable). Trait objects are used **only** when the programmer writes a type as a trait (or intersection of traits), e.g. `function foo(x: Display)` or `x: Display and Equatable`. We do **not** use trait objects for trait bounds on type parameters (those are monomorphized and statically dispatched).

### 8.1 Explicit Trait Object Types

- **Syntax:** In type position, a **trait name** denotes a trait object type. No `dyn` keyword. If the name resolves to a trait, the type is "trait object of that trait."
- **Generic traits:** `Iterator<T>` in type position = trait object implementing `Iterator` with type argument `T`.
- **Intersection:** `Display and Equatable` = trait object implementing both. The `and` keyword is used only for trait object types (not in `where` bounds). Intersection is a **subtype** of each component: `(Display and Equatable) <: Display` and `(Display and Equatable) <: Equatable`, so an intersection value can be used where a single trait is expected.

**Examples:**

```dovetail
function foo(x: Display) = ...

trait Iterable<T> =
    function iterator(self): Iterator<T>
```

### 8.2 Coercion to Trait Object

- **Implicit only:** A concrete value that implements a trait (or multiple traits) is coerced to a trait object when used where a trait object type is expected (argument, assignment, field). No explicit cast syntax.
- **Intersection:** When the target type is `Display and Equatable`, a value that implements both is implicitly coerced to that intersection trait object.

### 8.3 Vtable and Trait Object Layout

> **As implemented:** the `type_info` field below was never built. The shipped fat pointer is **two** fields `(data: (ref any), vtable)`, and the calling convention is `(anyref self, erased params…)` — the trait's generic params erase to `anyref` in the slot signatures (de-monomorphized vtables), with wrappers handling boxing. See [interface-objects-design §5](interface-objects-design.md).

- **Trait object value:** (data ref, type_info, vtable ref). Data is the implementing value; type_info and vtable identify the implementation (see below).
- **Type-info:** Represented as a **WASM-GC array** `(ref (array i32))`, not linear memory. Different impls have different numbers of type parameters (N = 0 for non-generic impl, N = 1 for `Array<T>`, N = 2 for `Pair<T,U>`, etc.), so the vtable **calling convention** cannot fix N. We use a **single convention:** the first parameter to every vtable entry is a **type_info array ref** (GC array of i32). For N = 0 the array is empty; for N > 0 it holds the type-info for the impl’s type parameters.
- **One vtable layout:** Every vtable entry has the same signature shape: `(type_info_array: (ref (array i32)), self, ...) -> Ret`. So we need **wrappers** for all implementation sources:
  - **Non-generic impl block:** Wrapper takes `(type_info_array, self, ...)`, ignores the array, calls impl `(self, ...)`.
  - **Generic impl block:** Wrapper takes `(type_info_array, self, ...)`, reads N i32s from the array, calls the (monomorphized) impl that expects N leading i32 params then self (the impl was compiled for a generic type with N type params; at call site we have concrete type args, so we have N type-info values to pass).
  - **Class implements (non-generic class):** Same as non-generic impl block — wrapper ignores type_info array, calls class method `(self, ...)`.
  - **Generic class implements:** Same as generic impl block — wrapper unpacks type_info array to N i32s, calls class’s method with `(i32, ..., self, ...)`.

So: one vtable layout; type_info passed as GC array; wrappers for both generic and non-generic cases (impl blocks and classes).

### 8.4 Creating a Trait Object

When a concrete value is coerced to a trait object (e.g. passing to `foo(x: Display)`), the compiler builds the trait object: allocate or assemble (data ref, type_info array, vtable ref). For non-generic impl the type_info array is empty (or a shared constant). For generic impl or generic class, the type_info array is filled with the type-info for the concrete type arguments. The vtable is the one for “trait X implemented by type Y” (with wrappers as above).

### 8.5 Intersection Representation

For `Display and Equatable`, one trait object carries **both** traits: one data ref, one type_info array, and either (a) one vtable that contains methods for both traits, or (b) two vtable refs. The design may choose one representation; the important point is a single value (one “parameter”) and subtyping so it can be used where only `Display` or only `Equatable` is required.

---

## 9. Trait Bounds and Elaboration

Bounds on functions, records, enums, classes, type aliases, modules, extensions,
and implementation blocks are checked at compile time. Function and method
instantiations are monomorphized; generic data representations are shared under
[full erasure](full-erasure-design.md).

Inference records the selected contract, receiver application, member, and type
arguments. Monomorphization resolves concrete implementation calls; coercion
inserts interface wrappers/upcasts; codegen emits direct calls or interface
vtable calls from the typed IR. See [trait inference](../dovetail/src/compiler/typechecker/infer/traits.rs),
[monomorphization](../dovetail/src/compiler/monomorphize/mod.rs), and
[coercion](../dovetail/src/compiler/coerce.rs).

Associated outputs through abstract bounds use `P.Output` or `W.Wrapped<T>`.
These retain their declaring trait and receiver identity until equality evidence
or a concrete implementation permits normalization. The
[appendix](trait-design-appendix.md#53-use-and-projection) describes the rules;
[trait completion](trait-completion-design.md) covers generic defaults and inherited overloads.

---

## 10. Implementation Status

Core traits, generic implementations/methods, bounds, class implementations, and
the orphan rule are implemented. Inheritance, defaults, explicit disambiguation,
and coherence are implemented as specified in the [appendix](trait-design-appendix.md).
Dynamic values and intersections are implemented as **interfaces**, including
class implementors. The original eleven-step rollout is obsolete; the completed
[interface phases](interface-objects-design.md) and
[requirement audit](trait-implementation-status.md) replace it.

The old `TraitObject` IR, three-field `type_info` representation, flat intersection
proposal, and promise of a separate generic record layout per instantiation are
not remaining tasks. They are superseded designs. Trait variance and reified type
IDs are outside this completed scope.

## Grammar Summary

- `trait Name<T> extends Parent<T> = ...` declares a contract; parameters, parents,
  and the body are optional.
- `interface Name<T> extends Parent<T> = ...` additionally permits value types,
  subject to declaration-time safety checks.
- `implement <T> Contract<T> for Box<T> where T: Other = ...` defines an implementation.
- `property name(self): Type` declares an instance getter; methods and properties
  may provide default bodies.
- `type Output` / `type Rebind<U>` declare associated types in traits;
  implementation blocks supply `= Type` definitions.
- Interface value intersections use `A and B`. Plain traits are valid in bounds,
  not in value type position.

The [grammar](../grammar.md) and [audit](trait-implementation-status.md) take
precedence over historical syntax sketches in section 8.
