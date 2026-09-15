# Async/Await and Associated Types in Traits

This document designs **async/await** for suspendable types (e.g. async I/O, durable execution) and the **Awaitable** trait they implement. It also specifies **associated types** in traits, including **generic associated types (GATs)**, so that traits can express “this type but with a different value type” (e.g. `Rebind<U>`) without full higher-kinded types. The impl explicitly defines that type (syntax similar to type alias). It aligns with [traits-design](traits-design.md), [grammar](../grammar.md), and the type system.

**In scope:** Associated types in traits (declaration in trait, definition in impl); generic associated types (e.g. `Rebind<U>`); Awaitable<T> trait (succeed, map, andThen using Rebind<U>); async functions and async closures with explicit return type; prefix `await`; desugaring to andThen/map/succeed in a separate phase; error handling via early return / railway; same-Awaitable-type rule inside an async body.

**Out of scope:** How to run an async value (executor, main, runtime); concrete definition of `Async<T, E>` or stdlib runtime; variance of Awaitable/Async; requiring `Rebind<U>: Awaitable<U>` (not needed for concrete async/await use).

**Implementation status:** Async functions, closures, and `async do` expressions are implemented.

---

## 1. Associated Types in Traits

### 1.1 Motivation

In a generic trait we sometimes need “the same context but with a different value type”: e.g. `map` takes a value of type `T` and returns the same wrapper type with value type `U`. Without higher-kinded types (HKT), we cannot express “type constructor” as a parameter. The **Rust-style** approach is an **associated type**: the trait declares a type (possibly generic in `U`), and each **impl** defines it explicitly. So “this type but with value U” is not inferred from the impl header; the impl spells it out (e.g. `type Rebind<U> = Async<U, E>`). This works for both generic implementing types (e.g. `Async<T, E>`) and concrete types (e.g. `AsyncString`).

### 1.2 Declaration in the Trait

A trait may declare one or more **associated types**. An associated type may be **generic** (generic associated type, GAT): it has its own type parameters.

**Implemented syntax:** In the trait body, alongside methods, we allow:

- `type Name` — non-generic associated type; each implementation supplies its definition.
- `type Name<T>` — generic associated type; each implementation supplies its definition.

The original associated-type default proposal is obsolete. See the
[trait/interface audit](trait-implementation-status.md) for current restrictions.

For Awaitable we need a GAT: “this context with value type U”. Naming it **Rebind<U>** (or similar) is conventional.

**Example:**

```dovetail
trait Awaitable<T> =
  type Rebind<U>
  function succeed(x: T): Self
  function defer(body: ByName<Self>, trace: SourceLocation): Self
  function map<U>(self, f: T => U): Rebind<U>
  function andThen<U>(self, f: T => Rebind<U>): Rebind<U>
```

Here **Self** keeps its usual meaning: the implementing type (e.g. `Async<T, E>`). The trait does not define `Rebind<U>`; each impl will.

### 1.3 Definition in the Impl

In an **implement** block (including a block targeting a class), the implementer **defines** each associated type. The syntax is the same as type alias: `type` *IDENT* *type_params*? `=` *type*.

**Generic implementing type:**

```dovetail
implement <T, E> Awaitable<T> for Async<T, E> =
  type Rebind<U> = Async<U, E>
  function succeed(x: T): Async<T, E> = ...
  function map<U>(self, f: T => U): Async<U, E> = ...
  function andThen<U>(self, f: T => Async<U, E>): Async<U, E> = ...
```

**Concrete implementing type (e.g. AsyncString):**

```dovetail
implement Awaitable<String> for AsyncString =
  type Rebind<U> = Async<U, Never>
  function succeed(x: String): AsyncString = ...
  function map<U>(self, f: String => U): Async<U, Never> = ...
  function andThen<U>(self, f: String => Async<U, Never>): Async<U, Never> = ...
```

So the impl explicitly says what “this type with value U” is. No inference from the shape of the implementing type is required; the compiler uses the defined Rebind<U> when type-checking method signatures and calls.

### 1.4 Use in Types

Where the trait is in scope (e.g. when a type is known to implement `Awaitable<T>`), the type checker can **project** the associated type: for a value of type `Async<T, E>`, `map` returns that type’s `Rebind<U>`, which the impl defined as `Async<U, E>`. So in practice the compiler always has a concrete type for Rebind<U> at call sites. We do **not** require a bound like `Rebind<U>: Awaitable<U>` for the current async/await design; concrete types are enough.

### 1.5 Relation to Type Aliases

The syntax for defining an associated type in an impl is the same as Dovetail’s type alias: `type Rebind<U> = Async<U, E>`. So we reuse the existing type-alias form; only the context (inside a trait declaration vs inside an impl) differs.

