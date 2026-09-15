# Records Design

This document designs **records** in Dovetail: definition, construction, field access, the `with` expression, and pattern matching on records (including literal field patterns). It aligns with the [type system book](book/06-type-system.md), [control flow book](book/04-control-flow.md), and [grammar](grammar.md).

---

## 1. Overview

- **Records** are immutable, data-only types with named fields. They map to **WASM-GC record types** in the component model. The compiler may later optimize some records to scalars (e.g. single-field or small value types).
- **Immutability:** Records are always immutable; there is no `mutable` modifier for record fields (unlike classes).
- **Scope of this design:** (1) Defining a record, (2) Creating a record, (3) Field access, (4) `with` expression, (5) Matching on records, including **literals inside record field patterns** and a concrete syntax that does not use `:` for field patterns (so `:` remains available for future use, e.g. type-annotated matches on enums). **Generics are out of scope** and will be designed separately.

**Implementation status:** Done (including generic records + match).

---

## 2. Defining a Record

Records are declared with the `record` keyword, a name, and a body of named, typed fields.

**Grammar** (from [grammar.md](grammar.md)):

```
record_decl         = [ doc_comment ] "record" IDENT [ type_params ] [ "private" ] [ where_clause ] [ "=" record_body ]
record_body         = BEGIN { record_field SEP } record_field [ SEP ] END
record_field        = [ doc_comment ] IDENT ":" type
```

The grammar allows optional `type_params`; **generic records are out of scope for this design** and will be designed in the future. This document considers only non-generic records.

**Examples:**

```dovetail
record Point =
    x: Int32
    y: Int32

record User =
    name: String
    email: String
    age: Int32
```

- Fields are ordered and named; each has a type. No default values in this design.

---

## 3. Creating a Record

A record value is created by giving the type name followed by a brace block of field initializers. Layout or semicolons separate initializers.

**Grammar:**

```
record_construct_expr = type_name "{" [ field_init_list ] "}"
field_init_list       = field_init { SEP field_init } [ SEP ]
field_init            = IDENT "=" block_expr
```

**Examples:**

```dovetail
let origin = Point { x = 0; y = 0 }
let user = User { name = "Alice"; email = "alice@example.com"; age = 30 }
```

Multi-line (layout):

```dovetail
let user = User {
    name = "Alice"
    email = "alice@example.com"
    age = 30
}
```

- Every field must be initialized exactly once. Order of fields in the initializer list is irrelevant; field names determine which field gets which value.
- The right-hand side of each `field = ...` is an **expression** (not a pattern).

---

## 4. Field Access

Fields are accessed with dot notation.

**Grammar:** `postfix_op` includes `"." IDENT` (field access).

**Examples:**

```dovetail
let userName = user.name
let userAge = user.age
let px = origin.x
```

- Typechecker resolves the field name against the record type of the receiver. No optional chaining or defaulting in this design.

---

## 5. With Expression

Records are immutable. The `with` expression produces a **new** record that copies an existing record and overrides specified fields.

**Grammar:**

```
with_expr   = expression "with" with_body
with_body   = BEGIN { field_init SEP } field_init [ SEP ] END
```

So `with_body` uses the same `field_init = IDENT "=" block_expr` as record construction.

**Examples:**

```dovetail
let older = user with age = user.age + 1
let renamed = user with name = "Bob"
let moved = origin with x = 10; y = 20
```

- The left-hand side is evaluated once; then a new record value is built with all fields from that value except those listed in `with_body`, which are replaced by the given expressions.
- Only fields of the record type may be overridden; no extra or typo’d field names.

---

## 6. Matching on Records

### 6.1 Goals

- **Destructuring:** Bind some or all record fields to variables in a match arm.
- **Literal field patterns:** Match only when certain fields equal specific literal values (e.g. “match when `x` is 0 and bind `y`”), without reserving `:` for this (we keep `:` available for future use, e.g. type-annotated patterns on enums).
- **Single syntax** that supports both “bind field to variable” and “field must equal literal” in the same pattern.

### 6.2 Current Grammar and Its Limitation

From [grammar.md](grammar.md):

```
record_pattern  = type_name "{" [ field_pattern { "," field_pattern } ] "}"
field_pattern   = IDENT [ ":" pattern ]
```

So today a field pattern is either a bare `IDENT` or `IDENT ":" pattern`. That allows:

- `Point { x, y }` — bare identifiers; could be interpreted as “bind x and y”.
- `Point { x: 0, y: 0 }` — colon form for “x and y must be 0”.

