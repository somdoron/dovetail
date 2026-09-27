# Tuples Design

This document designs **tuples** in Dovetail: anonymous positional types, construction, element access (0-based), destructuring, and their treatment in the inference layer and codegen. It aligns with the [type system book](../website/content/book/06-type-system.md), [grammar](grammar.md), and [compiler design](compiler.md).

---

## 1. Overview

- **Tuples** are **anonymous** types that group multiple values of possibly different types. They are **purely positional**: no named tuple fields; use records when you need named fields.
- **Construction:** Parentheses with comma-separated expressions: `(true, 1)`, `(10, 20.5, "x")`.
- **Element access:** 0-based numbered accessors: `t._0`, `t._1`, `t._2`, …
- **Destructuring:** `let (x, y) = t` binds the first and second elements to `x` and `y`; wildcard `_` allowed. **Match:** Tuple patterns (e.g. `case (0, _) =>`) are part of the initial feature.
- **Type syntax:** `(Int32, Boolean)`, `(Float64, String, Bool)` — parentheses and comma-separated types.
- **Inference layer:** Tuples are converted into **records with a mangled name**. Each distinct tuple type is instantiated as a record when first seen; the mangled name encodes the field types (e.g. `tuple$mangle_T1$mangle_T2$...`). The rest of the compiler sees only records.
- **Codegen:** Tuples are **completely transparent**. Codegen sees only the record type that the inference layer produced; no special tuple handling in codegen.

**Implementation status:** Not started.

---

## 2. Type Syntax and Semantics

A tuple type is written as a parenthesized, comma-separated list of types (two or more elements).

**Grammar** (positional only; named tuple fields are out of scope):

```
tuple_type = "(" type "," type { "," type } ")"
```

**Examples:**

```dovetail
(Int32, String)
(Bool, Int32, Float64)
(Array<Int32>, Option<String>)
```

- Tuples are **structural** by shape: two tuple types are the same if and only if they have the same number of elements and corresponding element types are the same. There is no nominal identity (no “name” for a tuple type in the surface language).
- **No empty or single-element tuples:** Tuples always have **two or more** elements. There is no 0-arity or 1-arity tuple. The expression `(x)` is a **parenthesized expression** of type `T`, not a tuple. (Unit, if present in the language, is a separate concept.)
- **No arity limit:** Tuples may have any number of elements (two or more); there is no maximum in the spec.

---

## 3. Construction

Tuple values are constructed with parentheses and comma-separated expressions.

**Grammar:**

```
tuple_expr = "(" expression "," expression { "," expression } ")"
```

**Examples:**

```dovetail
val t = (true, 1)
let pair = (1, "hello")
let triple = (10, 20.5, true)
```

- Each expression is evaluated; the resulting value is the corresponding element. Element types are inferred from the expressions (or required to match an expected tuple type).

---

## 4. Element Access

Tuple elements are accessed by **0-based** index using the `_0`, `_1`, `_2`, … accessors.

**Grammar:** Postfix field access with reserved names `_0`, `_1`, `_2`, … (or a production that allows these as special field names for tuple types).

**Examples:**

```dovetail
let t = (true, 42)
let a = t._0   // Bool, true
let b = t._1   // Int32, 42

let triple = (10, 20.5, "x")
let first  = triple._0   // 10
let second = triple._1   // 20.5
let third  = triple._2   // "x"
```

- Typechecker: the receiver must have a tuple type; the index is implied by the accessor (`_0` = first element, etc.). Out-of-range accessors (e.g. `t._3` on a pair) are a type error.

---

## 5. Destructuring

A tuple can be destructured in a `let` binding so that each element is bound to a variable or discarded. A **wildcard** `_` may be used for any element you are not interested in; that element is not bound to a name.

**Grammar** (from [grammar.md](grammar.md)):

```
tuple_pattern = "(" pattern "," pattern { "," pattern } ")"
```

**Examples:**

```dovetail
let (x, y) = t
// x and y are bound to the first and second elements of t

let (a, b, c) = triple

let (first, _) = pair      // bind first element only
let (_, second) = pair     // bind second element only
let (_, _, third) = triple // bind third element only
```

