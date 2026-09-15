mod common;

#[test]
fn generic_newtype_use_preserves_block_and_method_type_arguments() {
    common::compile_and_run(
        r#"
package a
newtype Holder<T> = T
implement <T> Usable<T, Never> for Holder<T> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder<T>, f: (T) => U, errorF: (Never) => E2): U = f(self.value)
function number(): Int32 =
    let value = use Holder("ok")
    assert value == "ok"
    42
function text(): String =
    let value = use Holder(7)
    assert value == 7
    "done"
function main(): Unit =
    assert number() == 42
    assert text() == "done"
"#,
    )
    .unwrap();
}

// Tests for the `use` prefix expression. Some only exercise the parser +
// typechecker (via `dovetail::check`); others run end-to-end via `compile_and_run`.

fn check_ok(source: &str) {
    let result = dovetail::check(source, "test.dove");
    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!(
            "expected typecheck to succeed; got errors:\n  {}",
            errors.join("\n  ")
        );
    }
}

fn check_err_contains(source: &str, needle: &str) {
    let result = dovetail::check(source, "test.dove");
    let messages: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    let combined = messages.join("\n");
    assert!(
        combined.contains(needle),
        "expected error containing '{needle}'; got:\n{combined}"
    );
}

#[test]
fn test_parse_use_in_let_binding() {
    check_ok(
        r#"
package a

record Holder = value: Int32

implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let h = Holder { value = 42 }
    let x: Int32 = use h
    assert x == 42
"#,
    );
}

#[test]
fn test_parse_use_in_sub_expression() {
    check_ok(
        r#"
package a

record A = value: Int32

implement Usable<Int32, Never> for A =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: A, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let a = A { value = 3 }
    let b = A { value = 4 }
    let total: Int32 = (use a) + (use b)
    assert total == 7
"#,
    );
}

#[test]
fn test_infer_use_type_is_resource_t() {
    // The type of `use expr` must match the trait's T parameter.
    // Here T = String — using the result as an Int32 should fail.
    check_err_contains(
        r#"
package a

record S = value: String

implement Usable<String, Never> for S =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: S, f: (String) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let s = S { value = "hi" }
    let n: Int32 = use s
    ()
"#,
        "expected 'Int32'",
    );
}

#[test]
fn test_error_operand_not_usable() {
    check_err_contains(
        r#"
package a

record Plain = value: Int32

function main(): Unit =
    let p = Plain { value = 1 }
    let x = use p
    ()
"#,
        "does not implement Usable",
    );
}

// ── Phase 3 end-to-end tests (desugar_use enables compile_and_run) ─────

#[test]
fn test_sync_use_round_trip() {
    common::compile_and_run(
        r#"
package a

record Holder = value: Int32

implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let h = Holder { value = 42 }
    let x: Int32 = use h
    assert x == 43 - 1
"#,
    )
    .expect("sync use round-trip");
}

#[test]
fn test_use_in_tail_position() {
    // `use h` is the entire body of main — identity continuation desugar.
    common::compile_and_run(
        r#"
package a

record Holder = value: Int32

implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function bare(h: Holder): Int32 = use h

function main(): Unit =
    let h = Holder { value = 7 }
    assert bare(h) == 7
"#,
    )
    .expect("tail use returns the resource");
}

#[test]
fn test_two_uses_lifo_order() {
    // Two `use` in one block. We verify the resource values are both visible
    // in the inner continuation (i.e. nested closures are wired correctly).
    common::compile_and_run(
        r#"
package a

record H = value: Int32

implement Usable<Int32, Never> for H =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: H, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let a = H { value = 3 }
    let b = H { value = 4 }
    let x: Int32 = use a
    let y: Int32 = use b
    assert x + y == 7
"#,
    )
    .expect("two uses, both bound");
}

#[test]
fn test_use_in_sub_expression_e2e() {
    // `(use a) + (use b)` — extraction depth-first.
    common::compile_and_run(
        r#"
package a

record H = value: Int32

implement Usable<Int32, Never> for H =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: H, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let a = H { value = 3 }
    let b = H { value = 4 }
    let total: Int32 = (use a) + (use b)
    assert total == 7
"#,
    )
    .expect("expression-level use");
}

// Phase 6 — sync Usable impls with non-trivial acquire/release semantics.
// Verifies Wrapped<U, E2> = U (sync) passes side effects through correctly
// without an async runtime in the loop.

#[test]
fn test_sync_usable_scoped_counter() {
    common::compile_and_run(
        r#"
package a

record Counter = value: Int32

implement Usable<Int32, Never> for Counter =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Counter, f: (Int32) => U, _errorF: (Never) => E2): U =
        let result = f(self.value)
        result

function main(): Unit =
    let c1 = Counter { value = 5 }
    let c2 = Counter { value = 10 }
    let total: Int32 =
        let a = use c1
        let b = use c2
        a * 2 + b
    assert total == 20
"#,
    )
    .expect("sync usable with scoped counter");
}

