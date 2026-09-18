# Enums Design

This document designs **enums** (discriminated unions) in Dovetail: definition, variant access (including qualified form `Enum.Variant`), variant payloads (tuple `(...)` or record `{ ... }` syntax), generics, match expressions, and codegen strategies. It aligns with the [type system book](book/06-type-system.md), [grammar](grammar.md), [match-expression-design](match-expression-design.md), and [generics-design](generics-design.md).

**In scope:** Enum declaration, generic enums (`Option<T>`, `Result<T,E>`), variant construction and pattern matching, qualified variant names (`Option.Some`), **bare names** for `Some`, `None`, `Ok`, `Error` (and optionally `Result`) when in scope, variant payloads (tuple or record syntax, see §3), exhaustiveness, and codegen (sub-type vs flat approach).

**Out of scope:** Trait bounds on enum type parameters (covered by generic design when added).

**Implementation status:** In progress (Enum + Match expression).

---

## 1. Overview

- **Enums** are discriminated unions: a value is exactly one of a fixed set of **variants**. Each variant may carry no data, or a payload in tuple form `(...)` or record form `{ ... }` (§3).
- **Variant access** is **qualified by the enum name**: e.g. `Option.Some(42)`, `Option.None`, `Result.Ok(x)`, `Result.Error("failed")`. The grammar and typechecker resolve `EnumName.VariantName` as the variant of that enum. When in scope (see §2.2), **bare** `Some`, `None`, `Ok`, and `Error` (and optionally the type `Result`) can be used without the enum prefix.
- **Generics:** Enums may have type parameters (e.g. `Option<T>`, `Result<T,E>`). Same **specialized-only** (monomorphized) rules as generic records/functions: one WASM type per instantiation. Prelude defines `Option<T>` and `Result<+T,+E>`.
- **Match:** `match expr with case Enum.Variant(...) => ...` or, for Option/Result when in scope, bare `Some`/`None`/`Ok`/`Error` in patterns. Exhaustiveness requires covering every variant. Constructor patterns destructure payloads.
- **Variant payloads** use either **tuple** syntax `Variant(Type1, Type2)` — parentheses, positional — or **record** syntax `Variant { x: Type1, y: Type2 }` — curly braces, named fields. One form per variant (§3).

---

## 2. Variant Access: Qualified by Enum Name

### 2.1 Rule

- **Construction and pattern matching use the qualified form** `EnumName.VariantName`.
- Examples:
  - `Option.Some(42)`, `Option.None`
  - `Result.Ok(100)`, `Result.Error("msg")`
  - `Color.Red`, `Shape.Circle(5.0)`, `Shape.Rectangle(10.0, 20.0)`

Resolution is the same as other qualified names (see [multi-project-multi-package-design](multi-project-multi-package-design.md) §8.10): the typechecker resolves the leading segment(s) via imports/registry; when the receiver is an enum type (or a type name referring to an enum), the trailing segment is the variant name.

### 2.2 Bare names for Option and Result

When the names are **in scope**, the following can be used **without** the enum prefix:

- **Variants:** `Some`, `None`, `Ok`, `Error` — in both construction and pattern matching.
- **Type (optional):** `Result` as the enum type (so `Result<Int32, String>` can be written without a package prefix when `Result` is in scope).

**Scope:** Prelude brings `Option`, `Result`, and their variants into scope for the root package (or for any package that imports the prelude). Explicit imports (e.g. `import prelude.Option` or `import prelude.Some`) can also bring these names into scope. Resolution: a bare `Some`, `None`, `Ok`, or `Error` in an expression or pattern is treated as `Option.Some`, `Option.None`, `Result.Ok`, or `Result.Error` respectively when that interpretation is unambiguous (e.g. `Some(42)` → `Option.Some(42)`). If both `Option` and `Result` (or other enums) defined a variant with the same name, qualified form is required.

**Implementation:** This is syntactic sugar; the compiler rewrites bare names to the qualified form during resolution. AST and codegen see only the qualified form. No new runtime behavior.

---

## 3. Variant Payloads: Tuple (parentheses) or Record (curly)

### 3.1 Concept

Each variant may have:

