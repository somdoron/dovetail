# Classes Design

This document designs **classes** in Dovetail: single-constructor model, inheritance and trait implementation, abstract classes, visibility, let bindings, methods and properties (instance/static, abstract/override/final), generics, subtyping, and codegen (WASMGC subtyping, vtable). It aligns with the [type system book](book/06-type-system.md), [traits design](traits-design.md), [grammar](grammar.md), and [compiler design](compiler.md).

Dovetail aims for **strong OOP capabilities** alongside its functional features. Classes are a major feature and will be implemented in **multiple phases**.

**In scope:** Class declaration; single primary constructor in class signature; constructor parameter semantics (visibility, mutability, default values, accessibility); abstract and **final** classes; inheritance (`extends` one class) and trait implementation (`implements` multiple traits), including **abstract trait members** (every trait-required method/property must be either implemented or explicitly declared abstract); visibility (class, constructor, members, let bindings), including **private** (that class only) and **protected** (that class and subclasses); let bindings and **top-level expressions** in class body (constructor body); methods (instance, static, abstract, override, final) and **super** calls; properties (same modifiers); generics, trait bounds, and **subtype constraints (class bounds)** on type parameters; where clause; subtyping (class hierarchy); runtime type testing (`is`) and casting (`as`) on class hierarchies; codegen using WASMGC subtyping and wasm table for vtable.

**Out of scope for initial design:** Overriding of fields (all variables final); variance on generic type parameters. **No secondary constructors** — the grammar does not allow them; use **static methods** (e.g. factory methods that call the primary constructor) for alternative construction.

**Implementation status:** Phase 1 complete (typecheck only, no codegen).

---

## 1. Overview

- **Classes** combine data (constructor parameters and let bindings) with behavior (methods and properties). A class has **one real constructor**, which is the signature of the class: `class Box(x: Int32) = ...`. The **class body** is the constructor body: let bindings and **top-level expressions** run during construction (expressions in order, for side effects).
- **Inheritance:** A class may **extend** at most one other class and **implement** multiple traits. When a class implements a trait, that behaves like an implement block for trait bounds and trait objects (see [traits-design](traits-design.md)).
- **Abstract classes** cannot be instantiated; they may declare abstract methods (and properties) that concrete subclasses must implement.
- **Final classes** cannot be extended: `final class Box(...) = ...` — no class may `extends` a final class. Used when the type should have no subclasses.
- **Constructor parameters** are private and immutable by default, and are accessible throughout the class. Visibility and mutability can be overridden per parameter; the constructor itself can be public (default), internal, private, or protected.
- **Visibility:** Class visibility is internal to the package by default; private classes are visible only in the same file. For **class members**: **private** = that class only; **protected** = that class and subclasses. See §5.
- **Let bindings** in the class body add additional stored (or computed) state; they are private by default. Overriding of variables is not allowed—all variables are final.
- **Methods** and **properties** support instance vs static, and (on instance members) abstract, override, and final.
- **Generics:** Classes may have type parameters with trait bounds, **subtype constraints (class bounds)**, and where clauses. Same bound syntax for both: when the bound is a class, the type parameter must be a subtype of that class.
- **Subtyping:** A subclass is a subtype of its superclass: `let b: Base = A()` is allowed when `A extends Base`.
- **Type testing and casting:** `is` and `as` work on class hierarchies, not just `Any`. If `b` has type `Base`, then `b is A` checks at runtime whether the value is an instance of `A` (or a subclass of `A`), and `b as A` casts (panics on failure). See §11.
- **Codegen:** Use WASMGC subtyping and a wasm table for the vtable (dynamic dispatch).

---

## 2. Inheritance and Trait Implementation

- A class may **extend** at most **one** class: `class A(...) extends Base(...) = ...`.
- A class may **implement** multiple traits: `class A(...) implements Trait1 and Trait2 = ...`. Order is irrelevant; all listed traits must be fully implemented by the class (and its body or inherited members).
- When a class declares `implements Trait`, the class must satisfy **every** method and property required by the trait (or inherit an implementation from the superclass). For each required member, the class must either **(a)** provide a concrete implementation, or **(b)** **explicitly** declare it **abstract** in the class body. There is no third option: even if the class is abstract, it must either implement each trait member or explicitly define it as abstract. If (b), the class does not provide a body for that member; the class must be **abstract**, and **concrete subclasses** must override and implement that member.

  Example: `trait Drawable = function draw(self): Unit`. An abstract class can implement the trait and leave `draw` abstract: `abstract class Shape() implements Drawable = abstract function draw(self): Unit`. Concrete subclasses of `Shape` must then override `draw` and provide a body.

