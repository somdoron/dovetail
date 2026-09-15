# Generic Variance, Any Type, and Type Test / Cast

This document designs **generic variance** (covariant `out` and contravariant `in`), the **Any** type as a real type, **type test** (`is`) and **cast** (`as`), and **type-annotated patterns** in match. It aligns with the [type system book](book/06-type-system.md), [grammar](grammar.md), [enums-design](enums-design.md), [generics-design](generics-design.md), and [match-expression-design](match-expression-design.md).

**In scope:** Definition-site variance on type parameters (`out T`, `in T`, no marker = invariant); applicability to generic enums and records (and future generic classes); prelude `Option` and `Result` covariant on all type parameters; bi-directional inference for uninferred variance (covariant → `Never`, contravariant → `Any`); `Any` as a type with assignability and codegen (boxing); `is` and `as` expressions; type-annotated patterns in match for type narrowing.

**Out of scope:** Higher-kinded types; variance on type aliases (alias expands, variance comes from the expanded type); detailed boxing representation (single heap layout vs multiple).

**Implementation status:** Variance, Any, is/as, and type-annotated match patterns are implemented. Variance position checking is implemented.

---

## 1. Overview

- **Variance** is declared at the definition site of a generic type: **`out T`** = covariant, **`in T`** = contravariant, **no marker** = invariant. It applies to generic **enums**, **records**, and (in the future) generic **classes**.
- **Covariant** type parameters preserve subtyping: if `A <: B` then `G<A> <: G<B>`. **Contravariant** flip it: if `A <: B` then `G<B> <: G<A>`. **Invariant** parameters do not allow subtyping in that position regardless of `A`/`B`.
- **Prelude:** `Option<out T>` and `Result<out T, out E>` are covariant in all type parameters. When a covariant type parameter cannot be inferred, the compiler uses **`Never`**; when a contravariant one cannot be inferred, it uses **`Any`**. Example: `let a = Ok(5)` infers type `Result<Int32, Never>`.
- **Any** is a real type. Any value can be assigned to `Any` (e.g. `let a: Any = "Hello"`). Codegen may require **boxing** primitives (and possibly other values) when stored in or passed as `Any`; boxing is also needed for closures. There is no `?`-style safe cast; **`as`** panics on failure.
- **Type test:** **`is`** — expression form `e is T` returns `Bool`. **Cast:** **`as`** — expression form `e as T`; panics at runtime if the value is not of type `T`.
- **Match:** **Type-annotated pattern** `case name: Type =>` narrows the scrutinee to `Type` in that arm when the scrutinee type is `Any` (or a supertype of `Type`). Combined with constructor and other patterns as today.

---

## 2. Generic Variance

### 2.1 Syntax

Variance is specified on **type parameters** at the definition site:

- **`out T`** — covariant in `T`.
- **`in T`** — contravariant in `T`.
- **`T`** (no prefix) — invariant in `T`.

**Grammar** (to be added or updated in [grammar.md](grammar.md)):

```
variant_type_param = [ "out" | "in" ] IDENT [ ":" type_bound { "+" type_bound } ]
```

So we have `out T`, `in T`, or plain `T`; trait bounds are unchanged. Examples:

```dovetail
enum Option<out T> = ...
enum Result<out T, out E> = ...
record Box<out A> = value: A
record Sink<in A> = dummy: Int32
record Cell<A> = get: () -> A; set: (A) -> Unit   // invariant A
```

### 2.2 Where variance is allowed

- **Generic enums** — Variance annotations allowed on type parameters. Prelude `Option<out T>` and `Result<out T, out E>`.
- **Generic records** — Variance annotations allowed. Future: **generic classes** will support variance on type parameters as well; the same rules apply.

Type aliases do **not** declare variance; they expand to the underlying type, whose variance (if it is an enum or record) applies.

### 2.3 Subtyping rules

Let `G` be a generic type with one type parameter for simplicity.

- **Covariant `out T`:** If `A <: B`, then `G<A> <: G<B>`.
- **Contravariant `in T`:** If `A <: B`, then `G<B> <: G<A>`.
- **Invariant `T`:** Neither direction is implied; `G<A>` and `G<B>` are in a subtyping relation only when `A` and `B` are equivalent for the purpose of that parameter (e.g. same type after normalization).

For multiple parameters, each parameter’s variance is applied independently when comparing `G<A1, A2, ...>` with `G<B1, B2, ...>`.

### 2.4 Never and variance

**Never** is already in the language (bottom type: no values; used for exhaustiveness and unreachable code). For variance:

- **Never** is a subtype of every type: `Never <: T` for all `T`. So it correctly fills **covariant** positions when a type cannot be inferred: the “smallest” type that is safe is `Never`.
- The typechecker must treat `Never` consistently in subtyping (assignability) so that, for example, `Result<Int32, Never>` is assignable to `Result<Int32, E>` for any `E` when `Result` is covariant in `E`.

### 2.5 Bi-directional inference and default type arguments

When the compiler infers type arguments for a generic type and a **covariant** type parameter has no constraint from the context, the inferred argument is **`Never`**. When a **contravariant** type parameter has no constraint, the inferred argument is **`Any`**.