Using `:` for the field–pattern association could conflict with future use of `:` for **type ascription** in patterns (e.g. when matching on enum types or other pattern features). So we want a different token for “field ↔ pattern” in record patterns.

### 6.3 How Other Languages Do It

| Language | Record/struct pattern syntax | Literals in fields |
|----------|------------------------------|---------------------|
| **F#**   | `{ fieldName = pattern }`    | Yes. RHS is a pattern: variable binds, literal matches. e.g. `{ hours = h; minutes = m; p = AM }`. |
| **Haskell** | `C{ field = pattern }`   | Yes. Same idea: `C{ x = 0, y = 0 }` or `C{ x = a, y = b }`. |
| **Scala** | Case classes: positional `Point(x, y)`. No named record pattern with literals in the spec; literals are positional. | Positional only. |
| **Rust** | `Struct { field: pattern, .. }` | Uses `:` between field and pattern; type ascription in patterns also uses `:`. |

**Conclusion:** F# and Haskell use **`=`** between field name and pattern. So in a record pattern we have `field = pattern`, where `pattern` can be a literal (match that value) or a variable (bind). This keeps `:` free for future use (e.g. type-annotated matches on enums) and reuses the same `=` that record construction already uses (`field = expr`), with the only difference that the right-hand side is a **pattern** in match and an **expression** in construction.

### 6.4 Record Field Pattern Syntax

Use **equals** in record patterns, with an optional shorthand when the bound variable has the same name as the field:

- **Grammar:**  
  `field_pattern = IDENT [ "=" pattern ]`  
  So the `:` form is dropped for record fields. If `= pattern` is present, the field is matched to that pattern; if omitted, it is shorthand for `IDENT = IDENT` (bind the field to a variable with the same name).

- **Semantics:**
  - **Bare identifier** — `x` alone means `x = x`: match the field and bind its value to that name. So `Point { x, y }` is equivalent to `Point { x = x, y = y }`.
  - `field = literal` — match only when the record’s field value equals that literal (and the literal’s type matches the field type).
  - `field = variable` — match and bind the field value to that variable (same or different name).
  - `field = _` — match and ignore the field.
  - `field = nested_pattern` — e.g. `field = Some(x)` when the field is an optional type; the nested pattern is matched against the field value.

This keeps literals and puns unambiguous: a bare `IDENT` always means “bind field to this name”; any literal or other pattern requires `= pattern`.

### 6.5 Examples

**Destructuring only (bind all fields):** When the variable name matches the field name, the shorthand is allowed:

```dovetail
let quadrant = match point with
    case Point { x, y } if x > 0 and y > 0 => "Q1"
    case Point { x, y } if x < 0 and y > 0 => "Q2"
    case Point { x, y } if x < 0 and y < 0 => "Q3"
    case Point { x, y } if x > 0 and y < 0 => "Q4"
    case _ => "Origin or on axis"
```

The form `Point { x = x, y = y }` remains valid and is equivalent.

**Literals in record fields (no colon):**

```dovetail
match point with
    case Point { x = 0, y = 0 } => "origin"
    case Point { x = 0, y = y } => "on Y axis at $y"
    case Point { x = x, y = 0 } => "on X axis at $x"
    case Point { x = x, y = y } => "other"
```

**Mix of literal and bind:**

```dovetail
record Config = level: Int32; name: String

match config with
    case Config { level = 0, name = n } => "off: $n"
    case Config { level = 1, name = n } => "low: $n"
    case Config { level = lvl, name = _ } => "level $lvl"
```

**Partial patterns (subset of fields):**  
We can either require all fields in a record pattern or allow a subset and treat “missing” fields as “don’t care” (match any value). The grammar can support subset matching by not requiring that every record field appear in the pattern; semantics would be “match the type and the listed field constraints; ignore unspecified fields.” Exact rules (exhaustiveness, optional “rest” syntax) can be decided in implementation.

### 6.6 Grammar Summary

The `field_pattern` rule is:

```
field_pattern = IDENT [ "=" pattern ]
```

So:

```
record_pattern = type_name "{" [ field_pattern { "," field_pattern } ] "}"
field_pattern   = IDENT [ "=" pattern ]
```

- **When `= pattern` is omitted:** the pattern is equivalent to `IDENT = IDENT` (bind the field to a variable with the same name). This matches the [control flow book](book/04-control-flow.md) style `case Point { x, y } =>` and is common in Haskell (record puns) and similar languages.
- **When `= pattern` is present:** the field is matched against that pattern (literal, variable, `_`, or nested pattern).

- In **record construction** and **with**: `field_init = IDENT "=" block_expr` (RHS expression; `=` required).
- In **record match**: `field_pattern = IDENT [ "=" pattern ]` (RHS optional; if omitted, bind field to same name).

