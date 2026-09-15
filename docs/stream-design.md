# Streams

`standard-io-stream` provides `standard.io.stream.Stream<out T, out E>`. It depends on
`standard-io` and `standard-collection`. A Stream is an immutable, reusable
sealed class hierarchy; each consumption allocates fresh execution state. Element
and error parameters are covariant.

## API

Sources:

```dovetail
property empty: Stream<T, E>
succeed(value: ByName<T>): Stream<T, E>
fail(error: ByName<E>): Stream<T, E>
fromChunk(chunk: ByName<ReadonlySlice<T>>): Stream<T, E>
fromList(values: ByName<List<T>>): Stream<T, E>
fromAsync(effect: ByName<Async<T, E>>): Stream<T, E>
fromQueue(queue: ByName<Queue<T>>): Stream<T, E> where T: Default
repeatAsync(effect: ByName<Async<T, E>>): Stream<T, E>
repeatAsyncChunk(effect: ByName<Async<ReadonlySlice<T>, E>>): Stream<T, E>
repeatAsyncOption(effect: ByName<Async<T, Option<E>>>): Stream<T, E>
repeatAsyncChunkOption(effect: ByName<Async<ReadonlySlice<T>, Option<E>>>): Stream<T, E>
repeatAsyncWithInterval(effect: ByName<Async<T, E>>, interval: Duration): Stream<T, E>
tick(interval: Duration): Stream<Unit, E>
unfold<S>(initial: ByName<S>, step: S => Option<(T, S)>): Stream<T, E>
unfoldChunk<S>(initial: ByName<S>, step: S => Option<(ReadonlySlice<T>, S)>): Stream<T, E>
unfoldAsync<S>(initial: ByName<S>, step: S => Async<(T, S), Option<E>>): Stream<T, E>
fromResource(resource: ByName<Resource<T, E>>): Stream<T, E>
bracket(acquire: ByName<Async<T, E>>, release: T => Async<Unit, Never>): Stream<T, E>
unfoldChunkAsync<S>(
    initial: ByName<S>,
    step: S => Async<(ReadonlySlice<T>, S), Option<E>>
): Stream<T, E>
```

`unfoldChunkAsync` signals EOF with `Async.fail(None)` and a typed failure with
`Async.fail(Some(error))`.

The Option repeat constructors and `unfoldAsync` use the same error-channel EOF
convention. Ordinary repeat constructors continue until failure or downstream
termination; their effect expressions are evaluated again on each pull. Chunk
variants preserve chunk boundaries and skip empty chunks. Pure unfolds stop
when their step returns `None`, with fresh initial state for each execution.
`repeatAsyncWithInterval` runs the first effect immediately and sleeps before
each subsequent effect, once downstream requests it. `tick` sleeps before every
Unit emission, including the first. These are delays, not a fixed-rate schedule.

`fromQueue` evaluates its queue argument once per execution and takes one item
per pull. An empty queue waits for an offer; it does not signal EOF. Consumption
stops through downstream termination or cancellation, leaving the queue usable.

`bracket` emits the acquired value once. Compose its body with `flatMap`.
`fromList` materializes one chunk during consumption. Sources and initializer
factories are lazy, including when a terminal Async is reused. Source arguments,
concat/merge inputs, and finalizers are taken by name.
Concat evaluates its right-hand expression only after the left stream finishes;
recursive descriptions therefore construct the right-hand side only when needed.
Callbacks remain functions, and counts and chunk sizes remain ordinary values.

Transformations are `map`, `filter`, `mapAsync`, `filterAsync`, `mapChunks`,
and `mapChunksAsync`. `StreamTransform` stores the chunk callback directly.
Stateful accumulation is deferred to future transducer support.

Composition is `take`, `drop`, `concat`, `flatMap`, `repeat`, and `rechunk`.
`take(n <= 0)` does not start upstream; `drop(n <= 0)` is identity. `repeat` is
indefinite, even for an empty stream, and yields between empty executions.
`rechunk` requires a positive size and flushes a nonempty tail at EOF.

Error and cleanup operations are `mapError`, `catchAll`, and `ensuring`.
`catchAll` handles typed errors only, preserving interruption and panic causes.
Concurrency is `merge(other)` and `mergeAll(List<Stream<T, E>>)`. An empty list
ends immediately; all inputs start on first demand, with no concurrency limit.

Consumers:

```dovetail
fold<S>(initial: ByName<S>, f: (S, T) => S): Async<S, E>
runCollect(): Async<List<T>, E>
drain(): Async<Unit, E>
foreach(f: T => Async<Unit, E>): Async<Unit, E>
foreachChunk(f: ReadonlySlice<T> => Async<Unit, E>): Async<Unit, E>
```

`fold` reduces elements in order, evaluating its initial state once per execution.
Its executor yields periodically while traversing chunks. `runCollect` uses
`fold` to accumulate a reversed list, then calls the list's `reverse` function.
List-source materialization also yields during its copying pass.
Consumers own an ordinary scope and explicitly close their producer on success or failure.
Cancellation also closes the scope, including during startup handoff.

## Chunks and demand

Chunks are existing read-only slice views, not snapshots. Reusing a description
does not snapshot external arrays, rewind I/O, or reset shared queues. Ordinary
transport and slicing preserve backing storage and offsets, including byte arrays.
Empty chunks are skipped.

Every effectful per-item transformation emits singleton chunks. Consequently
`mapAsync(f).take(1)` invokes `f` once, and `filterAsync(f).take(1)` stops at the
first accepted item. Explicit chunk operations process whole chunks and may
change their length. Pure map/filter preserve chunk boundaries apart from empty
results. The interpreter and library item loops yield after bounded work (256
steps) to allow other fibers and cancellation to progress. User-supplied pure
callbacks must themselves terminate.

The erased interpreter transports `ReadonlySlice<Any>` directly. Covariance
preserves the backing array and window without wrapper objects, reader closures,
or element copies. Primitive reads through erased views retain their original
boxed identity; statically typed reads remain native. Rechunk assembles elements
using a yielding typed adapter that fills the output array backwards from its
reversed pending list, without intermediate list copies. No `Default<T>`
constraint is imposed.

## Execution and ownership

Stream is a sealed abstract class with data-only final subclasses. There is no
separate instruction tree, abstract execution method, or per-operator advance
callback. `StreamExecution.advance` matches the current stream subclass directly.
`StreamExecutionState` holds the current description and progress: source state,
upstream execution, active child session, buffered chunk and element index.
Subclass source references and output slices retain their generic types.
Callback boundaries erase hidden input/state types; typed chunk callbacks recover
their slice view without copying elements.
Async continuations trampoline traversal, so deeply nested pure transformations
do not recurse on the host stack. Pure operators share an execution fiber.
Sources retain their state only in the execution, not in the blueprint.
`StreamFromChunk<T, E>` evaluates one chunk-producing Async and emits once. It handles
`fromChunk`, `succeed`, `fromList`, and `fromAsync` without unfolding state or an
EOF error. List materialization retains its cooperative yields. Concat and merge
hold factories for their lazy inputs; there is no separate deferred instruction.
After concat closes its first branch, its second branch runs in that same execution,
so lazy recursive right-hand tails do not retain a chain of completed execution states
and producer scopes.

An internal `StreamSession` pairs a runtime `Producer` with interpreter state.
The producer owns the execution and its ordinary bracket scope across
pulls. Embedded effects run on that fiber, so their forks and attached resources
have normal structured ownership. The fixed pull callback advances the current stream to
one nonempty chunk or EOF; there are no new stream-specific Async nodes.

`fromResource` calls public `Resource.attachToScope()` on the producing fiber.
`bracket` uses `Resource.make` and `fromResource`; `ensuring` attaches a resource
with a finalizer. There is no interpreter finalizer stack and no change to
ordinary scope finalizers. `Resource.acquire` remains internal. An embedded
Async's own lexical brackets remain lexical; they are not inspected by Stream.

Retained branches use child producers:

- `flatMap` retains its outer execution, an outer chunk/index, and at most one
  active inner execution at that level. Session requests route inner interpretation
  to the fiber owning the current output. That fiber creates the inner producer,
  making it a child of the outer resource scope, including through concat/take
  and nested flatMaps. Binding frames distinguish owned inner sessions from
  forwarded requests; binding EOF resumes the outer without closing it.
- `concat`, `repeat`, and `catchAll` finish a branch's cleanup before starting its
  successor, discarding completed handles.
- `take` isolates upstream. Once its limit is reached, it returns EOF; the
  session's ordinary scope shutdown stops the retained upstream producer.

A sequentially delivered chunk keeps its producer alive until the next demand
or cancellation. Early close calls `Producer.interrupt()` and awaits cleanup.
EOF and pull failures leave teardown to the producer's ordinary scope and
release callback, preserving the distinction between body and cleanup failures.
Scope shutdown closes child executions before enclosing resource attachments.
The release callback clears retained execution handles and continues after cleanup
defects. Unobserved producer cleanup panics
are included in that cleanup; ordinary child body failures remain join-only.
The interpreter owns no resource finalizers and requires no new Async nodes.

