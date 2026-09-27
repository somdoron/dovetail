# Newtypes Design

This document designs **newtypes** in Dovetail: zero-cost abstraction types, construction, unwrapping via `value`, inference rules (no direct assignment of inner type), codegen transparency, and private newtypes. It aligns with the [type system book](../website/content/book/06-type-system.md), [grammar](grammar.md), and [compiler design](compiler.md).

---

## 1. Overview

- **Newtypes** are **zero-cost** abstraction types: a distinct nominal type that wraps a single underlying (inner) type. At runtime they have no extra representation—they are transparent.
- **Syntax:** `newtype Cents = Int32`, `newtype Email private = String`. The optional `private` keyword restricts construction and access to the associated module only.
- **Construction:** Use the newtype name as a constructor: `Cents(123)`, `UserId(42)`. For private newtypes, only the associated module may construct.
- **Unwrapping (non-private only):** The underlying value is exposed via the **`value`** identifier: `cents.value` has type `Int32` when `Cents = Int32`. Private newtypes have no public `value`; only the associated module may access the inner value.
- **Pattern matching:** For non-private newtypes, `case Cents(n) =>` is allowed and binds the inner value; for private newtypes, such patterns are allowed only in the associated module.
- **Inference layer:** Newtypes are distinct types. Assigning a bare inner value to a variable of newtype type is **not** allowed (e.g. `let x: Cents = ...` then `x = 5` is an error). Construction must be explicit: `Cents(5)`.
- **Codegen:** Newtypes are **transparent**. The `Type` enum represents a newtype as wrapping an inner type; `type_to_valtype` (and related codegen) recurses on the inner type. No separate runtime representation for the newtype wrapper.

**Implementation status:** Implemented.

---

## 2. Type Syntax and Semantics

A newtype declaration introduces a nominal type and its inner type.

**Grammar** (to be added to [grammar.md](grammar.md)):

```
newtype_decl = "newtype" identifier [ variant_type_params ] [ "private" ] [ where_clause ] "=" type
```

**Examples:**

```dovetail
newtype Cents = Int32
newtype UserId = Int32
newtype Email = String
newtype Email private = String
```

- **Nominal identity:** Two newtypes are the same type only if they are the same declaration. `Cents` and `UserId` are different types even though both wrap `Int32`.
- **Single inner type:** Each newtype wraps exactly one type, which may reference its generic type parameters.
- **Private:** When `private` is present, only the **associated module** (the module with the same name as the newtype in the same package) may:
  - Construct the newtype (call `Email(...)`),
  - Access the inner value (e.g. `x.value` or pattern-match on the constructor).
  Outside the associated module, values of a private newtype are opaque: they can be passed around and compared (if the newtype implements Equatable) but not constructed or unwrapped.

---

## 3. Construction

Newtype values are constructed by calling the newtype name as a constructor with a single argument of the inner type.

**Examples:**

```dovetail
let c = Cents(100)
let id = UserId(42)
let email = Email("alice@example.com")
```

- The argument must have the inner type (or a type that unifies with it). No implicit conversion from other types.
- For **private** newtypes, construction is allowed only inside the associated module (e.g. `module Email` for `newtype Email private = String`). Elsewhere, `Email("x")` is a type/visibility error.

---

## 4. Unwrapping: the `value` identifier (non-private only)

For a **non-private** newtype, the underlying value is accessed via the **`value`** identifier.

**Examples:**

```dovetail
newtype Cents = Int32
let c = Cents(100)
let n: Int32 = c.value   // 100
```

- **Syntax:** `expr.value` where `expr` has a newtype type. The type of `expr.value` is the inner type of that newtype.
- **Only for non-private newtypes.** For `newtype Email private = String`, `email.value` is not allowed outside the associated module; inside the associated module, `value` may be used (or an equivalent internal unwrap).
- No other built-in unwrap syntax in this design (e.g. no `unwrap(x)` function); `value` is the single, consistent way to unwrap.

### 4.1 Pattern matching

Pattern matching on newtypes is **allowed when the newtype is not private**. A single-variant pattern binds the inner value.

**Examples:**

```dovetail
newtype Cents = Int32
let c = Cents(100)
match c with
    case Cents(n) => println(n)   // n: Int32
```

- **Non-private newtypes:** `case Cents(n) =>` is allowed anywhere; `n` has the inner type.
- **Private newtypes:** Pattern matching that exposes the inner value (e.g. `case Email(s) =>`) is allowed only inside the associated module. Outside, the newtype is opaque and such patterns are not in scope.

---

## 5. Inference Layer Rules

- **No assignment of inner type to newtype variable:** The typechecker must reject assigning a value of the inner type to a variable (or field) whose type is the newtype. Example:

  ```dovetail
  let x: Cents = Cents(5)
  x = 5   // Error: cannot assign Int32 to Cents
  ```

  Construction must be explicit: `x = Cents(5)`.