Same `=` token; context (expression vs pattern) distinguishes the interpretation. No use of `:` for record field patterns, leaving `:` available for type-annotated patterns elsewhere.

---

## 7. Exhaustiveness and Records

Records have a single “constructor” (the record type itself). So for a scrutinee of record type `R`:

- A single arm `case R { ... }` with only variable/wildcard field patterns matches every value of type `R`.
- If we allow **partial** record patterns (only a subset of fields), exhaustiveness is unchanged: one record arm that matches any value of `R` is enough; additional arms with literal constraints refine when that arm is taken but don’t require more arms for exhaustiveness.
- Exhaustiveness for a match whose scrutinee is a record type is therefore straightforward: we only need to consider whether the set of arms covers all values; typically a catch-all record pattern or a final `_` arm suffices when we have literal refinements.

(Exact algorithm can mirror the match-expression design: record patterns extend the pattern language; exhaustiveness stays in the Rules phase.)

---

## 8. Codegen and WASM-GC

- **Representation:** Records compile to **WASM-GC record types** (aggregates with named fields). No mutable fields; all fields are immutable in the component.
- **Non-null when possible:** The codegen should use **non-nullable** WASM-GC record types wherever the type system guarantees the value is never null (e.g. a variable of record type that is never optional). Use nullable (ref null <type>) only when the Dovetail type is optional or otherwise can be absent. Keeping records non-null when possible improves runtime behavior and allows simpler generated code.
- **Creation:** Record construction becomes allocation/initialization of a GC record.
- **Field access:** Direct field load from the record.
- **With:** Allocate a new record, copy fields from the original, overwrite the fields specified in the `with` body.
- **Future:** The compiler may later optimize certain records (e.g. single field or small, scalar-like shapes) to scalars or registers; that is an optimization and does not change the source-language semantics above.

### 8.1 Type definition order and recursive groups

WASM-GC’s type section requires types to be defined in an order that respects references: a type can only refer to type indices that are already defined **or** that are in the same **recursion group**. Records that reference other records (or themselves) therefore need careful ordering and use of recursion groups.

**Dependency graph:** Build a directed graph of record types where an edge A → B means “record type A has a field whose type is (or contains) record type B”. Primitives, tuples, and non-record types do not create edges.

**Definition order (no cycles):** If the graph is acyclic, emit record types in **topological order** (dependencies first). That way every record type is defined before any type that references it. Each type can be emitted as its own recursion group (e.g. `rec (struct ...)`) or as part of a single group for the whole DAG; the spec allows either.

**Recursive groups (cycles):** If the graph has cycles—e.g. record A has a field of type B and B has a field of type A, or a record has a field of its own type—those types must be defined in a **single recursion group** so that they can refer to each other by index within the group. The codegen should:

1. Compute **strongly connected components** (SCCs) of the record-type dependency graph.
2. For each SCC of size ≥ 1 (a single type that references itself, or a set of types that reference each other), emit one **rec** group containing exactly those record types, in an arbitrary but fixed order within the group. Types in the group can reference other types in the same group by index.
3. Emit **non-recursive** record types (those not in any multi-node SCC and not self-referential) in topological order, either each in its own small recursion group or grouped for clarity. They may reference types defined earlier or (for ref types) types in a recursion group that was already emitted.

**Summary:** Use topological order for acyclic record types; use recursion groups for any record that (transitively) references itself or participates in a cycle. This satisfies WASM-GC’s validation rules and keeps definition order and recursion groups explicit in the design.

---

## 9. Implementation plan and phases

Implementation is split into phases so that each delivers a testable slice and builds on the previous one. The pipeline (lexer → layout → parser → typechecker → codegen) is extended for records at each stage.

| Phase | Scope | Parser | Typechecker | Codegen |
|-------|--------|--------|-------------|--------|
| **1** | Record definition, construction, field access | Record decl; record construct `Type { field = expr ... }`; postfix `.field` | Registry + types for records; typecheck construction and field access | WASM-GC record types; struct.new; struct.get |
| **2** | With expression | `expr with field = expr ...` | Typecheck `with` (LHS record type, fields valid) | New struct, copy + override fields |
| **3** | Record patterns in match | Update `field_pattern` to `IDENT [ "=" pattern ]`; record patterns in match arms | Pattern vs record type; bindings; exhaustiveness for record scrutinees | Match on record: extract fields, branch or bind |
| **4** | Codegen robustness | — | — | Type definition order (topological); recursion groups for cyclic records; non-null when possible |

**Phase 1 — Records: definition, construction, field access**