#[test]
fn test_use_missing_from_impl_errors() {
    // E_resource = Int32, E_block = String. No From<Int32> for String impl
    // exists → compile error at the `use` site.
    check_err_contains(
        r#"
package a

record R = value: Int32

implement Usable<Int32, Int32> for R =
    type Wrapped<U, E2> = Result<U, E2>
    function use<U, E2>(self: R, f: (Int32) => Result<U, E2>, errorF: (Int32) => E2): Result<U, E2> =
        f(self.value)

function main(): Unit =
    let r = R { value = 1 }
    let p: Result<Int32, String> =
        let v = use r
        Ok(v + 1)
    ()
"#,
        "requires a 'From<Int32> for String' impl",
    );
}

#[test]
fn test_sync_usable_two_uses_in_block() {
    common::compile_and_run(
        r#"
package a

record Token = name: Int32

implement Usable<Int32, Never> for Token =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Token, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.name)

function main(): Unit =
    let t1 = Token { name = 1 }
    let t2 = Token { name = 2 }
    let combined: Int32 =
        let a = use t1
        let b = use t2
        a * 10 + b
    assert combined == 12
"#,
    )
    .expect("sync usable two uses in block");
}

// An async `use` block whose continuation ends in an `await` of a computation
// that never succeeds (`Async<Never, E>`). The desugar has to recognise that the
// `Usable.use(...)` call is ALREADY the function's Async result even though its
// success type is the narrower `Never`; wrapping it in `succeed` a second time
// fed the call itself to `succeed` and produced a module that failed wasm
// validation ("expected i32, found (ref ...)").
#[test]
fn test_async_use_with_never_typed_tail() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

function alwaysFails(): Async<Never, Int32> = Async.Fail(7i32)

async function withHandle(): Async<Unit, Int32> =
    let _id: Int32 = use Handle { id = 1 }
    await alwaysFails()

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(e) => assert e == 7i32
    case Async.Succeed(_) => panic "expected the failing tail to propagate"
"#,
    )
    .expect("async use with a Never-typed tail");
}

// A `use` in a `let` whose value starts on the line AFTER the `=` — the layout
// filter wraps the value in a `Block([Use])`. The desugar must peel that
// wrapper and lift the rest of the enclosing function as the continuation,
// exactly as it does for the single-line spelling. Under the bug, the inner
// block's tail-`use` path fired instead: it bound the raw `Usable.use(...)`
// call (an `Async` under `Wrapped = Async`) to a variable typed as the inner
// value, which failed wasm validation at codegen with a struct type mismatch.
// A `use` in tail position of a MULTI-statement `let` value block. Unlike the
// singleton case above, the peel cannot fire (the preceding statements have
// their own scope), so under the bug the inner block's tail-`use` path emitted
// an identity continuation typed `(U) => U` while the impl's `use` (with
// `Wrapped = Async`) actually returns `Async<U, E>` — the component failed
// wasm validation ("expected i32, found (ref ...)"). The fix hoists the
// prefix + operand into a temp binding and lifts the REST of the enclosing
// block as the continuation.
#[test]
fn test_async_use_multistatement_let_value() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function withHandle(): Async<Int32, Int32> =
    let id: Int32 =
        assert true
        use Handle { id = 41 }
    id + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Succeed(x) => assert x == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("multi-statement let-value use lifts the enclosing continuation");
}

// Same shape, but the prefix statement's binding feeds the `use` operand —
// verifies the prefix statements keep their own scope and stay visible to the
// operand expression after the desugar restructures the block.
#[test]
fn test_async_use_multistatement_let_value_prefix_feeds_operand() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function withHandle(): Async<Int32, Int32> =
    let id: Int32 =
        let base = 40
        use Handle { id = base + 1 }
    id + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Succeed(x) => assert x == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("prefix binding stays in scope for the use operand");
}

// Sync (`Wrapped<U, E2> = U`) variant of the multi-statement let value —
// regression coverage for the restructured desugar in the synchronous case.
#[test]
fn test_sync_use_multistatement_let_value() {
    common::compile_and_run(
        r#"
package a

record Holder = value: Int32

implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let x: Int32 =
        let base = 40
        use Holder { value = base + 2 }
    assert x == 42
"#,
    )
    .expect("sync multi-statement let-value use");
}

// A tail `use` as the LAST statement of an async function body (with preceding
// statements). The identity-continuation path must type the `Usable.use` call
// as `Async<T, E>` (via an AsyncBlock-wrapped identity continuation), not as
// the bare inner `T`.
#[test]
fn test_async_use_tail_statement_of_function() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function tailUse(): Async<Int32, Int32> =
    let h = Handle { id = 7 }
    use h

function main(): Unit =
    match (tailUse()).evaluate() with
    case Async.Succeed(x) => assert x == 7
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("tail use as an async function's final statement");
}