**Examples:**

```dovetail
let a = Ok(5)           // Result<Int32, Never>   (E uninferred, covariant → Never)
let b = Some("x")       // Option<String>        (T inferred from argument)
let c: Result<Int32, String> = a   // ok: Result<Int32, Never> <: Result<Int32, String>
```

This allows idiomatic success-only use of `Result` without writing the error type, and keeps contravariant positions (e.g. input types) as “accept anything” when unknown.

### 2.6 Option and Result in prelude

Prelude (to be merged) will define:

- **`Option<out T>`** — covariant in `T`.
- **`Result<out T, out E>`** — covariant in both `T` and `E`.

So `Option<Dog> <: Option<Animal>` when `Dog <: Animal`, and similarly for `Result`. This matches common practice and enables the `Ok(5)` → `Result<Int32, Never>` inference.

### 2.7 Function types

Function types have the usual logical variance: **contravariant in parameter types**, **covariant in return type**. The language’s existing **assignability** (subtyping) rules already encode this: e.g. a function that accepts `Animal` and returns `Dog` is assignable to a type that accepts `Dog` and returns `Animal` in the opposite direction. This design does not add new rules for function types; it only states that they align with variance. No syntax for annotating variance on function type parameters is required.

---

## 3. Any Type

### 3.1 Any as a real type

**Any** is a built-in type. A value of any type can be assigned to a variable or passed where **Any** is expected:

```dovetail
let a: Any = "Hello"
let b: Any = 42
let c: Any = someRecord
```

Assignability: **`T <: Any`** for every type `T`. So `Any` is the top type for values. (Never is the bottom type; no value has type `Never`.)

### 3.2 Using values of type Any

To use a value of type `Any` as a concrete type, the program must:

- **Type test:** `e is T` — returns true iff the value at runtime has type `T`.
- **Cast:** `e as T` — treats the value as type `T`; **panics** if it is not. No `as?` or optional cast; use `is` plus `as` or a type-annotated match arm for safe narrowing.
- **Match with type pattern:** `match e with case s: String => ...` — see §5.

### 3.3 Codegen and boxing

- **Any** can hold any value. For reference types (records, enums, classes, arrays, strings), the representation may be a single reference. For small integer types (**Int8**, **Int16**, **UInt8**, **UInt16**), codegen can use WASM-GC **i31** so no boxing is required. For other **primitives** (e.g. `Int32`, `Bool`), the compiler may need to **box** them when assigning to `Any` (e.g. store on heap or in a wrapper) so that a single representation can hold both references and primitives.
- **Closures** also require a uniform representation (e.g. a closure is a value that may need to be stored in a structure or passed as `Any`). Boxing (or a common closure representation) is needed for closures as well; the same machinery can be shared where appropriate.
- The exact layout (single heap box for all primitives vs type-tagged union, etc.) is left to the implementation. The design only requires that (1) every type can be assigned to `Any`, and (2) `is` / `as` can be implemented (runtime type information or tag for primitives and refs).

---

## 4. Type Test and Cast

### 4.1 `is` expression

**Syntax:** `expression "is" type`

**Semantics:** Evaluates the expression, then checks at runtime whether the value has the given type. Result is **`Bool`**. Does not change the type of the expression in the type system; for narrowing, use a type-annotated pattern in `match` or an `if e is T ... e as T` pattern.

**Examples:**

```dovetail
if x is String then ...
let ok: Bool = value is Int32
```

### 4.2 `as` expression

**Syntax:** `expression "as" type`

**Semantics:** Evaluates the expression and **casts** the value to the given type. The expression must have a type that is assignable to `Any` (or the target type). At runtime, if the value is not of the target type, the operation **panics**. There is no optional or “safe” cast (`as?`); use `is` to check first or a type pattern in `match`.

**Examples:**

```dovetail
let s: String = (someAny as String)
if x is String then use((x as String))
```

### 4.3 Grammar

Add to [grammar.md](grammar.md) (in expression and precedence as appropriate):

```
postfix_expr = ... | "is" type | "as" type
```

Or as binary operators with appropriate precedence. Exact placement follows existing expression grammar.

---

## 5. Type-Annotated Pattern in Match

The grammar already has **type_annotated_pattern** = `IDENT ":" type_expr`. In a **match** expression, when the scrutinee type is **Any** (or a supertype of the pattern type), a **type-annotated pattern** `case name: Type =>` means:

- **Match:** At runtime, test whether the scrutinee value has type `Type`. If yes, bind the value to `name` with type `Type` in that arm’s scope (and in the guard, if any). If no, try the next arm.
- **Exhaustiveness:** A type-annotated arm does not by itself make the match exhaustive; it only covers values of that type. For scrutinee type `Any`, exhaustiveness would require either a catch-all arm (e.g. variable or wildcard) or a finite set of type arms that covers all possible runtime types (generally not possible for `Any`).

**Examples:**

```dovetail
function describe(a: Any): String =
    match a with
        case s: String => "string: " ++ s
        case n: Int32  => "int: " ++ n.toString()
        case _         => "other"
```