---

## 2. Awaitable Trait

### 2.1 Purpose

**Awaitable<T>** is the trait for **suspendable** types: values that describe work that can be paused and resumed (e.g. async I/O, durable execution). It is **not** for Option/Result/List; those use other mechanisms (e.g. early return, railway). Only types that represent suspendable computations implement Awaitable.

### 2.2 Trait Definition

The trait declares the generic associated type **Rebind<U>** and uses it in the signatures of **map** and **andThen**. **Self** is the implementing type (e.g. `Async<T, E>`).

```dovetail
trait Awaitable<T> =
  type Rebind<U>
  function succeed(x: T): Self
  function map<U>(self, f: T => U): Rebind<U>
  function andThen<U>(self, f: T => Rebind<U>): Rebind<U>
```

- **succeed:** Puts a value into the context (constructor). Static: no receiver. Returns **Self**.
- **map:** Transforms the value inside the context; result type is **Rebind<U>** (defined per impl).
- **andThen:** Chains computations; `f` takes the inner value and returns **Rebind<U>**; result is **Rebind<U>** as well.

### 2.3 Example Implementations

**Generic type (Async<T, E>):**

```dovetail
implement <T, E> Awaitable<T> for Async<T, E> =
  type Rebind<U> = Async<U, E>
  function succeed(x: T): Async<T, E> = ...
  function map<U>(self, f: T => U): Async<U, E> = ...
  function andThen<U>(self, f: T => Async<U, E>): Async<U, E> = ...
```

**Concrete type (e.g. AsyncString):**

```dovetail
implement Awaitable<String> for AsyncString =
  type Rebind<U> = Async<U, Never>
  function succeed(x: String): AsyncString = ...
  function map<U>(self, f: String => U): Async<U, Never> = ...
  function andThen<U>(self, f: String => Async<U, Never>): Async<U, Never> = ...
```

Users may implement Awaitable for their own types (e.g. custom async runtimes, durable execution types), not only for the standard library’s `Async`.

---

## 3. Async Functions

### 3.1 Syntax

- A function may be marked **async** with the `async` keyword.
- The **return type must be explicit** and must be a type that implements **Awaitable** (e.g. `Async<String, Never>`). The compiler uses this to know which Awaitable “context” the function runs in.

**Grammar (existing in [grammar.md](../grammar.md), to be kept or updated):**

```
function_modifier = "async" | ...
function_decl     = ... [ function_modifier ] "function" IDENT ... ":" return_type ...
```

**Example:**

```dovetail
async function foo(): Async<String, Never> = Async.succeed("hello")
```

### 3.2 Same-Awaitable-Type Rule

Inside an **async function** that returns a type implementing Awaitable (e.g. `Async<T, E>`), every **await** expression must have a type that is the **same** Awaitable type (same implementing type). So you cannot await a `DurableWorkflow<U>` inside a function that returns `Async<...>`. This keeps the execution context unambiguous and avoids mixing runtimes.

### 3.3 Async Closures

Closures (lambdas) may also be async. Syntax and semantics are analogous: an async closure has an explicit return type that implements Awaitable, and its body may use `await`. Example (syntax to be fixed to match grammar): async closure with type `(X) => Async<Y, E>`.

### 3.4 Deferred expressions

`async do block_expr` is the sole async block expression syntax. `do` uses the
existing layout opener, so both inline and indented bodies work. The expression
constructs an Awaitable computation and its final expression supplies the
success value. The entire body is deferred; constructing it does not schedule
work. Each execution recreates body locals and follows normal closure capture
rules. Nested expressions establish independent await, early-return and loop
boundaries.

Expected type context selects the Awaitable implementation. Without context,
inference discovers this body's await operands, projects each implementation's
`Rebind<SuccessType>`, and combines compatible contexts using assignability and
variance. Nested computations do not contribute operands. No-await or ambiguous
expressions require context; the compiler never defaults to standard `Async`.

`Awaitable` now requires the static operation
`function defer(body: ByName<Self>, trace: SourceLocation): Self` (the current
trait syntax uses no `static` keyword). It must store the by-name value without
evaluating it, then execute the returned computation when driven, without
memoization. Existing custom implementations must add this operation.

Inference lowers the expression to a resolved `defer` call containing a by-name async
computation. Await lowering uses implementation signatures for Rebind and generated
method parameters, including custom wrappers with different parameter layouts.
The standard `Async` implementation uses `Async.thunk` followed by `andThen`; `Resource` delays
construction and acquisition while preserving finalizers. Existing async
function and closure evaluation behavior is unchanged.

---

## 4. Await Expression

### 4.1 Syntax

- **Prefix** form: **`await`** *expression*.
- **await** is a reserved keyword (lexer and grammar).

