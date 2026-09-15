# Closures and First-Class Functions

This document designs **closures** (lambda expressions), **function types** as first-class types, and **first-class use of named functions** (top-level, extension, impl block, module) in Dovetail. It aligns with [grammar](grammar.md), [layout rules](../layout_rules.md) (and the layout filter phase), [variance-any-design](variance-any-design.md) (boxing, Any), and the existing typechecker pipeline (Collect → Inference → Rules → Codegen).

**In scope:** Function type syntax and addition to the Type enum; closure syntax with `=>` as layout opener; capture semantics (box mutable, copy immutable; one environment per closure); bi-directional inference for closures; first-class use of named functions and methods (self captured; codegen uses trampoline functions so all function values share the same closure struct calling convention); capture/boxing pass before variance cast pass; no recursion in closure literals; call syntax unchanged.

**Out of scope:** Shared environment optimization (multiple closures sharing one environment); recursion in lambdas; optional/named parameters in function types or overloaded call.

**Implementation status:** Complete (all 8 phases). Phase 8 uses trampoline functions instead of the originally-designed closure enum — see §9.7.

---

## 1. Overview

- **Function type** is a first-class type: `(Type, Type) => Type` for multiple parameters, `Type => Type` for a single parameter. It is added to the **Type** enum so that values can have function type and be stored, passed, or returned.
- **Closure** is an expression that produces a value of function type and may **capture** variables from enclosing scopes. We **box** each captured **mutable** variable (one box per variable, one environment per closure). **Immutable** captured variables are **copied** into the closure’s environment. This simplifies the initial implementation; a future optimization may introduce a shared environment when multiple closures capture the same set of variables.
- **First-class named functions:** Top-level functions, extension methods, impl-block methods, and module functions can be used where a function type is expected. For functions that take **self**, the environment captures **self**. Codegen represents any function/closure value as a **closure enum**: either a **free function** (reference only) or **method with self** (captured self + method reference). Call site dispatches on the variant instead of generating a separate wrapper per method-as-value use; this is especially useful because methods will be used as values often.
- **Inference** for closures is **bi-directional**: we can infer closure parameter/return types from the expected function type (e.g. `map(x => x * 2)` where `map` expects `Int32 => Int32`), or infer the function type from an annotated closure (e.g. `let f = (x: Int32) => x * 2`).
- **Capture/boxing** is decided in a **separate pass after inference**, before the **variance cast pass**, so that the variance cast pass can cast to the boxed variable where needed. The pass only **marks** captured mutable variables as boxed; we create one environment per closure and do not yet optimize for shared environments.
- **Recursion** in closure literals is **not** supported; use a named function for recursion.
- **Call syntax** for values of function type is unchanged: `e(args)`.

---

## 2. Function Type

### 2.1 Syntax

- **Multiple parameters:** `(Type, Type) => Type`  
  Example: `(Int32, String) => Bool`
- **Single parameter:** `Type => Type`  
  Example: `Int32 => Int32`
- **Zero parameters (thunk):** Use a single parameter of type **Unit**: `Unit => T`. The value for that parameter is `()`. Lambda parameters **cannot be empty** in this design; for a thunk, use one parameter of type `Unit` (e.g. `(_: Unit) => expr` or `() => expr` if the grammar allows `()` as the single parameter for Unit).

When a closure has a **type annotation** on a parameter, **parentheses are mandatory** for that parameter list: `(x: Int32) => block`. Without annotations, single-parameter closures may be written `x => block`.

**Grammar** (to be added/updated in [grammar.md](grammar.md)):

- In the **type** grammar, add or update **function_type** to use `=>` and support single-param shorthand:
  - `function_type = "(" type { "," type } ")" "=>" type | type "=>" type`
  - So: `(A, B) => R` and `A => R`. Zero-arg is represented as `Unit => T`.

### 2.2 Type enum

Add a variant to the **Type** enum (e.g. in `dovetail/src/typechecker/types.rs`):

- **Function(param_types: Vec&lt;Type&gt;, return_type: Box&lt;Type&gt;)** — A function type. Parameter list must have at least one element; for thunks use `[Unit]` and the return type.

This makes function a first-class type: it can appear in variable types, generic arguments, type aliases, and anywhere else a type is allowed.

### 2.3 Variance

Function types are **contravariant in parameter types** and **covariant in return type**. This aligns with the existing assignability rules described in [variance-any-design](variance-any-design.md) (§2.7); no new syntax for variance on function types is required.

---

## 3. Closure Syntax

### 3.1 Forms

- **Single parameter (no type annotation):** `param => block_expr`  
  Example: `x => x * 2`
