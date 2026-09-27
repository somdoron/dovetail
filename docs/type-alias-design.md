# Type Alias Design

This document designs **type aliases** and **generic type aliases** in Dovetail: pure naming conveniences with no runtime representation, full application on the right-hand side, trait bounds on type parameters, visibility and imports, and their treatment across the pipeline. It aligns with the [type system book](../website/content/book/06-type-system.md), [grammar](grammar.md), and [compiler design](compiler.md).

---

## 1. Overview

- **Type aliases** are **pure aliases**: they give another name to a type (or type expression) and are used only for convenience. They are **not** represented in the `Type` type; the typechecker expands them and works with the underlying type.
- **Syntax:** `type OptionString = Option<String>`, `type Maybe<T> = Option<T>`, `type Pair<A, B> = (A, B)`. The right-hand side is any **fully applied** type expression.
- **Not newtypes:** Unlike newtypes, type aliases do not introduce a distinct type. Assigning a value of the underlying type to a variable of alias type is allowed: `type Cents = Int32`; `let mutable c: Cents = ...`; `c = 5` is allowed.
- **Generics:** Type aliases may have type parameters with optional **trait bounds** (multiple traits allowed, e.g. `A : Display + Eq`). The RHS must be fully applied (no partial application such as `type Maybe = Option`).
- **Visibility:** Type aliases have visibility (`public`, `internal`, `private`). When used from another package, they **must be imported**.
- **Recursion:** Recursive type aliases are allowed as long as the expanded type is valid (e.g. `type Tree<A> = Option<(A, Tree<A>, Tree<A>)>`).
- **Diagnostics:** Error messages show **both** the alias name and the expanded type when relevant (e.g. "expected `Cents` (Int32), got `String`").
- **Codegen:** Type aliases do not appear in the typed AST as a separate type; codegen sees only the expanded type.

**Implementation status:** Not started.

---

## 2. Syntax and Grammar

### 2.1 Declaration

**Grammar** (to be added or updated in [grammar.md](grammar.md)):

```
type_alias_decl     = [ doc_comment ] [ visibility ] "type" IDENT [ type_params ] "=" type
```

- **visibility:** `public` | `internal` | `private`. Same as other top-level declarations. Omitted means internal (or per-package default).
- **type_params:** Same as for records/classes: `<` type_param `{ "," type_param }` `>`, with `type_param = IDENT [ ":" type_bound { "+" type_bound } ]`. Only **trait bounds** are supported; multiple traits allowed (e.g. `A : Display + Eq`).
- **RHS:** Any `type` (fully applied). No partial application: `type Maybe = Option` is invalid; use `type Maybe<T> = Option<T>`.

**Examples:**

```dovetail
type OptionString = Option<String>
type Maybe<T> = Option<T>
type Pair<A, B> = (A, B)
type Cents = Int32
type DisplayList<A : Display> = List<A>
type ShowAndEq<A : Display + Eq> = List<A>
```

### 2.2 Right-hand side: full application only

The right-hand side must be a **fully applied** type. Every type constructor that expects type arguments must receive them.

- Allowed: `Option<String>`, `(A, B)`, `List<A>` when `A` is a type parameter of the alias, `Result<Int32, String>`.
- Not allowed: `Option` (missing type argument), `List` (missing type argument).

### 2.3 Trait bounds

Only **trait bounds** on type parameters are supported. Multiple traits per parameter are allowed with `+`.

- Allowed: `type Foo<A : Display> = List<A>`, `type Bar<A : Display + Eq> = List<A>`.
- Bounds that depend on other type parameters (e.g. `B : Ord<A>`) are out of scope for this design.

---

## 3. Semantics

### 3.1 Pure alias, not in `Type`

Type aliases are **not** represented as a variant in the `Type` enum. During typechecking, whenever a type alias is referenced, it is **expanded** to its definition. Subtyping, unification, and codegen all see only the expanded type. So:

- `Cents` and `Int32` are the same type for the typechecker after expansion.
- Assigning `5` to a variable of type `Cents` is allowed: e.g. `let mutable c: Cents = 0`; `c = 5` is valid. Type aliases have no constructors; values of the underlying type are used directly.