**Grammar (existing):** `extends type [ call_args ]`, `implements trait_list`, `trait_list = type { "and" type }`.

---

## 3. Abstract and Final Classes

- **Abstract class:** `abstract class Box() = ...`. Cannot be instantiated (no `Box()` at call sites; only concrete subclasses can be constructed). May declare **abstract methods** (and abstract properties) with no body; concrete subclasses must override and implement them. When a class **implements a trait**, it must for every trait-required member either implement it or **explicitly** declare it abstract (see §2); if it declares any as abstract, the class must be abstract. Only **instance-level** members can be abstract; static members must be concrete.
- **Final class:** `final class Box(...) = ...`. Cannot be **extended** — no class may `extends` a final class. Used when the type must have no subclasses. A class cannot be both abstract and final.

**Grammar:** `[ "abstract" | "final" ] "class" IDENT ...` (already has `abstract`; add `final`).

---

## 4. Constructor

### 4.1 Single primary constructor

- The **only** real constructor is defined in the class signature: `class Box(x: Int32) = ...`. All construction is done via this constructor (or, for subclasses, via `extends Base(...)` which ultimately chains to the root constructor).
- **No secondary constructors.** The grammar does not allow secondary constructors. Alternative construction (e.g. multiple “constructors” or named factories) is done via **static methods** that call the primary constructor (e.g. `function create() = Box(0)`).

### 4.2 Constructor parameters

Constructor parameters are the primary way to pass data into the instance:

- **Default visibility:** **private** (visible only within the class).
- **Default mutability:** **immutable**.
- **Scope:** Accessible **everywhere** in the class (methods, properties, let bindings, and in `extends` for passing to super).

**Optional modifiers on parameters:**

- **Visibility:** `public`, `internal`, **protected**, or leave as default (private). **Protected** = visible in this class and in subclasses.  
  Example: `class Box(public x: Int32) = ...` — `x` is visible to callers; `class Box(protected x: Int32) = ...` — `x` is visible in subclasses too.
- **Mutability:** `mutable` to allow reassignment.  
  Example: `class Box(mutable x: Int32) = ...`.
- **Default values:** A constructor parameter may have a default: `IDENT ":" type "=" expression`. At call sites, **trailing** arguments may be omitted; omitted parameters receive the default expression. Example: `class Box(x: Int32 = 0) = ...` allows `Box()` (same as `Box(0)`) and `Box(42)`.

**Constructor visibility:**

- The **constructor itself** (the ability to call `Box(...)`) can be **public** (default), **internal**, **private**, or **protected**.
- **Private constructor:** Callable only from **static methods on the same class** (e.g. factory methods). Subclasses cannot call it directly.
- **Protected constructor:** Callable from the same class and from **subclasses** (e.g. in their constructor chain or in static methods on the subclass). Not callable from unrelated code.
- Syntax: `class Box private (x: Int32) = ...`, `class Box protected (x: Int32) = ...`.

**Grammar (to align):** Constructor visibility before the parameter list: `"class" IDENT [ visibility ] [ type_params ] "(" ... ")"`. Parameter: `[ visibility ] [ "mutable" ] IDENT ":" type [ "=" expression ]` (default value optional; when present, trailing args may be omitted at call sites). Visibility for class members and constructor includes **protected**. Current grammar has `constructor_params` and `constructor_param`; ensure default for param is private and immutable.

### 4.3 Constructor body: top-level expressions

- **Expressions are allowed at the top level of the class body.** Those expressions are part of the **constructor**: they run when the instance is being constructed, in the order they appear.
- The constructor thus consists of: (1) running the super constructor (if any), (2) initializing constructor parameters, (3) evaluating let bindings and any top-level expressions in sequence. Top-level expressions are used for side effects during construction (e.g. validation, logging, registering the instance somewhere).
- Example: a class body may mix let bindings, methods, and bare expressions; the expressions execute during construction, after any let bindings that appear before them in the body.

**Grammar:** `class_body` must allow `expression` as a class member (in addition to field_decl, method_decl, etc.), so that a top-level expression is valid in the class body and is executed as part of the constructor.

---

## 5. Visibility