- **Single parameter (with type annotation):** `(param: Type) => block_expr`  
  Example: `(x: Int32) => x * 2`
- **Multiple parameters:** `(param_list) => block_expr`  
  Example: `(x, y) => x + y` or `(x: Int32, y: Int32) => x + y`

**Body:** The token **`=>`** is a **layout opener**. The closure body is the **block_expression** that follows under that layout (same as other layout blocks in Dovetail). No extra braces are required; indentation defines the block. See [grammar](grammar.md) and layout rules.

**Grammar** (to be added/updated in [grammar.md](grammar.md)):

- Replace or extend lambda so that the arrow is **`=>`** and the body is a layout block:
  - `closure_expr = lambda_params "=>" block_expr`
  - With **`=>`** listed as a **layout opener** (so the next line, if indented, starts a block).
- **lambda_params:** At least one parameter (no empty parameter list). For Unit thunk: one parameter of type `Unit`, e.g. `(_: Unit)` or `()` if the grammar allows it as a single-param form.
- **lambda_param:** `IDENT [ ":" type ]` — optional type annotation; when present, params must be in parentheses.

### 3.2 Layout

In the layout filter phase, **`=>`** is treated like `=`, `then`, `else`, `with`, `->`, `{`: it opens an indentation block. The closure body is the **block_expr** in that block (one or more expressions with layout). Update [grammar](grammar.md) layout openers table to include **`=>`** (closure arrow).

---

## 4. Capture and Environment

### 4.1 What is captured

A closure **captures** any variable from an **enclosing scope** (function or outer closure) that is **used** (read or written) in the closure body. Capture is allowed from **any** enclosing scope — including variables already captured by an outer closure. Because we **copy the box** (or the value for immutables) into each closure’s own environment, nested capture does not require special handling.

### 4.2 Box vs copy

- **Mutable** captured variables: **boxed**. One box per variable; the closure’s environment holds a reference to the box. Reads and writes in the closure go through the box so that the outer scope sees updates.
- **Immutable** captured variables: **copied** into the closure’s environment. No boxing.

We create **one environment per closure**; we do not currently merge or share environments across closures.

### 4.3 When we decide: capture/boxing pass

We run a **capture/boxing pass** after **inference** and before the **variance cast pass**. In this pass we:

- For each closure, determine the set of captured variables (from enclosing scopes).
- **Mark** each captured **mutable** variable as **boxed** (so that later phases and codegen can emit the right representation and the variance cast pass can cast to the boxed variable where needed).

A single pass is enough for the current design because we are not doing shared-environment optimization; we only need to mark mutable captures as boxed and create one environment per closure.

---

## 5. First-Class Named Functions and Methods

### 5.1 Where a function type is expected

Any of the following can be used where a **function type** is expected (argument to a function, assignment to a variable of function type, etc.):

- **Top-level function**
- **Extension method** (named extension function)
- **Impl-block method** (trait or class implementation)
- **Module function**

The typechecker must accept such a reference and treat its type as the corresponding function type (parameter types and return type), possibly with **self** handled as below.

### 5.2 Methods (functions with self)

For a **method** (function that has **self**), the **environment** of the resulting function value **captures self**. The **type** of the value is the function type without `self`: e.g. `(Int32, Int32) => Int32` for a method with `self` and two additional parameters returning `Int32`.

**Codegen — trampoline functions:** All function values (closures, FunctionRef, MethodRef) share a single **closure struct** representation: `(anyref env, ref funcref)`. A **trampoline function** adapts each named function or method to this calling convention:

- **FunctionRef trampoline** — ignores env (param 0), forwards params 1..N to the target function. The closure struct stores `(null, ref $trampoline)`.
- **MethodRef trampoline** — casts env (param 0) from `anyref` to the self type, then calls the target method with `(self, params 1..N)`. The closure struct stores `(boxed_self, ref $trampoline)`.

At **call site**, all function values use the same mechanism: extract env (field 0), push args, extract funcref (field 1), `call_ref`. No variant dispatch is needed. Trampolines are **deduplicated** per unique target mangled name, so each distinct function/method used as a value generates at most one small forwarding function.

> **Design note:** The original design specified a closure enum (`FreeFunction | MethodWithSelf`) with call-site dispatch. The trampoline approach was chosen instead because it (a) eliminates branching at every call site, (b) reuses the existing closure struct and `call_ref` mechanism unchanged, and (c) makes FunctionRef/MethodRef structurally identical to closures.

---

## 6. Inference

### 6.1 Bi-directional inference for closures