- **No payload** — e.g. `None`, `Red`, `Point`.
- **Payload with tuple syntax** — parentheses: `Variant(Type1, Type2)`. Positional, like a tuple. Construction: `Enum.Variant(expr1, expr2)`. Pattern: `case Enum.Variant(p1, p2) =>`.
- **Payload with record syntax** — curly braces: `Variant { field1: Type1; field2: Type2 }`. Named fields, like a record. Construction: `Enum.Variant { field1 = expr1; field2 = expr2 }`. Pattern: `case Enum.Variant { field1 = p1; field2 = p2 } =>`.

The declaration delimiter fixes the stored payload shape. A single-record tuple payload additionally accepts brace construction and patterns as shorthand (§3.4). **Empty enums** (zero variants) are **not** allowed; every enum must have at least one variant.

### 3.2 Definition syntax

Current grammar has:

```
enum_variant = [ doc_comment ] IDENT [ "(" type_list ")" ]
type_list    = type { "," type }
```

So today only **tuple-style** (positional) is allowed (e.g. `Rectangle(Float64, Float64)`).

To support **record-style** (named) payloads we add a second form:

- **Tuple form (existing):** `IDENT "(" type_list ")"` — e.g. `Circle(Float64)`, `Rectangle(Float64, Float64)`.
- **Record form (new):** `IDENT "{" record_field_list "}"` — same as record fields, e.g. `Point { x: Int32; y: Int32 }`.

Grammar extension (conceptual):

```
enum_variant = [ doc_comment ] IDENT [ payload_spec ]
payload_spec = "(" type_list ")"           // tuple: positional types
             | "{" record_field_list "}"   // record: named fields (record_field = IDENT ":" type)
```

So each variant has either a tuple payload (parens) or a record payload (curly), never both. No mixing.

### 3.3 Construction and pattern matching

- **Tuple variant:** `Option.Some(42)`, `Shape.Rectangle(10.0, 20.0)`. Pattern: `case Option.Some(x) =>`, `case Shape.Rectangle(w, h) =>`.
- **Record variant:** `Shape.Point { x = 0; y = 0 }`. Pattern: `case Shape.Point { x = px; y = py } =>`. Construction and pattern use the same `{ field = value }` / `{ field = pattern }` syntax as records.

---

### 3.4 Named record payload shorthand

A declaration such as `Placed(Placement)` remains a single-payload tuple variant. When its payload resolves to a record, brace expressions and patterns are shorthand for nesting that record:

```dovetail
OrderStatus.Placed { placedAt = now; total = total }
// Equivalent to:
OrderStatus.Placed(Placement { placedAt = now; total = total })

case Placed { placedAt; total } => ...
// Equivalent to:
case Placed(Placement { placedAt; total }) => ...
```

The positional forms remain available for passing or binding the whole record. Inline record variants retain their existing representation and construction rules.

- Resolve enum type parameters before identifying the record. Infer generic record parameters from fields and the expected enum type; an unconstrained payload parameter cannot be identified by field names alone.
- Unwrap one layer only. Non-record payloads, newtypes, classes, and multiple positional payloads do not accept this shorthand.
- Reuse ordinary record field validation and pattern semantics, including omitted pattern fields. Construction requires all fields; this feature adds no defaults or rest syntax.
- Check enum and record construction access independently. Matching follows inspection rules; wrapping an existing record does not construct another record.
- Bare patterns resolve against the scrutinee's enum. Custom expressions retain the enum qualifier; prelude `Some`, `Ok`, and `Error` support braces when context identifies the record. Ordinary record names retain precedence in expressions.
- Type checking lowers shorthand to the existing nested record and positional enum nodes. Code generation, runtime layout, and exhaustiveness therefore use the same representation as the explicit nested form.

---

## 4. Match Expression on Enums

### 4.1 Constructor patterns

- **Form:** `case EnumName.VariantName([ pattern_list ]) => body`. If the variant has no payload, the parentheses are omitted: `case Option.None =>`.
- **Semantics:** The scrutinee must have type that is (or includes) the enum type. The arm is taken when the discriminant indicates that variant; payload patterns are matched against the payload. Variables bound in the pattern are in scope in the guard (if any) and body.
- **Exhaustiveness:** For a scrutinee of enum type `E`, the match must cover every variant of `E` (and only those). So for `Option<T>`, both `Option.Some(_)` and `Option.None` (or equivalents) must appear, or the compiler reports a non-exhaustive match. The algorithm generalizes the one in [match-expression-design](match-expression-design.md) (e.g. “usefulness” / space-based exhaustiveness).

### 4.2 Guards and ordering