- **Class visibility:** Same as other top-level declarations. **Internal** by default (visible to the package). **Private** = visible only in the **same file**. **Public** = visible to other packages (with import). (No **protected** at class level—only for members and constructor.)
- **Constructor visibility:** **Public** by default (anyone who can see the class can call the constructor, unless the class is abstract). May be **internal**, **private**, or **protected** (see §4.2).
- **Methods and properties:** **Internal** by default (visible to the package). May be **public**, **private**, or **protected**.
- **Let bindings (class body):** **Private** by default (visible only within the class). May be **public**, **internal**, or **protected**.
- **Private vs protected (class members only):** For **members** (constructor params, methods, properties, let bindings), **private** means visible only **within that class**. **Protected** means visible within that class **and in subclasses**. This differs from top-level `private`, which is file-scoped. Protected is only meaningful in the context of inheritance (classes and their subclasses).

---

## 6. Let Bindings (Class Body)

- In addition to constructor parameters, a class body may define **let bindings** (see also §4.3 for top-level expressions in the constructor body):
  - `let x = 5`
  - `let mutable x = 5`
- Semantics: Stored (or computed) instance state; initialized during construction. Visibility: **private** by default (§5).
- **No overriding:** Overriding of variables is not allowed; all such variables are **final** (subclasses cannot override them). Shadowing of names in subclasses is also out of scope for this design.

**Grammar:** Class body must allow `let` and `let mutable` as class members (current grammar uses `field_decl` with `IDENT ":" type`; design prefers `let [ mutable ] IDENT [ ":" type ] "=" expression` for consistency with Dovetail let bindings; type may be inferred).

---

## 7. Methods

- **Instance methods:** Declared with `self` as the first parameter: `function foo(self) = ...`. Called on a value: `obj.foo()`.
- **Static methods:** Declared with **no** `self`: `function bar() = ...`. Called on the class: `ClassName.bar()`.
- **Abstract methods (instance only):** `abstract function foo(self): T = ...` (no body, or body omitted). Only in abstract classes; concrete subclasses must provide an implementation. When a class **implements a trait**, every trait-required method (or property) must be either concretely implemented or **explicitly** declared abstract in the class body; the class may not leave any trait member unspecified. Those declared abstract have no body; the class must be abstract; concrete subclasses must override and implement them.
- **Override (instance only):** `override function bar(self) = ...` — overrides a method from the **superclass** only. **Not used for traits:** when a class implements a trait, the class body simply provides the required method (same name/signature); the `override` keyword is **not required and not allowed** for trait methods—only for superclass method overrides.
- **Final (instance only):** `final function bar(self) = ...` — subclasses cannot override this method.
- **Super calls:** Inside a subclass, the superclass implementation of a method can be invoked with **super**: e.g. `super.greet(self)` or `super.foo(self, x)`. Valid only in a class that `extends` another class; `super` refers to the immediate superclass. Used to delegate to or extend the superclass behavior. The same form applies when overriding a method that came from the superclass (not from a trait; trait methods are not overridden with `super` in this sense—see traits-design for trait object dispatch).

**Grammar (to align):** `method_decl` and `static_method_decl`; add optional prefixes `abstract`, `override`, `final` to instance method declarations. Static methods cannot be abstract, override, or final.

---

## 8. Properties

- Classes support **properties** (getters or stored as needed; **no setters** in Dovetail). Like methods, properties can be:
  - **Instance** or **static**
  - **Abstract**, **override** (superclass only, not for traits), or **final** (instance level only)
- When a class implements a trait that requires a property, the class must either implement it or **explicitly** declare it as **abstract** (same as for methods in §7); concrete subclasses must then provide it.
- Visibility rules match methods (§5).

**Grammar:** Add `property_decl` to `class_member` (see [traits-design](traits-design.md) and [extension-methods-design](extension-methods-design.md) for property syntax). Align with existing `property_decl` in extensions where applicable.

---

## 9. Generics

- Classes may be generic: `class List<A>(...) = ...`.
- **Trait bounds** on type parameters: `class List<A : Display>(...) = ...`.
- **Subtype constraints (class bounds):** Class bounds use the **same syntax** as trait bounds and are allowed **wherever trait bounds are allowed**: generic functions, generic classes, generic records, where clauses, implement blocks, extensions, etc. When the bound is a **class**, the type parameter must be a **subtype** of that class (the class itself or any class that extends it). Example:

  ```dovetail
  abstract class Account() = ...

  function foo<A : Account>() = ...           // generic function
  class Container<A : Account>(value: A) = ... // generic class
  ```

  So `A : Account` means “A is a subtype of Account”. Trait bounds and class bounds can be combined (e.g. `A : Account` and `A : Serializable`).