- The number of patterns must match the tuple’s arity. Each sub-pattern is matched against the corresponding element (variable patterns bind the value; **wildcard `_`** discards it; literals and other patterns as per the match design). Destructuring is a single-level match on the tuple shape; no nested tuple patterns required in the first iteration if not needed.

### 5.1 Equality

Tuples are **equatable** when all element types are equatable. `(a, b) == (c, d)` is supported and compares element-wise; the same rule as for records. Inequality (`!=`) follows accordingly.

### 5.2 Tuple patterns in match

**Tuple patterns in `match` are part of the initial tuple feature.** For example: `case (0, _) =>`, `case (x, y) =>`. Exhaustiveness for a tuple scrutinee is the product of per-component coverage (see [match-expression-design.md](match-expression-design.md)).

---

## 6. Inference Layer: Tuples as Records

The **inference layer** (typechecker) converts each distinct tuple type into a **record type** with a **mangled name**. This makes tuples anonymous in the surface language but concrete in the compiler’s internal representation.

- **Mangled name format:** A name that encodes the tuple shape, e.g. `tuple$mangle_T1$mangle_T2$...` where each `mangle_Ti` is a mangled representation of the i-th element type. The exact mangling scheme is implementation-defined (e.g. reuse existing MangledName / FQN mangling for types).
- **Instantiation:** When the typechecker **first sees** a tuple type (e.g. from an expression `(e1, e2)` or an annotation `(Int32, Bool)`), it creates the corresponding record type and registers it (e.g. in the registry) with the mangled name. All later uses of the same tuple type refer to that same record type.
- **Record shape:** The record has one field per tuple element. Field names are the accessor names: `_0`, `_1`, `_2`, … (or the same names used for element access). Field types are the tuple element types. Order is preserved.
- **Downstream:** After the inference phase, the rest of the compiler (rules phase, codegen) sees only this record type. Tuple construction is record construction; `t._0` is field access on that record; destructuring is pattern matching on that record (or lowering to field reads). No separate “tuple” node in the typed AST for codegen.

**Summary:** Tuples exist only in the surface syntax and in the inference layer. By the time we have a typed AST and registry for codegen, every tuple has been replaced by its record representation.

---

## 7. Codegen

**Tuples are completely transparent in codegen.**

- Codegen receives only record types and record operations. The record that represents `(Int32, Boolean)` is a two-field record with fields `_0` and `_1` of types `Int32` and `Boolean`.
- Construction: emit record allocation/initialization as for any record.
- Access: emit field load for `_0`, `_1`, etc., as for any record.
- Destructuring: lower to a sequence of field loads into locals (or equivalent); no special tuple representation in the binary.

No WASM-GC or ABI concept of “tuple”; only records.

---

## 8. Open Questions

None at this time.

---

## 9. Implementation plan and phases

Implementation is split into phases so that each delivers a testable slice. The pipeline (lexer → layout → parser → typechecker → codegen) is extended for tuples at each stage. Because tuples are lowered to records in the inference layer, codegen treats them as records throughout.

| Phase | Scope | Parser | Typechecker | Codegen |
|-------|--------|--------|-------------|--------|
| **1** | Tuple type, construction, element access | `tuple_type` (positional only); `tuple_expr`; postfix `._0`, `._1`, … | Inference: create/register record per tuple shape (mangled name); typecheck construction and `._i` access | Record struct.new / struct.get (tuple is already a record) |
| **2** | Destructuring (`let (x, y) = t`) | `tuple_pattern` in let bindings | RHS must be tuple type; arity match; typecheck sub-patterns; wildcard `_` | Lower to sequence of field loads into locals |
| **3** | Equality | — | Tuple type is equatable when all element types are equatable (same rule as records) | Record equality (tuple record participates like any record) |
| **4** | Tuple patterns in match | `tuple_pattern` in match arms | Scrutinee tuple type; pattern arity; sub-patterns; exhaustiveness (per-component product) | Match on record: extract fields, branch/bind |

**Phase 1 — Tuple type, construction, element access**

