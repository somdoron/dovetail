# Errors and async

## Expected absence and failure

`Option<T>` is `Some(T)`/`None`; `Result<T, E>` is `Ok(T)`/`Error(E)`.
Use `map` to transform successes, `andThen` to chain fallible computations,
`mapError` to translate errors, `or` for a fallback, `toResult(error)` to give
absence a reason, and `toOption()` only when intentionally losing error information.

`try expression` and `expression.orReturn` unwrap success or return failure from
the enclosing function. Option requires an Option return; Result requires a
compatible Result error channel. These are not exception handling. `expect`,
`.require`, `panic`, and `assert` signal defects/invariants, not routine bad input.

## Deferred computation

Depend on `standard-io` and import `standard.io.Async`. `Async<T, E>` describes
work; `Task<T>` aliases `Async<T, Never>`. Construction does not execute it.

```dovetail
async function doubleLater(value: Int32): Async<Int32, Never> =
    let result = await Async.succeed(value * 2)
    result
```

Async functions/closures require an explicit return annotation. Their bodies
return the success value, not another Async. `await` is prefix and propagates
failure. Use `await Async.fail(error)` to fail. `async do` expressions defer their
whole body; an await-free body needs an expected Awaitable type. Running again
reexecutes the body with fresh locals; mutable captures remain shared.

`Async.run(program)` or `program.run()` drives work from synchronous
`function main(): Unit`. Distinguish typed failures, `Cause.Panicked`, and
`Cause.Interrupted`; do not accidentally turn defects into ordinary business errors.
Generalized async uses `Awaitable`, including associated `Rebind` and deferred
`defer(body: ByName<Self>, trace: SourceLocation): Self` semantics.

`while` conditions cannot contain await/use. A loop whose body suspends with
await/use cannot use break/continue to leave that iteration. Synchronous nested
loops retain ordinary controls. Each async expression has its own early-return
and loop context.

## Concurrency

Sequential awaits run sequentially. `await task.fork()` starts a scoped fiber;
`await fiber.join()` waits for it. Forks belong to the innermost use scope or the
current fiber's root scope.

| Spawn | Normal scope completion | Failure/interruption |
|---|---|---|
| `fork` | Wait for completion | Interrupt, then wait for cleanup |
| `forkBackground` | Interrupt, then wait for cleanup | Interrupt, then wait for cleanup |

Use background fibers for pumps/pollers that exist only while a resource remains
open. A plain fork that waits for resource closure can deadlock scope completion.
No unscoped escape is provided. Cancellation is cooperative; critical sections
can be uninterruptible, but keep them bounded.

## Checked deferred execution

<!-- book-example: {"name": "aieffects", "depends": ["standard-io"], "stdout": ""} -->
```dovetail
package aieffects

import standard.io.Async

async function calculate(): Async<Int32, Never> =
    let left = await Async.succeed(20).fork()
    let right = await Async.succeed(22).fork()
    let first = await left.join()
    let second = await right.join()
    first + second

async function verify(): Async<Unit, Never> =
    assert (await calculate()) == 42

function main(): Unit = verify().run()

test "scoped fibers finish" = main()
```