## Merge

Merge uses one shared FIFO `Rendezvous<MergeEvent>`:

```dovetail
enum MergeEvent =
    Chunk(ReadonlySlice<Any>)
    End
    Failed(Cause<Any>)
```

Each worker consumes its input and offers chunks sequentially. It publishes End
or Failed only after source cleanup. All events share the same rendezvous:
failures do not overtake queued chunks, and there is no separate error promise.
The merge consumer counts End events and ends only after all sources end. The
first dequeued failure stops the merge and awaits worker cleanup before failing.

An offer completes when its event is taken. The worker may immediately advance,
including closing the producing stream while downstream processes the delivered
chunk. This is the intended asynchronous lifetime boundary: source resources
cover production, not downstream processing. There is no acknowledgement after
downstream processing and no automatic copy of mutable backing storage.

The merge lifecycle is itself a Producer, created before any worker. Workers and
that lifecycle producer are siblings owned by the enclosing execution scope.
Each fork and handle registration is masked together. Lifecycle release sets a
stopping flag, signals **all** workers, then joins **all** workers, continuing
through cleanup failures. `Fiber.interrupt()` only signals; joining is explicit.

Workers catch source failure before terminal publication. Publication is outside
that handler so interruption of a blocked offer cannot become a replacement
offer. Workers skip terminal publication when stopping has begun. Scope
cancellation can initially interrupt a worker while it is consuming its source;
lifecycle release signals it again to cancel any intervening terminal offer.
The sibling topology lets release cancel those offers without first waiting for
workers to drain inside its own bracket. A pending failure publication may be
discarded by early termination; it never bypasses queued chunks. Cleanup panics
observed while stopping propagate through worker joins after all workers have
been signaled.

## Runtime and compiler support

Producer's API is public while its constructors, identity fields, and execution
nodes remain internal. The runtime's scope shutdown preserves unobserved producer
cleanup panics, including nested producers canceled while idle. Scope finalizer
signatures and child-before-attachment cleanup order are unchanged.

The scheduler returns to the event loop after a batch of 16 runnable turns when
host operations are pending. Each host-event batch delivers at most 64 completions:
if the runnable queue is empty, wait for the first event, then poll until no event
is ready or the shared event budget is exhausted. Otherwise start with polling.
Every fourth host-event batch explicitly yields before its first wait or poll.
This yield counter persists across batches and resets only on an explicit yield;
a wait does not reset it because it may return immediately. These budgets are
scheduling heuristics, not measured throughput optima.

`WaitableSet.poll` and `yieldToHost` use the existing component-model concurrency
builtins. Poll only checks ready events; yielding allows host tasks to advance.

Await lowering now sequences effectful match subjects and if conditions before
selecting a branch, in addition to lowering effects inside branch bodies.


## Byte transport adapters

`Stream<Uint8, E>.fromInputStream(input: AsyncInputStream<E>, chunkSize: Int64)`
borrows the input handle and requires `E: IoError`. The positive chunk size is
checked at construction. A private async helper reads one array per demand and
returns its readonly view; an empty read becomes the optional-error EOF used by
`repeatAsyncChunkOption`. There is no new execution node and no resource overload.

`Stream.writeToOutputStream(output: AsyncOutputStream<E>): Async<Unit, E>` is a module
method constrained by `where T: Uint8, E: IoError`. Primitive subtype bounds
allow its readonly chunks to pass directly to the byte output. It uses `foreachChunk` and awaits
each write before advancing. It performs no implicit flush or close, including
on failure; an empty source makes no output calls.

Both adapters keep natural transport errors. Resource acquisition can use a
different error type: filesystem code now exports `From<FileSystemError>` for
`FileStreamError`, alongside the existing networking conversion. Compose
`fromResource(...).flatMap(...)` and map the inner stream's errors as needed.
Explicit output close belongs in the owning async scope when late failures
must be reported.

The output contract, runtime write node, WASI component callback, and write
intrinsics accept readonly byte views throughout. Partial writes advance the
view without copying. WASM lowering casts the opaque backing reference to the
Uint8 array type and transfers only the selected window to pinned host memory.
TLS accepts readonly application data and copies directly into its required
owned plaintext record, avoiding an intermediate input array.