**Grammar (to be added):**

```
await_expr = "await" expression
```

Precedence: `await` binds to the following expression (e.g. `await foo()`); for chained calls, parentheses may be needed (e.g. `await (fetch(id).map(...))` if needed).

### 4.2 Type Rule

- **await** is allowed only inside an **async** function or async closure.
- The type of *expression* must implement **Awaitable<T>** for some `T`.
- The type of **await** *expression* is **T** (the inner value).
- If the Awaitable type can fail (e.g. `Async<T, E>` with `E ≠ Never`), **await** yields **T** and errors are handled by **early return** / railway (the async context propagates the error; the exact mechanism is part of the Awaitable impl / runtime, out of scope here).

### 4.3 Same-Awaitable-Type Rule (repeated)

The type of the awaited expression must be the **same** type as the return type of the enclosing async function or closure (same implementing type). So in `async function f(): Async<Int32, Never> = ...`, every awaited value must have type `Async<..., ...>`.

---

## 5. Desugaring (Lowering)

### 5.1 Separate Phase

The transformation from **async/await** syntax to **chained andThen/map/succeed** is a **separate** compiler phase. It is **not** interleaved with type inference. This keeps a single responsibility: one phase does rewriting, another does type assignment.

### 5.2 Placement

The desugar pass runs **after** typechecking. The typechecker sees async/await and enforces Awaitable and same-Awaitable-type rules on the original syntax; then lowering produces an AST without await for codegen. This way type errors and inference diagnostics refer to the user's async/await code (and the types they wrote), not the desugared andThen chain, so errors are clearer. Codegen and any phase that assumes “no await” only ever receives the output of this pass, so no phase should see Await/async nodes in an invalid place; defensive panics are only for pipeline misuse.

### 5.3 What the Desugar Produces

- An **async function** body is rewritten so that:
  - Each **await** *expr* becomes use of *expr* in an **andThen** (or equivalent) chain: the continuation after the await is passed as the function to andThen.
  - Final values are wrapped in **succeed** where needed.
- The result is an AST that uses only method calls (andThen, map, succeed) and lambdas; no Await or async-specific nodes remain in the lowered tree.

### 5.4 Span Preservation

When rewriting, the compiler preserves source spans from the original async/await nodes so that diagnostics (type errors, etc.) can point at the user’s code, not the desugared andThen chain.

### 5.5 Interaction with Early Return (try / orReturn)

When an **async** function body also uses **try** or **.orReturn** (see [railway-early-return-design](railway-early-return-design.md)), the function return type is an Awaitable type (e.g. `Async<T, E>`), not a Result or Option. The usual early-return rule says the function's return type must be a supertype of the operand's **OnFailure** so we can "return" that value — but in an async function we don't return a bare value; we produce an Awaitable. So we need special support.

**Rule:** Either

1. **Compilation error:** treat "try / orReturn inside async" as invalid, or  
2. **Allow when From exists:** allow it when the **async return type** (e.g. `Async<T, E>`) implements **From<OnFailure>** for the early-return operand's **OnFailure** type.

If we allow it, the **desugar** must recognize this situation:

- **Early-return desugar** normally rewrites **try** *expr* to: evaluate *expr*, call **unwrap**, **match**: **Ok(x)** → continue with **x**, **Error(r)** → **Return(r)**.
- In an **async** function, we cannot emit **Return(r)** because the function return type is **Async<T, E>** and **r** has type **OnFailure** (e.g. `Result<Never, E>`). Instead, on the **Error(r)** branch we produce a value of type **Async<T, E>** by calling **From::from(r)** (or the equivalent), and that value is fed into the async chain (e.g. as the "failure" branch of the andThen structure) so the rest of the async body is not executed.

**Example (From for Async):**

```dovetail
trait From<T> =
  function from(t: T): Self

implement <T, E> From<Result<Never, E>> for Async<T, E> =
  function from(r: Result<Never, E>): Async<T, E> =
    match r with
    case Error(e) => Async.fail(e)
    case Ok(x)    => ...  // unreachable (Never)
```

Or a single impl that converts any **Result<T, E>** to **Async<T, E>** (Ok → succeed, Error → fail); then **from(r)** for **r : Result<Never, E>** still works via the Error arm.

**Typechecker:** When the enclosing function is async and has return type **M** (e.g. `Async<T, E>`), and we typecheck **try** *expr* / *expr* **.orReturn** where *expr* has **EarlyReturn<T>** with **OnFailure = R**, require **M** to implement **From<R>** (or emit a clear error that "async + early return requires From<OnFailure> for <async type>"). The desugar pass then uses **from(r)** on the Error branch instead of **Return(r)**.

---

## 6. Implementation Phases

Suggested order for implementation; each phase is testable on its own.

