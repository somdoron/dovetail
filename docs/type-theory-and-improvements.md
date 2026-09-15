# Type Theory and Compiler Improvements: A Practical Guide

This document explains **type-system theory** that applies to Dovetail, **how Dovetail currently implements it**, **where we have gaps**, and **how to improve**—written for engineers who haven’t studied type theory. The goal is to make the typechecker more correct, predictable, and easier to extend.

**Audience:** Compiler and language engineers working on Dovetail. No academic background required.

**References:** When we mention "the theory" or "standard approach," we mean ideas from *Types and Programming Languages* (Pierce, MIT Press) and the type-inference and trait/effect literature. You don’t need to read those to use this doc.

**Implementation status:** Reference document. Improvements and gaps are tracked in [Backlog.md](Backlog.md) (e.g. item 38).

---

## Table of contents

1. [Type inference and unification](#1-type-inference-and-unification)
2. [Subtyping and assignability](#2-subtyping-and-assignability)
3. [Type error reporting](#3-type-error-reporting)
4. [Trait coherence and elaboration](#4-trait-coherence-and-elaboration)
5. [Effect systems](#5-effect-systems)
6. [Module abstraction](#6-module-abstraction)
7. [Formal semantics and safety](#7-formal-semantics-and-safety)
8. [Summary: priorities and next steps](#8-summary-priorities-and-next-steps)

---

## 1. Type inference and unification

### 1.1 Theory in plain English

**What is type inference?**
The compiler figures out types for expressions when you don’t write them. For example, in `let x = 1 + 2` the compiler infers `x : Int32` (or whatever your integer type is).

**What is unification?**
When the compiler has to make two types "match," it often has to **solve for type parameters**. For example:

- Function is `id<T>(x: T): T`.
- Call is `id(42)`.
- So we need: parameter type `T` and argument type `Int32` to match. The **solution** is: `T = Int32`. That’s unification: we **unify** the parameter type with the argument type and get a **substitution** (a mapping from type parameters to concrete types).

**The occurs check.**
Sometimes a "solution" would create an **infinite type**. Example: unify `T` with `Array<T>`. If we say `T = Array<T>`, then `T = Array<Array<T>> = Array<Array<Array<T>>> = ...` forever. So the rule is: **when binding a type parameter to a type, that type must not contain the same parameter**. Checking that is the **occurs check**. If it fails, we reject the unification (and report an error) instead of creating an infinite type.

**Constraint-based inference (optional but useful).**
Another way to structure inference is in two steps:

1. **Collect constraints:** Walk the program and, for each expression, generate constraints like "this type must equal that type" or "this type must be a subtype of that type."
2. **Solve constraints:** Run a single algorithm (unification + subtyping) on the whole set.

Benefits: clear separation of "what we need" from "how we solve it," and one place to improve error messages (e.g. "constraint X failed").

### 1.2 How Dovetail does it today

- **Where:** Type argument inference at **generic call sites** and in **extension method resolution**.
- **Mechanism:** `TypeParamSubstitution` in `dovetail/src/typechecker/infer/type_param_substitution.rs`. It has a `unify(param_ty, arg_ty)` method that:
  - If `param_ty` is a type parameter, performs an **occurs check** (rejects bindings like `T = Array<T>` that would create infinite types, while allowing identity bindings `T = T`), then binds it to `arg_ty` (or checks it's already bound to the same type).
  - If both are generic records or arrays, unifies component types recursively.
  - Otherwise requires exact equality.
- **Occurs check:** `Type::contains_type_parameter_named(name)` recursively checks whether a type contains a given type parameter. The `unify()` method calls this before binding: if the argument type contains the type parameter being bound (and is not a direct identity binding), unification returns `false`.
- **Flow:** For a call like `id(42)`, the typechecker gets the generic function's param types (e.g. `[T]`) and the inferred argument types (e.g. `[Int32]`), then calls `unify` for each param/arg pair. The resulting substitution gives the type arguments.

### 1.3 Expected type: where we use it and where we should

The `expected_type` field on `Inference` is the context type that the *parent* provides for the expression we’re about to infer (e.g. “this expression should have type `Array<Int32>`”). It is the main mechanism for **bidirectional** typing: we can *set* it before recursing into a subexpression and *read* it during inference to guide type argument inference or to check/infer the expression’s type.

#### Current use: bidirectional typing across expression forms

**Where we set `expected_type` (so that it can flow down):**

1. **Let (value):** When the binding has a type annotation, we set `expected_type` to that type before inferring the value. So in `let x: Array<Int32> = Array.empty()`, the value `Array.empty()` is inferred with `expected_type = Array<Int32>`.
2. **Bare function call (arguments):** When we have a single overload or all overloads agree on a parameter type, we set `expected_type` for that argument before inferring it. So in `takeArray(Array.empty())`, the argument is inferred with `expected_type = Array<Int32>` if `takeArray` expects `Array<Int32>`.
3. **Block (last expression):** When a block has `expected_type`, it is propagated to the last expression (which determines the block's type). Non-last expressions get `None`.
4. **If (condition + branches):** `expected_type = Bool` is set on the condition. The parent's `expected_type` is propagated to both then and else branches, enabling `let xs: Array<Int32> = if b then [] else [1]`.
5. **While (condition + body):** `expected_type = Bool` on condition, `expected_type = Unit` on body.
6. **Match (arm bodies):** The parent's `expected_type` is propagated to each arm body. The subject is inferred with `expected_type = None`.
7. **Assignment (value):** `expected_type` is set to the target variable's type (local or global) before inferring the value. So `x = []` can infer the empty array from x's type.
8. **RecordCreate (field initializers):** For non-pre-inferred fields, `expected_type` is set to the field's declared type before inferring each value.
9. **RecordWith (field values):** `expected_type` is set to the field's type (substituted for generic records) before inferring each override value.

**Where we read `expected_type`:**

- **Empty array literal:** When inferring `[]`, if `expected_type` is `Array<E>`, the literal gets type `Array<E>` instead of erroring. So `let xs: Array<Int32> = []` works.
- **Generic extension method resolution** (`generic_functions.rs`). When resolving `receiver.method(args)` and the method is generic, we first unify param types with arg types. If not all type parameters get bound, we fall back to: unify the method's **return type** with `self.expected_type` and use the resulting substitution to get the missing type arguments.
- **Bare generic function resolution** (`generic_functions.rs`). When `infer_type_args` fails to bind all type params from argument types alone, we fall back to unifying param types with args *plus* the return type with `self.expected_type`. This allows `let x: Array<Int32> = identity(Array<Int32>.empty())`.
- **Instance generic extension resolution** (`generic_functions.rs`). After unifying the receiver type and method param types with arg types, if type params are still unbound, we unify the return type with `expected_type`.

#### Solving type parameters: current behavior in detail

Type argument inference happens in three different code paths. All three read `expected_type` as a fallback.

**1. Bare generic function call** — `f(args)` or `f<T>(args)` where `f` is a top-level generic function.

- **Entry:** `infer_bare_function_call` → `resolve_generic_function` → `infer_type_args` (`generic_functions.rs`).
- **How type args are inferred:**
  - If the user wrote explicit type args (e.g. `id<Int32>(42)`), we resolve them and use them.
  - Otherwise we unify parameter types with argument types: for each `(param_ty, arg_ty)` we call `substitution.unify(param_ty, arg_ty)`. If that binds all type parameters, we're done.
  - **Fallback:** If `infer_type_args` doesn't bind all type params, `resolve_generic_function` retries: it re-unifies params with args (gathering partial bindings), then also unifies the function's **return type** with `self.expected_type`. If that binds all type params, it uses the result. This allows `let x: Array<Int32> = identity(Array<Int32>.empty())`.

**2. Static generic extension call** — `Array.empty()`, `Array.fill(5, 42)`, or `Array<Int32>.empty()`.

- **Entry:** `infer_method_call` with receiver like `Array` or `Array<Int32>` → `resolve_generic_static_extension` (`generic_functions.rs`).
- **How type args are inferred (in order):**
  1. **Unify parameter types with argument types.** For each `(param_ty, arg_ty)` we call `substitution.unify(param_ty, arg_ty)`. For `Array.empty()` there are no arguments, so no type params get bound here.
  2. **If not all type params are bound**, we fall back in this order:
     - **Explicit receiver type args:** If the user wrote `Array<Int32>.empty()`, we use `[Int32]` as the type arguments.
     - **`expected_type`:** If we still have unbound type params, we read `self.expected_type`. We create a fresh substitution and unify the method's **return type** with `expected_type` (e.g. return type `Array<T>` with expected `Array<Int32>` → `T = Int32`). If that unification succeeds and binds all type params, we use those as the type arguments.
     - If neither applies, we skip this candidate (and if no candidate succeeds, we error).

**3. Instance generic extension call** — `receiver.method(args)` where the receiver is a *value* (e.g. `arr.clone()`, `arr.get(0)`).

- **Entry:** `infer_method_call` → infer receiver → `resolve_generic_extension_instance` (`generic_functions.rs`).
- **How type args are inferred:**
  1. Unify the extension's `for_type` (e.g. `Array<T>`) with the **receiver type** (e.g. `Array<Int32>`). This binds type params that appear in the receiver type.
  2. Unify the method's **value parameter types** (excluding `self`) with the **argument types**. This binds type params that appear in method parameters.
  3. **Fallback:** If type params are still unbound, unify the method's **return type** with `self.expected_type`. This allows instance methods whose type params appear only in the return type to be inferred from context.

**Summary:**

| Call shape | Type args from | Uses `expected_type`? |
|------------|----------------|------------------------|
| Bare generic `f(args)` | Explicit type args, param/arg unification, **return type with `expected_type`** (fallback) | **Yes** — fallback |
| Static extension `Array.foo(args)` / `Array<T>.foo(args)` | (1) Param/arg unification, (2) explicit receiver type args, (3) **return type with `expected_type`** | **Yes** — fallback (3) |
| Instance extension `receiver.foo(args)` | (1) Receiver type, (2) param/arg unification, (3) **return type with `expected_type`** (fallback) | **Yes** — fallback (3) |

#### Summary of expected type uses

| Use | Where | Status |
|-----|--------|--------|
| **Solve type parameters** (generic functions/extensions) | See subsection "Solving type parameters: current behavior in detail" above. | ✓ **Implemented.** All three call paths (bare generic, static extension, instance extension) use `expected_type` as a fallback for solving type parameters. |
| **Check a subexpression against a required type** | Condition of if/while, body of while | ✓ **Implemented.** Before inferring, we set `expected_type = Bool` (if/while condition) or `expected_type = Unit` (while body). After inferring, we still call `check_assignable` for consistent error messages. |
| **Infer type of an expression that has no type on its own** | Empty array `[]`, branches of if, arms of match | ✓ **Implemented.** Empty `[]` uses `expected_type` when it is `Array<E>`. If branches and match arm bodies receive the parent's `expected_type` so that `[]` or other context-dependent expressions can infer their type. |
| **Propagate expected type to the "result" subexpression** | Last expression of a block; value of assignment; field initializers; record-with field values | ✓ **Implemented.** Blocks propagate to last expression. Assignments set `expected_type` to the target's type. RecordCreate and RecordWith set `expected_type` to each field's declared type. |
| **Disambiguate or validate** | Binary op result (optional), overload resolution (optional) | **Optional (not yet implemented).** When the whole binary expression has an expected type, we could check the result against it (earlier error) or use it to choose between overloaded operators. |

### 1.4 Where we lack it

1. ~~**No occurs check.**~~ ✓ **Implemented.** The occurs check is in `unify()` in `type_param_substitution.rs`. Before binding a type parameter, it calls `arg_ty.contains_type_parameter_named(name)` and rejects bindings that would create infinite types (while allowing identity bindings `T = T`).

2. **No explicit constraint phase.**
   Constraints are "solved" on the fly during expression typing. There's no intermediate representation of "constraint set" that we could inspect or use for better error reporting.

3. ~~**Bidirectional typing is local.**~~ ✓ **Implemented.** `expected_type` is now propagated through all major expression forms: blocks (last expr), if (condition + branches), while (condition + body), match (arm bodies), assignments, record create/with (field values), and empty array literals. See the expression form table in §1.6 for the full list.

### 1.5 How to implement improvements

**Occurs check** ✓ **Implemented.**

- `Type::contains_type_parameter_named(name)` checks recursively for the named type parameter in `Array`, `GenericRecord`, `Substituted` variants.
- `TypeParamSubstitution::unify()` calls this before binding. Identity bindings (`T = T`) are allowed; nested containment (`T = Array<T>`) returns `false`.
- Unit tests in `type_param_substitution.rs` verify: infinite type rejected, identity allowed, concrete binding succeeds, consistent rebinding works.

**Constraint-based inference (optional, larger refactor)**

- Introduce types like `Constraint::Equal(Type, Type)` and `Constraint::Subtype(Type, Type)`, and a `ConstraintSet` that collects them during a first pass over expressions.
- In a second phase, solve the constraint set: for equalities use unification; for subtyping use your existing `types_assignable` (or a variant that works on type variables). If solving fails, you have a handle to the constraint that failed and can attach span/source info for better errors.
- This can be done incrementally: e.g. first collect constraints only for generic call sites, then expand.

**Bidirectional typing** ✓ **Implemented.**

- `expected_type` is now propagated through all major expression forms listed in §1.6.
- When adding new expression forms, decide up front whether that position should propagate `expected_type` and add it to the table in §1.6.

### 1.6 Expression forms: synthesis vs check vs bidirectional

For each expression form we classify how it gets its type:

- **Synthesis only:** The type is determined entirely from the expression (e.g. literal type, or from subexpressions). We do *not* use `expected_type` when inferring this expression.
- **Check only:** The expression is checked against an expected type (e.g. condition must be `Bool`). The expression’s type is fixed by the context; we don’t “infer” it in the sense of choosing from multiple possibilities.
- **Bidirectional:** We *use* `expected_type` when it is set to help infer the expression’s type (e.g. inferring type arguments for a generic call, or the element type of an empty array). If no expected type is present, we fall back to synthesis.

Implementation detail: `expected_type` is a field on `Inference`; we set it before recursing into a subexpression and restore it after. So “bidirectional” means “this code path sets `expected_type` before calling `infer_expr` on some child.”

| Expression form | Mode | Status |
|-----------------|------|--------|
| **UnitLiteral** `()` | Synthesis | ✓ Type is `Unit`; no context needed. |
| **BoolLiteral**, **StringLiteral**, **CharLiteral** | Synthesis | ✓ Type from literal; no context needed. |
| **Int8Literal** … **Float64Literal** | Synthesis | ✓ Type is the literal's type. |
| **BinaryOp** | Synthesis | ✓ Left/right inferred; result from op rules (or trait). Optional: use `expected_type` for overload disambiguation. |
| **UnaryOp** | Synthesis | ✓ Operand inferred; result from op rules. |
| **Block** | **Bidirectional (last expr)** | ✓ When the block has `expected_type`, it is propagated to the last expression. Non-last expressions get `None`. |
| **Panic** | Check (subexpr) | ✓ Message must be `String`; result type is `Never`. |
| **Assert** | Check (subexpr) | ✓ Condition must be `Bool`, message must be `String`; result type is `Unit`. |
| **Let** | **Bidirectional (value)** | ✓ When there is a type annotation, `expected_type` is set to that type before inferring the value. |
| **Identifier** | Synthesis | ✓ Type from scope (variable or global). |
| **Assignment** | **Bidirectional (value)** | ✓ `expected_type` is set to the target variable's type (local or global) before inferring the value. |
| **FunctionCall** | **Bidirectional (args + return)** | ✓ Args get expected type when overloads agree on param types. Generic functions use `expected_type` as return-type fallback for type arg inference. |
| **MethodCall** | **Bidirectional (whole call)** | ✓ When the call has `expected_type`, it is used for generic extension return-type inference. Instance extensions also unify method param types with arg types. |
| **FieldAccess** | Synthesis | ✓ Type from the field's type in the object's record/type. |
| **If** | **Bidirectional (condition, branches)** | ✓ `expected_type = Bool` on condition. Parent's `expected_type` propagated to both then and else branches. |
| **While** | **Bidirectional (condition, body)** | ✓ `expected_type = Bool` on condition, `expected_type = Unit` on body. |
| **Break** / **Continue** | Synthesis | ✓ Type is `Never`. |
| **Match** | **Bidirectional (arm bodies)** | ✓ Parent's `expected_type` propagated to each arm body. Subject inferred with `expected_type = None`. |
| **RecordCreate** | **Bidirectional (field inits)** | ✓ For non-pre-inferred fields, `expected_type` is set to the field's declared type before inferring each value. |
| **RecordWith** | **Bidirectional (field values)** | ✓ `expected_type` set to each field's type (substituted for generics) before inferring override values. |
| **ArrayLiteral** | **Bidirectional (empty)** / Synthesis (non-empty) | ✓ Empty `[]`: when `expected_type` is `Array<E>`, literal gets type `Array<E>`. Non-empty: element type inferred from elements. |
| **Index** | Synthesis + check | ✓ Object and index inferred; index must be `Int32`; result = element type. |
| **Intrinsic** | N/A | ✓ Only in extension bodies; error in normal inference. |

*Code reference:* Expression dispatch is in `dovetail/src/typechecker/infer/expressions.rs` (`infer_expr`); `expected_type` is set in the Let, If, While, Block, Assignment branches, in `record_expressions.rs` for RecordCreate/RecordWith field values, in `match_expression.rs` for arm bodies, in `function_expressions.rs` for bare call args, and in `generic_functions.rs` for all three generic resolution paths (bare, static extension, instance extension).

#### Code examples

**Let with annotation (bidirectional for value):**

```dovetail
let x: Array<Int32> = Array.empty()
```

The typechecker sets `expected_type = Array<Int32>` before inferring `Array.empty()`. When resolving the generic extension method `empty`, type arguments can’t be inferred from the (empty) argument list, so we unify the return type `Array<T>` with `Array<Int32>` and get `T = Int32`.

**Let without annotation (synthesis for value):**

```dovetail
let x = 1 + 2
```

No `expected_type` is set. The value is inferred as `Int32` from the binary op; `x` gets type `Int32`.

**Function call with expected argument type (bidirectional for args):**

```dovetail
function takeArray(a: Array<Int32>): Unit = ()
let _ = takeArray(Array.empty())
```

When inferring the argument `Array.empty()`, the typechecker looks at `takeArray`’s parameter type and sets `expected_type = Array<Int32>` for that argument. So `Array.empty()` is inferred as `Array<Int32>` even without a let annotation.

**Method call in a typed context (bidirectional for whole call):**

```dovetail
let xs: Array<Int32> = Array.empty()
```

Here the *whole* expression `Array.empty()` is the value of the let; `expected_type = Array<Int32>`. Method-call resolution sees that the call’s expected type is `Array<Int32>` and uses it to infer the type arguments for the generic `empty` method (return type `Array<T>` unifies with `Array<Int32>`).

**Empty array with expected type:**

```dovetail
let xs: Array<Int32> = []
```

The typechecker sets `expected_type = Array<Int32>` before inferring `[]`. The empty array literal reads `expected_type`, finds `Array<Int32>`, and types itself as `Array<Int32>`.

**If/else branches with expected type:**

```dovetail
let xs: Array<Int32> = if b then [] else [1]
```

The if expression receives `expected_type = Array<Int32>` from the let. It propagates this to both branches: `[]` uses the expected type to infer `Array<Int32>`, and `[1]` is synthesis-inferred as `Array<Int32>`.

**Match arms with expected type:**

```dovetail
let xs: Array<Int32> =
    match n with
        case 0 => []
        case _ => [42]
```

The match propagates `expected_type = Array<Int32>` to each arm body. The `[]` in the first arm uses it to infer `Array<Int32>`.

**Block with expected type:**

```dovetail
let xs: Array<Int32> =
    let y = 42
    []
```

The block propagates `expected_type = Array<Int32>` to its last expression `[]`, which uses it to infer `Array<Int32>`.

**Assignment with expected type:**

```dovetail
let mutable xs: Array<Int32> = [1, 2, 3]
xs = []
```

The assignment sets `expected_type` to the target's type (`Array<Int32>`) before inferring `[]`.

**Record field with expected type:**

```dovetail
record Holder =
    items: Array<Int32>

let h = Holder { items = [] }
```

Each field initializer gets `expected_type` set to the field's declared type, so `[]` is inferred as `Array<Int32>`.

**Remaining optional improvement:**

| Priority | Expression | Desired change |
|----------|------------|----------------|
| Optional | **BinaryOp** | When whole expr has expected type, check result or use for overload disambiguation. |

---

## 2. Subtyping and assignability

### 2.1 Theory in plain English

**Subtyping** means "can be used where." If type `A` is a subtype of type `B`, then any value of type `A` can be used where a value of type `B` is expected (e.g. as an argument, or in a variable). So we say "actual is assignable to expected" when the actual type is a subtype of (or equal to) the expected type.

**Structural vs nominal.**
- **Structural:** Two types are related if their *structure* matches (same fields, same element type for arrays). Dovetail’s records and arrays are structural.
- **Nominal:** Two types are related only if one is *declared* to be a subtype of the other (e.g. class inheritance). Dovetail will have classes; that’s where nominal subtyping will matter.

**Variance.**
For a type constructor like `Array<T>`, the question is: if `A` is a subtype of `B`, is `Array<A>` a subtype of `Array<B>`?
- **Covariant:** Yes. So "array of dogs" can be used where "array of animals" is expected.
- **Invariant:** No; we require `A = B`.
- **Contravariant:** We’d have `Array<B>` subtype of `Array<A>` (rare for containers).

Dovetail’s `types_assignable` treats `Array` and `GenericRecord` covariantly (element-wise / field-wise assignability). That’s the usual choice for read-only or immutable data.

### 2.2 How Dovetail does it today

- **Where:** `dovetail/src/typechecker/infer/types.rs`: function `types_assignable(expected, actual)` and the `is_assignable` / `check_assignable` methods on `Inference`.
- **Rules:**
  - Exact equality, or `actual.is_error()` / `expected.is_error()` / `actual.is_never()` → assignable.
  - Same `TypeParameter` name (for generic bodies) → assignable.
  - `GenericRecord`: same FQN, same arity, assignability for each type argument.
  - `Array`: assignability of element types.
  - Otherwise false.
- **Used for:** Overload resolution (e.g. `matches_args` with `is_assignable`), checking let bindings and return types, match subject vs pattern types.

### 2.3 Where we lack it

- **Classes/nominal subtyping:** When Dovetail adds classes and inheritance, you’ll need to extend `types_assignable` so that a class type is assignable to its superclass (and possibly to interfaces/traits). The design should state whether subtyping is structural only, nominal only, or both (structural for records/arrays, nominal for classes).
- **Variance is implicit:** The code doesn’t document "Array is covariant." As you add mutable data or type parameters in contravariant positions (e.g. function arguments), you’ll need to be explicit about variance to avoid soundness bugs.
- **Never:** You already treat `Never` as assignable to everything, which is correct (no value of type `Never` exists, so the rule is vacuously safe).

### 2.4 How to implement improvements

- **Document variance:** In this doc or in `types.rs`, add a short comment: "Array and GenericRecord are covariant in their type arguments; assignability is structural." When you add function types (e.g. `A -> B`), document that function types are typically contravariant in the argument and covariant in the return.
- **Nominal subtyping (when adding classes):** In `types_assignable`, add a case: if `expected` is a class (or trait object) and `actual` is a class, then allow if `actual` is a subclass of `expected` (or implements the trait). That will require a way to query the registry for "is this class a subtype of that class / does it implement this trait."
- **No immediate code change required** for current behavior; the main work is documentation and a clear plan for when classes land.

---

## 3. Type error reporting

### 3.1 Theory in plain English

When type checking fails, the compiler has to explain **why** in a way that helps the programmer fix the code. Academic work on "type error diagnosis" focuses on:

- **Localizing the error:** Which expression or constraint is the real source?
- **Explaining the mismatch:** "Expected X because of call at line N; found Y because of literal at line M."
- **Suggestions:** "Did you mean to pass an Int32 here?" or "Perhaps add a type annotation."

A **constraint-based** formulation helps: each constraint can carry a **span** or **reason** (e.g. "expected arg 2 of call at span S"). When solving fails, you report the failing constraint with that context.

### 3.2 How Dovetail does it today

- Errors are emitted in `check_assignable` and similar places: "type mismatch: expected 'X', found 'Y'." The span is usually the expression or sub-expression being checked.
- There’s no structured "constraint" object; failures are detected at the point where we compare or unify types, so the message is local to that comparison.

### 3.3 Where we lack it

- **Single blame:** We don’t distinguish "the call site expected this" vs "the definition required that." So we might say "expected Int32, found String" without saying "because the function parameter at line X is Int32."
- **No chain of reasons:** We don’t show a short chain like "required by return type of foo → required by call at line 5."
- **Generic failures:** When type argument inference fails (e.g. unify returns false), we might not give a clear message like "cannot unify T with Array<T> (infinite type)" or "ambiguous: multiple overloads match."

### 3.4 How to implement improvements

- **Richer diagnostics:** When calling `check_assignable` or when unification fails, pass optional "reason" or "expected because" context (e.g. "argument 2 of 'id' at line 3"). Extend the diagnostic type to include an optional "because" or "required by" field so the UI can show "Expected Int32, found String. Required by: argument 2 of call to 'id' at line 3."
- **Occurs check message:** When you add the occurs check, emit a dedicated message: "Type parameter T cannot be unified with a type containing T (would create an infinite type)."
- **Constraint-based refactor:** If you move to constraint collection + solving, attach a span and a short reason to each constraint. On failure, report the constraint’s reason and span; that automatically improves locality and explanation without inventing a whole new error engine.

---

## 4. Trait coherence and elaboration

### 4.1 Theory in plain English

**Traits** (or type classes) are contracts: "any type that implements this trait has these methods." The compiler must **resolve** which implementation to use at each call site (e.g. "this value has type T; T implements Display; so use the Display impl for T").

**Coherence** means: for any given (trait, type) pair, there is **at most one** valid implementation that the compiler will use. If two different crates could each provide an impl for the same (trait, type), the compiler wouldn’t know which to pick—and behavior could change depending on import order. So languages like Rust enforce **orphan rules**: you can only implement a trait for a type if either the trait or the type is defined in the current crate. That prevents two crates from defining conflicting impls for the same (trait, type).

**Elaboration** is the process of turning trait method calls into concrete code: either **dictionary passing** (the compiler passes a record of function pointers for each method) or **monomorphization** (the compiler generates a specialized version for each type that implements the trait). Dovetail’s design (monomorphization only for generics) is a form of elaboration; the key is to make the rules **explicit** so that "when does resolution succeed or fail?" and "what code do we generate?" are well-defined.

**Generic traits** add type parameters to the trait (e.g. `From<T>`) and possibly to the impl (e.g. `implement From<Int32> for String`). Then coherence must consider **all** (trait + type arguments, implementing type). The same idea applies: at most one applicable impl per (trait, type) in the sense of "after substituting type arguments."

### 4.2 How Dovetail does it today

- **Orphan rule:** [orphan checking](../dovetail/src/compiler/typechecker/rules/orphan_impl.rs)
  requires ownership of the trait or implementing type.
- **Coherence:** [coherence checking](../dovetail/src/compiler/typechecker/rules/coherence.rs)
  rejects overlapping implementation heads, including generic shapes. Bounds do
  not prove disjointness; disjoint concrete applications remain legal.
- **Resolution:** Module members, imported named extensions, and implementations
  have defined priority. Explicit qualification resolves same-priority ambiguity.
  Inherited parent contracts prefer a direct implementation, otherwise a unique
  applicable child provider.
- **Elaboration:** [trait inference](../dovetail/src/compiler/typechecker/infer/traits.rs)
  records the selected member and application. Monomorphization resolves concrete
  implementation calls; [coercion](../dovetail/src/compiler/coerce.rs) inserts
  interface wrappers/upcasts, and codegen emits direct or vtable calls.
- **Generic traits:** Implemented. Runtime contracts use explicit `interface`
  declarations, not arbitrary traits in value position. See the
  [trait/interface audit](trait-implementation-status.md).

### 4.3 Disposition of the earlier recommendations

The recommendations to define coherence, reject overlap, document elaboration,
and add selection regressions are complete. Evidence is linked in the
[audit](trait-implementation-status.md#implemented-requirements-and-evidence).
The proposed “more specific impl wins” interpretation is obsolete for overlapping
implementations of the same trait; coherence rejects them. A new dictionary or
evidence-passing IR is not a remaining requirement: the current typed-call,
monomorphization, and interface-coercion pipeline carries the needed information.

Unsupported historical proposals are explicitly retired in the audit. This
reference section does not create a second trait implementation backlog.

---

## 5. Effect systems

### 5.1 Theory in plain English

**Effects** are "what a piece of code can do" besides computing a value: throw an exception, perform I/O, be asynchronous, etc. An **effect system** tracks these in the type so that:

- Callers know they must handle failure (e.g. `Result`) or await (e.g. `async`).
- The compiler can enforce that pure code doesn’t do I/O or throw.

**Simple form:** Types like `Result<E, T>` or `Option<T>` already encode "this can fail" or "this might be absent." The type system doesn’t *force* the caller to handle the case (you can ignore the Result), but the *information* is there. **Railway-oriented programming** is a style of chaining operations that propagate failure via Result/Option; it doesn’t require new type theory.

**Richer form:** Some languages have an **effect row** or **effect marker** in the type, e.g. "this function can throw and is async." Then the type checker enforces that callers are in a context that handles those effects (e.g. inside a try block or inside an async function). That’s more than just "returns Result"; it’s a first-class notion of effect in the type.

**Why it matters for Dovetail:** You have Result/Option and async on the backlog. If you only add types and syntax without a clear "effect story," you may end up with ad-hoc rules (e.g. "async functions can only be called from async context") scattered in the typechecker. A small amount of effect theory helps: **decide what you want to track** (failure, async, maybe IO) and **where** (function types, blocks, modules), then implement that consistently.

### 5.2 How Dovetail does it today

- **Panic / assert:** Control flow that doesn’t return (e.g. `Never` or bottom type). No explicit "can throw" in the type.
- **Result / Option:** Not yet in the language (on backlog). When added, they’ll likely be sum types; the question will be whether the type system *requires* handling (e.g. pattern match on Result before using the value) or only *allows* it.
- **Async:** On backlog; no effect typing yet.

### 5.3 Where we lack it

- **No explicit effect taxonomy:** We haven’t defined "Dovetail tracks these effects: failure (Result), async, …" and "these are the rules (e.g. async only in async context)."
- **No effect in function types:** Function types are likely `A -> B`; we could later have `A -{throws}-> B` or "this function is async" as part of the type so that the typechecker can enforce calling conventions.
- **Purity:** We don’t distinguish "pure" vs "impure" functions in the type system; that’s optional but useful for optimization and reasoning.

### 5.4 How to implement improvements

- **Short-term (no new type theory):**
  - Add Result and Option as sum types and document: "Functions that can fail should return Result; callers are encouraged to match on the result."
  - When adding async, document: "Async functions return a future/promise; they may only be called from an async context (or we provide a way to block)."
  That’s a clear, implementable policy without formal effect rows.

- **Medium-term (lightweight effect discipline):**
  - Introduce a simple notion of "effectful" vs "pure" if you want: e.g. only certain built-ins or only functions marked `async` or `can_fail` are effectful; the typechecker could reject calling effectful code from a context that’s supposed to be pure (if you ever add such a context).
  - Optionally, add a type-level marker for async (e.g. `Async<T>` or a keyword on function type) so that "must be in async context" is one check in one place.

- **Long-term (if you want stronger guarantees):**
  Look at "effect handlers" or "algebraic effects" (e.g. Koka, Unison) for a principled way to track and handle multiple effects. That’s a bigger design; the doc can just mention it as a possible direction and reference.

- **Document the policy:** In a short "effects" section in the language book or design docs, write: "Dovetail models failure via Result and absence via Option. Async is modeled by [X]. The type system [does / does not] enforce that callers handle failure or async." That gives implementers and users a single source of truth.

---

## 6. Module abstraction

### 6.1 Theory in plain English

**Modules** group types, values, and functions under a name. **Abstraction** means hiding implementation details: a module can export a type without exposing its internal representation (e.g. the fact that it’s a record with three fields). That way, only the module’s own code can depend on that structure; everyone else uses the type through the module’s public functions. This is called **abstract types** or **sealed types** in the literature.

**Phase separation** is about *when* things are decided: compile time (types, generics, constants) vs runtime (values, I/O). Module boundaries are a natural place to make that clear (e.g. "this module’s public API is stable; we can compile clients against the interface without the implementation").

### 6.2 How Dovetail does it today

- **Modules:** Designed in `docs/modules-design.md`: standalone modules (namespace) and modules for a type (instance + static members). Import is module-level only; no abstract type syntax yet.
- **Visibility:** There is public/internal (and possibly other visibility); the registry and visibility rules control what is visible across packages.

### 6.3 Where we lack it

- **No abstract type export:** You can’t currently say "this type is exported but its definition is hidden." So clients could depend on the fact that e.g. `User` is a record with fields `id` and `name`, and if you change the representation, you break clients. Abstract types would let the module say "User is a type; the only way to construct or inspect it is through these functions."
- **No explicit "signature" or "interface" for a module:** The design describes what a module contains but not necessarily a separate "module signature" that hides implementation. That’s optional but useful for large codebases.

### 6.4 How to implement improvements

- **Abstract types (when you want them):** Add syntax for "abstract type" or "opaque type" in a module (e.g. `abstract type User` or `type User = private ...`). Rules: outside the module, you can only use `User` in type positions and through the module’s public functions; you cannot construct or pattern-match on it. Inside the module, it’s the real definition. The typechecker enforces that no external code sees the constructors or fields. This might require a notion of "module boundary" in the typechecker (e.g. when resolving a type, check if we’re inside the defining module).
- **Document the boundary:** In the modules design doc, add a subsection "Abstraction (future or current): abstract types, visibility of type representation." Even if you don’t implement abstract types immediately, stating the goal helps.
- **Phase separation:** When you have const evaluation or compile-time vs runtime distinctions, document that "module public API is a compile-time contract; implementation can change as long as the API is stable." That’s a design principle rather than a single code change.

---

## 7. Formal semantics and safety

### 7.1 Theory in plain English

**Formal semantics** means writing down the meaning of the language in a precise way (e.g. "this expression evaluates to that value under this environment"). **Type safety** usually means two theorems:

- **Progress:** A well-typed expression either is a value or can take a step (e.g. evaluate one more step).
- **Preservation:** If an expression has type T and takes a step, the resulting expression still has type T.

Together they say: "well-typed programs don’t get stuck" (no undefined behavior at the type level—e.g. no "integer used as function" at runtime). Real compilers have more going on (unchecked casts, FFI, etc.), but the **core** of the language can still be specified and argued to be safe.

**Why it matters for Dovetail:** A small formal core (e.g. a subset of expressions + functions + match + generics) gives you a **reference**: when in doubt, "what would the semantics say?" That helps avoid subtle bugs when extending the typechecker or codegen. You don’t have to prove theorems in Coq; even an informal but precise description (e.g. "evaluation is small-step; here are the rules for application, match, …") is valuable.

### 7.2 How Dovetail does it today

- **Specification:** The grammar and layout rules are documented (`grammar.md`, `layout_rules.md`); the compiler architecture is in `compiler.md`. There’s no separate "operational semantics" document that defines evaluation step-by-step.
- **Testing:** Integration tests run Dovetail code and assert on behavior; that’s a form of "the implementation matches our expectations" but not a formal spec.

### 7.3 Where we lack it

- **No formal evaluation rules:** We don’t have a written definition of "how does this expression reduce?" for a core subset. So "what is the correct behavior?" is defined only by the implementation and tests.
- **No explicit safety argument:** We don’t document "the typechecker guarantees X, and the codegen preserves X," so it’s harder to know what could go wrong when we change the compiler.

### 7.4 How to implement improvements

- **Small-step or big-step semantics for a core:** Pick a minimal subset (e.g. literals, variables, application, let, if, match, one form of generic call). Write a short doc (e.g. `docs/semantics-core.md`) that defines:
  - Syntax of the core (can reference grammar).
  - Evaluation relation: e.g. "e → e'" (e reduces to e') with rules like "application: (λx.e) v → e[x:=v]", "match: match C(v) with C(y) -> e → e[y:=v]", etc.
  - Typing relation: "Γ ⊢ e : T" with rules for the same constructs.
  You don’t need to prove progress/preservation; just having the rules makes the intended behavior precise and gives a target for "does the implementation match?"
- **Reference interpreter (optional):** Implement a tiny interpreter for that core that follows the semantics doc. Use it to cross-check the compiler on small examples. This is a significant effort; only do it if you want a strong guarantee.
- **Document guarantees:** In `compiler.md` or this doc, add a paragraph: "The typechecker ensures [e.g. no application of non-function, no match on wrong constructor]. The codegen assumes these invariants; it does not re-check them at runtime except where explicitly specified (e.g. bounds checks)." That sets expectations for maintainers.

---

## 8. Summary: priorities and next steps

| Area | Theory in one line | Dovetail today | Top improvement |
|------|--------------------|--------------|------------------|
| **Type inference** | Unification + occurs check + optional constraints | ✓ Unification with occurs check; bidirectional typing across all expression forms; `expected_type` fallback for all generic call paths | Optionally collect constraints for better errors |
| **Subtyping** | Assignability = subtype or equal; variance matters | Structural assignability; Never/Error; no classes yet | Document variance; add nominal subtyping when classes land |
| **Error reporting** | Explain why and where | Local "expected vs found" | Add reason/context to diagnostics; use constraint spans if you refactor |
| **Traits** | Coherence + elaboration | Implemented: orphan rule, no overlap, generic traits, explicit interface values | Earlier recommendations closed; see [audit](trait-implementation-status.md) |
| **Effects** | Track failure/async in types | Not yet | Define policy (Result/Option, async); document; optional effect markers later |
| **Modules** | Abstraction = hide representation | Modules as namespaces; visibility | Consider abstract types for encapsulation |
| **Formal semantics** | Precise eval + typing rules | Grammar + architecture; no formal core | Write a small semantics for a core subset; document guarantees |

**Suggested order for implementation:**

1. ~~**Occurs check** in unification.~~ ✓ **Done.**
2. ~~**Bidirectional typing** across expression forms.~~ ✓ **Done.**
3. **Document** variance and effect policy (no new features yet); the trait coherence recommendation is complete.
4. **Richer diagnostics** (reason/context) where you already emit errors.
5. **Constraint-based inference** only if you want to invest in better error reporting and a cleaner inference pipeline; otherwise defer.
6. **Abstract types and formal semantics** when the language and team are ready for that level of design and documentation.

This document should be updated as Dovetail implements these ideas and as new theory becomes relevant (e.g. when adding classes, trait objects, or more advanced effects).
