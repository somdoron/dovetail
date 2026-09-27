# Resources and streams: lifetime checks

Read [Resources](https://dovetaillang.org/book/resources.md), [Streams](https://dovetaillang.org/book/streams.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- `use` acquires a resource and scopes its lifetime to the enclosing block's
  remainder. Cleanup is LIFO across success, failure, panic, and interruption.
  Returning a raw handle does not extend that lifetime.
- Scoped fibers finish/unwind before resources close. Use `forkBackground` for a
  pump whose termination depends on closure; otherwise scope completion can deadlock.
- Acquisition/release are uninterruptible by default. Keep finalizers bounded;
  making a wait interruptible must preserve the atomic transfer of ownership.
- `use Async.scope()` opens a scope. `await resource.attachToScope()` acquires into
  the existing scope; it is not `use ...attachToScope()`. Consider whether a helper
  unintentionally extends ownership to its caller or the root fiber.
- Compose Resource descriptions for reusable acquisition, and make error conversion
  explicit. Query actual signatures before mixing Resource and Async operations.
- A stream is a reusable description. A terminal operation creates Async, which
  still needs execution. Reconsumption cannot rewind a shared external socket.
- Bound infinite streams and memory use: `runCollect` retains everything. Prefer
  folds/writes for large input and retain incomplete protocol data across chunks.
- Early termination must release scopes and stop producers. Preserve cancellation
  and defects when recovering typed errors. Finish consumption before closing the
  underlying byte transport.