- **Grammar:** Update [grammar.md](grammar.md): remove `named_tuple_field` alternative from `tuple_type`; keep only `"(" type "," type { "," type } ")"`. Ensure `tuple_expr` and postfix field access for `_0`, `_1`, `_2`, … (or a rule that allows these identifiers for tuple types).
- **Parser:** Parse `tuple_type`, `tuple_expr` `(e1, e2, ...)`, and postfix `._0`, `._1`, etc. (reserved or contextual).
- **Typechecker:** When a tuple type is first seen (in an expression or annotation), create the corresponding record type with mangled name `tuple$mangle_T1$mangle_T2$...` and register it. Typecheck tuple construction: element types match expected tuple type (or infer tuple type from elements). Typecheck `value._i`: receiver must have a tuple type; `i` must be in range (0 to arity − 1).
- **Codegen:** Tuples are already records at codegen; emit `struct.new` for construction and `struct.get` for `_0`, `_1`, … using the registered record type.
- **Tests:** Tuple type `(Int32, Bool)`; construct `(1, true)`; access `t._0`, `t._1`; tuple as function parameter/return; multiple distinct tuple types.

**Phase 2 — Destructuring**

- **Parser:** `let` with `tuple_pattern` on the left: `let (x, y) = t`, `let (a, _, c) = triple`. Grammar already has `tuple_pattern`; ensure it is wired for let bindings.
- **Typechecker:** RHS must have a tuple type. Number of sub-patterns must equal tuple arity. Typecheck each sub-pattern against the corresponding element type; wildcard `_` discards. Bind variables for guard/body.
- **Codegen:** Lower destructuring to a sequence of field loads (struct.get) into the bound locals.
- **Tests:** `let (x, y) = pair`; `let (_, second) = pair`; `let (a, _, c) = triple`; destructuring in nested scopes.

**Phase 3 — Equality**

- **Typechecker:** Ensure the record type that represents a tuple is considered equatable when all its field (element) types are equatable—same rule as for named records. No parser change.
- **Codegen:** Use the same equality lowering as for records (tuple is a record).
- **Tests:** `(a, b) == (c, d)` when types match; `(a, b) != (c, d)`; tuples with equatable and non-equatable elements (expect error for latter).

**Phase 4 — Tuple patterns in match**

- **Parser:** Match arms with `tuple_pattern`: `case (0, _) =>`, `case (x, y) =>`. Grammar already has `tuple_pattern`; ensure it is wired for match.
- **Typechecker:** Scrutinee type must be (or unify with) a tuple type. Pattern arity must match. Typecheck each sub-pattern against the corresponding element type. Exhaustiveness: product of per-component coverage (see [match-expression-design.md](match-expression-design.md)).
- **Codegen:** For each tuple arm: load fields from the scrutinee record, then match literals or bind locals; generate branches/joins as for existing match. Support guards that use bound variables.
- **Tests:** `match t with case (0, _) => ... case (x, y) => ...`; exhaustiveness (e.g. pair of bools); guards on tuple patterns.

**Dependencies:** Phase 1 is the base. Phase 2 and 3 depend on Phase 1. Phase 4 depends on Phase 1 and on the existing match implementation (literals, variables, guards). Phase 3 can be done in parallel with Phase 2 if record equality already exists.

---

## 10. Grammar and book alignment

- **Remove named tuples from grammar:** [grammar.md](grammar.md) currently has `tuple_type` with an alternative for `named_tuple_field`. This design drops named tuples; the grammar should be updated so that `tuple_type` is only the positional form `"(" type "," type { "," type } ")"`.
- **Book:** The [type system book](../website/content/book/06-type-system.md) has been updated to describe only positional tuples and 0-based access (`_0`, `_1`); named tuples have been removed.

---

## 11. Summary

| Topic | Design |
|-------|--------|
| **Nature** | Anonymous, purely positional types; no named tuple fields. |
| **Type** | `(T1, T2, ...)` — two or more elements; no arity limit. |
| **Construction** | `(e1, e2, ...)`. |
| **Access** | 0-based: `t._0`, `t._1`, … |
| **Destructuring** | `let (x, y) = t`; wildcard `_` allowed. **Equality:** `(a, b) == (c, d)` when element types are equatable (same rule as records). **Match:** Tuple patterns (e.g. `case (0, _) =>`) are part of the initial tuple feature. |
| **Inference** | Each tuple type → record with mangled name `tuple$mangle_T1$...`; instantiated when first seen; fields `_0`, `_1`, …. |
| **Codegen** | Transparent: only the record is emitted; no tuple-specific codegen. |
