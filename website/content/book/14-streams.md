# Part 14: Streams

An `Async<T, E>` describes one asynchronous result. A `Stream<T, E>` describes a sequence of values of type `T`, which can end normally or fail with an error of type `E`. The sequence can be finite, or it can keep producing values until its consumer stops.

Streams belong to the `standard-io-stream` project. Add it to your project's `depends` list and import `standard.io.stream.Stream`. The examples below also use `standard.io.Async`; examples involving time use `standard.time.Duration`.

---

## 14.1 Descriptions and Consumption

Like an async computation or a resource, a stream is a reusable description. Constructing a stream does not start its source. A terminal operation such as `runCollect` produces an `Async` value; running that value starts consumption.

```dovetail
import standard.io.Async
import standard.io.stream.Stream

async function streamExample(): Async<Unit, Never> =
    let numbers = Stream<Int32, Never>.fromList([1, 2, 3, 4])
        .filter((number: Int32) => number % 2 == 0)
        .map((number: Int32) => number * 10)

    assert (await numbers.runCollect()) == [20, 40]
    assert (await numbers.runCollect()) == [20, 40]

function main(): Unit = streamExample().run()
```

Each consumption gets fresh execution state. Reusing a description does not rewind an external queue, restore a file position, or snapshot an array. Those objects keep their own state.

Source constructors use by-name arguments where evaluating the argument belongs to execution. For example, `Stream.succeed(nextValue())` evaluates `nextValue()` when that source is reached during consumption. You do not need to pass a thunk. The same applies to the initial state of an unfold and the second stream in `concat`.

Both type parameters are covariant: a `Stream<Uint8, Never>` can be used as a `Stream<Any, Never>`. This widening preserves the original values and their types.

---

## 14.2 Creating a Source

Use a constructor that matches where the values come from:

| Constructor | Source behavior |
| --- | --- |
| `Stream<T, E>.empty` | Ends without emitting a value; this is a property. |
| `succeed(value)` | Emits one value. |
| `fromList(values)` | Emits the elements of a list in order. |
| `fromChunk(chunk)` | Emits a `ReadonlySlice<T>`. |
| `fromAsync(effect)` | Runs an `Async<T, E>` once and emits its result. |
| `fromQueue(queue)` | Takes values from a standard-io `Queue<T>`, waiting when it is empty. |
| `fail(error)` | Fails with a typed error. |

A queue source has no end-of-stream marker. It continues taking values until it fails, is cancelled, or a downstream operation such as `take` stops demand. `fromQueue` currently requires `T: Default`, as the queue API does. Every consumption uses the queue supplied to it, so consuming the same description twice can receive different values.

### Unfolding state

An unfold constructs the next item and the next state together. Returning `None` ends a pure unfold.

```dovetail
async function unfoldExample(): Async<Unit, Never> =
    let numbers = Stream<Int32, Never>.unfold(0, (state: Int32) =>
        if state < 4 then Some((state, state + 1))
        else None
    )
    assert (await numbers.runCollect()) == [0, 1, 2, 3]
```

The step for `unfold<S>` has type `S => Option<(T, S)>`. Its chunk counterpart, `unfoldChunk<S>`, returns `Option<(ReadonlySlice<T>, S)>`.

For asynchronous unfolds, normal completion travels through the optional error channel:

```dovetail
async function asyncUnfoldExample(): Async<Unit, String> =
    let numbers = Stream<Int32, String>.unfoldAsync(0, async (state: Int32) =>
        if state < 3 then (state, state + 1)
        else await Async<(Int32, Int32), Option<String>>.fail(None)
    )
    assert (await numbers.runCollect()) == [0, 1, 2]
```

The step for `unfoldAsync<S>` returns `Async<(T, S), Option<E>>`. Success emits an item and installs the next state, failure with `None` ends the stream normally, and failure with `Some(error)` fails the stream with `error`. `unfoldChunkAsync<S>` follows the same convention with a `ReadonlySlice<T>` in place of the item.

