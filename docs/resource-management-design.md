# Resource Management and the Usable Trait

This document designs **scoped resource management** for Dovetail: the **Usable** trait for types that provide bracketed acquire-use-release semantics, the **`use`** prefix expression for scoped resource binding, desugaring to continuation-passing via the trait's `use` method, and the **Resource** library type for async resource lifecycle. It aligns with [async-await-design](async-await-design.md) (same desugaring philosophy), [async-runtime-design](async-runtime-design.md) (bracket primitive), [railway-early-return-design](railway-early-return-design.md) (error handling), [crypto-library-design](crypto-library-design.md) (sync use case: secrets), and [grammar](../grammar.md).

**In scope:** `Usable<T>` trait with GAT `Wrapped<U>`; `use` prefix expression (like `await` and `try`); desugaring to continuation-passing; `Resource<T, E>` async library type; `Async.bracket` runtime primitive; sync use cases (e.g. `Secret<T>`); error type compatibility via `From` and `mapError`; scopes that acquire nothing (`Async.scope`) and attaching a resource to an already-open scope (`Resource.attachToScope`).

**Out of scope:** Higher-level resource combinators (resource pools, retries); `Closeable` / `Disposable` simple-cleanup trait (may be added later as a simpler interface for types that only need `close()`); durable execution resource lifecycle.

**Implementation status:** Implemented. This revision adds §11 (`Async.scope` / `Resource.attachToScope`) and removes `useForever`.

**Generic resource helpers:** An abstract `Usable` bound preserves the resource's
wrapper as `R.Wrapped<U, E2>`. A generic `use` continuation must return that
associated result; the helper cannot assume a particular container. See the
[complete example](../website/content/book/26-advanced-generics.md#generic-resource-helpers).

**Prerequisites:** [async-await-design](async-await-design.md) (prefix expressions, desugaring infrastructure), [async-runtime-design](async-runtime-design.md) (Async runtime, Cause error model), [traits-design](traits-design.md) (associated types and GATs).

---

## 1. Overview

Resource management in Dovetail follows the **bracket pattern**: a resource is acquired, passed to a scoped continuation, and released when the continuation completes — regardless of success or failure. This is the pattern used by ZIO's `acquireRelease`, cats-effect's `Resource.make`, and Python's `with` / context managers.

Dovetail provides:

1. **`Usable<T>`** — A trait for types that know how to acquire a resource of type `T`, pass it to a continuation, and release it. The trait uses a GAT (`Wrapped<U>`) so the same trait works for async resources (`Wrapped<U> = Async<U, E>`), sync resources (`Wrapped<U> = U`), or any other context.

2. **`use` expression** — A prefix expression (like `await` and `try`) that desugars into a call to the `Usable.use` method, capturing the rest of the current block as the continuation. Writing `let socket = use TcpSocket.connect(host, port)` is syntactic sugar for `TcpSocket.connect(host, port).use(socket => <rest>)`.

3. **`Resource<T, E>`** — A library type (record) in the async runtime package that wraps an acquire function and a release function. It implements `Usable<T>` with `Wrapped<U> = Async<U, E>`, using the `Async.bracket` primitive for guaranteed finalization.

The `use` expression is **not** limited to async functions. It works in any context where the `Usable` trait is implemented. Sync use cases include cryptographic secrets (zeroize after use) and scoped locks.

---

## 2. Usable Trait

### 2.1 Trait Definition

```dovetail
public trait Usable<T, E> =
    type Wrapped<U, E2>
    function use<U, E2>(self, f: (T) => Wrapped<U, E2>, errorF: (E) => E2): Wrapped<U, E2>
```

- **`T`** — The type of the resource that the continuation receives (e.g. `Socket`, `File`, `Key`).
- **`E`** — The impl's *source* error type (the type that the resource's acquire/release can produce). Sync impls use `E = Never`.
- **`Wrapped<U, E2>`** — A generic associated type (GAT) parameterized by the continuation's result type `U` and the *target* error type `E2`. For async resources: `Async<U, E2>`. For sync resources: `U` (identity, `E2` is phantom).
- **`use`** — Acquires the resource, passes it to `f`, applies `errorF` to any source-error to convert it to the target error type, and guarantees release. Returns the wrapped result.

The `errorF` parameter is what implements *implicit `From` conversion* at `use` sites. The compiler synthesizes it at every `use expr`:

- `E == E2` → identity closure `(e) => e`.
- `E == Never` → a panic closure (it can never be invoked because Never has no values).
- `From<E> for E2` impl exists → `(e) => E2.from(e)`.
- Otherwise → compile error at the `use` site.

This collapses what would otherwise be a separate `ErrorMappable` trait (or hardcoded `Resource.mapError` logic) into the `Usable` trait itself. Every impl participates; sync impls treat `errorF` as unused.