| Phase | Scope | Deliverables |
|-------|--------|---------------|
| **1. Associated types (non-generic)** | Traits can declare `type Foo`; impls define `type Foo = ...`. Parser, collect, typecheck: resolve and check associated type definitions; project type in method signatures and at use sites. | Grammar for trait/impl type members; typechecker support; tests (e.g. a trait with one associated type, impl defines it). |
| **2. Generic associated types (GAT)** | Associated type may have type params: `type Rebind<U>`, impl defines `type Rebind<U> = Async<U, E>`. Typechecker: substitute and project GAT in signatures. | GAT in parser/collect/infer; tests for Rebind<U> style. |
| **3. Awaitable trait and prelude Async** | Define Awaitable<T> (type Rebind<U>, succeed, map, andThen). Add minimal Async<T, E> (or stub) in prelude; implement Awaitable<T> for Async<T, E>. | Trait + impl; typechecker resolves Awaitable and Rebind<U> at method calls; tests that call map/andThen/succeed. |
| **4. async keyword and async functions** | Parser: `async` function modifier; explicit return type required. Typechecker: return type must implement Awaitable (for some T). No await yet — bodies are plain expressions that produce the Awaitable (e.g. Async.succeed(...)). | Lexer/grammar for async; collect/infer for async functions; tests (async fn returning Async<T, Never>). |
| **5. await expression and same-type rule** | Lexer: reserve `await`. Parser: `await` expr. Typechecker: await only inside async fn/closure; expr type must implement Awaitable<T>; result type T; same-Awaitable-type rule (awaited type matches function return type). | Full typecheck of async/await; no desugaring yet (can still run typecheck-only tests). |
| **6. Desugaring pass** | After typechecking, lower async function bodies: await → andThen chain, final values → succeed. Input: typed AST with await; output: typed AST without await. Preserve spans. | New pass; codegen receives lowered AST; integration tests that compile and run (if runtime exists) or at least codegen. |
| **7. Async closures** | Grammar and typecheck for async lambdas (explicit return type implementing Awaitable; body may use await). Desugar async closures in same pass as async functions. | Async closure syntax; same-Awaitable-type and desugar for closures. |
| **8. Async + early return** | When async body contains **try** / **.orReturn**: typechecker requires async return type **M** to implement **From<OnFailure>** (error otherwise). Desugar: on **Error(r)** branch, emit **from(r)** and feed into async chain instead of **Return(r)**. Depends on both async desugar and early-return desugar (see [railway-early-return-design](railway-early-return-design.md)); order of passes or combined pass must handle this case. | Type rule for try/orReturn in async; desugar uses From::from(r) on failure branch; tests (async fn with try, From impl for Async). |

**Dependencies:** 2 depends on 1; 3 depends on 2; 4 depends on 3; 5 depends on 4; 6 depends on 5; 7 depends on 6 (or 5 if desugar for closures is deferred); 8 depends on 6 (or 7) and on the early-return desugar pass from [railway-early-return-design](railway-early-return-design.md). Prelude Async in phase 3 can be a minimal stub (succeed/map/andThen that build a representation); full runtime/executor is out of scope.

---

## 7. Summary

| Topic | Rule |
|-------|------|
| **Associated types** | Trait declares (e.g. `type Rebind<U>`); impl defines (`type Rebind<U> = Async<U, E>`). Syntax like type alias. |
| **GAT** | Associated type may have type params (Rebind<U>); impl gives concrete RHS. No inference from impl shape; impl is explicit. |
| **Self** | Unchanged: implementing type; may be bare in trait/impl. |
| **Awaitable<T>** | type Rebind<U>; succeed (static): Self; map, andThen: Rebind<U>; for suspendable types only. |
| **async function** | Explicit return type implementing Awaitable; body may use await. |
| **await** | Prefix; only in async function/closure; type of expr must be Awaitable<T>; result type T; same Awaitable type as enclosing return type. |
| **Errors** | Early return / railway; await yields T when E ≠ Never (propagation by context). |
| **Desugar** | Separate phase; rewrites async/await to andThen/map/succeed; preserve spans. |
| **Async + early return** | try/orReturn inside async: require async return type to implement **From<OnFailure>**; desugar uses **from(r)** on Error branch and feeds into async chain (see §5.5). |

---

## 8. References

- [traits-design.md](traits-design.md) — Generic traits, impl blocks, Self. Associated types (and GATs) extend this.
- [railway-early-return-design.md](railway-early-return-design.md) — EarlyReturn, try/orReturn; §5.5 references async + early return interaction.
- [grammar.md](../grammar.md) — function_modifier, postfix, expression grammar.
- [type-theory-and-improvements.md](type-theory-and-improvements.md) — Effects, async on backlog.
