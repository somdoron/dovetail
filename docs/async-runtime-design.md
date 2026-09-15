# Async Runtime and Fiber Library Design

This document designs the **async runtime** for Dovetail: the `Async<T, E>` effect type, the `Fiber` green-thread abstraction, the `Promise` coordination primitive, and the single-threaded cooperative fiber scheduler built on the WASI event loop. Inspired by [ZIO](https://github.com/zio/zio), simplified for Dovetail's WASM-GC + WASI target.

**In scope:** `Async<T, E>` enum (free monad); `Cause<E>` error model; `Fiber<T, E>` green threads; `Promise<T, E>` user-completable handles; `Awaitable<T>` implementation for `Async`; cooperative fiber scheduler; WASI `Poll.wait`-based event loop; structured concurrency; interruption model; `Async.run` entry point.

**Out of scope:** Resource management / finalizers (future design); higher-level combinators (race, zipPar, timeout — built on primitives later); HTTP / TLS; durable execution.

**Implementation status:** Not started.

**Prerequisites:** [async-await-design](async-await-design.md) (syntax and desugaring), [io-library-design](io-library-design.md) (WASI pollables and low-level I/O).

---

## 1. Overview

### 1.1 Architecture

```
┌──────────────────────────────────────────────────────┐
│  User code (async functions, await expressions)      │
├──────────────────────────────────────────────────────┤
│  Async<T, E>   — effect description (free monad)     │
│  Fiber<T, E>   — green thread handle                 │
│  Promise<T, E> — user-completable signal             │
├──────────────────────────────────────────────────────┤
│  Fiber Scheduler + Interpreter                       │
│    - Interprets Async enum variants                  │
│    - Manages fiber run queues                        │
│    - Tracks parent/child relationships               │
│    - Handles interruption                            │
├──────────────────────────────────────────────────────┤
│  WASI Event Loop                                     │
│    - Poll.wait(pollables) — single wait primitive    │
│    - MonotonicClock timers for sleep/timeout          │
├──────────────────────────────────────────────────────┤
│  WASI I/O (io-library-design.md)                     │
│    - Pollable, InputStream, OutputStream, TcpSocket  │
└──────────────────────────────────────────────────────┘
```

### 1.2 Design Principles

- **Free monad:** `Async<T, E>` is a lazy data structure describing a computation, not the computation itself. The runtime interprets it.
- **Single-threaded:** WASM is single-threaded. All fibers run cooperatively on one thread. No locks, no atomics.
- **Cooperative yielding:** A fiber yields at suspension points: `Pollable`, `AwaitPromise`, `Join`. Between suspension points, a fiber runs uninterrupted.
- **Structured concurrency:** `fork` attaches a fiber to the innermost open scope — a `use`/`bracket` block, or the forking fiber's body. Closing that scope waits for the fiber (normal exit) or cancels it (failure/interruption); `forkBackground` always cancels. There is no third form that escapes scopes.
- **Pollable-centric:** Every I/O wait reduces to a WASI `Pollable`. The event loop calls `Poll.wait` with all pending pollables and wakes the corresponding fibers.

---

## 2. Source Location and Tracing

### 2.1 SourceLocation

A compile-time-known source position, used for async stack traces.

```dovetail
record SourceLocation =
    file: String
    line: Int32
    functionName: String
```

`SourceLocation` values are emitted as **WASM-GC globals** by the compiler — one global per unique (file, line, function) triple. At call sites, the compiler references the pre-allocated global (a `global.get`), so passing a `SourceLocation` has **zero allocation cost** at runtime.

---

## 3. Error Model — `Cause<E>`

### 3.1 Definition

`Cause<E>` captures the full reason a fiber failed, along with the async stack trace at the point of failure. It is flat (no recursive composition for now).

```dovetail
enum Cause<E> =
    Failed(error: E, trace: Array<SourceLocation>)
    Panicked(message: String, trace: Array<SourceLocation>)
    Interrupted(trace: Array<SourceLocation>)
```

- **Failed(error, trace):** A typed business error. The `E` in `Async<T, E>`. The `trace` is the async stack trace at the point of failure — collected from the fiber's continuation stack.
- **Panicked(message, trace):** An unrecoverable defect — the async equivalent of a language `panic`. Should not be caught in normal code; indicates a bug.
- **Interrupted(trace):** The fiber was cancelled by its parent, by an explicit `fiber.interrupt()`, or as part of structured concurrency cleanup.

### 3.2 Future Extension

If parallel composition is added (e.g. `zipPar`, `race`), `Cause` may grow a `Multiple(causes: Array<Cause<E>>)` variant to preserve all failure information from concurrent operations. This is deferred.

---

## 4. Core Effect Type — `Async<T, E>`

### 4.1 Enum Definition

`Async<T, E>` is a discriminated union (enum) whose variants describe computation steps. The runtime interprets these variants to drive execution.

```dovetail
enum Async<out T, out E> =
    Succeed(value: T)
    FailCause(cause: Cause<E>)
    Map(source: Async<Any, E>, f: Any => T, trace: SourceLocation)
    AndThen(source: Async<Any, E>, f: Any => Async<T, E>, trace: SourceLocation)
    Fold(source: Async<Any, Any>, onFailure: Any => Async<T, E>, onSuccess: Any => Async<T, E>, trace: SourceLocation)
    FoldCause(source: Async<Any, Any>, onCause: Cause<Any> => Async<T, E>, onSuccess: Any => Async<T, E>, trace: SourceLocation)
    Fork(source: Async<Any, Any>)
    Join(fiber: Fiber<T, E>)
    Pollable(pollable: standard.wasi.Pollable, whenReady: Unit => Async<T, E>)
    AwaitPromise(promise: Promise<T, E>)
    MakePromise
    CompletePromise(promiseId: PromiseId, result: Result<Any, Cause<Any>>)
    Uninterruptible(source: Async<T, E>)
    Interruptible(source: Async<T, E>)
```

**Type erasure note:** The `Map`, `AndThen`, `Fold`, `FoldCause`, and `Fork` variants have existential intermediate types (the source's `T` and/or `E` are not visible in the outer `Async<T, E>`). These are represented as `Any` at runtime. The type safety comes from the construction sites (the `map`, `andThen`, `fold`, `foldCause`, `fork` methods) which accept correctly-typed arguments. The runtime interpreter casts `Any` back to the correct type — this is safe because the construction guarantees the types match.

### 4.2 Variant Semantics

| Variant | Meaning |
|---------|---------|
| `Succeed(value)` | Immediately completes with `value`. Terminal. |
| `FailCause(cause)` | Immediately fails with the given `Cause`. Terminal. |
| `Map(source, f)` | Interpret `source`, then apply `f` to its result. |
| `AndThen(source, f)` | Interpret `source`, then interpret the `Async` returned by `f(result)`. |
| `Fold(source, onFailure, onSuccess)` | Interpret `source`. On typed error `E`, call `onFailure(e)` and interpret the result. On success, call `onSuccess(value)` and interpret the result. Panics and interruptions are **not** caught — they propagate. |
| `FoldCause(source, onCause, onSuccess)` | Like `Fold` but catches **all** failures including panics and interruptions via the full `Cause<E>`. The most powerful error-handling primitive. |
| `Fork(source)` | Create a new child fiber to interpret `source`. The current fiber immediately succeeds with a `Fiber<T, E>` handle. |
| `Join(fiber)` | Suspend the current fiber until `fiber` completes. Re-raises the fiber's result: if the fiber succeeded, the join succeeds with its value. If the fiber failed or was interrupted, the join fails with the same cause. |
| `Pollable(pollable, whenReady)` | Suspend the current fiber and register `pollable` with the event loop. When `Poll.wait` reports the pollable is ready, call `whenReady()` and interpret the resulting `Async`. The `whenReady` callback may return another `Pollable` if the operation would block again. |
| `AwaitPromise(promise)` | Suspend the current fiber until the `promise` is completed (by calling `promise.succeed(value)` or `promise.fail(error)` from any fiber). |
| `MakePromise` | Allocate a new promise ID, construct a `Promise<T, E>` class instance with `Pending` state, and succeed with it. |
| `CompletePromise(promiseId, result)` | Wake all fibers waiting on `promiseId`, and succeed with `true`. The promise's state was already set eagerly by `succeed`/`fail`. |
| `Uninterruptible(source)` | Interpret `source` with interruption suppressed. Any pending interrupt is deferred until the uninterruptible region completes. Used for cleanup code, critical sections, and every bracket's acquire. |
| `Interruptible(source)` | Interpret `source` interruptibly inside a region that is not — ZIO's `restore`. Restores through the same frame, so the enclosing region's interruptibility comes back on the way out. For the parking half of an acquire, and nothing else — it punches the *soft* acquire mask only; inside a *hard* mask (`Uninterruptible`: releases, finalizer chains, user critical sections) it is a no-op, tracked by a per-fiber `hardMaskDepth`. Entering the region delivers a pending interrupt before the wait starts. |

### 4.3 Static Constructors

```dovetail
module Async =
    function succeed<T>(value: T): Async<T, Never> =
        Succeed(value)

    function fail<E>(error: E): Async<Never, E> =
        FailCause(Cause.Failed(error))

    function panic<T, E>(message: String): Async<T, E> =
        FailCause(Cause.Panicked(message))

    function fromCause<T, E>(cause: Cause<E>): Async<T, E> =
        FailCause(cause)

    function pollable<T, E>(p: standard.wasi.Pollable, whenReady: Unit => Async<T, E>): Async<T, E> =
        Pollable(p, whenReady)

    function promise<T, E>(): Async<Promise<T, E>, Never> =
        MakePromise

    function uninterruptible<T, E>(source: Async<T, E>): Async<T, E> =
        Uninterruptible(source)

    function sleep(duration: standard.wasi.clock.Duration): Async<Unit, Never> =
        let p = MonotonicClock.subscribeDuration(duration)
        Pollable(p, _ => Async.succeed(()))

    function never(): Async<Never, Never>
        // An Async that never completes. Useful as identity for race.

    function run<E>(program: Async<Unit, E>): Unit where E : Display
        // Entry point. Creates the main fiber, starts the event loop,
        // blocks until the main fiber completes. If the main fiber fails,
        // prints the error and exits with a non-zero status code.
```

### 4.4 Instance Methods

```dovetail
module Async<T, E> =
    function map<U>(self, f: T => U, trace: SourceLocation): Async<U, E> =
        Map(self, f, trace)

    function andThen<U>(self, f: T => Async<U, E>, trace: SourceLocation): Async<U, E> =
        AndThen(self, f, trace)

    function fold<U, E2>(self, onFailure: E => Async<U, E2>, onSuccess: T => Async<U, E2>, trace: SourceLocation): Async<U, E2> =
        Fold(self, onFailure, onSuccess, trace)

    function foldCause<U, E2>(self, onCause: Cause<E> => Async<U, E2>, onSuccess: T => Async<U, E2>, trace: SourceLocation): Async<U, E2> =
        FoldCause(self, onCause, onSuccess, trace)

    function fork(self): Async<Fiber<T, E>, Never>
        // Spawn a child fiber (structured). The child is cancelled when the parent completes.
        Fork(self)

    // NOTE: an earlier draft had a `forkDaemon` here — a fiber attached to the
    // main fiber's root scope, escaping every enclosing `use`. It was removed:
    // every use for one turned out to be a fiber that should have been forked
    // from a scope one level further out, and a daemon outliving the resource it
    // was reading from is exactly the hazard scopes exist to prevent.

    function mapError<E2>(self, f: E => E2): Async<T, E2> =
        self.fold(e => Async.fail(f(e)), t => Async.succeed(t))

    function catchAll<E2>(self, handler: E => Async<T, E2>): Async<T, E2> =
        self.fold(handler, t => Async.succeed(t))

    function orElse(self, alternative: Async<T, E>): Async<T, E> =
        self.catchAll(_ => alternative)
```

### 4.5 Awaitable Implementation

`Async<T, E>` implements the `Awaitable<T>` trait from [async-await-design](async-await-design.md), enabling `async`/`await` syntax. The `map` and `andThen` methods accept a `SourceLocation` parameter, which the compiler fills in automatically during async/await desugaring (see §10 Async Stack Traces).

```dovetail
implement <T, E> Awaitable<T> for Async<T, E> =
    type Rebind<U> = Async<U, E>

    static function succeed(x: T): Async<T, E> =
        Async.Succeed(x)

    function map<U>(self, f: T => U, trace: SourceLocation): Async<U, E> =
        Async.Map(self, f, trace)

    function andThen<U>(self, f: T => Async<U, E>, trace: SourceLocation): Async<U, E> =
        Async.AndThen(self, f, trace)
```

This means `await` works on any `Async<T, E>` value, and `async` functions returning `Async<T, E>` desugar into `andThen`/`map`/`succeed` chains as specified in [async-await-design](async-await-design.md). Every desugared call carries the source location of the original `await` expression.

---

## 5. Fiber

### 5.1 Definition

A `Fiber<T, E>` is a handle to a green thread. It is an opaque type (the internal representation is managed by the runtime).

```dovetail
newtype Fiber<T, E> = Int32
```

The `Int32` is a fiber ID — an index into the runtime's fiber table.

### 5.2 Methods

```dovetail
module Fiber<T, E> =
    function join(self): Async<T, E>
        // Suspend the calling fiber until this fiber completes.
        // Re-raises the result: if this fiber succeeded, join succeeds.
        // If this fiber failed or was interrupted, join fails with the same cause.
        Join(self)

    function interrupt(self): Async<Unit, Never>
        // Request cancellation of this fiber. The fiber will be interrupted
        // at its next suspension point. Returns immediately (does not wait
        // for the fiber to actually finish).

    function awaitCause(self): Async<Cause<E>, Never>
        // Suspend the calling fiber until this fiber completes.
        // Returns the full Cause on failure, without re-raising.
        // On success, this is not directly useful — use join for that.
        // Primarily for inspection/logging.
```

### 5.3 Fiber Lifecycle

A fiber has the following states:

```
Created → Running → Suspended → Running → ... → Completed
                                                    ↓
                                              Succeeded | Failed | Interrupted
```

| State | Description |
|-------|-------------|
| **Created** | Fiber allocated but not yet started. Enters the run queue immediately. |
| **Running** | Fiber is actively being interpreted by the scheduler. Only one fiber runs at a time (single-threaded). |
| **Suspended** | Fiber is waiting on a `Pollable`, a `Promise`, or a `Join`. It is registered with the event loop or the promise's waiter list and will be resumed when the condition is met. |
| **Completed** | Fiber has terminated with a result: `Succeeded(value: T)`, `Failed(cause: Cause<E>)`, or `Interrupted`. Fibers waiting on `Join` are woken. |

### 5.4 Structured Concurrency

Fibers belong to **scopes**, not directly to fibers. Every fiber has a *root scope* (its whole body), and each `use`/`Async.bracket` block opens a nested scope inside it. `Async.scope()` opens one that acquires nothing. A fiber is attached to whichever scope was innermost when it was forked; so is a finalizer registered by `Resource.attachToScope`.

**Rules:**
1. `fork` attaches the new fiber to the forking fiber's **innermost open scope**. So does `Resource.attachToScope`, which registers a finalizer rather than a fiber. There is no way to name any other scope — a scope is never a value.
2. Closing a scope **normally** waits for its structured fibers; closing it **abnormally** (the body failed or was interrupted) interrupts them first, then waits.
3. `forkBackground` attaches to the same scope but is always interrupted at the close, never waited for on its own. This is the pre-scopes behaviour of `fork`.
4. Every close waits for its fibers to finish **unwinding**, interrupted or not, so a resource is never released while a fiber is part-way through its own finalizers on it.
4a. A close then runs, in order: the scope's **attached finalizers**, most recent first, and then the **bracket's own release** if a `use`/`bracket` opened it. That release was attached first, when the scope opened, so this is one reverse-acquisition order over both — an `attachToScope` inside a `use` block releases before the `use`'s own resource.
4b. The **acquire** is masked too, as ZIO's `acquireRelease` does it. The resource becomes real inside the acquire, invisibly, and the frame that owns its release is armed only when the acquire's value comes back; an interrupt in between stranded it. Masking makes the acquisition atomic — it reaches its owner or never happened. An acquire that *parks* for an unbounded time (an accept, a mutex queue) opts back in for the wait alone with `Async.interruptible` — ZIO's `restore` — wrapping the park and never the hand-off; without it a scope tearing down a background acceptor deadlocks against it.
5. That whole unwind — attachments and release together, under one mask — runs **uninterruptibly**, so a finalizer with suspension points cannot be abandoned half-done, and an interrupt can no more land between two finalizers than inside one; an interrupt arriving during it is deferred to the mask's pop. The wait in rule 4 is *not* masked — it has to stay interruptible or an interrupt could not end a fiber whose child never finishes.
6. There is no escape from scopes. A fiber that must outlive an enclosing `use` is forked from the scope that should own it, one level further out. `use Async.scope()` is how a block gets a scope it had no resource to open one with.
7. A structured fiber that *fails* does not fail its scope; the failure is observable only through `join`. That holds for a **defect** as much as a typed error, and whenever the child dies — `decideRace`'s promotion arm, which lets a finished racer's panic supersede a decided result, is gated on a `isRace` marker on the record precisely so an ordinary scope's outcome does not depend on whether its owner happened to be parked draining when the child blew up. A `race` is the one scope that owes its caller a child's defect (the loser's finalizer panic), because there is no `join` on a racer to see it through. Nursery-style propagation is a deliberate non-goal for now — adding it later means a `firstFailure` field on `ScopeRecord`, set in `leaveScope` when a structured fiber finalizes `FiberFailed`, rewriting `resume` from `ResumeValue` to `ResumeCause` and escalating the remaining fibers. That changes observable outcomes, so it is a decision to make once rather than drift into.

**Rationale:** Kotlin's `coroutineScope` rule rather than ZIO's — waiting on a normal exit and cancelling on an abnormal one — because the resource case demands it. `use` releases at *block* exit, which is finer-grained than the fiber, so tying fibers to the fiber alone let a handle be dropped under a live reader. Under p3 that is a trap, not a leak: handle indices are reused after drop. It also gives graceful shutdown for free — a serve loop that ends on its own waits for in-flight handlers.

The failure mode this introduces is a scope waiting on a fiber that can only finish once the scope closes; `forkBackground` is the answer, and the deadlock detector names both fibers rather than hanging.

**Implementation:** `ScopeId` + `ScopeRecord { owner, structured, background, finalizers, resume, isRace }` on the runtime, allocated on a scope's first fork *or* first attachment and dropped when nothing is left in it, so a scope that does neither costs one counter bump and one missed map lookup. `FiberStatus.Draining(ScopeId)` parks the closing fiber; the bracket release frame is **re-pushed unchanged** while it drains, so re-entry re-runs the same check and an interrupt landing mid-drain simply escalates the close.

`closeScope` returns `Draining | Closed(finalizers)` rather than a bool, so a caller cannot take a scope out of the map and forget what was owed on it; it is the one place a record leaves the map on a close. Two details follow from attachments existing:

- The empty-scope fast path still has to run them, so `hasNoChildren` (the drain decision) and `isDroppable` (the removal decision) are separate questions, and `leaveScope` resumes on the first while dropping the record only on the second.
- Every close arms its unwind with **frames, never nodes**: the consuming `MapFrame`/`AndThenFrame` is pushed, then the hard `UninterruptibleFrame`, and the bare finalizer chain goes into `asyncValue` (`runMaskedUnwind`). Handing the chain over wrapped in an `Uninterruptible` node — the earlier shape — left it owned by nothing for one step after the release frame popped, and a fairness-boundary split there let a sibling's interrupt discard the release. The acquire mask is a frame push for the same reason.
- The **root scope** is the one close with no frame under it — the body is over, and a finalizer can suspend. `completeFiber` therefore checks for attachments first and, if there are any, pushes a `ScopeCloseFrame` and re-enters the interpreter with the result the fiber was about to finish with. The frame drains, runs the chain masked, reproduces the result, and lands back in `completeFiber` with nothing left — unless a finalizer attached or forked something of its own, which is what the second pass is for. Wrapping every fiber body in an implicit bracket instead would break the one-step interrupt deferral that `forkAndUse` depends on: that step would settle the wrapper's no-op acquire while the real program, holding an already-accepted socket, was still unstepped inside `useFn`.
- Because the root scope can now drain in two roles, `interruptDrainingFiber` discriminates on the record's `resume` (`FinishFiber` = the body ending, nothing left to unwind) rather than on the scope id.

---

## 6. Promise

### 6.1 Definition

A `Promise<T, E>` is a single-assignment coordination primitive. It represents a value that will be provided in the future by some other fiber.

`Promise` is a **mutable GC-heap class**, not a handle into a runtime dictionary. WASM-GC has no finalizers, so a handle-based approach would leak dictionary entries since we'd never know when to remove them. As a GC object, the promise is automatically reclaimed when nothing references it. The `id` is assigned by the runtime (via `MakePromise`) and used to key the waiter registry. The `state` is set to `Completed` when `succeed`/`fail` is called, so late `await` calls see the result immediately without any runtime lookup.

```dovetail
newtype PromiseId = Int32

enum PromiseState<T, E> =
    Pending
    Completed(result: Result<T, Cause<E>>)

class Promise<T, E> =
    id: PromiseId
    mutable state: PromiseState<T, E>
```

### 6.2 Methods

```dovetail
module Promise<T, E> =
    function succeed(self, value: T): Async<Bool, Never> =
        match self.state with
            Completed(_) => Succeed(false)
            Pending =>
                self.state = Completed(Ok(value))
                CompletePromise(self.id, Ok(value))

    function fail(self, error: E): Async<Bool, Never> =
        self.failCause(Cause.Failed(error))

    function failCause(self, cause: Cause<E>): Async<Bool, Never> =
        match self.state with
            Completed(_) => Succeed(false)
            Pending =>
                self.state = Completed(Error(cause))
                CompletePromise(self.id, Error(cause))

    function await(self): Async<T, E> =
        AwaitPromise(self)
```

### 6.3 Creation

Promises are created via `Async.promise()`, which the runtime interprets to allocate a new promise ID:

```dovetail
async function example(): Async<String, Never> =
    let promise = await Async.promise<String, Never>()
    // Pass `promise` to another fiber that will complete it.
    let fiber = await (async function (): Async<Unit, Never> =
        promise.succeed("hello")
    ).fork()
    // Wait for the promise to be completed.
    await promise.await()
```

### 6.4 Runtime Handling

The runtime handles three promise-related variants:

- **`MakePromise`:** Allocates a new promise ID, constructs a `Promise` class instance with `state = Pending`, and succeeds with it.
- **`AwaitPromise(promise)`:** Reads `promise.state` directly on the class instance. If `Completed(result)`, continues with the result immediately. If `Pending`, adds the fiber to the `promiseWaiters` registry (keyed by `promise.id`) and suspends. When the fiber is later woken by `CompletePromise`, it re-enters the interpreter, hits `AwaitPromise` again, and this time sees `Completed` on the object.
- **`CompletePromise(promiseId, result)`:** The promise's state was already set to `Completed` eagerly by `succeed`/`fail`. The runtime looks up waiting fibers in `promiseWaiters` by `promiseId`, moves them to the run queue, removes the waiter entry, and succeeds with `true`.

---

## 7. Runtime Internals

### 7.1 Runtime State

The runtime is a singleton (module-level mutable state) created by `Async.run`. It manages all fibers, promises, and the event loop.

```
RuntimeState =
    nextFiberId: Int32
    nextPromiseId: PromiseId
    fibers: Map<FiberId, FiberState>
    runQueue: Queue<FiberId>
    pollRegistry: Map<PollableHandle, FiberId>
    promiseWaiters: Map<PromiseId, Array<FiberId>>
    mainFiberId: FiberId
```

Promise state (`Pending`/`Completed`) lives on the `Promise` class instance itself — there is no runtime map for promise state. The runtime only tracks `promiseWaiters` (which fibers are suspended waiting on which promise). Entries in `promiseWaiters` are removed when the promise is completed. Late `await` calls read `promise.state` directly on the GC object and see `Completed` immediately.

### 7.2 Fiber State (internal)

Each fiber in the fiber table holds:

```
FiberState =
    id: FiberId
    status: FiberStatus           // Running | Suspended | Draining(ScopeId) | Completed
    asyncValue: Async<Any, Any>   // The current Async being interpreted
    continuation: Stack<Frame>    // Continuation stack (for AndThen/Map/Fold chains)
    parent: Option<FiberId>       // diagnostics only; `childScope` owns the lifetime link
    rootScope: ScopeId            // this fiber's body as a scope (replaces `children`)
    currentScope: ScopeId         // innermost open scope — what the next `fork` joins
    interruptible: Bool           // false inside Uninterruptible regions
    interruptPending: Bool        // true if interrupt requested while uninterruptible
    result: Option<FiberResult>   // Set when completed
```

Where `Frame` is a continuation frame (each carries a `SourceLocation` for async stack traces):

```
Frame =
    MapFrame(f: Any => Any, trace: SourceLocation)
    AndThenFrame(f: Any => Async<Any, Any>, trace: SourceLocation)
    FoldFrame(onFailure: Any => Async<Any, Any>, onSuccess: Any => Async<Any, Any>, trace: SourceLocation)
    FoldCauseFrame(onCause: Cause<Any> => Async<Any, Any>, onSuccess: Any => Async<Any, Any>, trace: SourceLocation)
    UninterruptibleFrame(previousInterruptible: Bool, wasHard: Bool)   // hard = Uninterruptible node; soft = acquire mask, punchable by Interruptible
```

And `FiberResult`:

```
FiberResult =
    Succeeded(value: Any)
    Failed(cause: Cause<Any>)
```

### 7.3 Fiber Interpreter

The interpreter runs one fiber at a time, stepping through the `Async` enum tree. It uses a loop with a continuation stack (rather than recursion) to avoid stack overflow on deep `andThen` chains.

**Critical invariant: `step` must never recurse.** Every branch of the `match` must set `fiber.asyncValue` (or suspend/complete the fiber) and **return** — never call `step` again, directly or indirectly. The flat `while` loop in `runFiber` (§7.6) drives the next step. This is what makes the interpreter a trampoline: no matter how deep the async recursion, the WASM call stack stays bounded to a single `step` call. Violating this invariant reintroduces the stack overflow risk.

**Interpreter loop (for a single fiber):**

```
function step(fiber: FiberState):
    match fiber.asyncValue with
        Succeed(value) =>
            if fiber.continuation.isEmpty() then
                completeFiber(fiber, FiberResult.Succeeded(value))
            else
                let frame = fiber.continuation.pop()
                match frame with
                    MapFrame(f) =>
                        fiber.asyncValue = Succeed(f(value))
                    AndThenFrame(f) =>
                        fiber.asyncValue = f(value)
                    FoldFrame(_, onSuccess) =>
                        fiber.asyncValue = onSuccess(value)
                    FoldCauseFrame(_, onSuccess) =>
                        fiber.asyncValue = onSuccess(value)
                    UninterruptibleFrame(prev) =>
                        fiber.interruptible = prev
                        checkPendingInterrupt(fiber)
                        fiber.asyncValue = Succeed(value)

        FailCause(cause) =>
            // Walk up the continuation stack looking for a Fold/FoldCause frame.
            // While unwinding, collect SourceLocations from skipped frames into the trace.
            let handler = findErrorHandler(fiber, cause)
            if handler is Some((frame, causeWithTrace)) then
                match frame with
                    FoldFrame(onFailure, _, _) =>
                        match causeWithTrace with
                            Failed(e, _) => fiber.asyncValue = onFailure(e)
                            _ => // Panics and interruptions skip FoldFrame
                                 continue unwinding
                    FoldCauseFrame(onCause, _, _) =>
                        fiber.asyncValue = onCause(causeWithTrace)
            else
                // No handler found — complete the fiber with the cause.
                // The cause's trace is built from the SourceLocations on the
                // continuation frames that were unwound.
                completeFiber(fiber, FiberResult.Failed(causeWithTrace))

        Map(source, f, trace) =>
            fiber.continuation.push(MapFrame(f, trace))
            fiber.asyncValue = source

        AndThen(source, f, trace) =>
            fiber.continuation.push(AndThenFrame(f, trace))
            fiber.asyncValue = source

        Fold(source, onFailure, onSuccess, trace) =>
            fiber.continuation.push(FoldFrame(onFailure, onSuccess, trace))
            fiber.asyncValue = source

        FoldCause(source, onCause, onSuccess, trace) =>
            fiber.continuation.push(FoldCauseFrame(onCause, onSuccess, trace))
            fiber.asyncValue = source

        Fork(source) =>
            let childId = createFiber(source, parent = Some(fiber.id))
            fiber.asyncValue = Succeed(Fiber(childId))

        Join(targetFiber) =>
            let target = fibers.get(targetFiber.id)
            match target.result with
                Some(Succeeded(value)) =>
                    fiber.asyncValue = Succeed(value)
                Some(Failed(cause)) =>
                    fiber.asyncValue = FailCause(cause)
                None =>
                    // Target still running — suspend until it completes
                    suspendForJoin(fiber, targetFiber.id)

        Pollable(pollable, whenReady) =>
            if pollable.ready() then
                // Already ready — call whenReady immediately
                fiber.asyncValue = whenReady(())
            else
                // Not ready — register and suspend
                suspendForPollable(fiber, pollable, whenReady)

        AwaitPromise(promise) =>
            match promise.state with
                Completed(result) =>
                    match result with
                        Ok(value) => fiber.asyncValue = Succeed(value)
                        Error(cause) => fiber.asyncValue = FailCause(cause)
                Pending =>
                    promiseWaiters.getOrCreate(promise.id).push(fiber.id)
                    suspendFiber(fiber)

        MakePromise =>
            let id = runtime.nextPromiseId
            runtime.nextPromiseId = id + 1
            let promise = Promise(id = id, state = Pending)
            fiber.asyncValue = Succeed(promise)

        CompletePromise(promiseId, result) =>
            // State already set on the Promise object by succeed/fail.
            // Wake all waiting fibers.
            let waiters = promiseWaiters.remove(promiseId)
            if waiters is Some(fiberIds) then
                for waiterId in fiberIds do
                    let waiter = fibers.get(waiterId)
                    waiter.status = Running
                    runQueue.enqueue(waiter.id)
            fiber.asyncValue = Succeed(true)

        Uninterruptible(source) =>
            fiber.continuation.push(UninterruptibleFrame(fiber.interruptible))
            fiber.interruptible = false
            fiber.asyncValue = source
```

### 7.4 Interruption

**Cooperative model:** Interruption is only checked at **suspension points** — when a fiber is about to suspend (for a `Pollable`, `AwaitPromise`, or `Join`).

**Flow:**
1. `fiber.interrupt()` is called (from any fiber). The runtime sets `interruptPending = true` on the target fiber.
2. At the target fiber's next suspension point, the runtime checks `interruptPending`. If `true` and `interruptible` is `true`, the fiber is immediately completed with `Cause.Interrupted` instead of suspending.
3. If the fiber is inside an `Uninterruptible` region (`interruptible = false`), the interrupt is deferred. When the region exits (the `UninterruptibleFrame` is popped), `checkPendingInterrupt` fires and the fiber is interrupted.

**Interrupt propagation through Join:** When a fiber is interrupted, any fibers waiting on `Join(fiber)` receive `FailCause(Cause.Interrupted)`.

### 7.5 Event Loop

The event loop is the outermost loop of the runtime. It alternates between running fibers from the run queue and calling `Poll.wait` when all fibers are suspended.

```
function eventLoop():
    while mainFiber is not Completed do

        // 1. Run all fibers in the run queue until the queue is empty.
        while runQueue is not empty do
            let fiberId = runQueue.dequeue()
            let fiber = fibers.get(fiberId)
            // Run the fiber for some number of steps (or until it suspends/completes).
            runFiber(fiber)

        // 2. All fibers are suspended. Collect all pending pollables.
        if mainFiber is not Completed then
            let pollables = collectPollables()
            if pollables.isEmpty() then
                // No pollables and no runnable fibers. Even if fibers are waiting on
                // promises, no fiber is running to complete them — this is a deadlock.
                // Print all suspended fibers with their async stack traces so the user
                // can see where each fiber is stuck.
                for (id, fiber) in fibers do
                    if fiber.status == Suspended then
                        let trace = buildAsyncStackTrace(fiber.continuation)
                        Stderr.print("Fiber " ++ id.format() ++ " suspended at:")
                        for location in trace do
                            Stderr.print("  " ++ location.file ++ ":" ++ location.line.format() ++ " in " ++ location.functionName)
                panic("Deadlock: no runnable fibers and no pending pollables")
            else
                // 3. Block on WASI Poll.wait until at least one pollable is ready.
                let readyIndices = Poll.wait(pollables)

                // 4. For each ready pollable, call its whenReady callback and
                //    move the corresponding fiber back to the run queue.
                for index in readyIndices do
                    let (fiberId, whenReady) = pollRegistry.getByIndex(index)
                    let fiber = fibers.get(fiberId)
                    fiber.asyncValue = whenReady(())
                    fiber.status = Running
                    runQueue.enqueue(fiberId)

    // Main fiber completed. Return or handle exit.
    match mainFiber.result with
        Succeeded(()) => ()
        Failed(cause) =>
            match cause with
                Failed(error) =>
                    // E : Display, so we can format it
                    Stderr.print("Error: " ++ error.format())
                    Process.exitWithCode(1)
                Panicked(message) =>
                    Stderr.print("Panic: " ++ message)
                    Process.exitWithCode(2)
                Interrupted =>
                    Stderr.print("Main fiber interrupted")
                    Process.exitWithCode(3)
```

### 7.6 Fiber Run Batching (Trampoline)

The interpreter loop is a **trampoline** — a flat `while` loop that counts steps and yields after a bounded number. This serves two purposes:

1. **Stack safety:** Since `andThen` takes a function (`f: T => Async<U, E>`), recursive async calls are always inside a closure and deferred to the interpreter. The interpreter never recurses (see the invariant in §7.3) — it calls `f(value)`, gets back an `Async` value (data), sets it as the next `asyncValue`, and returns. The `while` loop drives the next step. The WASM call stack stays bounded to a single `step` call regardless of how deep the async recursion goes.
2. **Fairness:** No single fiber can monopolize the scheduler. After `MAX_STEPS_PER_TICK` interpreter steps, the fiber yields back to the run queue.

```
const MAX_STEPS_PER_TICK = 1024

function runFiber(fiber: FiberState):
    let steps = 0
    while steps < MAX_STEPS_PER_TICK and fiber.status == Running do
        step(fiber)
        steps = steps + 1
    if fiber.status == Running then
        // Fiber hasn't suspended or completed — put it back in the queue
        runQueue.enqueue(fiber.id)
```

### 7.7 Fiber Completion and Structured Concurrency Cleanup

When a fiber completes:

A fiber's body ending is just its **root scope** closing (§5.4), so `completeFiber`
is a thin wrapper over the one close primitive, and finalization is what hands the
fiber back to whatever scope owns it.

```
function completeFiber(fiber: FiberState, result: FiberResult):
    removeAllRegistrationsForFiber(fiber.id)
    // A body that ended normally waits for its structured fibers; one that
    // failed or was interrupted cancels them first. Background fibers are
    // cancelled either way, and both kinds are waited for.
    let abnormal = result is FiberFailed
    if closeScope(fiber, fiber.rootScope, abnormal, FinishFiber(result)) then
        ()                        // parked in Draining(rootScope)
    else
        finalizeFiber(fiber, result)

function finalizeFiber(fiber: FiberState, result: FiberResult):
    fiber.handle.setResult(result)
    fiber.status = Completed

    // 1. Wake all fibers waiting on Join(this fiber).
    for waiterId in fiber.joinWaiters do
        wakeWaiter(fibers.get(waiterId))

    // 2. This fiber's own root scope drained before we got here.
    scopes.remove(fiber.rootScope)

    // 3. Leave the scope that owns this fiber — possibly the last thing a
    //    closing scope was waiting for, which lets its release run.
    match childScope.get(fiber.id) with
        Some(scopeId) =>
            childScope.remove(fiber.id)
            leaveScope(scopeId, fiber.id)
        None => ()               // main fiber
```

`leaveScope` drops the fiber from the record and, when that empties it, resumes
the owner — but only while the owner is still `Draining` on *that* scope. An owner
interrupted out of its drain is already runnable and re-checks the (now absent)
record itself, so there is no lost wakeup and no double resume.

---

## 8. `Async.run` — Entry Point

### 8.1 Signature

```dovetail
module Async =
    function run<E>(program: Async<Unit, E>): Unit where E : Display
```

### 8.2 Semantics

`Async.run` is a **synchronous, blocking** function called from `main`. It:

1. Initializes the runtime state (fiber table, run queue, poll registry, promise waiter registry).
2. Creates the **main fiber** from `program`, adds it to the run queue.
3. Enters the event loop (§6.5).
4. When the main fiber completes:
   - **Success:** Returns `Unit` normally.
   - **Failure:** Formats the error using `Display.format`, prints to stderr, and calls `Process.exitWithCode(1)`.
   - **Panic:** Prints the panic message to stderr and calls `Process.exitWithCode(2)`.

### 8.3 Example Usage

```dovetail
function main(): Unit =
    Async.run(myApp())

async function myApp(): Async<Unit, Never> =
    let message = await fetchGreeting()
    Console.println(message)

async function fetchGreeting(): Async<String, Never> =
    await Async.sleep(Duration(1_000_000_000))  // 1 second
    "Hello, World!"
```

---

## 9. Derived Combinators

These are built from the core enum variants and do not require additional runtime support.

### 9.1 sleep

```dovetail
module Async =
    function sleep(duration: standard.wasi.clock.Duration): Async<Unit, Never> =
        let p = MonotonicClock.subscribeDuration(duration)
        Async.pollable(p, _ => Async.succeed(()))
```

### 9.2 zipPar (future)

Run two `Async` values concurrently and combine their results. Built from `fork` + `join`:

```dovetail
module Async<T, E> =
    function zipPar<U>(self, other: Async<U, E>): Async<(T, U), E> =
        async function (): Async<(T, U), E> =
            let fiber = await other.fork()
            let a = await self
            let b = await fiber.join()
            (a, b)
```

### 9.3 race (future)

Run two `Async` values concurrently, complete with the first to finish, cancel the loser. Built from `fork` + `Promise` + interruption:

```dovetail
module Async<T, E> =
    function race(self, other: Async<T, E>): Async<T, E> =
        async function (): Async<T, E> =
            let (promise, waiter) = await Async.promise<T, E>()
            let f1 = await (self.andThen(v => promise.succeed(v).map(_ => v))).fork()
            let f2 = await (other.andThen(v => promise.succeed(v).map(_ => v))).fork()
            let result = await waiter
            await f1.interrupt()
            await f2.interrupt()
            result
```

### 9.4 timeout (future)

```dovetail
module Async<T, E> =
    function timeout(self, duration: standard.wasi.clock.Duration): Async<Option<T>, E> =
        self.map(v => Some(v)).race(Async.sleep(duration).map(_ => None))
```

---

## 10. Async Stack Traces

### 10.1 Overview

Async stack traces provide meaningful error diagnostics that show the logical chain of `await` calls, not the runtime interpreter's internal stack. When a fiber fails, the trace shows exactly which async functions were on the call chain — similar to [ZIO's execution traces](https://github.com/zio/zio).

Dovetail has a significant advantage over ZIO: the compiler controls the async/await desugaring, so it injects trace information **automatically** into every desugared `andThen`/`map` call. No macros, no manual annotation, and it is impossible to accidentally lose the trace.

### 10.2 How It Works

1. **Compile time:** The compiler desugars each `await expr` into an `andThen(continuation, trace)` call, where `trace` is a `SourceLocation` capturing the file, line, and function name of the original `await` expression.

2. **Codegen:** Each unique `SourceLocation` is emitted as a **WASM-GC global constant** — one `struct.new` per unique (file, line, function) triple. At call sites, the compiler emits a `global.get` — zero allocation at runtime.

   ```wasm
   ;; One global per unique source location
   (global $trace_App_42_loadProfile (ref $SourceLocation)
     (struct.new $SourceLocation
       (string.const "App.dove")
       (i32.const 42)
       (string.const "loadProfile")))

   ;; At the await call site — just a reference, no allocation
   (call $andThen
     (local.get $expr)
     (local.get $continuation)
     (global.get $trace_App_42_loadProfile))
   ```

3. **Runtime:** When the interpreter pushes a continuation frame (for `Map`, `AndThen`, `Fold`, `FoldCause`), each frame carries the `SourceLocation` from the variant. The continuation stack naturally mirrors the logical async call chain.

4. **On failure:** When a fiber fails (typed error, panic, or interruption), the runtime walks the continuation stack, collects `SourceLocation`s from each frame, and stores them in the `Cause`'s `trace: Array<SourceLocation>` field.

### 10.3 Awaitable Trait Integration

The `Awaitable<T>` trait's `map` and `andThen` methods include a `SourceLocation` parameter:

```dovetail
trait Awaitable<T> =
    type Rebind<U>
    static function succeed(x: T): Self
    function map<U>(self, f: T => U, trace: SourceLocation): Rebind<U>
    function andThen<U>(self, f: T => Rebind<U>, trace: SourceLocation): Rebind<U>
```

The compiler fills in the `trace` argument automatically during async/await desugaring. Implementors that don't need tracing simply ignore the parameter. `Async<T, E>` stores it in the enum variant for the runtime to use.

### 10.4 Always On

Since `SourceLocation` globals exist regardless and the only runtime cost is passing one extra reference per `andThen`/`map` call, tracing is **always on** — no debug flag needed. The trace is only materialized into an `Array<SourceLocation>` when a failure occurs (the exceptional path), so the happy path has near-zero overhead.

### 10.5 Example Output

Given this code:

```dovetail
async function main(): Async<Unit, AppError> =
    let user = await loadUser(42)
    await sendEmail(user)

async function loadUser(id: Int32): Async<User, AppError> =
    let data = await fetchFromDatabase(id)
    User.parse(data)

async function fetchFromDatabase(id: Int32): Async<String, AppError> =
    Async.fail(AppError.ConnectionRefused)
```

The error output would be:

```
Error: ConnectionRefused
  at fetchFromDatabase (Database.dove:12)
  at loadUser (UserService.dove:8)
  at main (App.dove:3)
```

---

## 11. Type Aliases

For convenience:

```dovetail
type Task<T> = Async<T, Never>
```

`Task<T>` is an `Async` that cannot fail with a typed error (it can still panic or be interrupted, but those are not part of `E`).

---

## 12. Design Decisions

### 12.1 Why a free monad (data structure) instead of callbacks/continuations

The `Async<T, E>` enum describes computation as data. The runtime interprets it. This has several advantages:

- **Inspectable:** The runtime can examine the structure before running it (useful for optimization, debugging, tracing).
- **Composable:** Combinators like `fold`, `foldCause`, `fork` are just data constructors — no magic.
- **Testable:** An `Async` value can be constructed and inspected in tests without running the event loop.
- **Single interpreter:** All scheduling, interruption, and concurrency logic lives in one place (the interpreter loop), not spread across callbacks.

### 12.2 Why `Cause<E>` instead of just `E`

Separating typed errors (`E`) from panics (defects) and interruptions is essential:

- **Typed errors** are expected, part of the API contract. The caller handles them.
- **Panics** are bugs. They should not be silently caught by `catchAll` — only `foldCause` can intercept them.
- **Interruptions** are not errors — they are normal control flow in structured concurrency. `Fold` does not catch them; only `FoldCause` does.

This three-tier error model prevents accidentally swallowing panics or interruptions.

### 12.3 Why `Promise` is a mutable class

`Promise<T, E>` is a mutable GC-heap class with an `id` and a `state` field. The state (`Pending`/`Completed`) lives directly on the object rather than in a runtime map. This avoids memory leaks — WASM-GC has no finalizers, so a handle-based approach (where the runtime stores promise state in a dictionary) would leak entries since the runtime never knows when to remove them. As a GC object, the promise is reclaimed when nothing references it. The `id` field is still needed for the runtime's waiter registry (since Dovetail has no reference equality). The `succeed`/`fail` methods eagerly set `promise.state = Completed` and return `CompletePromise(promiseId, result)`, which wakes all waiters within a single interpreter step.

### 12.4 Why the interpreter loop is a trampoline (no `Suspend` needed)

A separate `Suspend` variant is unnecessary because `andThen` already takes a **function** (`f: T => Async<U, E>`). Every `await` desugars to an `andThen` call where the continuation (including any recursive call) is inside a closure. The recursive call is never evaluated eagerly — it's deferred until the interpreter pops the `AndThenFrame` and calls `f(value)`.

The interpreter loop is a flat `while` loop that counts steps. When `f(value)` returns a new `Async` value, the interpreter sets it as the next `asyncValue` and **returns** — no recursion. The `while` loop in `runFiber` drives the next step. This makes the WASM call stack depth bounded regardless of how deep the async recursion goes.

**This relies on a critical invariant: `step` must never recurse** — every branch must set `fiber.asyncValue` and return to the loop. This is harder to implement (the natural temptation is to recursively process the result of `f(value)` or handle chained operations inline), but necessary for stack safety. The step counter (`MAX_STEPS_PER_TICK`) additionally ensures fairness by yielding to other fibers after a bounded number of steps.

### 12.5 Why cooperative interruption (not preemptive)

WASM is single-threaded. There is no way to preemptively interrupt a running computation. Cooperative interruption at suspension points is the only viable model. The time-slicing in §7.6 ensures fairness between fibers, and `Uninterruptible` regions protect critical sections.

### 12.6 Why structured concurrency by default

Structured concurrency prevents fiber leaks. Without it, a forked fiber that is never joined runs indefinitely (or until it finishes), potentially holding resources. With structured concurrency:

- Forgetting to `join` is safe — the child is cancelled when the parent finishes.
- Errors in children propagate naturally through `join`.
- Long-lived background tasks attach to a scope that lives long enough, rather than escaping scopes altogether.

---

## 13. Summary

| Component | Type | Role |
|-----------|------|------|
| `Async<T, E>` | enum (free monad) | Describes an async computation |
| `Cause<E>` | enum | Full error model: typed error, panic, interruption |
| `Fiber<T, E>` | newtype (handle) | Green thread handle |
| `Promise<T, E>` | class (mutable GC object) | Single-assignment coordination primitive |
| `Awaitable<T>` | trait | Implemented by `Async<T, E>` for async/await syntax |
| `Task<T>` | type alias | `Async<T, Never>` — infallible async |
| `SourceLocation` | record | File, line, function name — for async stack traces |
| `Async.run` | function | Entry point — creates main fiber, runs event loop |

| Enum Variant | Purpose |
|-------------|---------|
| `Succeed` | Pure success value |
| `FailCause` | Failure with full `Cause` |
| `Map` | Transform success value |
| `AndThen` | Sequence computations |
| `Fold` | Handle typed errors (not panics/interruptions) |
| `FoldCause` | Handle all failures including panics/interruptions |
| `Fork` | Create child fiber |
| `Join` | Wait for fiber completion |
| `Pollable` | Suspend on WASI pollable |
| `AwaitPromise` | Suspend on promise |
| `MakePromise` | Allocate promise handle |
| `CompletePromise` | Complete promise and wake waiters |
| `Uninterruptible` | Suppress interruption |

---

## 14. Implementation Plan

| Phase | Scope | Notes |
|-------|-------|-------|
| **1** | `Cause<E>` enum, `Async<T, E>` enum (all variants), static constructors, instance methods | Pure data types — no runtime needed. Can be typechecked and unit tested for construction/composition. Depends on async/await phases 1–3 from [async-await-design](async-await-design.md). |
| **2** | `Fiber<T, E>` (newtype) and `Promise<T, E>` (class) types, method signatures | Type definitions only. |
| **3** | `Awaitable<T>` implementation for `Async<T, E>` | Connects to async/await desugaring. Depends on async-await phase 3. |
| **4** | Fiber interpreter (§6.3) — core `step` function | Interpret `Succeed`, `FailCause`, `Map`, `AndThen`, `Fold`, `FoldCause`, `Uninterruptible`. No concurrency yet — single-fiber execution. |
| **5** | `Fork`, `Join`, structured concurrency | Multi-fiber support: fiber table, run queue, parent/child tracking, completion cleanup. |
| **6** | WASI event loop (§6.5) — `Pollable` variant, `Poll.wait` integration | Fiber suspension on pollables, event loop tick, `sleep`. |
| **7** | `Promise` runtime — `MakePromise`, `AwaitPromise`, `CompletePromise` | Promise class construction, waiter registry, awaiting, completion, waking waiters. |
| **8** | Interruption — cooperative interrupt, `Uninterruptible`, pending interrupt | Interrupt requests, suspension-point checking, deferred interrupts. |
| **9** | `Async.run` entry point, time-slicing | Full runtime integration. End-to-end test: `main` calling `Async.run` with real async I/O. |
| **10** | Derived combinators — `sleep`, `zipPar`, `race`, `timeout` | Built on primitives. May require `Cause` extension for parallel failures. |
| **11** | Async stack traces — `SourceLocation`, trace on `Cause`, codegen for globals | `SourceLocation` record type. Compiler emits `SourceLocation` as WASM-GC globals. Async/await desugar pass injects `trace` argument into every generated `andThen`/`map` call. Runtime collects `SourceLocation`s from continuation frames on failure and stores them in `Cause`. `Async.run` prints the trace on error. Update `Awaitable` trait to include `SourceLocation` param on `map`/`andThen`. |

**Dependencies:** 1 → 2 → 3; 1 → 4 → 5 → 6 → 7 → 8 → 9 → 10; 9 → 11 (tracing requires the full runtime to be in place).

---

## 15. References

- [ZIO](https://github.com/zio/zio) — Inspiration for the effect type and fiber model.
- [async-await-design.md](async-await-design.md) — Async/await syntax, `Awaitable` trait, desugaring.
- [io-library-design.md](io-library-design.md) — WASI low-level I/O: Pollable, InputStream, OutputStream, TCP, etc.
- [traits-design.md](traits-design.md) — Trait system, impl blocks, Self.
- [railway-early-return-design.md](railway-early-return-design.md) — Early return and interaction with async.
