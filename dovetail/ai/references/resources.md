# Resources and streams

## Ownership scopes

`Resource<T, E>` couples acquisition and release. In async code use
`let value = use resource` or bare `use resource` if the acquired value is unwanted.
The scope covers the remaining enclosing block. Nested resources release in reverse
acquisition order on success, typed failure, panic, and interruption. `use` also
supports synchronous Usable implementations; it is not limited to external I/O.

The resource remains alive until scoped fibers finish/unwind. Use
`forkBackground` for a fiber whose termination depends on resource closure.
Acquisition/release are uninterruptible by default to avoid half-owned resources.
Keep finalizers bounded. Waiting acquisitions may opt only their interruptible wait
back into cancellation; preserve the atomic ownership hand-off.

`use Async.scope()` opens a scope without acquiring an external handle.
`await resource.attachToScope()` acquires into the currently active scope without
opening a new scope; do not spell it `use ...attachToScope()`. If no explicit scope
exists it attaches to the root fiber. Consider whether a helper is extending a
resource's lifetime to its caller. Acquisitions made this way and via use share
reverse acquisition release order.

An `async function acquire(): Resource<T, E>` can `await` other Resource values
to compose acquisition/release, return the acquired success value, and defer the
whole lifecycle until a caller uses it. Cleanup remains LIFO across composition.
Compose Resource descriptions when building a reusable acquisition. Error conversion
at a use site may use a suitable `From` implementation; make the conversion contract
explicit and do not suppress meaningful acquisition failures. Returning a raw handle
from inside its owning scope does not extend its lifetime.

## Streams

Depend on `standard-io-stream`; import `standard.io.stream.Stream` and the Async
API. `Stream<T, E>` is a reusable description. A terminal operation creates Async;
you must execute that Async. Reconsumption creates fresh stream state, but cannot
rewind an external socket or other shared input.

```dovetail
let values = Stream<Int32, Never>.fromList([1, 2, 3, 4])
    .filter((value: Int32) => value % 2 == 0)
    .map((value: Int32) => value * 10)
let collected = await values.runCollect()
```

Choose finite sources, effectful/repeating sources, or byte transports to match
the task. Transform with map/filter; flatten with flatMap; concatenate for ordered
consumption and merge for concurrent sources. Inspect exact signatures in the
installed dependency before choosing chunked/effectful variants.

Stream processing uses chunks internally; avoid unnecessarily flattening/copying
chunks. Early termination must release the source scope and interrupt background
producers. Recover typed errors deliberately and preserve defects/cancellation.
Repeating a timed effect requires execution each iteration, not merely reusing an
already obtained value. Bound consumption of infinite streams and memory usage;
runCollect retains all output. Prefer folds or streaming writes for large data.

Byte input/output uses `Uint8` streams and the library's transport adapters.
Output operations such as `writeToOutputStream` have byte-specific bounds. Resource
ownership still applies across all adapters; the consumer must finish before the
underlying transport closes.

## Checked resource-scoped stream

<!-- book-example: {"name": "aistreams", "depends": ["standard-io", "standard-io-stream"], "stdout": ""} -->
```dovetail
package aistreams

import standard.io.Async
import standard.io.stream.Stream

async function verify(): Async<Unit, Never> =
    use Async.scope()
    let values = Stream<Int32, Never>.fromList([1, 2, 3, 4])
        .filter((value: Int32) => value % 2 == 0)
        .map((value: Int32) => value * 10)
    assert (await values.runCollect()) == [20, 40]

function main(): Unit = verify().run()

test "consumes a stream in a scope" = main()
```
