# Railway-Oriented Programming and Early Return

This document designs **railway-style** error and absence propagation in Dovetail: a dedicated mechanism for “unwrap or return” on `Result` and `Option` without using Awaitable. It introduces the **EarlyReturn** trait (with an associated type for the failure/return type), **try** (prefix) and **orReturn** (postfix) expressions, an internal **return expression** used only after desugaring, and a desugaring pass that rewrites to `unwrap` + match + return. It aligns with [async-await-design](async-await-design.md) (same philosophy: dedicated mechanism, desugar after typecheck), [grammar](../grammar.md), and the type system.

**In scope:** EarlyReturn trait with associated type; `try` and `orReturn` syntax; type rule that function return type must accept the expression’s OnFailure type; internal return expression (no surface syntax); desugaring to unwrap + match + return.

**Out of scope:** General-purpose `return` statement in the language; variance of Result/Option; full specification of Result/Option in prelude.

**Implementation status:** Implemented. `EarlyReturn` associated types, `try`,
and `orReturn` are covered by the trait regression suite. Through an abstract
bound, `try` needs enough information to resolve `OnFailure`; see the
[trait/interface audit](trait-implementation-status.md).

---

## 1. EarlyReturn Trait

### 1.1 Purpose

**EarlyReturn<T>** is the trait for types that carry either a success value (T) or a “failure” value that should be returned from the current function (railway propagation). Only types that represent success/failure or presence/absence implement EarlyReturn — e.g. `Result<T, E>` and `Option<T>`. This is **not** for suspendable types; those use [Awaitable](async-await-design.md) and `await`.

### 1.2 Trait Definition (Associated Type)

The trait has one type parameter **T** (the success value type) and one **associated type** that represents “the type we produce when we early-return” (the failure case). Each impl defines this associated type explicitly.

**Syntax:** Non-generic associated type in the trait; impl defines it (same style as [async-await-design](async-await-design.md) for non-GAT associated types).

```dovetail
trait EarlyReturn<T> =
  type OnFailure
  function unwrap(self): Result<T, OnFailure>
```

- **OnFailure** — Associated type: the type of the value used when we early-return (failure/absence). Defined per impl.
- **unwrap(self): Result<T, OnFailure>** — Returns **Ok(inner_value)** on success (so the expression yields T), or **Error(on_failure_value)** on failure (so the expression is rewritten to return that value from the function).

So **Ok(t)** means “continue with t”; **Error(r)** means “return r from the current function”. The enclosing function’s return type must be a supertype of **OnFailure** so that returning **r** is type-correct.

### 1.3 Example Implementations

**Result<T, E>:**

```dovetail
implement <T, E> EarlyReturn<T> for Result<T, E> =
  type OnFailure = Result<Never, E>
  function unwrap(self): Result<T, Result<Never, E>> =
    match self with
    case Ok(x) => Ok(x)
    case Error(e) => Error(Error(e))
```

So on **Error(e)** we produce **Error(Error(e))** — the “return” value is **Result<Never, E>** (i.e. the error case of the function’s return type). The function’s return type must be **Result<R, E>** for some R; **Result<Never, E>** is a subtype (covariant in the success type), so returning **Error(e)** is valid.

**Option<T>:**

```dovetail
implement <T> EarlyReturn<T> for Option<T> =
  type OnFailure = Option<Never>
  function unwrap(self): Result<T, Option<Never>> =
    match self with
    case Some(x) => Ok(x)
    case None => Error(None)
```

On **None** we produce **Error(None)** — the “return” value is **Option<Never>**. A function returning **Option<U>** can return **None** (Option<Never> is a subtype of Option<U>).

### 1.4 Relation to Associated Types in Traits

The syntax for declaring **OnFailure** in the trait and defining it in the impl matches the associated-type machinery described in [async-await-design](async-await-design.md). No type parameters on the associated type here; it is a plain associated type. If that design uses `type IDENT = type` in the trait for a default, we can either omit a default (every impl must define **OnFailure**) or leave the trait without `= type` and require the impl to define it.