- **Guards:** `case Option.Some(x) if x > 0 => ...` — same as for other patterns; the guard may use variables bound in the pattern.
- **Order:** Arms are tried in source order; the first matching arm (pattern + guard) wins.

### 4.3 Example

```dovetail
function describe(opt: Option<Int32>): String =
    match opt with
        case Option.Some(n) if n < 0 => "negative some"
        case Option.Some(n) => "non-negative: " ++ n.toString()
        case Option.None => "none"
```

---

## 5. Codegen: Sub-type vs Flat Approach

We need to choose how to represent enums in WASM-GC: either each variant as a **sub-type** of a common base, or a **flat** tagged union with a single base type and a tag.

### 5.1 Sub-type approach

- **Idea:** The enum type is an abstract base; **each variant is a distinct WASM-GC struct type** that extends (is a sub-type of) that base.
- **Layout:** Base has no (or minimal) fields; each variant struct has the payload fields. A value is always a reference to one of the variant structs; the runtime type identifies the variant.
- **Discriminant:** No explicit tag field; the **runtime type** of the reference is the discriminant (e.g. `ref.as` / type checks to see which variant).
- **Pros:**
  - No extra tag field; variant identity is the type.
  - Natural fit for WASM-GC subtyping; good for future extensions (e.g. methods per variant).
  - Clear representation: each variant is its own “record” shape.
- **Cons:**
  - More type definitions and more indirection if the engine doesn’t optimize small variants well.
  - Switching on variant requires type checks (e.g. cast or type-test), which may be less compact than a single tag comparison.

### 5.2 Flat approach

- **Idea:** One **base struct type** with a **tag field** (e.g. `i32` discriminant) plus a **single** “payload” area (e.g. a tuple or a union of inline/ref fields). Every enum value is this one struct; the tag indicates the variant; payload layout is a union (or fixed layout with unused bytes for smaller variants).
- **Layout:** e.g. `{ tag: i32, payload_0: ..., payload_1: ... }` or a single `(ref any)` for payload. Each variant is a **sub-type** of this base only in the sense of “same layout, different tag”; or we use one type and only the tag differs.
- **Discriminant:** One load + integer comparison per match.
- **Pros:**
  - Single type; simple switch on tag; predictable layout and cache behavior.
  - Easy to add new variants without new WASM types.
- **Cons:**
  - Tag and possibly padding; payload is a union so we may need more casts or larger common size.
  - Less “object-oriented”; variant identity is not the type but the tag.

### 5.3 Decision

**We adopt the sub-type approach:** Each variant is a distinct WASM-GC struct type (subtype of a common enum base). No explicit tag field; the runtime type of the reference is the discriminant. This fits WASM-GC’s type hierarchy, aligns with MoonBit’s choice (§5.4), and keeps a clear one-type-per-variant representation.

**Option-of-reference** means `Option<T>` when `T` is a **reference type** (e.g. `Option<String>`, `Option<SomeRecord>`). In the **language**, Option is a regular enum (same syntax, typing, and match as any other). An **optional codegen** optimization is to represent `Option<RefType>` as a **nullable reference**: **null** = `None`, **non-null** = `Some(ref)`. That avoids a separate discriminant and can save space; the source and type system are unchanged. Same idea as Rust’s “null pointer optimization” for `Option<&T>`. Implementation may use this special case or stick to the uniform sub-type layout for all enums.

### 5.4 How other languages implement it

**Rust (native):** Enums are **tagged unions**: one discriminant (tag) plus a union of payloads, with size equal to the largest variant (plus alignment). So Rust uses the **flat** approach: one layout, one tag. For `Option<&T>` and similar, it uses the **null pointer optimization**: no tag byte; the pointer value itself is the discriminant (null = `None`, non-null = `Some`). So Rust mixes flat layout with a special-case “sub-type-like” encoding for option-of-reference.

**OCaml (Wasm):** In OCaml’s WebAssembly runtime, **constant constructors** (variants with no payload) are represented as `(ref i31)` — small integers in a reference. **Blocks** (variants with payload) use `(array (mut (ref eq)))` with the **first field as the tag** (`ref i31`). So OCaml on Wasm uses a **flat**-style representation: one block shape per constructor, with an explicit tag field. The GC proposal’s “tagged unions” and instructions like `br_on_case` are a possible future way to avoid storing tags in a separate field.