---

## 14.3 Chunks and Transformations

Streams transport values in chunks. A chunk is a `ReadonlySlice<T>`: a bounded view over array storage. Read-only access does not freeze the array. Writes through another alias remain visible, so a source must not overwrite a chunk while its consumer still needs those values. Use `toArray().readonly` when an independent copy is needed.

`map` and `filter` work with individual values while processing their input chunks. `mapChunks` and `mapChunksAsync` give the callback a whole slice. A chunk callback may return a different number of values, including none. Empty chunks are skipped.

```dovetail
async function chunkExample(): Async<Unit, Never> =
    let source = Stream<Int32, Never>.fromChunk([|1, 2, 3, 4|].readonly)
    let selected = source.mapChunks((chunk: ReadonlySlice<Int32>) => chunk.drop(1))
    assert (await selected.runCollect()) == [2, 3, 4]
```

### Effectful operations work one item at a time

`mapAsync` and `filterAsync` break chunks into single items before applying their effect. This makes downstream demand control how many item effects run.

```dovetail
async function itemEffectsExample(): Async<Unit, Never> =
    let mutable calls = 0
    let values = Stream<Int32, Never>.fromChunk([|1, 2, 3|].readonly)
        .mapAsync(async (value: Int32) =>
            calls = calls + 1
            value * 10
        )
        .take(1)

    assert (await values.runCollect()) == [10]
    assert calls == 1
```

Likewise, `filterAsync(predicate).take(1)` runs the predicate only until the first accepted item. A whole-chunk operation behaves differently: `mapChunksAsync(f).take(1)` runs `f` for the entire first input chunk it reaches. Pure `map` also processes a whole input chunk before returning it downstream.

`take(count)` limits the number of emitted items; `drop(count)` skips items, crossing chunk boundaries when necessary. `rechunk(size)` combines or splits input chunks into chunks of the requested positive size, with a possibly smaller final chunk. Rechunking preserves item order but copies into new backing arrays.

---

## 14.4 Concatenation and Flat Mapping

`concat` consumes its first stream completely before evaluating and consuming the second. If the first stream fails, the second is not started.

`flatMap` replaces each item with another stream. It consumes that inner stream completely before moving to the next outer item. This is sequential composition.

```dovetail
async function compositionExample(): Async<Unit, Never> =
    let expanded = Stream<Int32, Never>.fromList([1, 2])
        .flatMap((value: Int32) =>
            Stream<Int32, Never>.fromList([value, value * 10])
        )
        .concat(Stream<Int32, Never>.succeed(99))

    assert (await expanded.runCollect()) == [1, 10, 2, 20, 99]
```

`repeat()` restarts a stream after normal completion. It reuses the description with fresh execution state each time. An error stops repetition. Repeating an empty stream produces no items; use a source that can make progress when downstream is waiting for a value.

---

## 14.5 Repeating Effects and Time

Repeating an effect is useful for polling or reading from a source whose next value is obtained asynchronously.

| Constructor | Effect type and completion |
| --- | --- |
| `repeatAsync(effect)` | Repeats `Async<T, E>` until failure or cancellation. |
| `repeatAsyncChunk(effect)` | Repeats `Async<ReadonlySlice<T>, E>`. |
| `repeatAsyncOption(effect)` | Repeats `Async<T, Option<E>>`; `None` ends normally. |
| `repeatAsyncChunkOption(effect)` | The same completion convention for chunk effects. |
| `repeatAsyncWithInterval(effect, interval)` | Runs immediately, then waits the duration before each subsequent execution. |
| `Stream<Unit, E>.tick(interval)` | Waits before every tick and emits `Unit`. |

These sources produce values on demand. They do not accumulate missed ticks or run a polling effect independently of consumption. An interval is a delay between executions, not a fixed wall-clock schedule.