- **Expected type known (e.g. parameter type):** Infer closure parameter and return types from the expected function type.  
  Example: `function map(f: Int32 => Int32): ...` and call `map(x => x * 2)` — we infer `x: Int32` and return type `Int32` from `map`’s parameter type.
- **Expected type from closure:** Infer the variable’s or expression’s function type from the closure’s annotated types and body.  
  Example: `let f = (x: Int32) => x * 2` — we infer `f: Int32 => Int32`.

Standard bi-directional (bidir) type checking for lambda: push expected type into the closure and infer parameter/return from it when possible; otherwise infer from annotations and body.

### 6.2 Named functions and methods

When a named function or method is used where a function type is expected, we resolve its signature to the corresponding function type (params + return). For methods, the function type is (non-self params) => return type; self is not part of the type, it is captured in the wrapper at codegen.

---

## 7. Call Syntax

Calling a value of function type uses the **same** call syntax as today: **`e(args)`**. No special form (e.g. `e.(args)`) is introduced. Overloaded call or optional/named arguments are out of scope for this design.

---

## 8. Recursion and Thunks

- **No recursion in closure literals.** Recursive call must go through a **named** function. We do not introduce a way to name the closure inside its body (e.g. no `let rec f = x => ... f(x-1)` in this design).
- **Thunks:** For “no-argument” closures, use **one parameter of type Unit** and the unit value **`()`**. Type: `Unit => T`. Lambda parameter list is not empty; use `(_: Unit) => expr` or `() => expr` as the single-param form for Unit.

---

## 9. Pipeline Integration

### 9.1 Lexer / Layout

- **`=>`** is a distinct token (arrow for closure/function type).
- **`=>`** is a **layout opener**; the closure body is the block_expression in the following layout block.

### 9.2 Parser

- **TypeExpr:** Add (or extend) a **function type** form: `(Type, ...) => Type` and `Type => Type`, and parse it as part of `type` / `type_expr`.
- **Expr:** Parse closure: `lambda_params "=>" block_expr` with at least one parameter.

### 9.3 Collect

- Function types in signatures (parameters, return types, type aliases, etc.) are resolved like other types. No special collection for closures beyond what is needed for the enclosing function/block.

### 9.4 Inference

- **Function type** in **Type** enum is used when inferring variables, arguments, and return types.
- **Closure:** Infer type of the closure using bi-directional inference: when an expected function type is known, use it to infer parameter and return types of the closure; otherwise infer from annotated params and body.
- **Named function/method** used as a value: resolve to function type (params => return); for methods, function type is (non-self params) => return type.

### 9.5 Capture/boxing pass (new)

- **When:** After inference, **before** the variance cast pass.
- **What:** For each closure, compute captured variables from enclosing scopes. Mark each captured **mutable** variable as **boxed**. Immutable captures are copied (no mark). One environment per closure.

### 9.6 Rules

- Subtyping/assignability for **Type::Function** uses contravariance in parameters and covariance in return type. No change to the variance cast pass’s position; it runs after the capture/boxing pass so it can cast to the boxed variable when needed.

### 9.7 Codegen

- **Closure struct:** Every value of function type is represented at runtime as a WASM-GC struct with two fields: `(anyref env, ref $call_func)` where `$call_func = (func (param anyref <param_types>...) (result <ret_type>))`. Each unique function type signature gets its own pair of WASM types (call func type + closure struct type), deduplicated by parameter/return types.

- **Closures:** Emit a closure value with an **environment struct** (one per closure). For each captured variable: if mutable, environment holds a reference to the mut-box; if immutable, environment holds a copy of the value. The closure's body is **lifted** to a top-level WASM function with signature `(anyref, params...) -> result`; the prologue casts `anyref` to the env struct type and extracts captures into locals. Closures with no captures use `null` as env. Lifted functions are pre-scanned in **wave order** (wave 0 = closures in user code, wave 1 = closures nested in wave 0 bodies, etc.) to ensure they are emitted before any closures that reference them.

- **Closure call:** Extract env (field 0) and funcref (field 1) from the closure struct, push env as first arg, push user args, then `call_ref` with the call func type index.

- **FunctionRef (trampoline):** A named function used as a value is wrapped in a closure struct: `(null env, ref $trampoline)`. The trampoline is a generated WASM function that ignores param 0 (env) and forwards params 1..N to the target function. One trampoline per unique target mangled name (deduplicated).

- **MethodRef (trampoline):** A bound method reference `obj.method` is wrapped in a closure struct: `(boxed_self, ref $trampoline)`. The object is stored as the env field (boxed via `struct.new` if it is a primitive type). The trampoline casts param 0 from `anyref` back to the self type (`ref.cast` for reference types, unbox for primitives), then calls the target method with `(self, params 1..N)`. One trampoline per unique target method mangled name (deduplicated).