### 2.2 GAT Design

`Wrapped` has two parameters:

- **`U`** — continuation's result type.
- **`E2`** — target error type (chosen by the call site's context).

| Trait | GAT | Impl for `Resource<T, E>` |
|-------|-----|---------------------------|
| `Awaitable<T>` | `Rebind<U>` | `Rebind<U> = Async<U, E>` (E fixed) |
| `Usable<T, E>` | `Wrapped<U, E2>` | `Wrapped<U, E2> = Async<U, E2>` (E2 chosen by use site) |

This is the only 2-param GAT in the codebase. The trait collector handles arbitrary-arity GATs uniformly.

### 2.3 Example Implementations

**Async resource (Resource<T, E>):**

```dovetail
implement <T, E> Usable<T, E> for Resource<T, E> =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self, f: (T) => Async<U, E2>, errorF: (E) => E2): Async<U, E2> =
        let trace = SourceLocation { file = ""; line = 0; column = 0; functionName = "" }
        let converted: Async<T, E2> = self.acquire().mapError(errorF, trace)
        Async.bracket(converted, self.release, f)
```

**Sync secret (Secret<T>):** sync impls use `Usable<T, Never>` and ignore `errorF`:

```dovetail
implement <T> Usable<T, Never> for Secret<T> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self, f: (T) => U, _errorF: (Never) => E2): U =
        let value = self.unseal()
        let result = f(value)
        zeroize(value)
        result
```

Here `Wrapped<U, E2> = U` (the identity — `E2` is phantom). The `use` method unseals the secret, calls the continuation, zeroizes the key material, and returns the result. Since Dovetail has no exceptions (errors use `Result`), `f(value)` always returns normally, so `zeroize` always runs.

**Note on panics:** If `f` panics, `zeroize` will not run. Panics in Dovetail are unrecoverable program errors (like Rust's panics), so leaking key material during a panic is acceptable — the process is terminating.

---

## 3. `use` Expression

### 3.1 Syntax

`use` is a **prefix expression**, syntactically parallel to `await` and `try`:

```dovetail
let socket = use TcpSocket.connect("localhost", 8080)
let data = await socket.read()
data
```

The type of `use expr` is `T` where `expr` has a type implementing `Usable<T>`. The enclosing block's result type must be compatible with `Wrapped<U>` for some `U`.

### 3.2 Precedence

`use` is a **unary prefix operator** at the same precedence level as `try`, `await`, `!`, `-`, `~`:

| Precedence | Operators | Associativity |
|------------|-----------|---------------|
| 11 (highest unary) | `!` `-` `~` `try` `await` `use` | Right (prefix) |
| 12 | postfix (call, field, method, `orReturn`) | Left |
| 13 | primary (literals, identifiers) | — |

This means `use Socket.create.mapError(f)` parses as `use (Socket.create.mapError(f))` — postfix operations bind tighter, so the entire chain resolves before `use` is applied.

### 3.3 Grammar Change

Update the grammar from the current binding-level `use_expr` to a prefix unary operator:

**Remove** from binding/control level:

```
/* OLD — remove */
use_expr            = "use" IDENT "=" expression
```

**Add** `"use"` to the unary prefix operators:

```
/* NEW */
unary_expr          = ( "!" | "-" | "~" | "try" | "await" | "use" ) unary_expr
                    | postfix_expr
```

(`await` is also added here — it was designed in [async-await-design](async-await-design.md) but not yet reflected in the grammar.)

With this change, `use` is used in `let` bindings like any other expression:

```dovetail
let x = use expr          // use in let binding
let y = (use a) + (use b) // use in sub-expressions (LIFO resource order)
```

### 3.4 Type Rules

1. **`use` is allowed in any function** (async or sync) — it is not restricted to async functions. The `Wrapped<U>` GAT determines the context.

2. The operand of `use` must have a type that implements `Usable<T>` for some `T`. The type of the `use` expression is `T`.

3. The enclosing block's result type must be compatible with `Wrapped<U>`. The typechecker infers `U` from the continuation's result type and verifies that `Wrapped<U>` matches. When inside an async function returning `Async<R, E>`, the `Wrapped<U>` must be `Async<U, E>` (same as the function's Awaitable context).

4. **Same-Wrapped-type rule:** Within a single block, all `use` expressions must have the same `Wrapped` type constructor. This parallels the same-Awaitable-type rule for `await` — you cannot mix `Usable` implementations with different `Wrapped` types in one function.

5. **Interaction with async:** When `use` and `await` coexist in the same async function, the `Wrapped<U>` of the `Usable` impl must match the function's Awaitable return type. For example, in a function returning `Async<T, E>`, any `use` expression must have `Wrapped<U> = Async<U, E>`. This is enforced by type inference (the continuation's return type unifies with both `Wrapped<U>` and the Awaitable chain's type).

### 3.5 Interaction with `await` and `try`

All three prefix operators (`try`, `await`, `use`) create continuation boundaries during desugaring. They can interleave freely:

```dovetail
async function processFile(path: String): Async<String, AppError> =
    let file = use File.open(path).mapError(e => AppError.io(e))
    let content = await file.readAll()
    let parsed = try parseJson(content)
    parsed.toString()
```

Desugaring order (each captures the rest of the block as a continuation):

1. `use` → `File.open(path).mapError(...).use(file => ...)`
2. `await` → `file.readAll().andThen(content => ...)`
3. `try` → `match parseJson(content).unwrap() ...`

---

## 4. Desugaring

### 4.1 Basic Transform

The `use` expression desugars by capturing the rest of the current block (from the `use` point onward) as a closure passed to the `Usable.use` method.

**Source:**

```dovetail
let x = use expr
<rest>
```

**Desugared:**

```dovetail
expr.use(x => <rest>)
```

Where `<rest>` is all the remaining statements and the final expression in the current block.

### 4.2 Multiple `use` Expressions

Multiple `use` expressions nest, producing LIFO (last-in, first-out) resource release order:

**Source:**

```dovetail
let db = use Database.connect(url)
let cache = use Cache.open(config)
doWork(db, cache)
```

**Desugared:**

```dovetail
Database.connect(url).use(db =>
    Cache.open(config).use(cache =>
        doWork(db, cache)))
```

`cache` is released first (inner continuation completes), then `db` (outer continuation completes). This is the correct order for dependent resources.

### 4.3 Interleaving with `await`

`use` and `await` interleave naturally in continuation chains:

**Source:**

```dovetail
let socket = use TcpSocket.connect("localhost", 8080)
let data = await socket.read()
let file = use File.open("output.txt")
await file.write(data)
```

**Desugared:**

```dovetail
TcpSocket.connect("localhost", 8080).use(socket =>
    socket.read().andThen(data =>
        File.open("output.txt").use(file =>
            file.write(data))))
```

The key insight: `use` and `await` are both continuation-creating operations. A `use` calls `.use(continuation)`, an `await` calls `.andThen(continuation)` or `.map(continuation)`. They compose into a single nested chain.

### 4.4 Desugaring Pass Placement

The `desugar_use` pass runs as a **separate pass before `desugar_await`** in the typechecker pipeline:

```
rules → desugar_for → desugar_try → desugar_use → desugar_await → coerce_byname → capture
```

**Why a separate pass:** `use` is not tied to async — it works in sync functions too. Handling it in a separate pass keeps responsibilities clean.

**Why before `desugar_await`:** When `use` creates a continuation closure inside an async function, the closure may contain `await` nodes. The `desugar_await` pass handles these if the closure body is wrapped in `AsyncBlock` (see §4.5).

### 4.5 Async Closure Wrapping

When `desugar_use` creates a continuation closure inside an **async function** (one whose body is an `AsyncBlock`), it wraps the closure body in an `AsyncBlock` node (copying the `succeed_method` from the enclosing function). This marks the closure as an async closure so that `desugar_await` will find and process any `await` nodes inside it via `walk_expr_for_async_closures`.

In **non-async functions**, the continuation closure is a plain closure — no `AsyncBlock` wrapping. The `desugar_await` pass ignores it.

This ensures clean separation: `desugar_use` does not need to understand `await`, and `desugar_await` does not need to understand `use`.

### 4.6 Expression-Level `use`

Like `await`, `use` can appear inside expressions, not just in `let` bindings:

```dovetail
let total = (use resourceA) + (use resourceB)
```

The desugaring extracts the first `use` node depth-first (same algorithm as `extract_first_await`), replaces it with a `VarRef` to a fresh temporary, and captures the rest as the continuation:

```dovetail
resourceA.use($use_0 =>
    resourceB.use($use_1 =>
        $use_0 + $use_1))
```

Resource A is acquired first, then B. B is released first (inner), then A (outer). LIFO order.

### 4.7 Tail `use`

If `use expr` is the **last expression** in a block (no continuation), the desugaring passes an identity-like continuation. For `Wrapped<U> = Async<U, E>`, this would be `expr.use(x => Async.succeed(x))`. For `Wrapped<U> = U`, this would be `expr.use(x => x)`.

In practice, a bare `use` at the tail of a block is unusual — the resource would be acquired and immediately released with no work done. The compiler may emit a warning for this pattern.

---

## 5. Resource Type (Async)

### 5.1 Definition

`Resource<T, E>` is a record in the async runtime package that describes how to acquire and release a resource:

```dovetail
record Resource<T, E> =
    acquire: () => Async<T, E>
    release: (T) => Async<Unit, Never>
```

- **`acquire`** — A thunk that produces the resource asynchronously. May fail with `E`.
- **`release`** — A function that releases the resource. Returns `Async<Unit, Never>` — release must not fail (errors during release are logged or swallowed, since the primary computation's result takes precedence).

**Factory function:**

```dovetail
function Resource.make<T, E>(acquire: () => Async<T, E>, release: (T) => Async<Unit, Never>): Resource<T, E> =
    Resource { acquire = acquire; release = release }
```

**Example: TCP socket resource:**

```dovetail
function TcpSocket.connect(host: String, port: Int32): Resource<TcpSocket, IoError> =
    Resource.make(
        () => TcpSocket.connectRaw(host, port),
        (socket) => socket.close()
    )
```

### 5.2 Bracket Primitive

The `Async` runtime needs a **bracket** primitive that guarantees finalization. This is a new variant in the `Async<T, E>` free monad:

```dovetail
Bracket(acquire: Async<Any, Any>, release: Any => Async<Unit, Never>, use: Any => Async<Any, Any>)
```

Every bracket is a **fiber scope** (see [async-runtime-design](async-runtime-design.md) §5.4), starting at `acquire`: fibers forked anywhere inside it are finalized before `release` runs. There is no unscoped form. A scope that never forks allocates no record, so the cost of the guarantee where it is not needed — the plumbing brackets guarding a single host op — is a counter bump and one failed lookup when it closes.

**Runtime interpreter behavior:**

1. Run `acquire`. If it fails, the bracket completes with that failure (no release needed — nothing was acquired).
2. If `acquire` succeeds with value `resource`, run `use(resource)`.
3. Regardless of whether `use` succeeds or fails (produces a `Cause`), run `release(resource)`.
4. If `use` succeeded, return its result. If `use` failed, return its failure.
5. If `release` fails (which shouldn't happen since its error type is `Never`, but defensively), log and discard the release failure — the `use` result/failure takes precedence.
6. `release` runs **uninterruptibly** (wrapped in `Uninterruptible` by the interpreter, not by the caller), so a finalizer with its own suspension points cannot be abandoned half-done. An interrupt arriving during it is deferred to the mask's pop. Note this is only the release: waiting for the scope's fibers, which happens *before* it, stays interruptible on purpose.

**Public API on Async:**

```dovetail
module Async =
    function bracket<T, U, E>(
        acquire: Async<T, E>,
        release: (T) => Async<Unit, Never>,
        use: (T) => Async<U, E>
    ): Async<U, E>
```

This is the foundational primitive. `Resource<T, E>.use` is built on top of it.

### 5.3 Usable Implementation for Resource

```dovetail
implement <T, E> Usable<T, E> for Resource<T, E> =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self, f: (T) => Async<U, E2>, errorF: (E) => E2): Async<U, E2> =
        let trace = SourceLocation { file = ""; line = 0; column = 0; functionName = "" }
        let converted: Async<T, E2> = self.acquire().mapError(errorF, trace)
        Async.bracket(converted, self.release, f)
```

The `errorF` lets the impl convert the resource's source error `E` into the call-site's target error `E2` before bracket runs. Release errors don't need conversion because `Async<Unit, Never>` widens trivially.

### 5.4 mapError

`Resource<T, E>` provides `mapError` for error type conversion:

```dovetail
extension <T, E> for Resource<T, E> =
    function mapError<E2>(self, f: E => E2): Resource<T, E2> =
        Resource {
            acquire = () => self.acquire().mapError(f)
            release = self.release
        }
```

This allows converting a `Resource<File, FileError>` to `Resource<File, AppError>` when the enclosing function uses a broader error type.

---

## 6. Error Type Compatibility

The trait's `errorF: (E) => E2` parameter is the mechanism for source-to-target error conversion. The compiler synthesizes `errorF` at every `use expr` site by inspecting the operand's `Usable<T, E>` impl and the enclosing context's target error type `E2`.

### 6.1 How the Compiler Picks `E2`

In priority order:

1. The enclosing block's expected wrapped error, if the block has an explicit annotation like `let program: Async<_, E2> = …`. The Block handler in inference exposes this via `block_wrapped_error`.
2. The enclosing async function's return error (`async_return_type`).
3. Default: `E2 = E` (no conversion needed).

### 6.2 `errorF` Synthesis

| Case | Synthesized `errorF` |
|------|----------------------|
| `E == E2` | identity `(e) => e` |
| `E == Never` | panic body (closure can never be invoked) |
| `From<E> for E2` exists | `(e) => E2.from(e)` via ImplFunctionCall |
| None of the above | compile error at the `use` site |

### 6.3 Explicit `mapError` (still available)

`Resource<T, E>` provides `mapError` as an escape hatch when users want explicit control:

```dovetail
let file = use File.open(path).mapError((e: FileError) => AppError.io(e))
```

This converts the `Resource<File, FileError>` to `Resource<File, AppError>` before the `use` resolves. The synthesized `errorF` then has `E = E2 = AppError` and degenerates to identity.

### 6.4 Implicit From Conversion (the usual case)

When `AppError implements From<FileError>`, no explicit `mapError` is needed:

```dovetail
implement From<FileError> for AppError =
    public function from(e: FileError): AppError = AppError.io(e)

async function processFile(path: String): Async<String, AppError> =
    let file = use File.open(path)        # errorF synthesized as (e) => AppError.from(e)
    await file.readAll()
```

---

## 7. Sync Use Cases

### 7.1 Cryptographic Secrets

The motivating sync use case: cryptographic key material that must be zeroized after use. A `Secret<T>` type wraps sensitive data and implements `Usable<T>` with `Wrapped<U> = U` (no async, no wrapping):

```dovetail
newtype Secret<T> = T

implement <T> Usable<T> for Secret<T> =
    type Wrapped<U> = U
    function use<U>(self, f: T => U): U =
        let value = self.value
        let result = f(value)
        zeroize(value)
        result
```

Usage:

```dovetail
function encryptMessage(secret: Secret<SymmetricKey>, plaintext: Array<Uint8>, nonce: Nonce): Array<Uint8> =
    let key = use secret
    Aes256Gcm.encrypt(key, nonce, plaintext, Array.empty())
```

The raw key is exposed only within the continuation. After `f` returns, `zeroize` clears the key material from memory. The key never escapes the `use` scope.

### 7.2 Other Sync Examples

**Scoped temporary directory:**

```dovetail
implement Usable<FilePath> for TempDir =
    type Wrapped<U> = Result<U, IoError>
    function use<U>(self, f: FilePath => Result<U, IoError>): Result<U, IoError> =
        let path = createTempDir()
        let result = f(path)
        removeDirRecursive(path)
        result
```

**Scoped mutex lock:**

```dovetail
implement <T> Usable<T> for Mutex<T> =
    type Wrapped<U> = U
    function use<U>(self, f: T => U): U =
        let value = self.lock()
        let result = f(value)
        self.unlock()
        result
```

---

## 8. Typed AST

### 8.1 New Node: `Use`

Add a `Use` variant to `TypedExprKind`:

```rust
TypedExprKind::Use {
    operand: Box<TypedExpr>,        // the Usable value
    inner_type: Type,               // T (resource type)
    wrapped_type: Type,             // Wrapped<U> (return type of use)
    use_method: ResolvedImplMethod, // resolved Usable::use method
}
```

The `use_method` carries the fully resolved trait method reference (trait FQN, type params, for-type), same as `and_then_method` and `map_method` on `Await` nodes.

### 8.2 Typechecker: Inference

During inference, when the typechecker encounters a `use` expression:

1. Resolve `Usable<T>` for the operand's type. Extract `T` (inner type) and `Wrapped<U>` (GAT).
2. The `use` expression's type is `T`.
3. Record the resolved `use` method as `ResolvedImplMethod` on the `Use` node.
4. Verify that `Wrapped<U>` is compatible with the enclosing context's expected type (function return type or block result type). This is handled by normal type inference unification.

---

## 9. Implementation Phases

| Phase | Scope | Deliverables |
|-------|-------|--------------|
| **1. Usable trait in prelude** | Define `Usable<T>` trait with GAT `Wrapped<U>` and `use` method in prelude. Depends on GATs being implemented (from async-await phases 1–2). | Trait definition; typechecker resolves it. |
| **2. `use` as prefix expression** | Parser: change `use` from binding-level to unary prefix. Typechecker: infer `use` expressions — resolve `Usable` impl, extract inner type, record resolved method. Add `Use` node to typed AST. | Lexer/parser update; inference for `Use`; type rule tests. |
| **3. Desugaring pass (`desugar_use`)** | New pass after `desugar_try`, before `desugar_await`. Walks typed AST, finds `Use` nodes, captures rest of block as closure, emits `ImplFunctionCall` to the resolved `use` method. When inside `AsyncBlock`, wraps continuation closure body in `AsyncBlock`. | `desugar_use.rs`; integration with existing desugar pipeline. |
| **4. Async.bracket primitive** | Add `Bracket` variant to `Async<T, E>` free monad. Implement interpreter support: acquire → use → release (guaranteed). Add `Async.bracket` public API. | Runtime variant; interpreter logic; tests for bracket guarantee. |
| **5. Resource<T, E> library type** | Define `Resource<T, E>` record (acquire/release). Implement `Usable<T>` for `Resource<T, E>` using `Async.bracket`. Add `mapError` extension. Add factory functions on IO types (e.g. `TcpSocket.connect` returning `Resource`). | Library types; Usable impl; integration tests. |
| **6. Sync Usable implementations** | Implement `Usable<T>` for `Secret<T>` in crypto package. Test sync `use` in non-async functions. | Sync impl; tests for zeroization semantics. |
| **7. From auto-conversion at `use` sites** | When `Wrapped<U>` type doesn't match the expected type but a `From` conversion exists, insert automatic error conversion (same mechanism as `try`). | Typechecker coercion; tests for mixed error types. |

**Dependencies:** Phase 2 depends on GATs (async-await phases 1–2). Phase 3 depends on 2. Phase 4 depends on [async-runtime-design](async-runtime-design.md). Phase 5 depends on 3 and 4. Phase 6 depends on 3. Phase 7 depends on 2 and the `From` trait infrastructure.

---

## 10. Summary

| Topic | Rule |
|-------|------|
| **Usable<T>** | Trait with GAT `Wrapped<U>` and `use` method. Implementor controls acquire/release lifecycle. |
| **GAT** | Single parameter `Wrapped<U>`. Error type fixed per impl (not parameterized). Mirrors `Awaitable`'s `Rebind<U>`. |
| **`use` expression** | Prefix unary operator (like `await`, `try`). Type of `use expr` is `T`. Desugars to continuation-passing. |
| **Desugaring** | Separate pass before `desugar_await`. Captures rest of block as closure, calls `.use(closure)`. Wraps closure in `AsyncBlock` when inside async function. |
| **Resource<T, E>** | Record wrapping acquire + release. Implements `Usable<T>` via `Async.bracket`. |
| **Async.bracket** | Runtime primitive: acquire → use → release (guaranteed even on failure). New `Async` free monad variant. |
| **Error compat** | `mapError` on `Resource` for explicit conversion. `From` auto-conversion for implicit (same as `try`). |
| **Sync use** | `Wrapped<U> = U` for sync resources (e.g. `Secret<T>`). No async runtime needed. |
| **Scope** | Continuation scope (rest of current block). Not limited to function — can be any block. |
| **Resource order** | Multiple `use` expressions produce LIFO release order (inner released first). |

---

## 11. Scope and Attachment as Separate Operations

`use r` does two things at once: it **opens a scope** and it **attaches** `r` to that scope. Splitting them gives a resource a lifetime other than "the rest of this block" without lifting everything into `Resource` combinator style.

| | opens a scope | attaches a resource | spelled |
|---|---|---|---|
| `use r` | yes | yes | `let x = use r` |
| `Async.scope()` | yes | no | bare `use Async.scope()` |
| `Resource.attachToScope` | no | yes | `let x = await r.attachToScope()` |
| `Async.bracket` | yes | yes | the primitive under all of it; still public |

### 11.1 `Async.scope()`

```dovetail
public function scope(): Resource<Unit, Never> = Resource.succeed(())
```

That is the whole implementation. A `Bracket` whose acquire is pure and whose release is a no-op is *already* a fiber scope — the interpreter opens one at every `Bracket` node, before the acquire — and `Resource.succeed` already builds exactly that resource. No new node, no new `Usable` impl, no compiler change. The function exists so a call site can say what it means.

It is written bare, with no binding: a scope is never a value here, so there is nothing worth naming. That desugar path (a `use` statement that is not a `let`, so `extract_first_use` substitutes a fresh temp and the rest of the block becomes the continuation) had no coverage before this change and now has a test.

### 11.2 `Resource.attachToScope()`

Acquires the resource and registers its finalizer on the fiber's **innermost open scope**, returning the value. The scope may be an `Async.scope()`, an ordinary `use` block's, or — if neither is open — the fiber's root scope, in which case the resource is released when the fiber ends.

**There is no `Scope` type.** A scope is never a value; `attachToScope` targets the innermost one implicitly, exactly as `fork` does. This is deliberate: a first-class scope handle is a capability that escapes — stored in a record, used after its scope closed, used from another fiber — and none of that is checkable. Making the target implicit makes attaching to anything but the innermost scope unrepresentable rather than merely discouraged.

**It is always `await`ed, never `use`d.** A `use` on it would open a fresh scope and release at the end of the block, which is precisely what it exists not to do.

### 11.3 The ordering invariant

**Finalizers run in exact reverse acquisition order, globally**, whichever operation acquired them. A scope's close runs, in order: the drain of its fibers, then its attachments most-recent-first, then the bracket release that opened it (attached first, when the scope opened).

This is why a plain `use` bracket must be a valid attach target. Given

```dovetail
let conn = use Db.connect(url)
let stmt = await conn.prepare(sql).attachToScope()
```

an attach that skipped the bracket and landed on some outer scope would close the statement *after* the connection it was derived from. Under WASI p3, where handle indices are reused after a drop, that is not a leak but a close landing on somebody else's handle — the same failure class the fiber-scope work fixed.

The accepted cost of innermost-wins is that a helper meant to acquire into its *caller's* scope breaks if it opens a bracket first: the attach lands on the bracket and the helper returns an already-released handle. The failure is loud (the next operation fails) rather than silently reordered. The rule that avoids it: **inside a resource-building helper, use `attachToScope` throughout, never `use`.**

### 11.4 Acquisition is uninterruptible

Both `use`/`Async.bracket` and `attachToScope` mask their acquire, matching ZIO's `acquireRelease`.

A resource becomes real somewhere *inside* the acquire, and the runtime cannot see where — it can only take ownership when the acquire's value reaches the frame that owns it. Everything in between is a window in which an interrupt strands a live resource with nothing left that could close it. `settlePendingAcquire` repaired the last step of that window (a pending success directly under an acquire frame) but could not repair the rest: `Resource.make` builds its acquisition as `acquire().map(value => (value, finalizer))`, so an interrupt landing while that pairing map was on top discarded it as "a pure continuation" and the finalizer was never built for a resource that already existed. Every `use` over a `Resource.make` resource had that window.

Peeling and running pending pure maps was considered and rejected: the runtime cannot tell a pairing map from a map that performs the acquisition itself, so running them makes an interrupt *cause* the acquisition it was cancelling. Masking closes the whole window instead of chasing its last step.

The mask is applied by pushing an `UninterruptibleFrame` in the `Bracket`/`Attach` node arms, **not** by wrapping the acquire in an `Uninterruptible` node, and the difference is load-bearing. A node would still be sitting in `asyncValue` when the first-step interrupt deferral fires, and `settlePendingAcquire` only recognises a pending `Succeed` — so a fiber interrupted before its first step would unwind reading its acquire as failed. That is exactly `forkAndUse`, whose acquire is a `Succeed` holding a socket its parent already accepted.

### 11.5 `Async.interruptible` — the escape hatch the mask forces

Masking the acquire is not sufficient on its own, and the reason is structural rather than incidental: **some acquires park for an unbounded time.** `TcpListener.accept` is an acquire that waits for a connection that may never arrive; `Mutex.acquire` is an acquire that waits for another fiber to release. Masked, neither can be cancelled — so a scope tearing down a background acceptor waits forever on a fiber that is waiting for the scope, which is a deadlock, not a leak. Three `TcpListener` teardown tests found this immediately.

So `Async.interruptible` exists — ZIO's `restore`, the half of `uninterruptibleMask` that hands interruptibility back. It is the exact mirror of `uninterruptible`: it restores through the same `UninterruptibleFrame`, so whatever the fiber's interruptibility was, it comes back on the way out.

The discipline it comes with is the whole point: **wrap the park, never the hand-off.**

```dovetail
let copied = await Async<Option<TcpSocket>, NetError>
    .streamRead(source, 1i32)
    .interruptible()
```

The fiber is interruptible while it waits and masked again from the moment it holds something, so the ownership transfer stays atomic. Used around the part that takes ownership, it puts back exactly the leak the mask exists to prevent.

The acquisitions in the tree that use it are all waits rather than acquisitions in the strict sense: the accept park and the connect/DNS subtasks in `standard-io-net/src/intrinsics.dove`, the open syscall in `standard-io-fs/src/intrinsics.dove` (a FIFO or a hung mount can block an open without bound), and `Mutex.acquire` / `ReentrantMutex.acquire` — taking a mutex is not a resource coming into existence, so nothing is stranded if it never completes. SQLite's open/`BEGIN` stay masked: their waits are bounded by the busy-handler configuration, which is what "keep acquisitions bounded" asks for.

Two interpreter rules keep interrupts from stranding values in the gaps between steps. A **wake** that hands the fiber elements (a read's bytes, an accepted socket) always marks it as carrying, so an interrupt landing before its next step defers to a safe point; any other successful wake or synchronously-returned host result carries too, unless an interrupt is *already* pending — then the mask that deferred it is about to pay it, and carrying would only push delivery past that pop. Carrying ends at every safe point whether or not an interrupt arrives — the acquire frames (owned) and every park — because a flag that outlived its value made mask pops skip their delivery and the Running arm skip its lock repair. And a **mask's own result** is carried until the next frame consumes it: `Queue.take` dequeues inside its mask, and the element left the mask as a pending value nothing owned for one step — the one step a fairness boundary could split — so an interrupt there lost it. Carrying applies only where the value is *exposed* — the fiber interruptible — since inside a mask the mask itself defers every interrupt and no fairness boundary can split a masked run; and a failure outcome never carries, because a `FailCause` owns nothing. The same rule decides a **raced cancel**: when interrupting a parked fiber cancels its host call and the cancel loses — the host had already completed the call — an `Ok` payload (bytes, an accepted socket) is handed to the fiber, carried, and the interrupt lands once the fiber owns it; a typed `Error` owns nothing, so the fiber is interrupted on the spot exactly as if the cancel had won, the interrupt superseding the error as it does at every mask pop. Resuming with the error *and* a pending interrupt would leave the fiber interruptible with nothing carried, and the next value-bearing mask pop after a `catchAll` would pay the interrupt instead of exposing its result. The accepted cost of the general rule: a fiber interrupted in the one step after it leaves a mask holding a pure value is deferred to its next safe point or fairness boundary rather than stopped on the spot. And a fiber interrupted **before its first step** is interrupted before the step if its head node is a leaf (a `Thunk`, a `Fork`, a bare `Succeed` — none of which build a frame, all of which would otherwise do their work in the one granted step); only a combinator head gets the step, so the unwind has a frame to run.

`interruptible` punches the **acquire mask only**. Masks come in two kinds: the acquire mask is *soft*, and the mask the interpreter wraps releases and finalizer chains in — the same `Uninterruptible` node as a user critical section — is *hard*. Inside a hard mask `interruptible` is a deliberate no-op, so a finalizer that takes a mutex still cannot be cut in half. (TLS teardown's write-lock take is the deliberate exception: it runs inside `shutdown`'s `.timeout` racer — a fresh fiber outside the mask — precisely so the bound can cut it.) The one escape hatch a parking finalizer has is `.timeout(...)`, which works under any mask because the timer races in its own interruptible fiber.

The one release in the tree whose progress is peer-controlled — TLS teardown's `close_notify` flush, which advances only as fast as the peer drains its TCP window — uses that escape hatch: `TlsConnection.shutdown` bounds `initiateClose` with a 3-second `.timeout`, so a peer that stops reading forfeits its close_notify instead of stalling the release forever. The losing close unwinds through its own armed frames (write lock released, in-flight copy cancelled), and the transport socket is closed by its own resource either way.

The residual cost is unchanged in kind but much smaller in scope: an acquire that parks without opting back in cannot be cut short. That is now a property of the individual acquisition, visible where it is written, rather than of every `use` in the language.

### 11.6 The accepted footgun

An attach with no scope of its own lands on the fiber's root scope. It never dangles, and for a server that forks a fiber per request it is exactly right — but a long-running loop that attaches per turn accumulates a finalizer per turn. `Async<T, E>` has no environment parameter, so there is nothing for the compiler to check against; this is documentation, not types. The answer is a scope in the loop body, the same shape `Resource.whileLoop` already exists for.

**And it stays a footgun — no runtime diagnostic.** A "warning past N root-scope attachments on one fiber" was considered and rejected: N is unguessable (a fiber that legitimately attaches hundreds of buffers is as ordinary as a loop that leaks three connections), the check costs a counter on every attach on the one path that is meant to be free, and a warning nobody can silence for the legitimate case trains people to ignore it. The distinction the diagnostic would need — "this attach is per-turn work" — is exactly the one the type system cannot make, which is why this is documentation in the first place. If it ever becomes a real source of bugs, the fix is an environment parameter on `Async`, not a heuristic.

Open policy items found during the review of this design — behaviours that are consistent with a documented doctrine but deserve a deliberate decision — are tracked in [issues.md](issues.md).

### 11.7 Removed: `useForever`

A leak with a name — it dropped the finalizer, so release never ran. `attachToScope` with no enclosing scope is the honest spelling of "lives as long as the program", and unlike `useForever` it actually releases at the end. Its production callers loaded PEM certificates and keys whose finalizers wipe key material, so the migration turned a leak of secret material into a wipe at fiber end.

---

## 12. References

- [async-await-design.md](async-await-design.md) — Awaitable trait, async/await syntax, desugaring to andThen/map/succeed.
- [async-runtime-design.md](async-runtime-design.md) — Async<T, E> free monad, Cause error model, fiber runtime. Bracket primitive extends this.
- [railway-early-return-design.md](railway-early-return-design.md) — EarlyReturn trait, try/orReturn, From-based error conversion.
- [crypto-library-design.md](crypto-library-design.md) — Secret/key types, zeroization (sync Usable use case).
- [io-library-design.md](io-library-design.md) — WASI I/O types that will expose Resource-returning factory functions.
- [traits-design.md](traits-design.md) — Traits, impl blocks, associated types, GATs.
- [closures-design.md](closures-design.md) — Closure capture semantics (relevant for desugaring continuations).
- [grammar.md](../grammar.md) — Expression grammar, unary operators, precedence.

---