- **Distinct types:** A newtype is not a subtype of its inner type and not vice versa. Function `f(c: Cents)` does not accept an `Int32`; function `g(n: Int32)` does not accept a `Cents` unless the caller explicitly unwraps with `c.value`.

- **Equality:** The newtype must explicitly implement `Equatable` (or derive it for a public wrapper): `Cents(5) == Cents(5)`. Comparison uses that implementation. No cross-type comparison: `Cents(5) == 5` is a type error.

---

## 6. Codegen: Transparency

Newtypes do not introduce a separate runtime type or value layout.

- **Representation in the compiler:** In the `Type` enum (or equivalent), a newtype holds a reference to its **inner type**. There is no separate valtype or struct for the newtype itself.
- **type_to_valtype:** When codegen maps a Dovetail type to a WASM valtype, it recurses on newtypes: `type_to_valtype(Newtype(inner)) = type_to_valtype(inner)`. So `Cents` and `Int32` produce the same valtype.
- **Construction and unwrap:** `Cents(5)` is compiled as the same operation as producing an `Int32` (e.g. a constant or local). Reading `x.value` when `x` has type `Cents` is compiled as reading the same storage as for the inner type—no extra load or cast.
- **Alternative (rejected for simplicity):** Stripping newtypes from the typed AST so that downstream only sees the inner type would also achieve transparency but requires a separate pass and more invasive changes. The chosen approach is: keep newtypes in the typed AST and make `Type` carry the inner type, and recurse in codegen.

---

## 7. Private Newtypes and the Associated Module

- **Associated module:** The module that may create and inspect a private newtype is the one with the **same name** as the newtype in the **same package**. Example: `newtype Email private = String` and `module Email = ...` in the same package.
- **Inside the associated module:** Construction `Email(s)` and access to the inner value (e.g. `x.value`) are permitted. The module can expose safe constructors (e.g. validation) and methods that use the inner value without exposing it.
- **Outside the associated module:** Values of type `Email` can be used only as opaque values: passed to functions, returned, stored, and compared (if the inner type supports equality). Construction and `value` access are forbidden.

---

## 8. Out of scope (initial design)

- **Generic newtypes** (e.g. `newtype Id<T> = T`): not in scope.

---

## 9. Summary

| Topic | Design |
|-------|--------|
| **Nature** | Zero-cost, nominal wrapper around a single inner type. |
| **Syntax** | `newtype Name [private] = InnerType`. |
| **Construction** | `Name(expr)` where `expr` has type `InnerType`; for private, only in associated module. |
| **Unwrapping** | Non-private: `expr.value` has type `InnerType`. Private: no public unwrap; only associated module may access inner value. |
| **Pattern matching** | Non-private: `case Name(n) =>` allowed; `n` has inner type. Private: only in associated module. |
| **Inference** | Newtype is distinct; no assigning inner type to newtype variable; explicit construction required. |
| **Codegen** | Transparent: `Type` has inner type; `type_to_valtype` recurses; no extra representation. |
| **Private** | Only associated module (same name, same package) may construct or access `value`. |

---

## 10. Implementation plan and phases

Implementation is split into phases so that each delivers a testable slice. The pipeline (lexer → layout → parser → typechecker → codegen) is extended for newtypes at each stage. Codegen keeps newtypes in the typed AST and recurses on the inner type for valtype and operations.

| Phase | Scope | Parser | Typechecker | Codegen |
|-------|--------|--------|-------------|--------|
| **1** | Newtype declaration, construction, transparency | `newtype_decl`; constructor call `Name(expr)` resolving to newtype | Collect: register newtype (FQN, inner type). Type: newtype variant with inner type. Typecheck construction: arg type = inner type. Reject assigning inner type to newtype variable. | `type_to_valtype` recurses on newtype; construction and value flow as inner type (no extra representation). |
| **2** | `.value` access (non-private) | Postfix `.value` (or reuse field access) | Receiver must be newtype; newtype must be non-private. Type of `expr.value` = inner type. | `.value` compiles to same storage as receiver (no-op / same local/stack slot). |
| **3** | Equality | — | Require an explicit Equatable implementation; compare through its method. Reject `newtypeVal == innerVal`. | Call the explicit equality implementation; storage remains transparent. |
| **4** | Pattern matching `case Name(n) =>` | Constructor pattern for newtype (single sub-pattern) | Scrutinee newtype; non-private: allow anywhere; bind `n` to inner type. Exhaustiveness: single variant. | Lower to binding scrutinee value (same as inner); no extra load. |
| **5** | Private newtypes | `newtype Name private = InnerType` | Collect: mark newtype private. Visibility: only associated module (same name, same package) may construct, use `.value`, or pattern-match. Elsewhere: opaque (pass, store, compare only). | No change; transparency unchanged. |

**Phase 1 — Newtype declaration, construction, transparency**

