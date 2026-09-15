# Part 12: Async Programming

Dovetail models asynchronous work as **values**, not as a hidden runtime feature bolted onto regular functions. An asynchronous computation is a description of work to be performed — it doesn't run until something explicitly drives it.

This chapter covers the `Async` type, how to write async functions, the `await` operator, how to run an async program from `main`, and how to compose work in parallel.

---

## 12.1 The Async Type

`Async<T, E>` describes a computation that, when run, eventually produces either a value of type `T` or fails with an error of type `E`.

```dovetail
let greeting: Async<String, Never> = Async.succeed("hello")
let broken: Async<Int32, String> = Async.fail("not found")
```

The two type parameters are independent. `T` is the success type; `E` is the typed error channel. A computation that cannot fail uses `Never` for `E` — this is common enough to have a dedicated alias:

```dovetail
type Task<T> = Async<T, Never>
```

`Async` values are inert. Constructing one does no work — it merely builds a description. Work only happens when something runs the value (see 12.4).

### Constructing Async Values

The two primitive constructors are `Async.succeed` and `Async.fail`:

```dovetail
let ok: Async<Int32, Never> = Async.succeed(42)
let bad: Async<Int32, String> = Async.fail("oops")
```

Most async values, however, come from async functions (12.2) or from library calls such as `Async.sleep`, file I/O, or network operations.

### Errors, Panics, and Interruption

A failure in an async program is more than just a typed error. The runtime distinguishes three kinds of failure, captured by `Cause<E>`:

- **`Failed(e)`** — a typed error of type `E` (the same channel as `Async.fail`)
- **`Panicked(message)`** — an assertion or other panic from inside the computation
- **`Interrupted`** — the fiber was cancelled (see 12.5)

Most code only cares about typed failures; the other variants surface through panic handlers and structured-concurrency cancellation.

---

## 12.2 Async Functions

An async function is declared with the `async` keyword before `function`. Inside its body, `await` is allowed.

```dovetail
async function fetchGreeting(): Async<String, Never> =
    await Async.succeed("Hello, World!")
```

The body of an async function returns the **success type** (`T`), not an `Async<T, E>` value. To take a value out of an existing async value, use `await`; to propagate a typed failure, `await Async.fail(...)`.

**The return type is required.** The compiler does not infer the return type of an async function — you must write `: Async<T, E>` explicitly.

```dovetail
async function add(a: Int32, b: Int32): Async<Int32, Never> =
    await Async.succeed(a + b)

async function lookup(key: String): Async<String, NotFound> =
    if key == "answer" then
        await Async.succeed("42")
    else
        await Async.fail(NotFound(key))
```

### Async Closures

`async` may also be applied to lambdas. The return type is still required:

```dovetail
let f: (Int32) => Async<Int32, Never> =
    async (x: Int32): Async<Int32, Never> => await Async.succeed(x * 2)
```

### Async Expressions

Use `async do` to construct a deferred computation directly inside an expression:

```dovetail
let program = async do
    let first = await Async.succeed(20)
    let second = await Async.succeed(22)
    first + second
```

The entire body is deferred, including ordinary statements before the first
`await`. Its final expression supplies the success value. Running the value
again executes the body again, with fresh local variables. Captures follow
closure rules: immutable bindings are copied and mutable bindings are shared.
Constructing the value does not start a fiber.

The type is inferred from the awaited values using their `Awaitable.Rebind`
implementations. `Async` is one such type; `Resource` and custom Awaitable types
work the same way. Awaits inside nested async expressions or closures do not
determine the enclosing expression's type. Incompatible computation contexts
require an explicit annotation or a conversion.

Without an await, provide the Awaitable type through context:

```dovetail
let program: Async<Int32, Never> = async do 42

function makeProgram(): Async<Int32, Never> = async do 42
```

A function parameter can also supply that context. There is no default
Awaitable type for `async do 42` without context.

An async expression has its own early-return and await context. `orReturn`
exits that computation, and loops inside it cannot break or continue an outer
loop. A final expression that itself produces an Awaitable remains a nested
value; use `await` to obtain its result.

Currently, `while` conditions cannot contain `await` or `use`, and an async loop
whose body contains either cannot use `break` or `continue` to exit that iteration.
Synchronous loops inside an async expression support ordinary loop control.

Custom implementations must provide `Awaitable.defer(body: ByName<Self>,
trace: SourceLocation): Self`. This operation stores the by-name value without
evaluating it, then evaluates it and drives its result on each execution.

---

## 12.3 The `await` Operator

`await` is a **prefix** operator — it goes in front of the expression being awaited, not after it. It is only legal inside an async function, async closure, or `async do` expression.

```dovetail
async function greetTwice(): Async<Unit, Never> =
    let first: String = await fetchGreeting()
    let second: String = await fetchGreeting()
    Console.println(first)
    Console.println(second)
```

### Awaiting in Control Flow

`await` composes with `if`, `match`, and `while`:

```dovetail
async function pickGreeting(formal: Bool): Async<String, Never> =
    if formal then
        await fetchGreeting()
    else
        await Async.succeed("yo")

async function countUp(n: Int32, sink: (Int32) => Unit): Async<Unit, Never> =
    let mutable i: Int32 = 0
    while i < n do
        let v: Int32 = await Async.succeed(i + 1)
        sink(v)
        i = i + 1
```

### Restrictions

- `await` outside an async function, async closure, or `async do` expression is a compile error.

---

## 12.4 Running Async Code

`main` is a regular synchronous function returning `Unit`. To launch async work, call `Async.run` with the program:

```dovetail
function main(): Unit =
    Async.run(app())

async function app(): Async<Unit, Never> =
    let message: String = await fetchGreeting()
    Console.println(message)
```

`Async.run`:

- Spins up the runtime and drives the event loop until the program finishes.
- Returns normally when the program succeeds with `Unit`.
- On a typed failure or panic, prints the error to stderr and calls `Process.exit` with an error code.

### Allocation Happens When the Program Runs

`Mutex.make`, `Async.makePromise`, and `Queue.make` return descriptions that allocate fresh state when executed. Running the same constructor program twice creates two objects. Internally, allocation uses `Async.thunk`, even though constructing a class or a waiter itself needs no scheduler instruction.

```dovetail
async function freshPromises(): Async<Unit, Never> =
    let make: Async<Promise<Int32, Never>, Never> = Async.makePromise()
    let first = await make
    let second = await make
    assert !ClassIdentity.equals(first, second)
```

By contrast, constructing one object and putting it in `Async.succeed` captures that existing object for every execution. Preserve this distinction when building reusable programs. Process-wide mutexes use a separate internal constructor for the library's global streams.

### Useful Library Calls

The standard `Async` API in `standard-io` provides the common building blocks:

```dovetail
async function delayed(): Async<Unit, Never> =
    await Async.sleep(Duration.ofMillis(500i64))
    Console.println("half a second later")
```

---

## 12.5 Parallel Execution

Two async values run sequentially when you `await` them in order. To run work concurrently, **fork** it.

### Fork and Join

`fork` schedules an async value as a child fiber and returns immediately with a handle. `join` waits for that fiber to finish.

```dovetail
async function inParallel(): Async<Int32, Never> =
    let a: Fiber<Int32, Never> = await slowAdd(1, 2).fork()
    let b: Fiber<Int32, Never> = await slowAdd(3, 4).fork()
    let x: Int32 = await a.join()
    let y: Int32 = await b.join()
    await Async.succeed(x + y)
```

Both fibers run concurrently. The two `join` calls wait for whichever fiber needs more time; the second `join` returns immediately if its fiber already finished.

### Structured Concurrency

Every fiber you fork belongs to a **scope**: the innermost enclosing `use` block, or the whole body of the fiber that forked it when there is no `use`. A scope does not finish — a `use` does not release its resource — until every fiber in it is gone. That is what stops a resource from being closed underneath a fiber still reading from it.

What "gone" means depends on how the scope ends and how you forked:

| | scope ends normally | scope ends by failure or interruption |
|---|---|---|
| `fork` | waits for the fiber | interrupts it, then waits |
| `forkBackground` | interrupts it, then waits | interrupts it, then waits |

```dovetail
async function serveAndWait(): Async<Unit, NetError> =
    let listener: TcpListener = use Tcp.bind("127.0.0.1:8080")
    // Structured: the `use` block will not release the listener until this
    // fiber has finished, so it can never read from a closed listener.
    let handler: Fiber<Unit, Never> = await handleOne(listener).fork()
    await handler.join()
```

Use `fork` for work the scope should finish, and `forkBackground` for work that is only meaningful while the scope lives — a poller, a pump, a loop that ends only when the resource it reads from is closed. That last case is the one to watch: a fiber that stops *only* when the resource closes must not be a plain `fork`, or the scope will wait for the fiber while the fiber waits for the scope. The runtime detects that and panics naming both fibers rather than hanging silently.

Note that even a background fiber is waited for once it has been interrupted — the scope waits for it to finish *unwinding*, so its own finalizers always run before the resource is released.

There is deliberately no third option that escapes scopes altogether. A fiber that outlives the resource it was reading from is exactly the hazard scopes exist to prevent, and every use for one turned out to be better served by forking it from a scope that lives long enough — usually the caller's, one level further out.

### Interruption

`Fiber.interrupt` requests cancellation of a fiber. Interruption surfaces inside the fiber as `Cause.Interrupted`. Code that needs to be uncancellable for a critical section can wrap it in `Async.uninterruptible`.

---

## Summary

- `Async<T, E>` is a value describing future work; building one does no work.
- `async function f(): Async<T, E> = ...` declares an async function; the return type is mandatory.
- `await e` is a prefix operator that yields the success value of an async value inside an async body.
- `Async.run(program)` from a synchronous `main` drives the runtime to completion.
- `fork` plus `join` runs work concurrently; structured concurrency means a scope waits for the fibers forked inside it, and `forkBackground` marks the ones it should cancel instead.