---

## 2. Return Expression (Internal Only)

### 2.1 Purpose

Desugaring **try** / **orReturn** needs to express “return this value from the current function”. We do **not** add a **return** keyword or surface syntax for a return statement. We introduce an internal **return expression** only: an AST node that exists only after desugaring (or in a hypothetical internal IR). The lexer and parser do not accept **return**; the typechecker and codegen treat the internal node as “exit the current function with this value”.

### 2.2 Semantics

- **Return(expr)** — Evaluate **expr**; its value has the function’s return type **R**. Control leaves the current function and the function’s result is that value.
- Produced only by the desugar pass when rewriting **try** / **orReturn** (on the **Error** branch of the match on **unwrap**).
- Not representable in user source.

---

## 3. Syntax: try (Prefix) and orReturn (Postfix)

### 3.1 Grammar

- **Prefix:** **`try`** *expression*.
- **Postfix:** *expression* **`.orReturn`** (already present in [grammar](../grammar.md) as postfix_op).

**To add:** Reserve **try** in the lexer (if not already). Add a prefix expression form so that **try** binds to the following expression (same precedence level as other prefix forms, or as in the grammar for **await**).

**Grammar (prefix, to be added):**

```
try_expr = "try" expression
```

**Precedence:** **try** binds to the following expression (e.g. **try foo()**); for chained calls, parentheses may be needed (e.g. **try (foo().bar())**). **expr.orReturn** already has postfix precedence.

### 3.2 Equivalence

**try** *expr* and *expr* **.orReturn** are equivalent: same type rule and same desugaring. The choice is stylistic (prefix vs method-style chaining).

---

## 4. Type Rules

### 4.1 Where try / orReturn Is Allowed

- **try** *expr* and *expr* **.orReturn** are allowed only inside a **function body** (or closure body that has a return type that supports early return — same rule as for functions below).

### 4.2 Type of the Operand

- The type of *expr* (in **try** *expr* or *expr* **.orReturn**) must implement **EarlyReturn<T>** for some **T**.
- The type of the whole **try** *expr* / *expr* **.orReturn** expression is **T** (the success type).

### 4.3 Return-Type Compatibility

- Let **OnFailure** be the associated type of the **EarlyReturn<T>** impl for the type of *expr*.
- The **enclosing function’s return type R** must be a **supertype** of **OnFailure** (or equal). So we can return the failure value from the function (e.g. **Result<Never, E>** when **R = Result<U, E>**; **Option<Never>** when **R = Option<U>**).

So: **OnFailure <: R**. The typechecker enforces this so that the desugared **return** is always well-typed.