- **Grammar:** Add `newtype_decl = "newtype" identifier [ variant_type_params ] [ "private" ] [ where_clause ] "=" type` to [grammar.md](grammar.md). Constructor calls `Name(expr)` already parse; resolution distinguishes newtype constructor from enum/other.
- **Lexer:** Tokenize `newtype` and `private` if not already present.
- **Parser:** Parse `newtype_decl`; produce AST node (name, optional private, inner type). Constructor expression: ensure `Name(expr)` can resolve to a newtype (single-arg constructor).
- **Typechecker (Collect):** Register each newtype in the package (FQN, inner type, private flag). No type parameters.
- **Typechecker (Inference / Rules):** Add `Type::Newtype(inner)` or equivalent carrying the inner type. Construction `Name(expr)`: resolve `Name` to newtype, require `expr` to have the inner type. Reject assignment of a value of inner type to a variable (or field) whose type is the newtype.
- **Codegen:** In `type_to_valtype` (and any type-to-representation mapping), when type is newtype, recurse on inner type. Emit construction as the inner value (no wrapper). No new WASM types for newtypes.
- **Tests:** Declare `newtype Cents = Int32`; construct `Cents(100)`; use as parameter/return type; reject `let x: Cents = 5` and `x = 5` when `x: Cents`.

**Phase 2 — `.value` access**

- **Parser:** `.value` is field access; ensure `value` is accepted as identifier (or reserved for newtype receiver).
- **Typechecker:** On `expr.value`, require `expr` to have a newtype type; require that newtype to be non-private. Type of the expression is the inner type. For private newtypes, allow `.value` only in the associated module.
- **Codegen:** `expr.value` compiles to the same value as `expr` (transparent); no extra load or cast.
- **Tests:** `Cents(100).value == 100`; type of `c.value` is `Int32` when `c: Cents`; reject `email.value` outside module for `newtype Email private = String`.

**Phase 3 — Equality**

- **Typechecker:** Equality requires an explicit Equatable implementation. `a == b` for newtypes: same newtype, call its equality implementation. Reject `newtypeVal == innerVal` or `innerVal == newtypeVal` (distinct types).
- **Codegen:** Equality calls the newtype’s explicit implementation; the wrapper still has no runtime allocation.
- **Tests:** `Cents(5) == Cents(5)`; `Cents(5) != Cents(6)`; reject `Cents(5) == 5`.

**Phase 4 — Pattern matching on newtypes**

- **Parser:** Constructor pattern `Name(pattern)` where `Name` resolves to a newtype; single sub-pattern. Reuse or extend existing constructor-pattern grammar.
- **Typechecker:** Scrutinee type must be the newtype. For non-private newtypes, allow `case Name(n) =>` anywhere; `n` has the inner type. For private newtypes, allow only in the associated module. Exhaustiveness: one variant (always covered).
- **Codegen:** Match on newtype: no discriminant; bind sub-pattern to the scrutinee value (already the inner representation).
- **Tests:** `match c with case Cents(n) => assert n == 100`; private newtype pattern only inside associated module.

**Phase 5 — Private newtypes**

- **Parser:** Already support `newtype Name private = InnerType` (Phase 1 grammar).
- **Typechecker (Collect):** Record `private: bool` on newtype. **Rules:** For construction `Name(expr)` and `.value` and pattern `case Name(p) =>`, if the newtype is private, allow only in the **associated module** (module with same name as newtype in same package). Elsewhere, treat values of private newtype as opaque: can be passed, returned, stored, compared (if explicitly Equatable); construction and unwrap forbidden.
- **Codegen:** No change; private is a visibility rule only.
- **Tests:** `newtype Email private = String`; in `module Email`, `Email("x")` and `e.value` allowed; in another module, `Email("x")` and `e.value` rejected; passing `Email` values and `e1 == e2` allowed.

**Dependencies:** Phase 1 is the base. Phase 2 and 3 depend on Phase 1. Phase 4 depends on Phase 1 and on the existing match implementation. Phase 5 depends on Phase 1–4 (private adds visibility to construction, `.value`, and pattern match).

## Private operations in trait implementations

`private` appears after the name and type parameters, before any `where` clause
and `=`. The old `newtype Name = private Type` syntax is rejected.

Only the associated module in the defining package can construct, access `.value`,
or unwrap a private newtype in a pattern. Trait implementations and extensions
have no exemption, including generated implementations: call module functions
for these operations. This intentionally differs from private records and enums,
which expose fields and patterns while restricting construction and record updates.

## Explicit operator capabilities

Newtypes inherit no operators from their representation. Implement `Add<R>`,
`Sub<R>`, `Mul<R>`, `Div<R>`, `Concat<R>`, `Equatable`, or `Comparable` explicitly
for the desired behavior. Each arithmetic/concatenation implementation declares
its associated `Output`, which need not be the newtype. Generic bounds may require
`Output = T`. Unary negation, remainder, bitwise, and shift operators have no
newtype overload traits yet. Construction and storage remain runtime-transparent.