### 3.2 No duplicate type names in the same package

Within a package, **no two type names may coincide**. A type alias cannot share a name with a class, trait, enum, record, or another type alias in that package. The same uniqueness rule applies as for other type-level declarations (e.g. after stub collection, duplicate names are an error).

### 3.3 Forward references

Forward references are allowed. A type alias may refer to another type alias or type defined later in the same file or package. Resolution is done after **stub collection**: in the Collect phase, we first collect stubs for all declarations (including type aliases), then resolve the RHS of type aliases so that order of declaration does not matter.

### 3.4 Recursive type aliases

Recursive type aliases are allowed provided the resulting type is valid. For example:

```dovetail
type Tree<A> = Option<(A, Tree<A>, Tree<A>)>
```

Expansion is well-defined as long as the structure is valid (e.g. recursion through type constructors like `Option` and tuples). The typechecker expands aliases and validates the underlying type; no special cycle detection beyond what is already required for valid types.

---

## 4. Visibility and Imports

- **Visibility:** Type aliases have **public**, **internal**, or **private** visibility.
  - **public:** Visible to other packages when imported.
  - **internal:** Visible only within the same package.
  - **private:** Visible only within the **same file**.
- **Use from other packages:** A type alias must be **imported** to be used from another package. There is no implicit re-export; the dependent package must explicitly import the alias (or the module that contains it) to refer to it by name.

---

## 5. Diagnostics

When reporting type errors, the compiler should show **both** the alias name (if the user wrote it) and the expanded type when that helps clarity.

- Example: expected type is `Cents` (alias for `Int32`), actual type is `String`. Prefer a message like: expected `Cents` (Int32), got `String`.
- This applies to mismatch errors, wrong arity, and similar; the exact format can be chosen during implementation.

---

## 6. Pipeline Integration

### 6.1 Collect phase

- **Stub collection:** Type alias declarations are collected in the same stub pass as other type-level declarations. Each alias is registered by name (FQN) with its visibility and (after resolution) its RHS.
- **Resolution:** After stubs are known, the RHS of each type alias is resolved. The RHS may refer to types and aliases defined later in the package. Duplicate type names in the package are rejected.
- **Registry:** Public (and internal, as per existing rules) type aliases are part of the package’s registry so that dependents can resolve them after import. Internal-only entries are stripped before exposing the registry to other packages.

### 6.2 Inference and Rules

- **Expansion:** Whenever the typechecker sees a type that is a reference to a type alias, it **expands** it to the aliased type (with type parameters substituted). The `Type` type does not have a variant for aliases; only the expanded form is stored in the typed AST.
- **Trait bounds:** For a generic type alias, when the alias is used (e.g. `DisplayList<X>`), the type arguments must satisfy the alias’s type parameter bounds (e.g. `X : Display`). Checking is done at the use site after expansion.

### 6.3 Codegen

- Type aliases do **not** appear in the typed AST as a distinct type. Codegen sees only the expanded type; no special handling for aliases is required.

---

## 7. Relation to Newtypes

| Aspect            | Type alias              | Newtype                    |
|-------------------|-------------------------|----------------------------|
| Identity          | Same as underlying type | Distinct nominal type      |
| Assignment        | `c = 5` allowed if `Cents = Int32` | `x = 5` disallowed; must use constructor |
| Representation    | Not in `Type`; expanded | In `Type` as wrapper       |
| Construction      | No constructor; use underlying type | Constructor `Cents(5)`     |
| Use case          | Naming convenience      | Abstraction, invariants    |

---

## 8. Out of scope (initial design)

- **Partial application:** `type Maybe = Option` (RHS as type constructor) is not supported.
- **Bounds beyond traits:** Bounds that reference other type parameters (e.g. `B : Ord<A>`) are not in scope.
- **Type aliases inside classes/traits/functions:** Only top-level type aliases are supported.
- **Higher-kinded aliases:** No kind polymorphism; only types of kind `Type` (fully applied).

---

## 9. Summary

