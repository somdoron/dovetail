# Runtime producers

Producer is a public, demand-driven child execution. It replaces the proposed
retained-scope operations. The standard-io-stream interpreter uses it for execution
lifetimes; merge workers exchange ordered events through Rendezvous.

## API

The public API is `Producer.make`, `producer.next()`, and
`producer.interrupt()`. Construction and fiber metadata are internal to standard.io;
the Async nodes and runtime records remain internal. The logical operations are:

```text
CreateProducer<S, A, E>(
    acquire: Async<S, E>,
    pull: S => Async<A, Option<E>>,
    release: S => Async<Unit, Never>
): Async<Producer<A, E>, E>

Pull<A, E>(producer): Async<A, Option<E>>
InterruptProducer<A, E>(producer): Async<Unit, Never>
```

`Producer<A, E>` stores its producer and creator fiber IDs. Acquired state stays
in the runtime. `Producer.make<S>` supplies erased argument adapters and a typed
handle factory, like Fork, preserving concrete handle/state/result types.
`next` and `interrupt` construct the nodes. All Async subclasses live in
`standard-io/src/types.dove`.

Creation acquires once on a background child fiber owned by the caller's current
scope. Its ordinary bracket stays active around successive pulls. Creation returns
only after release ownership is armed. Acquisition keeps existing bracket masking,
including its limitations for unbounded waits. No pull is eager.

Only the creating fiber may pull or explicitly interrupt a live producer.
Lookup uses the fiber ID and ClassIdentity equality with the stored handle.
Missing or nonmatching handles behave as closed. Reusing a creation description
creates independent executions.

## Lifecycle

ProducerState holds the fixed `pull` function and a phase:
Acquiring, Idle(state), Pulling(state), Closing(cleanupFailure), Closed, or
Failed(cause, cleanupFailure). Closed and Failed are terminal after cleanup.
Acquired state exists only in Idle and Pulling and is threaded between them.
The bracket retains its own state for release during cleanup. Its scope is recorded
before acquisition begins, so attachment cleanup is observed even if acquisition
fails or is cancelled before publication. Cleanup failures also live in the phase,
not in a separate field.

Idle children park without unwinding their bracket. A pull callback, including
construction of its Async description, executes on the producer fiber. Its forks
and attached resources therefore belong to the producer's scope.

An item is handed directly to the waiting caller; the producer returns to Idle.
There is no stored ready item, prefetch, participant list, selection scan, or
completion sequence. The next pull starts only on the owner's next demand.

EOF is Cause.Failed(None), converted to successful bracket completion before
unwind so normal structured children are awaited. Errors and interruption unwind
abnormally. Terminal results reach the caller only after children, attachments,
and release finish. Observed terminal failures are forgotten; later pulls return
EOF. Explicit interruption and parent closure also stop idle producers.

## Single-owner handoff

Each operation has one request referencing one producer. The producer holds one
optional waiting request. The runtime also tracks the caller's request for
cancellation and diagnostics, and a caller continuation retains cleanup ownership.

Pull starts the child and parks the owner. Completion checks the active waiting
request, detaches it, writes Succeed(item) into the caller's asyncValue, and wakes
the caller. It does not wake the caller to scan or retry selection. Acquisition
and terminal outcomes use the same guarded notification. The existing run queue
and fairness budget schedule fibers; this does not introduce direct fiber switching.

The cleanup frame remains armed after notification until the caller processes the
result. Cancellation in this window closes the producer before caller recovery.
Cancellation before notification detaches the request, so late completion cannot
overwrite the caller's cancellation continuation. A value abandoned by cancellation
is not saved for another pull.

Shutdown is represented by a ProducerShutdownFrame on the parent, containing the
request and original failure to restore. ProducerRequest contains no closing flag
or saved failure. Explicit interruption installs the shutdown frame directly;
failure of a ProducerRequestFrame replaces that operation with shutdown.

A ProducerAwaitFrame waits for the child's terminal phase under a hard mask.
ProducerShutdownFrame runs after that mask exits, so it can preserve a cleanup
panic even when cancellation was deferred during shutdown. Both frames belong to
the parent; the saved original failure is held only by ProducerShutdownFrame.
Every child notification follows the same delivery path; only the parent's frames
decides whether to return an item or continue waiting for termination. If a pull
recovers interruption and returns an item, shutdown interrupts the now-idle child
again instead of leaving both fibers parked. A cleanup
panic takes precedence over the original failure saved in the parent frame. Expected child
interruption is suppressed by InterruptProducer; cleanup panics remain observable.
When shutdown forwards a child cleanup panic through another producer's active
pull, its value continuation retains the cleanup origin until the pull unwinds.
A continuation keeps that origin while a cause handler runs. A panic from the
handler replaces the pending cleanup panic; successful recovery discards the
origin before later continuations run. Handled failures do not mark later pulls
or unrelated body defects as cleanup failures.
Repeated interruption after closure succeeds. Resource finalizer signatures and
ordinary bracket semantics are unchanged.

## Bookkeeping and validation

Leaving the active phases drops their acquired-state reference; completed
executions clear the pull function. Unobserved
terminal failures remain owned by their scope until observed or the scope closes.
Successful exhausted records are removed promptly. Scope records retain producer
registrations even after the last producer fiber finishes. Diagnostics distinguish
acquisition, pull, and cleanup waits.

Tests cover lazy and repeated execution, EOF, acquisition/pull failures, owner and
handle identity validation, structured/background children, nested producers,
parent closure, cleanup-panic precedence, tuple state, and ReadonlySlice results.
Step-driven tests exercise cancellation during acquisition and between direct
notification and caller continuation, recovered interruption during shutdown, and
attachment cleanup before acquisition publication, and cancellation during
already-running shutdown. Repeated execution checks ensure runtime
bookkeeping is released. Run Producer, Runtime, and Rendezvous regressions,
then the full standard-io suite and workspace check.


## Stream integration

The stream interpreter uses the public `Producer.make/next/interrupt` API.
Stream-specific instructions and binding continuations stay in standard-io-stream;
there are no stream Async subclasses. A session can request nested interpretation
on its producer fiber so inner producers are children of the scope owning the
current output. This preserves resource/fork nesting through composition.

A scope closing an unobserved producer also reports its recorded cleanup panic.
Observed producers have already been removed, so cleanup is not reported twice;
ordinary child body failures keep their existing join-only behavior. This matters
when an idle producer is canceled with a nested producer still holding resources.