```dovetail
import standard.time.Duration

async function ticksExample(): Async<Unit, Never> =
    let ticks = Stream<Unit, Never>.tick(Duration.ofMillis(1i64))
    assert (await ticks.take(3).runCollect()).length == 3
```

An infinite stream needs a stopping condition when consumed by a terminal operation that waits for completion. `take` provides an item limit; ordinary async cancellation and timeout operations apply to the resulting `Async` as well.

---

## 14.6 Errors and Resource Lifetimes

Normal completion and typed failure are different outcomes. `mapError` transforms a typed error. `catchAll` replaces a failed stream with a recovery stream; values already emitted remain before the recovery values.

```dovetail
async function recoveryExample(): Async<Unit, Never> =
    let source = Stream<Int32, String>.succeed(1)
        .concat(Stream<Int32, String>.fail("unavailable"))
        .catchAll((_: String) => Stream<Int32, Never>.succeed(9))

    assert (await source.runCollect()) == [1, 9]
```

`catchAll` handles the typed error channel, not defects or interruption. Failures and cancellation propagate through the consuming `Async`, which still runs cleanup.

### Acquiring resources inside a stream

`fromResource(resource)` acquires a `Resource<T, E>` and emits its value. Its lifetime covers the downstream stream work in that scope. A common pattern is to acquire a handle and `flatMap` into the stream that reads it. `bracket(acquire, release)` is the equivalent constructor when you have acquisition and release effects directly.

```dovetail
async function lifetimeExample(): Async<Unit, Never> =
    let mutable releases = 0
    let source = Stream<Int32, Never>.bracket(
        Async.succeed(10),
        async (_: Int32) =>
            releases = releases + 1
    ).flatMap((value: Int32) =>
        Stream<Int32, Never>.fromList([value, value + 1])
    )

    assert (await source.take(1).runCollect()) == [10]
    assert releases == 1
```

Acquisition happens when consumption reaches the resource. Release runs when its stream scope finishes, including on early termination, failure, and cancellation. An inner stream's resources close before sequential `flatMap` advances to the next outer item. Fibers forked while interpreting that stream belong to its scope, following the structured-concurrency rules from [Async Programming](12-async.md) and [Resource Management](13-resources.md).

`ensuring(finalizer)` attaches a finalizer to a stream without emitting a resource value. The finalizer is also evaluated as part of execution.

---

## 14.7 Merging Concurrent Sources

`merge(other)` consumes two sources concurrently; `Stream<T, E>.mergeAll(streams)` does the same for a list of sources. Each source preserves its own order, while order between sources depends on which one produces the next chunk.

```dovetail
async function mergeExample(): Async<Unit, Never> =
    let left = Stream<Int32, Never>.fromList([1, 2])
    let right = Stream<Int32, Never>.fromList([3, 4])
    let total = await left.merge(right).fold(0, (sum: Int32, value: Int32) => sum + value)
    assert total == 10
```

Merge completes normally once all its sources finish. A source failure is delivered through the same handoff as chunks, so it does not jump ahead of chunks already handed off to the consumer. When the merge fails or downstream stops early, the remaining producers are interrupted and their resources are released.

The handoff applies backpressure: a producer waits for the consumer to accept its chunk. Acceptance does not mean downstream has finished processing every item in it. The producer may continue, finish, and close its resources while the consumer is processing that chunk. Emit values whose use does not depend on the producer remaining open. In particular, do not treat a resource handle passed through a merge as if it retained the sequential `flatMap` lifetime guarantee.

---

## 14.8 Running and Folding

Terminal operations turn the stream description into an async computation:

| Operation | Result |
| --- | --- |
| `runCollect()` | `Async<List<T>, E>`, retaining all emitted items in order. |
| `fold(initial, combine)` | `Async<S, E>`, accumulating one state with a pure `(S, T) => S` function. |
| `foreach(effect)` | `Async<Unit, E>`, awaiting an effect for each item in order. |
| `foreachChunk(effect)` | `Async<Unit, E>`, awaiting an effect for each chunk. |
| `drain()` | `Async<Unit, E>`, consuming values without retaining them. |