| Topic           | Design |
|-----------------|--------|
| **Nature**      | Pure alias; not represented in `Type`; expanded during typechecking. |
| **Syntax**      | `type Name = Type` or `type Name<Params> = Type` with optional trait bounds on params. |
| **RHS**         | Any fully applied type expression. No partial application. |
| **Trait bounds**| Only trait bounds on type parameters; multiple allowed (`A : Display + Eq`). |
| **Scope**       | Top-level only. |
| **Visibility**  | public / internal / private (same file). Must be imported to use from another package. |
| **Uniqueness**  | No duplicate type names within the same package. |
| **Forward refs**| Allowed; stub collection in Collect, then resolve. |
| **Recursion**   | Allowed if the expanded type is valid. |
| **Diagnostics** | Show both alias name and expanded type when relevant. |
| **Codegen**     | Transparent; only expanded type appears. |

---

## 10. Implementation plan and phases

Implementation extends the pipeline (lexer → layout → parser → typechecker) so that type aliases are collected, expanded, and never appear as a separate type in the typed AST or codegen.

| Phase | Scope | Parser | Typechecker | Codegen |
|-------|--------|--------|-------------|--------|
| **1** | Non-generic type alias, visibility | `type_alias_decl` with optional `[ visibility ]` | Collect: stub collection for all decls; register type aliases (FQN, visibility, RHS). Resolve RHS after stubs. Reject duplicate type names. Inference/Rules: expand alias to RHS; no `Type` variant for alias. | No change; sees expanded type. |
| **2** | Generic type alias, trait bounds | Reuse `type_params` with bounds | Collect: register generic alias; RHS may use alias type params. Rules: at use site, check type args satisfy bounds; expand and substitute. | No change. |
| **3** | Imports and diagnostics | — | Public aliases in registry; require import to use from other package. Diagnostics: include both alias name and expanded type where helpful. | No change. |

**Phase 1 — Non-generic type alias**

- **Grammar:** Ensure `type_alias_decl = [ doc_comment ] [ visibility ] "type" IDENT [ type_params ] "=" type` in [grammar.md](grammar.md) (add `[ visibility ]` if not present).
- **Lexer:** `type` is already a keyword.
- **Parser:** Parse `type_alias_decl`; produce AST node (visibility, name, optional type_params, RHS type).
- **Typechecker (Collect):** Stub collection: register each type alias (FQN, visibility, RHS). Resolve RHS after all stubs are collected. Reject duplicate type names in package. Enforce full application on RHS (no bare `Option` etc.).
- **Typechecker (Inference/Rules):** When resolving a type name to a type alias, expand it to the RHS (with no type params, just substitute). Do not add a `Type::TypeAlias` variant; store only the expanded type in the typed AST.
- **Tests:** `type Cents = Int32`; use `Cents` as variable type, parameter type, return type; assign `5` to `c: Cents`; reject duplicate name in same package.

**Phase 2 — Generic type alias and trait bounds**

- **Parser:** Reuse existing `type_params`; no grammar change if type_params already support bounds.
- **Typechecker (Collect):** Register generic type alias; resolve RHS with alias type params in scope; RHS must be fully applied.
- **Typechecker (Rules):** When typechecking a use (e.g. `DisplayList<X>`), check that `X` satisfies bounds (e.g. `X : Display`). Expand to `List<X>` and continue.
- **Tests:** `type Maybe<T> = Option<T>`, `type Pair<A,B> = (A,B)`; use with type args; `type DisplayList<A : Display> = List<A>` and use with type that implements Display; reject use with type that does not satisfy bound.

**Phase 3 — Imports and diagnostics**

- **Typechecker:** Public type aliases are exported in the registry; other packages must import the alias (or its module) to use it. Enforce visibility (private = same file, internal = same package, public = importable).
- **Diagnostics:** In type error messages, when the expected or actual type came from a type alias, show both the alias name and the expanded type (e.g. expected `Cents` (Int32), got `String`).
- **Tests:** Cross-package use of public alias only after import; private alias not visible in another file; diagnostic format for alias types.

**Dependencies:** Phase 1 is the base. Phase 2 builds on Phase 1. Phase 3 can be done after Phase 1 or 2.