Here `s` and `n` are bound with types `String` and `Int32` in their respective arms. The wildcard arm catches any other type. No separate `as` is needed in the arm; the match performs the test and the binding.

When the scrutinee is **not** `Any` (or a supertype of the pattern type), a type-annotated pattern may still be used if the type system allows (e.g. scrutinee is a union or a supertype); the same runtime check and binding apply. When the scrutinee type is a subtype of the pattern type, the arm may be redundant (always matches) and the compiler can warn or simplify.

---

## 6. Pipeline Integration

### 6.1 Collect phase

- **Variance:** For each generic enum and record, collect the variance of each type parameter (`out`, `in`, or invariant). Store in the type’s descriptor for use in subtyping and inference. The Rules phase validates that type parameters are used in positions consistent with their declared variance (e.g. an `out` param must not appear in a contravariant position).
- **Any / Never:** Treated as built-in types; no new collection beyond what exists today for built-ins.

### 6.2 Inference and Rules

- **Subtyping:** When comparing applied generic types (e.g. `Option<A>` vs `Option<B>`), use the declared variance to decide subtyping: covariant parameter → `A <: B` implies `G<A> <: G<B>`; contravariant → flip; invariant → require equivalence.
- **Default type arguments:** When inferring type arguments for a generic type, for a covariant parameter with no constraint, use `Never`; for a contravariant parameter with no constraint, use `Any`.
- **`is` / `as`:** Typecheck `e is T` and `e as T`: the expression must be assignable to `Any` (or the relationship to `T` must be checkable at runtime). The type of `e is T` is `Bool`. The type of `e as T` is `T`.
- **Type-annotated pattern in match:** When the scrutinee has type `Any` (or a supertype of the pattern type), typecheck the pattern type as a valid type; the bound variable has that type in the arm. Ensure exhaustiveness rules account for type patterns (e.g. `Any` plus type arms plus catch-all).

### 6.3 Codegen

- **Variance:** No direct codegen for variance; it only affects typechecking and subtyping. Monomorphization and WASM emission are unchanged.
- **Any:** Values stored in or passed as `Any` use the chosen representation (e.g. boxed primitives, references). Runtime type information (or tags) must support `is` and `as`.
- **`is` / `as`:** Emit runtime type check (or tag check) for `is`; for `as`, check then use value or panic.
- **Match on type patterns:** Same as `is` + branch; bind the value in the arm without an extra cast.

---

## 7. Summary

| Topic | Design |
|-------|--------|
| **Variance syntax** | `out T` covariant, `in T` contravariant, no marker invariant. Definition-site only. |
| **Where** | Generic enums, records, future generic classes. Not type aliases (they expand). |
| **Prelude Option/Result** | `Option<out T>`, `Result<out T, out E>`. Covariant in all parameters. |
| **Uninferred covariant** | Default to `Never`. |
| **Uninferred contravariant** | Default to `Any`. |
| **Never** | Already in language; bottom type; satisfies variance (Never <: T for all T). |
| **Any** | Real type; T <: Any for all T. Boxing for primitives (and closures) as needed. |
| **Type test** | `e is T` → Bool. |
| **Cast** | `e as T` → T; panics on failure. No `as?`. |
| **Match** | Type-annotated pattern `case name: Type =>` for narrowing from Any (or supertype). |
| **Function types** | Assignability already gives contravariant in, covariant out; no new syntax. |

---

## 8. Out of scope (initial design)

- **Higher-kinded types** and variance at higher kinds.
- **Variance on type aliases** — aliases expand; variance is on the underlying enum/record/class.
- **Safe cast operator** (e.g. `as?` returning `Option`) — not desired; use `is` + `as` or match.
- **Reflection beyond `is` / `as`** — no general reflection API in this design.
- **Exact boxing layout** — implementation choice; design only requires assignability to Any and support for `is`/`as`.

---

## 9. Implementation plan (high level)

| Phase | Scope | Notes |
|-------|--------|--------|
| **1** | Variance syntax and subtyping | Grammar: `variant_type_param` with optional `out`/`in`. Collect variance; Rules: subtyping for applied generics using variance. Variance position checking validates type params are used in compatible positions. |
| **2** | Convert Option and Result to covariant | Update prelude: `Option<out T>`, `Result<out T, out E>`. Ensure existing code and tests still typecheck with variance. |
| **3** | Bi-directional inference | Inference: default covariant param to Never, contravariant to Any. Tests: `Ok(5)` → `Result<Int32, Never>`. |
| **4** | Any type and assignability | T <: Any for all T. Codegen: boxing for primitives (and any shared closure representation). |
| **5** | `is` and `as` | Parse, typecheck, codegen; `as` panics on failure. |
| **6** | Type-annotated pattern in match | Match on Any (or supertype) with `case x: Type =>`; runtime test and binding. Exhaustiveness with type patterns. |

Dependencies: 1 is base. 2 depends on 1. 3 depends on 2. 4 can start after 1; 5–6 depend on 4 for Any and runtime type information.
