# Part 13: Resource Management

External resources — file handles, network sockets, database connections, locks — need to be released at a precise, known point. Dovetail provides the `use` keyword for that: it binds acquisition and release together so the release always happens when control leaves the scope, whether the scope completes normally, returns early, or fails.

---

## 13.1 The `use` Keyword

`use` is a prefix operator for acquiring a resource inside an async function. It binds the acquired value to a name and guarantees the resource is released when the surrounding scope finishes.

```dovetail
async function copyFile(src: String, dst: String): Async<Unit, IoError> =
    let input = use File.open(src)
    let output = use File.create(dst)
    let bytes: Array<Uint8> = await input.readAll()
    await output.writeAll(bytes)
```

The usual form is `let <name> = use <expression>`, though the binding is optional — `use <expression>` on a line of its own opens the scope and discards the value, which is how `Async.scope()` below is written. The resource expression is evaluated first; the resulting value is bound to the name; everything that follows in the enclosing block runs with the resource alive; when that follow-up finishes, the resource is released.

In `copyFile` above, both files are closed automatically when the function finishes — and they are closed even if `readAll` or `writeAll` fails partway through.

The resources in this part are all asynchronous — external resources almost always have asynchronous acquire and release steps, and `use` is designed around that. `use` itself is not limited to them: the `Usable` trait it desugars through has a synchronous form too, used for things like a secret that must be zeroized after use.

### Fibers forked inside a `use` block

A `use` block is also a **fiber scope**: any fiber forked inside it belongs to the block, and the resource is not released until that fiber is gone. So a fiber cannot end up reading from a socket the block already closed.

```dovetail
async function serveOne(): Async<Unit, NetError> =
    let listener: TcpListener = use Tcp.bind("127.0.0.1:8080")
    // The block will not release the listener until this fiber has finished.
    let worker: Fiber<Unit, Never> = await acceptOne(listener).fork()
    await worker.join()
```

A block that ends normally waits for the fibers forked with `fork`; one that ends by failure interrupts them first. Fibers forked with `forkBackground` are interrupted either way — use that for work that is only meaningful while the resource is open, such as a loop that ends only when the resource closes. See [Part 12](12-async.md) for the full comparison.

### Acquisition runs to completion too

Acquiring is **uninterruptible**. A resource becomes real somewhere inside the acquire — the moment the host hands back a socket, say — and the runtime cannot see when; it can only take ownership once the acquire returns. An interrupt landing in between would leave a live resource with nothing left that could close it, so interrupts are held until the acquire finishes and then delivered.

The trade is that an acquisition which parks indefinitely can no longer be cut short. Keep acquisitions bounded and do the waiting inside the block, which stays interruptible. When an acquisition *is* the wait — accepting a connection, taking a lock — it opts back in for that part alone with `.interruptible()`, wrapping the park and never the hand-off, so the fiber can still be cancelled while it waits but the resource still reaches its owner atomically.

### Release runs to completion

Releasing is **uninterruptible**. A finalizer that awaits — a close that flushes, a TLS shutdown that sends `close_notify` — cannot be cut in half by an interrupt arriving part-way through, which would leave the resource neither open nor closed. An interrupt that arrives during a release is deferred and delivered the moment the release finishes, so it is never lost.

The trade is that a finalizer which blocks forever now blocks its fiber forever. Keep release paths bounded: prefer a best-effort close over one that waits indefinitely for a peer.

---

## 13.2 LIFO Release Order

Multiple `use` bindings in the same block release in **last-in, first-out** order, matching the natural nesting:

```dovetail
async function copy(src: String, dst: String): Async<Unit, IoError> =
    let input = use File.open(src)    // released second
    let output = use File.create(dst) // released first
    await pipe(input, output)
```

This matches how you would write the cleanup by hand: outer resources outlive inner ones.

---

## 13.3 Release on Failure

The release step runs whether the body succeeds, fails with a typed error, or panics. There is no `finally` block to remember and no `try`/`catch` to thread around the work — the keyword itself binds acquisition and release together.

```dovetail
async function process(path: String): Async<Result, ProcessError> =
    let file = use File.open(path)
    await analyze(file)   // even if this fails, the file is closed
```

This is the main reason to reach for `use`: it eliminates the entire class of "I forgot to close that on the error path" bugs.

---

## 13.4 Scopes Without a Resource

A `use` block does two things at once: it opens a scope, and it attaches a resource to it. `Async.scope()` does only the first — it is a resource that acquires nothing, so `use` on it opens a scope and nothing else.

```dovetail
async function handleOne(conn: TcpStream): Async<Unit, NetError> =
    use Async.scope()
    ...
```

Write it bare, with no binding: a scope is never a value, so there would be nothing worth naming. The scope covers **the rest of the enclosing block**, exactly like any other `use`, because it is one. To bound something narrower, give it a block of its own — a loop body, an `if` arm, or a helper function.

What a scope bounds is the two things that attach to one: fibers forked with `fork` / `forkBackground`, and finalizers registered with `attachToScope`. Without a scope of their own, both fall back to the fiber's root scope and live as long as the fiber.

---

## 13.5 Attaching to a Scope You Already Have