- **Where clause** on the class: `class C<A, B>(...) where A : Trait1, B : Trait2 = ...` (and similarly `where A : SomeClass` for subtype constraints).
- Same **specialized-only** (monomorphized) model as the rest of Dovetail: one WASM type per instantiation.

**Grammar:** `class IDENT [ type_params ] ...` and `where_clause` (already present in grammar for classes). Type param bounds use the same syntax for both traits and classes everywhere bounds are permitted; the typechecker treats a bound that is a class as a subtype constraint.

---

## 10. Subtyping

- If `class A(...) extends Base(...) = ...`, then **A is a subtype of Base**.
- **Allowed:** `let b: Base = A()` — a value of type `A` can be used where `Base` is required (including trait objects when `Base` is or implements a trait).
- Subtyping is **nominal** (declared via `extends`), not structural. Codegen will use WASMGC subtyping and vtables (see §13).

---

## 11. Type Testing and Casting (`is` / `as`)

Dovetail already supports `is` (type test) and `as` (type cast) on values of type `Any` (see [variance-any-design.md](variance-any-design.md)). With class hierarchies, `is` and `as` are extended to work on **base class types** — the subject no longer needs to be `Any`.

### 11.1 `is` on class hierarchies

When the subject has a class type that is a superclass (or the same class), `is` checks at runtime whether the value is an instance of the target class (or any subclass of it):

```dovetail
abstract class Animal(public name: String) = ...
class Dog(name: String) extends Animal(name) = ...
class Cat(name: String) extends Animal(name) = ...

function isDog(a: Animal): Bool = a is Dog
```

`is` returns `true` if the runtime value is an instance of the target type or any subclass of it. It returns `false` otherwise.

### 11.2 `as` on class hierarchies

`as` performs a **downcast** within a class hierarchy. If the runtime value is not of the target type, it **panics** (same semantics as `as` on `Any`):

```dovetail
function getName(a: Animal): String =
    if a is Dog then
        let d = a as Dog
        "Dog: ${d.name}"
    else
        "Not a dog"
```

### 11.3 Allowed subject types

The typechecker allows `is` and `as` when the subject is:

1. **`Any`** — already supported (Phase 5 of variance-any-design).
2. **A class type** where the target is a subclass (downcast) or a class in the same hierarchy. The subject must be a supertype of the target, or the compiler emits a warning/error for impossible casts (e.g. casting between sibling classes with no subtyping relationship is always false for `is` and always panics for `as`).
3. **An interface value type** — implemented for classes and generic class instances; see [interface-objects-design](interface-objects-design.md).

### 11.4 Codegen

Since classes use WASMGC subtyping, the existing `ref.test` (for `is`) and `ref.cast` (for `as`) instructions work directly — no boxing or unboxing is needed. The WASM type index for the target class is looked up from `type_indices`, same as for records and enums today.

### 11.5 Examples

```dovetail
abstract class Shape() = ...
class Circle(public radius: Float64) extends Shape() = ...
class Rectangle(public width: Float64, public height: Float64) extends Shape() = ...

function area(s: Shape): Float64 =
    if s is Circle then
        let c = s as Circle
        3.14159 * c.radius * c.radius
    else if s is Rectangle then
        let r = s as Rectangle
        r.width * r.height
    else
        0.0

// Also works with Any
function describeAny(x: Any): String =
    if x is Shape then "it's a shape"
    else "not a shape"
```

---

## 12. Grammar Summary (Updates)

The following summarizes required grammar and pipeline alignment:

| Area | Design choice | Grammar / pipeline note |
|------|----------------|--------------------------|
| Class modifier | abstract or final | `[ "abstract" | "final" ] "class"`; final class forbids `extends`. |
| Constructor | One primary only; constructor visibility | Add constructor visibility before `(`. Keep single `constructor_params` list. |
| Constructor params | private, immutable by default; optional public, internal, protected, mutable | `constructor_param`: ensure visibility (including protected) and mutable optional; default = private, immutable. |
| Class body members | Let bindings; top-level expressions | Add `let [ mutable ] IDENT ...` and allow `expression` as class_member. Top-level expressions run in order as part of the constructor. |
| Methods | abstract / override / final (instance only) | Add optional keywords to method_decl (instance). |
| Properties | Instance/static, abstract/override/final | Add property_decl to class_member with same modifiers. |
| Secondary constructor | Not in grammar | Do not allow. Use static methods (e.g. factory methods) that call the primary constructor. |
| Type param bounds | Trait bounds and class bounds (subtype) | Same syntax `A : Type`; when Type is a class, A must be a subtype of that class. Allowed **wherever** trait bounds are allowed (generic functions, classes, records, where clause, implement, extension). |