**MoonBit (WASM-GC):** The MoonBit compiler (see `moonbitlang/moonbit-compiler`, `transl_mtype_gc.ml`) uses a **sub-type–style** encoding for general enums: each variant is translated to a **separate WASM-GC type** (`Ref_constructor { args }`), i.e. each constructor is its own struct-like type with its payload fields. The enum type as a whole is represented as `ref_enum` (a common reference type). So **each variant is a distinct type**; matching is done by type tests/casts. For **Option** over reference types, MoonBit uses a **nullable reference** (`Ref_nullable { tid }`): no separate tag, null = `None`, non-null = `Some` — same idea as Rust’s null pointer optimization.

**WASM-GC:** The current GC proposal has **struct** and **array** types and **recursive types** with **sub** (subtyping). There is no built-in “variant” or “tagged union” type in the MVP; sum types are implemented by compilers either as (1) **one struct + tag field + payload** (flat), or (2) **multiple struct types in a recursive group with a common supertype** (sub-type). A post-MVP extension may add tagged unions and instructions such as `br_on_case` to branch on variant tags.

**Summary:** Languages targeting WASM-GC today either use a **flat** layout (one struct + tag, like OCaml-on-Wasm) or a **sub-type** layout (one type per variant, like MoonBit). Option-of-reference is often special-cased as a nullable reference (no tag). Dovetail can follow either strategy; MoonBit’s choice (sub-type for enums, nullable ref for Option) aligns with WASM-GC’s struct subtyping and keeps a single representation style for all enums except the Option special case.

---

## 6. Variants as Types (Open Design Choice)

**Question:** Should each variant be a **type** in its own right?

- **Example:** Is `function foo(x: Option.Some<Int32>)` or `function foo(x: Some<Int32>)` allowed? That would mean `foo` accepts only the `Some` variant of `Option<Int32>`, not `Option.None`.

**Options:**

- **A — Variant is not a type:** Only the full enum type is a type. You cannot write `x: Some<Int32>` or `x: Option.Some<Int32>`; you must use `x: Option<Int32>`. Matching is the only way to narrow.
- **B — Variant is a type:** The type `Option.Some<Int32>` exists and represents “the `Some` variant of `Option<Int32>`”. Function parameters and other type positions can use it. Construction with `Option.Some(42)` has type `Option.Some<Int32>` which is a **subtype** of `Option<Int32>`. Then `function foo(x: Option.Some<Int32>)` is allowed and accepts only `Some` values.

**Implications:**