- **Boxing:** Mutable captures use a uniform box representation. We need boxing not only for primitives (as in [variance-any-design](variance-any-design.md) for Any) but also for **reference types**: **one box for all reference types** (`MUT_BOX_REF`), with `ref.cast` when **reading** the captured variable (writing does not need a cast, due to WASM-GC subtyping). MethodRef env uses the immutable boxing structs (`BOX_INT32`, etc.) for primitive self types.

---

## 10. Summary Table

| Topic | Design |
|-------|--------|
| **Function type syntax** | `(A, B) => R`, `A => R`; thunk `Unit => T`. Parentheses mandatory when param has type annotation in closure. |
| **Type enum** | Add `Function(Vec<Type>, Box<Type>)` (at least one param; use `[Unit]` for thunk). |
| **Closure syntax** | `x => body`, `(x: T) => body`, `(x, y) => body`. `=>` is layout opener; body is block_expr. |
| **Capture** | Any enclosing scope; mutable → box, immutable → copy. One environment per closure. |
| **Boxing pass** | After inference, before variance cast pass; mark captured mutable variables as boxed. |
| **Boxing (codegen)** | One box for primitives (per variance-any-design) and **one box for all reference types**; casting when reading (writing uses WASM-GC subtyping). |
| **First-class named** | Top-level, extension, impl, module functions usable where function type expected. |
| **Methods (self)** | Environment captures self; codegen uses trampoline functions so all function values share the same closure struct calling convention; one trampoline per unique target (deduplicated). |
| **Inference** | Bi-directional for closures; named functions/methods resolve to function type. |
| **Recursion** | Not supported in closure literals. |
| **Call** | Same as today: `e(args)`. |
| **Zero-arg** | No empty param list; use `Unit => T` and one param of type `Unit` (e.g. `(_: Unit) => expr`). |

---

## 11. Out of Scope (initial design)

- Shared environment optimization (multiple closures sharing one env).
- Recursion inside closure literals.
- Optional or named parameters in function types; overloaded call syntax.
- Empty closure parameter list (use `Unit` and one param).

---

## 12. Implementation plan (high level)

| Phase | Scope | Notes |
|-------|--------|--------|
| **1** | Function type in type system | Add `Type::Function(Vec<Type>, Box<Type>)`. Grammar: function type `(A, B) => R` and `A => R`. Parser: type_expr for function types. Collect/Inference: resolve function types; Rules: subtyping (contra in params, co in return). No closure syntax yet. |
| **2** | Closure syntax | Lexer: `=>` token. Layout: `=>` as layout opener. Grammar: closure_expr, lambda_params (≥1 param). Parser: closure AST. Typecheck can emit "not implemented" or minimal inference. |
| **3** | Closure inference | Bi-directional inference for closure literals; infer/check type `Type::Function`. Call site: typecheck `e(args)` when `e` has function type. |
| **4** | Capture/boxing pass | New pass after inference, before variance cast. Per closure: compute captured variables; mark captured mutable variables as boxed. One environment per closure. |
| **5** | First-class named functions | Use top-level, extension, impl, module function where a function type is expected. Resolve to `Type::Function(params, return)`. Typecheck only. |
| **6** | Methods as values | When method (with self) used as value: type is (non-self params) => return; capture self in environment. Typecheck only (codegen in phase 8). |
| **7** | Codegen: closures | Emit closure value with environment. Boxed mutable: one box for primitives, one box for all reference types; cast on read (WASM-GC subtyping for write). Copy immutable. Closure call loads env and runs body. Depends on boxing machinery (e.g. from variance-any-design where applicable). |
| **8** | Codegen: named functions and methods | Generate trampoline functions that adapt FunctionRef/MethodRef to the closure calling convention `(anyref, params...) -> result`. All function values (closures, FunctionRef, MethodRef) are structurally identical closure structs. One trampoline per unique target (deduplicated). |

**Dependencies:** 1 is base. 2 depends on 1. 3 depends on 1 and 2. 4 depends on 3. 5 depends on 1. 6 depends on 5. 7 depends on 4 (and boxing for Any/primitives if present). 8 depends on 5 and 6.

---

## 13. References

- [grammar.md](grammar.md) — Syntax and layout.
- [layout_rules.md](../layout_rules.md) — Layout filter and block structure.
- [variance-any-design.md](variance-any-design.md) — Boxing for primitives/Any; variance; variance cast pass.
- [compiler.md](compiler.md) — Pipeline stages.