- **Parser:** Add/align `record_decl`, `record_construct_expr`, and field access (`.` IDENT) if not already present. Ensure layout handles record bodies and brace blocks.
- **Typechecker:** In Collect, register record types (name, field names and types). In Inference, typecheck record construction (all fields present, types match) and field access (receiver has record type, field exists). In Rules, no extra rules for plain records.
- **Codegen:** Emit a WASM-GC struct type per record (field types in order). Emit `struct.new` for construction and `struct.get` for field access. Type section: emit record types in dependency order (Phase 4 can refine to full topological + recursion groups).
- **Tests:** Integration tests: define a record, construct it, read fields; multiple records; record as function parameter/return.

**Phase 2 — With expression**

- **Parser:** `with_expr` and `with_body` (already in grammar); ensure layout/parsing is wired.
- **Typechecker:** Scrutinee must be a record type; overrides must be valid field names and expression types must match field types.
- **Codegen:** Allocate new struct, copy all fields from LHS, then overwrite with override expressions. Reuse same struct type as LHS.
- **Tests:** `r with x = e`; multiple overrides; chained `with`.

**Phase 3 — Record patterns in match**

- **Grammar:** Update `field_pattern` to `IDENT [ "=" pattern ]` in grammar.md and parser.
- **Parser:** Parse bare `IDENT` and `IDENT = pattern` in record patterns; record pattern as a whole.
- **Typechecker:** For `case R { ... } =>`, require scrutinee type to be (or unify with) record type R. Typecheck each field pattern against the field type; bind variables in guard and body. Exhaustiveness: record type has one “constructor”; catch-all or full record pattern covers it.
- **Codegen:** For record match arms: load fields (struct.get), match literals or bind locals; generate branches/joins as for existing match. Support guards that use bound field variables.
- **Tests:** Match with destructuring only; match with literals in fields; bare-field shorthand (`Point { x, y }`); mix of literal and bind; guards on record patterns.

**Phase 4 — Codegen: definition order and recursion groups**

- **Codegen:** Build record-type dependency graph from field types. Compute topological order for acyclic part; compute SCCs for cycles. Emit type section: non-recursive records in topological order (each in its own rec or grouped); each cyclic SCC in one rec group. Apply non-null rule: use non-nullable refs when the Dovetail type is not optional.
- **Tests:** Two records A, B with A referencing B (order); two records referencing each other (recursion group); self-referential record (recursion group).

**Dependencies:** Phase 1 is the base. Phase 2 depends on Phase 1. Phase 3 depends on Phase 1 (record types) and on the existing match implementation (literals, variables, guards). Phase 4 can be done after Phase 1 or in parallel with 2/3 if codegen is structured so that type emission is centralized.

---

## 10. Summary

| Topic | Design |
|-------|--------|
| **Definition** | `record Name =` with `IDENT ":" type` fields. Generic records (type_params) are out of scope. |
| **Creation** | `TypeName { field = expr; ... }`; all fields required; RHS is expression. |
| **Field access** | Dot notation: `value.field`. |
| **With** | `expr with field = expr; ...`; produces new record with overrides. |
| **Match** | `case TypeName { field [ = pattern ]; ... } => body` (bare `field` = bind to same name). |
| **Field pattern syntax** | `field = pattern` required for literals/nested/`_`; **bare `field`** allowed as shorthand for `field = field` (identifier pun). |
| **Rationale for `=`** | Matches F#/Haskell; keeps `:` for future use (e.g. type-annotated patterns on enums); consistent with record construction. |
| **Grammar** | `field_pattern = IDENT [ "=" pattern ]` (omit `= pattern` → bind field to same name). |
| **Immutability** | Records are immutable; no mutable record fields. |
| **Backend** | WASM-GC records; non-null when possible; definition order (topological) and recursion groups for cyclic record types; future optimizations may use scalars where valid. |

This design aligns records with the existing book and grammar, adds literal-friendly record matching via `field = pattern`, and leaves type-annotation syntax (`:`) available for future use in patterns. Generics (including generic records) are out of scope and will be designed separately.

## Private construction

Declare `record Name private = ...` to restrict construction to `module Name`
in the defining package. For generic types, place `private` after the type
parameters and before any `where` clause. Leading declaration visibility remains
independent: `public record Name private = ...` exposes the type.

Callers can inspect values and pattern-match normally. Field reads remain available, but
`with` updates require the associated module, just like construction. Empty
private records use `record Token private`.

Trait implementations, extensions, other modules in the same package, and
same-named modules in other packages have no construction privilege. Call module
functions instead. Generated code follows the same rules.