- **Subtyping:** With B, we have subtyping: `Option.Some<T> <: Option<T>`, and similarly for other enums/variants. This fits the sub-type codegen approach (each variant is a distinct type).
- **Generics:** With B, `Option.Some<T>` would be a type constructor (variant “applied” to type args). Prelude would define `Option` with type parameter `T` and variant `Some(T)`; the type of `Option.Some(42)` could be inferred as `Option.Some<Int32>` and also as `Option<Int32>`.
- **Simplicity:** A keeps the type system simpler (no variant types, no subtyping for enums). B is more expressive and matches languages that have “variant types” (e.g. F#, TypeScript discriminated unions with narrowing).

### 6.1 How other languages treat it

- **Rust:** Variants are **not** types. You cannot write `fn foo(x: Option::Some<i32>)`; the compiler treats variant names as constructors, not types. The enum is the single type; all variants share it. To accept “only one variant,” the idiomatic approach is the **newtype pattern**: define a separate struct (or enum with one variant) for that payload and have the main enum wrap it; then functions take the inner type (e.g. `fn only_some(x: UserData)` where `enum Role { User(UserData), Admin(AdminData) }`).
- **F#, OCaml, Haskell:** Same idea — the **sum type** is the type. You don’t get a distinct type for “only the `Some` case.” F#’s “single-case discriminated union” is a different thing: it’s a type that has only one case (like a newtype), not “one variant of a multi-variant union as a type.”
- **TypeScript:** Discriminated unions are union types; there is no first-class “variant type.” You can approximate “only this variant” with generics and `Extract<Union, { tag: T }>`, but it’s a type-level trick, not a dedicated language construct.

So most statically-typed languages with sum types use **A**: only the full enum/sum is a type; narrowing is by pattern matching (or in TS by control flow on the discriminant).

### 6.2 Decision

**We adopt A:** Only the full enum type is a type. Variants are not types. You cannot write `x: Option.Some<Int32>` or `x: Some<Int32>` in parameter or other type positions; you use `Option<Int32>` and narrow by matching. This keeps the type system simpler and aligns with Rust, F#, OCaml, and Haskell.

---

## 7. Generic Enums

### 7.1 Declaration

- Enums may declare type parameters: `enum Option<T> = ...`, `enum Result<T, E> = ...`. Grammar already has `enum_decl = ... "enum" IDENT [ type_params ] [ "private" ] [ where_clause ] "=" enum_body`.
- Variance (e.g. `Result<+T, +E>`) is as in the type system book and will be enforced in the typechecker; codegen follows the same specialized-only rules as other generics.

### 7.2 Specialized only

- One WASM type per instantiation (e.g. `Option<Int32>`, `Option<String>`, `Result<Int32, String>`). No type-info in the value; no shared representation.

### 7.3 Type and registry

- **Type:** Enum types in the type system are represented with type arguments, e.g. `Enum(Fqn, MangledName, Vec<Type>)`, analogous to `Record(...)`. Variants are not separate types (see §6.2).
- **Registry:** Enum declarations produce entries (e.g. `EnumDef`) with FQN, type parameters, and list of variants (name + payload types/names). Generic enums are instantiated (specialized) during typechecking like generic records.

---

## 8. Grammar and AST

### 8.1 Variant access in expressions and patterns

- **Expressions:** Constructor calls use qualified form: `Option.Some(42)`. The grammar already allows `type_name "(" [ arg_list ] ")"` for calls; `type_name` can be `IDENT` or `package_path "." IDENT` (and type_args). So `Option.Some(42)` parses if `Option.Some` is resolved as a “constructor” (variant). We may need to allow `Option.Some` (without type args in the source) and infer type arguments from the argument `42`; same as today for generic functions.
- **Patterns:** `constructor_pattern = type_name "(" [ pattern { "," pattern } ] ")"`. So `Option.Some(x)` and `Option.None` (zero patterns) are already in the grammar; `type_name` here is the variant, optionally qualified. Exhaustiveness and type-checking of constructor patterns are done in the typechecker.

### 8.2 Variant payload: tuple vs record syntax

- **Tuple form:** Constructor and pattern use parentheses: `Enum.Variant(exprs)` / `case Enum.Variant(patterns) =>`. Grammar already has `type_name "(" ... ")"` for both.
- **Record form:** Constructor and pattern use **curly braces**: `Enum.Variant { field = expr; ... }` / `case Enum.Variant { field = pattern; ... } =>`. Same as record construction and record patterns; extend grammar so that after a variant name we allow either `"(" ... ")"` or `"{" field_pattern_list "}"`. No ambiguity: delimiter distinguishes tuple vs record payload.

### 8.3 AST

- Enum declaration: AST node for `enum_decl` with name, type_params, and list of variant definitions. Each variant: name + payload kind (tuple: list of types; record: list of field names + types).
- Constructor expression: Call-like node with either arg list (tuple) or field initializers (record). “Receiver” is the variant (e.g. `Option.Some`). Resolution attaches to the enum and variant.
- Constructor pattern: Pattern node that refers to the enum and variant; payload is either a list of sub-patterns (tuple) or a list of field–pattern pairs (record). Typechecker resolves to the enum definition and checks shape (arity or field names).

---

## 9. Summary Table

| Topic | Decision / content |
|-------|--------------------|
| Variant access | Qualified form always valid. Bare `Some`, `None`, `Ok`, `Error` (and optionally type `Result`) when in scope (§2.2). |
| Payload | **Tuple:** `Variant(Type1, Type2)` — parentheses, positional. **Record:** `Variant { x: Int32, y: Int32 }` — curly braces, named fields. One form per variant. Construction/pattern: `Variant(exprs)` or `Variant { field = expr }`. |
| Match | `case Enum.Variant(...) =>`; exhaustiveness over all variants; guards allowed. |
| Codegen | **Sub-type:** each variant is a distinct WASM-GC struct type; discriminant = runtime type (§5.3). Option-of-ref may use nullable reference. |
| Variant as type | **A:** Variant is not a type. Only the full enum type exists; narrow by match. (See §6.1–6.2.) |
| Generics | Specialized only (one instantiation per type-argument list); `Option<T>`, `Result<T,E>` in prelude. |
| Empty enum | **Not allowed.** Every enum must have at least one variant. |

---

## 10. Implementation Phases

Suggested order for implementing enums. Each phase delivers a testable slice. **Prerequisite:** Match expression (literals, variable, wildcard, guards) and basic typechecking exist; generic records/functions (specialized only) are in place if we do generic enums in the same release.

| Phase | Scope | Notes |
|-------|--------|--------|
| **1. Enum declaration (non-generic)** | Parse `enum IDENT =` with variants `IDENT [ "(" type_list ")" ]`. AST and Collect: register enum (FQN, list of variants with payload types). Positional payloads only. | Lexer, layout, parser, AST. Registry: `EnumDef` with variant name + list of types per variant. No construction or match yet. |
| **2. Constructor expressions** | Qualified `Enum.Variant(args)` in expressions. Resolution: resolve leading segment to enum, trailing to variant. Infer type of call as enum type; check arg count and types. Codegen: allocate variant (sub-type per §5.3). | Typecheck: resolve qualified name to enum + variant; type-check args against variant payload. Codegen: emit one WASM-GC struct type per variant (sub-type of common base); constructor allocates the appropriate variant struct. |
| **3. Match on enums** | Constructor patterns `case Enum.Variant(patterns) =>` and `case Enum.Variant =>` (no payload). Exhaustiveness: require all variants covered. Codegen: lower match to discriminant check + payload access. | **Concrete enums only** (non-generic or specialized instantiation). Extend match-expression-design: constructor pattern resolution, exhaustiveness over enum variants. Codegen: branch on type-test (ref.test / cast) per variant type; project payload, evaluate arm. |
| **4. Generic enums** | `enum IDENT [ type_params ] =` and prelude `Option<T>`, `Result<T,E>`. Specialized only (same as records). Type-arg inference at **constructor**. Variance for Result. | Collect: enum with type_params; instantiate (specialized). Prelude: define Option, Result. Typecheck: type args at construction. Codegen: one WASM type per instantiation. Match on generic enums: scrutinee is always concrete. |
| **5. Match on generic enums** | Matching when scrutinee is a generic enum with **concrete** type arguments (e.g. `Option<Int32>`, `Result<String, Error>`). | Same as match on non-generic enums: scrutinee type is always fully resolved; constructor patterns and exhaustiveness as in Phase 3. No reified path (generics are specialized only). |
| **6. Bare names** | Prelude (or import) brings `Some`, `None`, `Ok`, `Error` (and optionally type `Result`) into scope. Resolution rewrites bare name to `Option.Some` / `Option.None` / `Result.Ok` / `Result.Error`. | Resolution only: when name is in scope as bare variant/type, rewrite to qualified form before typecheck/codegen. No AST or codegen change. |
| **7. Record-style variant payloads (optional)** | Variants with **curly** payload: `Variant { field: Type }`. Construction: `Enum.Variant { field = expr }`; pattern: `case Enum.Variant { field = p } =>`. | Grammar: enum_variant allows `"{" record_field_list "}"`; constructor/pattern allow `Variant { ... }`. Same field/pattern rules as records. Codegen: same layout as tuple variant (field order). Can defer to a later release. |

**Dependencies:** 2 depends on 1. 3 depends on 2 (need constructor type and codegen). 4 can follow 3 (generics build on non-generic enums). 5 is match on generic enums with concrete type args (same as Phase 3; no reified path). 6 depends on 4 (Option/Result in prelude). 7 is optional and can follow 2/3.

**Codegen:** Phase 2 and 3 use the **sub-type** approach (one WASM type per variant) per §5.3. Option-of-reference can be special-cased as nullable ref in the same phases or in a small follow-up. Generic enums (Phase 4–5) use one WASM type per instantiation; match scrutinee is always concrete.

---

## 11. Open Points / Further Questions

The following can be decided later or left to implementation:

1. **Record-style variants in v1:** Ship with **tuple-only** (parentheses) variant payloads first and add record-style (curly) in Phase 7, or include both from the start?

2. **Recursive enums:** e.g. `enum Tree = Leaf(Int32) | Node(Tree, Tree)`. Any extra rules (e.g. boxing for codegen, or forbidding recursion in certain positions)?

## Private construction

Declare `enum Name private = ...` to restrict construction to `module Name`
in the defining package. For generic types, place `private` after the type
parameters and before any `where` clause. Leading declaration visibility remains
independent: `public enum Name private = ...` exposes the type.

Callers can inspect values and pattern-match normally. All variant payload forms
remain available in patterns; using any variant as a construction expression
requires the associated module, including variants without payloads.

Trait implementations, extensions, other modules in the same package, and
same-named modules in other packages have no construction privilege. Call module
functions instead. Generated code follows the same rules.
