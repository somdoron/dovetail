# Effects: execution and ownership

Read [Error handling](https://dovetaillang.org/book/error-handling.md), [Async](https://dovetaillang.org/book/async.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- Use typed Result errors for expected rejection, Option for absence, and explicit
  conversion when information is lost. `.require`, `panic`, and `assert` indicate
  defects/invariants rather than ordinary bad input.
- `try` / `.orReturn` propagate through the enclosing compatible result channel;
  they are not exception handling.
- Constructing Async does not execute it. Async bodies return the success value,
  `await` propagates failure, and the synchronous entry point needs a runner.
  Explicit discard acknowledges intent but does not execute the work.
- Re-running deferred work reexecutes it with fresh locals; captured mutable state
  remains shared. Do not confuse a stored result with a repeatable effect.
- Preserve typed failures, panics, and interruption as distinct outcomes.
- Sequential awaits are sequential. A scoped `fork` waits on normal scope exit;
  `forkBackground` interrupts then waits. Both interrupt on scope failure.
- A worker waiting for resource closure must use the appropriate background policy
  or it can deadlock scope completion. Handles cannot outlive their owning scope.
- Cancellation is cooperative. Keep uninterruptible critical sections bounded.
- Await/use in loops have control-flow restrictions: consult the chapter before
  putting them in a while condition or suspending an iteration with break/continue.