**Exception — async functions:** When the enclosing function is **async**, its return type is an Awaitable type (e.g. `Async<T, E>`), not a supertype of **OnFailure**. Then we do **not** use subtyping; instead we require the async return type **M** to implement **From<OnFailure>**. The desugar pass (see [async-await-design §5.5](async-await-design.md#55-interaction-with-early-return-try--orreturn)) then uses **From::from(r)** on the Error branch and feeds that into the async chain instead of emitting **Return(r)**. If **From<OnFailure>** for **M** is not implemented, the compiler reports an error.

---

## 5. Desugaring

### 5.1 Strategy

We do **not** lower to **andThen**/map/succeed (that would introduce many closures and is better suited to async). We lower to:

1. Evaluate the operand.
2. Call **unwrap** on it.
3. **Match** on the result: **Ok(x)** → use **x** as the value of the expression; **Error(r)** → emit the internal **return** expression with **r**.

So the desugar pass produces an AST that uses **match** and the internal **return** expression only.

### 5.2 Placement

- The desugar pass runs **after** typechecking (same as in [async-await-design](async-await-design.md)). The typechecker sees **try** / **orReturn** and enforces EarlyReturn and return-type compatibility; lowering produces an AST without **try**/orReturn, using **match** and the internal return node.
- Spans from the original **try** / **orReturn** are preserved for diagnostics.

### 5.3 What the Desugar Produces

For **try** *expr* (and similarly for *expr* **.orReturn**):

1. Let **tmp** be a fresh binding for *expr*.
2. Let **unwrapped** = **tmp.unwrap()** (type **Result<T, OnFailure>**).
3. Replace the **try** *expr* node with:
   - **match unwrapped with**
   - **case Ok(x) =>** *continuation using x as the value*
   - **case Error(r) =>** *Return(r)*

So the continuation is the rest of the expression/statement that uses the value of the **try** expression. The exact shape depends on how the AST is structured (e.g. a let binding **let y = try foo()** becomes: evaluate **foo()**, unwrap, match; in the Ok arm bind **y** and continue with the rest of the body; in the Error arm, **Return(r)**).

### 5.4 Span Preservation

When rewriting, the compiler preserves source spans from the original **try** / **orReturn** nodes so that diagnostics can point at the user’s code.

---

## 6. Implementation Phases

Suggested order; each phase is testable on its own.

| Phase | Scope | Deliverables |
|-------|--------|--------------|
| **1. Associated type OnFailure** | Trait declares `type OnFailure`; impl defines `type OnFailure = ...`. (May be covered by async-await Phase 1 if that lands first.) | Grammar for trait/impl; typechecker: resolve and project OnFailure. |
| **2. EarlyReturn trait and prelude impls** | Define EarlyReturn<T> (type OnFailure, unwrap). Implement for Result<T, E> and Option<T> in prelude. | Trait + impls; typechecker resolves unwrap return type. |
| **3. try and orReturn in parser** | Lexer: reserve **try**. Parser: **try** expr and **expr.orReturn** (orReturn already in grammar). | AST nodes for try and orReturn. |
| **4. Type rules for try / orReturn** | Typechecker: operand must implement EarlyReturn<T>; expression type T; enclosing function return type R must be supertype of OnFailure. | Full typecheck of try/orReturn. |
| **5. Internal return expression** | Add internal AST node for “return value from function”; no parser/lexer surface. Codegen (or later phase): implement semantics for this node. | Internal Return(expr) node; typecheck ensures expr type = R. |
| **6. Desugaring pass** | After typechecking, lower try/orReturn to match on unwrap + Return(r) in Error branch. Preserve spans. | AST without try/orReturn; codegen receives lowered AST. |

**Dependencies:** 2 depends on 1; 3 is independent (grammar); 4 depends on 2 and 3; 5 can be minimal (node + typecheck that return value type = R); 6 depends on 4 and 5.

---

## 7. Summary

| Topic | Rule |
|-------|------|
| **EarlyReturn<T>** | trait with **type OnFailure** and **function unwrap(self): Result<T, OnFailure>**; Ok = success (value T), Error = value to return from function (type OnFailure). |
| **Result<T, E>** | impl **EarlyReturn<T>** with **OnFailure = Result<Never, E>**; unwrap(Error(e)) → Error(Error(e)). |
| **Option<T>** | impl **EarlyReturn<T>** with **OnFailure = Option<Never>**; unwrap(None) → Error(None). |
| **try** / **orReturn** | Prefix **try** *expr* and postfix *expr* **.orReturn**; only inside function (or compatible closure); expr : EarlyReturn<T> ⇒ expression type T; function return type R must be supertype of expr’s OnFailure. |
| **Return expression** | Internal only; no surface syntax; produced by desugar when Error(r); type of r must be R. |
| **Desugar** | try/orReturn → evaluate expr, unwrap, match: Ok(x) ⇒ x, Error(r) ⇒ Return(r). Separate phase after typecheck; preserve spans. |

---

## 8. References

- [async-await-design.md](async-await-design.md) — Associated types, Awaitable, desugaring approach; Option/Result explicitly use early return / railway.
- [grammar.md](../grammar.md) — postfix_op already includes `.orReturn`; expression grammar.
- [traits-design.md](traits-design.md) — Traits and impl blocks.