The initial state passed to `fold` is by-name and is evaluated afresh for each execution. Folding can keep memory bounded when the accumulator itself has bounded size. `runCollect` needs memory for the entire result and waits for the stream to end, so reserve it for finite streams of an appropriate size.

Use `await` to consume a stream inside a larger async program, or `.run()` at the application boundary, just as in [Part 12](12-async.md).

---

## 14.9 Byte Transports

Files and TCP connections expose `AsyncInputStream<E>` and `AsyncOutputStream<E>` handles. Adapt a borrowed input with `Stream<Uint8, E>.fromInputStream(input, chunkSize)`. The chunk size is a required, positive `Int64`; `65536i64` requests up to 64 KiB per read. Short reads produce chunks normally, and an empty read ends the stream.

For output, `Stream.writeToOutputStream(output)` accepts an `AsyncOutputStream<E>` destination. Its `where T: Uint8, E: IoError` bounds make it available on byte streams. It awaits each chunk write before asking for another chunk. Both adapters use the handles' natural I/O error type.

Neither adapter closes a handle, and `writeToOutputStream` does not flush. The code that owns the resource decides when those operations happen. An empty source performs no writes.

### Copying a file

```dovetail
import standard.io.Async
import standard.io.fs.File
import standard.io.fs.FileStreamError
import standard.io.fs.Path
import standard.io.stream.Stream
import standard.wasi.fs.FileSystemError

async function copyFile(source: Path, destination: Path): Async<Unit, FileStreamError> =
    let input = use File.openInputStream(source)
    let output = use File.openOutputStream(destination)
    await Stream<Uint8, FileSystemError>.fromInputStream(input, 65536i64).writeToOutputStream(output).mapError(FileStreamError.from)
    await output.close().mapError(FileStreamError.from)
```

The resource errors are `FileStreamError`, while reads and writes report `FileSystemError`. The filesystem library provides `From<FileSystemError>` for `FileStreamError`, used explicitly with `mapError` to convert transport errors.

The explicit `close()` observes late output failures that may arrive after the host accepts the bytes. Resource cleanup still owns releasing the handles on every exit path.

### Owning a source through composition

A stream that should open a fresh file for every consumption can compose the resource constructor:

```dovetail
function fileBytes(path: Path): Stream<Uint8, FileStreamError> =
    Stream.fromResource(File.openInputStream(path))
        .flatMap(input =>
            Stream<Uint8, FileSystemError>.fromInputStream(input, 65536i64)
                .mapError(FileStreamError.from)
        )
```

This preserves the file's lifetime through sequential downstream processing, and closes it on completion, failure, cancellation, or early termination. Reusing a borrowed handle instead continues from that handle's current position; stopping partway through a chunk does not put its unread items back into the transport.

TCP uses the same pattern, except a connection resource owns both directions together:

```dovetail
import standard.io.net.Tcp
import standard.io.net.NetError
import standard.wasi.net.NetworkError

async function echoOnce(address: String): Async<Unit, NetError> =
    let listener = use Tcp.bind(address)
    let connection = use listener.accept()
    await Stream<Uint8, NetworkError>.fromInputStream(connection.input, 65536i64)
        .writeToOutputStream(connection.output).mapError(NetError.from)
    await connection.output.close().mapError(NetError.from)
```

The connection stays alive while its input and output are used together. The existing conversion from `NetworkError` to `NetError` is used with `mapError` for transport errors.

### Readonly output windows

`AsyncOutputStream.write` accepts `ReadonlySlice<Uint8>`. Use `array.readonly` or `slice.readonly` for existing buffers. A stream chunk, including an interior window, passes directly to the output adapter without an intermediate array copy. WASI still copies the selected bytes into host memory, and TLS builds the owned record buffer required for encryption.