// An async function declared `Async<Animal, E>` whose body ends in a
// use-desugared `Usable.use(...)` call typed with the continuation's own
// NARROWER success type (`Async<Dog, E>`). `Async<out T, out E>` is covariant,
// so the call is already the function's Async result; under the bug
// `widens_to` only accepted `Never` or exact equality, so the call got a
// second `succeed` wrap (`Async<Async<Dog, E>, ...>` into an `Animal` slot)
// and the component failed wasm validation.
#[test]
fn test_async_use_covariant_narrower_continuation_tail() {
    common::compile_and_run_async(
        r#"
package a

class Animal(public name: String)

class Dog(public breed: String) extends Animal("Rex")

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function makeAnimal(): Async<Animal, Int32> =
    let _id: Int32 = use Handle { id = 1 }
    Dog("lab")

function main(): Unit =
    match (makeAnimal()).evaluate() with
    case Async.Succeed(a) => assert a.name == "Rex"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("narrower covariant continuation type is not double-wrapped");
}

// A NON-tail `use` inside a multi-statement `let` value block: the inner
// statement-position `use` desugars with the rest of the INNER block as its
// continuation (release before the binding), so the rewritten block evaluates
// to the lifted `Async<U, E>` while the `let` still expects the plain `U`.
// Under the bug the lifted value was bound raw — the component failed wasm
// validation at load ("type mismatch: expected i32, found (ref ...)"). The fix
// wraps the block's tail in a synthetic Await that `desugar_await` consumes.
#[test]
fn test_async_use_non_tail_in_let_value_block() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function withHandle(): Async<Int32, Int32> =
    let x: Int32 =
        let a = use Handle { id = 20 }
        assert a == 20
        a * 2
    x + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Succeed(x) => assert x == 41
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("non-tail use in a let value block awaits the lifted block");
}

// Same shape with statements BEFORE the `use` inside the value block — the
// desugar keeps them as the block prefix ahead of the lifted `Usable.use`
// call, and the synthetic Await must land on the block's TAIL statement
// (`prepend_before` stamps unit-blocks Unit, so the lift is only visible
// there).
#[test]
fn test_async_use_non_tail_with_prefix_statements() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function withHandle(): Async<Int32, Int32> =
    let x: Int32 =
        let base = 10
        let a = use Handle { id = base }
        a * 2
    x + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Succeed(x) => assert x == 21
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("prefix statements stay ahead of the awaited lifted tail");
}

// Nested variant: a value block with a non-tail `use` INSIDE another value
// block that also has a non-tail `use`, where the second operand depends on
// the first binding. The outer synthetic Await's block then contains the inner
// synthetic Await as a sibling statement — `desugar_await` must lower the
// block statement-wise (like a function body), not hoist the second use's
// operand above the `let inner` binding it reads ("undefined local" at
// codegen under the bug).
#[test]
fn test_async_use_nested_value_blocks_non_tail() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

async function withHandles(): Async<Int32, Int32> =
    let x: Int32 =
        let inner: Int32 =
            let a = use Handle { id = 3 }
            a + 1
        let b = use Handle { id = inner }
        b * 10
    x + 2

function main(): Unit =
    match (withHandles()).evaluate() with
    case Async.Succeed(x) => assert x == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("nested value blocks each with a non-tail use");
}

// A failing tail after a non-tail use in a value block — the lifted block's
// error channel must propagate through the synthetic Await like any awaited
// Async.
#[test]
fn test_async_use_non_tail_failure_propagates() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

function alwaysFails(): Async<Int32, Int32> = Async.Fail(9i32)

async function withHandle(): Async<Int32, Int32> =
    let x: Int32 =
        let a = use Handle { id = 1 }
        let b = await alwaysFails()
        a + b
    x + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(e) => assert e == 9i32
    case Async.Succeed(_) => panic "expected the failure to propagate"
"#,
    )
    .expect("failure inside the lifted value block propagates");
}

// Sync (`Wrapped<U, E2> = U`) variant of the non-tail shape — no lift happens
// (the rewritten block is already typed `U`), so the template match must leave
// it untouched.
#[test]
fn test_sync_use_non_tail_in_let_value_block() {
    common::compile_and_run(
        r#"
package a

record Holder = value: Int32

implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

function main(): Unit =
    let x: Int32 =
        let a = use Holder { value = 20 }
        a * 2
    assert x == 40
"#,
    )
    .expect("sync non-tail use in a let value block");
}

#[test]
fn test_async_use_multiline_let_value() {
    common::compile_and_run_async(
        r#"
package a

record Handle = id: Int32

implement Usable<Int32, Int32> for Handle =
    type Wrapped<U, E2> = Async<U, E2>
    function use<U, E2>(self: Handle, f: (Int32) => Async<U, E2>, _errorF: (Int32) => E2): Async<U, E2> =
        f(self.id)

function makeHandle(value: Int32): Handle = Handle { id = value }

async function withHandle(): Async<Int32, Int32> =
    let id: Int32 =
        use makeHandle(
            41
        )
    id + 1

function main(): Unit =
    match (withHandle()).evaluate() with
    case Async.Succeed(x) => assert x == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected success"
"#,
    )
    .expect("multi-line let-value use lifts the enclosing continuation");
}