`attachToScope` is the other half of `use`. It acquires a resource and hands its release to the innermost open scope, instead of opening a block of its own:

```dovetail
async function serve(certPath: Path, keyPath: Path): Async<Unit, ServerError> =
    use Async.scope()
    let cert = await Certificate.load(certPath).attachToScope()
    let key = await ServerKey.load(keyPath).attachToScope()
    await listen(cert, key)
    // key released, then cert, at the end of the scope
```

Reach for it when the resource's lifetime is a scope that already exists and the acquisition should not swallow the rest of the block as its continuation: a resource acquired in one arm of an `if`, a helper acquiring on its caller's behalf, a loop acquiring a number of resources known only at runtime. When the lifetime *is* the rest of the block, `use` says so more directly.

Note the spelling: always `await`, never `use`. A `use` on it would open a new scope and release at the end of the block, which is exactly what it exists not to do.

### Which scope it attaches to

Whichever is innermost when it runs — the same rule `fork` follows. A scope is never a value, so there is nothing to pass and nothing to pass wrongly. That scope may be an `Async.scope()`, or an ordinary `use` block's, or, if neither is open, the fiber's own root scope, in which case the resource is released when the fiber ends.

A plain `use` block is a perfectly good attach target, and it has to be. Everything releases in **reverse acquisition order**, whichever spelling acquired it:

```dovetail
let socket = use Tcp.connect(host, port)
let file = await File.openInputStream(path).attachToScope()
// file closed first, then socket
```

If the attach could skip the enclosing `use` and land further out, the file would be closed after the socket it was read into — a resource outliving what it was derived from.

### The one thing to watch

An `attachToScope` with no scope of its own lands on the fiber's root scope. That never dangles, and for a server that forks a fiber per request it is exactly right. But a long-running loop that attaches once per turn holds every one of them until the fiber ends. Give each turn a scope and it releases before the next acquires:

```dovetail
while more() do
    await handleOne()          // whose own body opens `use Async.scope()`
```

---

## 13.6 Composing Resources

The `Resource<T, E>` type is itself awaitable. An async function that **returns** a `Resource<T, E>` can use `await` to combine other resources into a single composite resource. Nothing is acquired by the function call itself — what comes back is a description of the combined acquire/release lifecycle, ready to be handed to a `use` site.

```dovetail
async function dbTransaction(): Resource<Transaction, DbError> =
    let conn = await openConnection()
    let txn = await Transaction.begin(conn)
    txn
```

`openConnection` and `Transaction.begin` each return a `Resource<...>`. Inside `dbTransaction`, `await` threads the acquired values through the body, and the function returns a single `Resource<Transaction, DbError>`. The caller never sees the connection — only the transaction it cares about — but the connection is still acquired and, crucially, still released. When the surrounding `use` scope ends, the transaction is closed first and then the connection underneath it, LIFO.

A caller uses that composite resource as if it were any other:

```dovetail
async function runMigration(steps: Array<Migration>): Async<Unit, DbError> =
    let txn = use dbTransaction()
    await applyAll(steps, txn)
```

This is the main reason to write a function that returns a `Resource` rather than `use`-ing one inline: you can package a multi-step setup — connection plus transaction, listener plus accepted socket, lock plus loaded snapshot — into a single named resource and hand it around, exposing only the value the caller actually needs. The LIFO release contract carries across the composition boundary, so callers never have to know how many pieces the resource is made of.

---

## 13.7 Error Conversion at the Use Site

If the resource's failure type differs from the surrounding function's error type, the compiler will convert it automatically — as long as a `From` instance exists between them:

```dovetail
newtype FileError = String
newtype AppError = String

implement From<FileError> for AppError =
    public function from(e: FileError): AppError = AppError("io: " ++ e.value)

async function loadConfig(path: String): Async<Config, AppError> =
    let file = use openConfigFile(path)   // raises FileError, converted via From
    await parseConfig(file)
```

No explicit `.mapError(...)` is needed at the `use` site — the conversion is wired in for you.

---

## Summary

- `use` acquires a resource inside an async function and guarantees release when the enclosing scope exits — on success, failure, or panic.
- A `use` block is a fiber scope: fibers forked inside it finish (or are cancelled) before the resource is released.
- Acquisition and release are both uninterruptible by default: an interrupt arriving during either is deferred until it completes, so a resource is never stranded half-owned or left half-closed. An acquisition that *is* a wait opts its park back in with `.interruptible()`; a release never can.
- Syntax is `let x = use expr`, or bare `use expr` when the value is not wanted; multiple `use` bindings in a block release in LIFO order.
- `use Async.scope()` opens a scope that acquires nothing, for bounding forked fibers and attached resources.
- `await r.attachToScope()` acquires `r` and releases it when the innermost open scope closes, rather than opening a scope of its own. Everything releases in reverse acquisition order, whichever spelling acquired it.
- An async function returning `Resource<T, E>` can use `await` to compose other resources into a single composite resource.
- `From` instances are used automatically to convert a resource's error type into the surrounding function's error type.

For helpers that accept an unknown resource type through a `Usable` bound, see
[Generic Resource Helpers](26-advanced-generics.md#generic-resource-helpers).