---

## 13. Codegen

- **WASMGC subtyping:** Use the GC type system’s subtyping so that a subclass type is a proper subtype of its superclass in the WASM type system.
- **Vtable:** Use a **wasm table** for the vtable (virtual method table). Each class has a vtable; subclass vtables extend/override slots from the superclass and add new slots for new/overridden methods. Method calls through a base type (or trait object) dispatch through the table.
- **Layout:** Object layout includes superclass fields first, then subclass fields; vtable pointer (or equivalent) as required by the chosen representation. Details (e.g. field order, single vs multiple vtables for traits) to be specified in a codegen-focused section or follow-up doc.

---

## 14. Implementation Phases

Classes are a large feature; implementation is split into phases so that each phase is shippable and testable. **Class codegen is not done without vtable:** we avoid building a codegen path that would be replaced later. So class codegen starts in Phase 2 (with vtable); Phase 1 is typecheck-only for classes.

| Phase | Scope | Notes |
|-------|--------|--------|
| **Phase 1** | Non-generic classes; single constructor; constructor params (visibility, mutability); let bindings in body; top-level expressions; instance and static methods; basic visibility (class, constructor, members). **No class codegen** — pipeline stops after typecheck for classes (e.g. `dovetail check` only, or codegen skips class types until Phase 2). | **Complete.** No inheritance, no traits, no abstract. |
| **Phase 2** | Inheritance (`extends`); super constructor call; subtyping; override and final method; **final class** (no extends); **class codegen** (WASMGC subtyping, vtable for class hierarchy). | **Complete.** Single inheritance only; first phase with class codegen. |
| **Phase 3** | Abstract classes; abstract methods; concrete subclasses implementing abstract members. | **Complete.** |
| **Phase 4** | `implements Trait1 and Trait2`; **every** trait-required method and property must be either implemented or **explicitly** declared abstract (no unspecified members); if any are abstract, class must be abstract; integration with trait bounds and trait objects (per traits-design). | **Complete.** |
| **Phase 5** | Generic classes; type params with trait bounds and where clause; generic methods on classes (both generic and non-generic); type arg inference from constructor args; typecheck-only pass for generic class bodies. | **Complete.** Standalone generic classes with construction, field access, methods, codegen. |
| **Phase 5b** | Generic class integration with the rest of the type system: variance on generic class type params (`+T`/`-T`); `implements` clause on generic classes; `extends` on generic classes; let binding initializers on generic classes; trait object coercion for generic class instances; `resolve_trait_impl_method_for_type` for `GenericClass`. | **Deferred from Phase 5.** See §14.1 for details. |
| **Phase 6** | **Subtype constraints (class bounds)** on type parameters wherever trait bounds are allowed (generic functions, classes, records, where clause, implement, extension). Same syntax `A : ClassName`; typechecker treats class bounds as subtype constraints. | |
| **Phase 7** | Properties (instance/static, abstract/override/final); polish and diagnostics. | |
| **Phase 8** | **`is` and `as` on class hierarchies.** Extend the typechecker to allow `is`/`as` when the subject is a class type (not just `Any`). Codegen uses existing `ref.test`/`ref.cast` — no boxing needed for classes. Update the book ([09-classes.md](book/09-classes.md)) with examples of type testing and downcasting in class hierarchies. | Depends on Phase 2 (inheritance + codegen). Builds on `is`/`as` infrastructure from variance-any-design Phase 5. |

Later (out of scope for this doc): field override/shadowing policy.

### Phase 1 Implementation Notes

Key design decisions and internal representations from Phase 1:

- **Unified `Visibility` enum.** The parser originally had a separate `ClassMemberVisibility` enum; this was removed in favor of using the existing `Visibility` enum (`Public`, `Internal`, `Private`, `Protected`) for all class members, constructor params, and the constructor itself.
- **Constructor as `TypedFunction`.** The constructor is registered in the function registry as a regular function with `MangledName::for_constructor(&fqn)` (uses `$ctor` suffix to avoid collision with the type mangled name). The constructor's body is a `TypedFunction` built during inference: it contains let binding initializations, top-level expressions, and a final `ClassCreate` node that assembles all fields.
- **`ClassTypeSignature` in registry.** Stores `fields: Vec<ClassFieldInfo>` (a unified list of constructor params and public let bindings) and `constructor_param_count: usize` to distinguish params from let bindings. Only **public** let bindings are collected into the registry; non-public let bindings are skipped at collect time and deferred to inference.
- **`ClassTypeDef` built during inference.** During `infer_class`, a `ClassTypeDef` is built with **all** fields (public and non-public) after type inference resolves let binding types. This is stored in `class_type_defs` on the `Inference` struct and used for same-package field resolution.
- **Dual field resolution.** `try_resolve_class_field` uses two strategies: (1) for same-package access, look up `ClassTypeDef` from `class_type_defs` (has all fields with inferred types); (2) for cross-package access (or fallback), use the registry `ClassTypeSignature` (only public fields). Visibility checks (`Private`/`Protected` = inside class only, `Internal` = same package, `Public` = anywhere) are enforced at both paths.
- **`container_name` for qualified resolution.** The `Inference` struct tracks `container_name` (set to the class name during `infer_class`) so that method FQNs are qualified as `ClassName.methodName` and visibility checks can determine if code is "inside" the class.
- **`strip_internal` for cross-package.** Before exposing the registry to dependent packages, `strip_internal` removes non-public class types and strips non-public fields/methods from remaining class type signatures. `constructor_param_count` is recomputed after stripping.

### Phase 5b: Deferred Generic Class Integration

**Historical plan:** The trait-related deferrals below are complete: generic
class implementations, interface coercion, dispatch, and bounds are implemented.
The old inference helper names and “currently skipped” claims are obsolete; see
the [trait/interface audit](trait-implementation-status.md). Generic classes now inherit instance defaults, including generic method defaults; see
[trait completion](trait-completion-design.md).

The following features were deferred from Phase 5 (standalone generic classes) to Phase 5b:

- **Variance on generic class type parameters.** `Type::GenericClass` currently stores `type_args: Vec<Type>` without variance annotations, unlike `GenericRecord`/`GenericEnum` which store `Vec<(Variance, Type)>`. Assignability is invariant-only. Phase 5b adds `+T` (covariant) and `-T` (contravariant) support matching records/enums.
- **`implements` clause on generic classes.** Currently skipped during collect (`if !class.implements.is_empty() && !is_generic`). Phase 5b registers `TraitImplInfo` per concrete instantiation so trait method dispatch and trait object coercion work.
- **`extends` on generic classes.** `instantiate_generic_class` currently sets `parent_mangled_name: None`. Phase 5b substitutes the parent type during instantiation (e.g., `class Child<T> extends Parent<T>` instantiated as `Child<Int32>` triggers `Parent<Int32>` instantiation), inherits parent fields/vtable, and handles extends args.
- **Let binding initializers on generic classes.** `instantiate_generic_class` currently sets `initializer: vec![]`. Phase 5b stores let binding AST bodies in `ClassTypeSignature` during collect and infers them with substituted types during instantiation.
- **Trait object coercion for generic class instances.** `type_satisfies_trait` and `drain_pending_trait_instantiations` use `to_fqn()` which loses type args for `GenericClass`, causing different instantiations (e.g., `Box<Int32>` vs `Box<String>`) to be treated as identical. Phase 5b fixes dedup keying and trait impl lookup for generic classes.
- **`resolve_trait_impl_method_for_type` for generic classes.** Trait method dispatch on `GenericClass` instances (e.g., calling a trait method on a value of type `Box<Int32>` where `Box` implements the trait) does not work because `resolve_trait_impl_method_for_type` uses `to_fqn()` which loses type args. Phase 5b ensures trait impls registered per instantiation are found correctly.
- **Trait bounds on functions with generic class arguments.** When a function has a trait bound like `function foo<T>(x: T) where T: Display` and is called with a generic class instance `Box<Int32>` that implements `Display`, the trait satisfaction check must find the impl registered for that specific instantiation. Currently broken due to the same `to_fqn()` issue.

---

## 15. References

- [grammar.md](grammar.md) — Class and trait grammar.
- [traits-design.md](traits-design.md) — Traits, impl blocks, class `implements`, trait objects.
- [generics-design.md](generics-design.md) — Monomorphization, bounds, where clause.
- [compiler.md](compiler.md) — Pipeline, typechecker, codegen.
- [variance-any-design.md](variance-any-design.md) — Variance, Any type, `is`/`as` expressions.
- [book/09-classes.md](book/09-classes.md) — Class language reference.
