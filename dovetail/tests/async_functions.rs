mod common;

// ── Basic async function ────────────────────────────────────────────

#[test]
fn async_function_basic() {
    common::compile_and_run_async(
        r#"
package a

async function foo(): Async<Int32, Never> = 42

function main(): Unit =
    let result = foo()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("basic async function");
}

// ── Async function call + pattern match ─────────────────────────────

#[test]
fn async_function_call_pattern_match() {
    common::compile_and_run_async(
        r#"
package a

async function computeAge(): Async<Int32, String> = 30

function main(): Unit =
    let age = computeAge()
    match (age).evaluate() with
    case Async.Succeed(v) => assert v == 30
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async function call with pattern match");
}

// ── Async function with generic type params ─────────────────────────

#[test]
fn async_function_generic() {
    common::compile_and_run_async(
        r#"
package a

async function wrap<T>(x: T): Async<T, Never> = x

function main(): Unit =
    let result = wrap(99)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 99
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async function with generic type params");
}

// ── Error: missing return type ──────────────────────────────────────

#[test]
fn async_function_missing_return_type_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function foo() = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async functions must have an explicit return type")),
        "expected 'async functions must have an explicit return type' error, got: {:?}",
        errors
    );
}

// ── Error: non-Awaitable return type ────────────────────────────────

#[test]
fn async_function_non_awaitable_return_type_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function foo(): Int32 = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async function return type must implement Awaitable")),
        "expected 'async function return type must implement Awaitable' error, got: {:?}",
        errors
    );
}

// ── Async + chaining ────────────────────────────────────────────────

#[test]
fn async_function_chaining_map() {
    common::compile_and_run_async(
        r#"
package a

async function getNumber(): Async<Int32, Never> = 10

function main(): Unit =
    let result = getNumber().map(v => v * 3)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 30
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async function with map chaining");
}

#[test]
fn async_function_chaining_and_then() {
    common::compile_and_run_async(
        r#"
package a

async function getNumber(): Async<Int32, Never> = 5

function main(): Unit =
    let result = getNumber().andThen(v => Async.Succeed(v + 10))
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 15
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async function with andThen chaining");
}

// ── Module static async method ──────────────────────────────────────

#[test]
fn async_module_static_method() {
    common::compile_and_run_async(
        r#"
package a

module Api =
    async function fetchValue(): Async<Int32, Never> = 100

function main(): Unit =
    let result = Api.fetchValue()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 100
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async module static method");
}

// ── Module instance async method (with self) ────────────────────────

#[test]
fn async_module_instance_method() {
    common::compile_and_run_async(
        r#"
package a

record Request =
    url: String

module Request =
    async function execute(self): Async<Int32, String> = 200

function main(): Unit =
    let req = Request { url = "http://example.com" }
    let result = req.execute()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 200
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async module instance method");
}

// ── Class instance async method ─────────────────────────────────────

#[test]
fn async_class_instance_method() {
    common::compile_and_run_async(
        r#"
package a

class Service(public name: String)

module Service =
    async function call(self): Async<Int32, Never> = 42

function main(): Unit =
    let svc = Service("test")
    let result = svc.call()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async class instance method");
}

// ── Class static async method ───────────────────────────────────────

#[test]
fn async_class_static_method() {
    common::compile_and_run_async(
        r#"
package a

class Client(public url: String) =
    public async function defaultRequest(): Async<Int32, Never> = 0

function main(): Unit =
    let result = Client.defaultRequest()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 0
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async class static method");
}

// ── Extension async method ──────────────────────────────────────────

#[test]
fn async_extension_instance_method() {
    common::compile_and_run_async(
        r#"
package a

import a.QueryExt

record Query =
    text: String

extension QueryExt for Query =
    async function run(self): Async<Int32, String> = 42

function main(): Unit =
    let q = Query { text = "select" }
    let result = q.run()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async extension instance method");
}

// ── Extension static async method ───────────────────────────────────

#[test]
fn async_extension_static_method() {
    common::compile_and_run_async(
        r#"
package a

import a.DbExt

record Db =
    name: String

extension DbExt for Db =
    async function connect(): Async<Db, String> =
        Db { name = "test" }

function main(): Unit =
    let result = Db.connect()
    match (result).evaluate() with
    case Async.Succeed(db) => assert db.name == "test"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async extension static method");
}

// ── Implement block async method ────────────────────────────────────

#[test]
fn async_implement_method() {
    common::compile_and_run_async(
        r#"
package a

trait Fetchable =
    function fetch(self: Self): Async<Int32, String>

record Endpoint =
    url: String

implement Fetchable for Endpoint =
    async function fetch(self: Endpoint): Async<Int32, String> = 200

function main(): Unit =
    let ep = Endpoint { url = "http://example.com" }
    let result = ep.fetch()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 200
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async implement method");
}

// ── Class body async method (uses module-for-type to avoid class codegen limitation) ─

#[test]
fn async_class_body_static_method() {
    common::compile_and_run_async(
        r#"
package a

class Client(public url: String) =
    public async function health(): Async<Bool, Never> = true

function main(): Unit =
    let result = Client.health()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == true
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async class body static method");
}

// ── Error: async class method with non-Awaitable return type ────────

#[test]
fn async_class_method_non_awaitable_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

class Bad(public x: Int32) =
    public async function compute(self: Bad): Int32 = self.x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async function return type must implement Awaitable")),
        "expected Awaitable error for async class method, got: {:?}",
        errors
    );
}

// ── Error: async extension method with non-Awaitable return type ────

#[test]
fn async_extension_method_non_awaitable_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

import a.FooExt

record Foo =
    x: Int32

extension FooExt for Foo =
    async function bar(self): Int32 = self.x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async function return type must implement Awaitable")),
        "expected Awaitable error for async extension method, got: {:?}",
        errors
    );
}

// ── Error: async module method with non-Awaitable return type ───────

#[test]
fn async_module_method_non_awaitable_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

module Api =
    async function health(): Bool = true

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async function return type must implement Awaitable")),
        "expected Awaitable error for async module method, got: {:?}",
        errors
    );
}

// ── Await expression tests ──────────────────────────────────────────

#[test]
fn await_basic() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let x = await Async.Succeed(42)
    x

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await basic");
}

#[test]
fn await_let_binding() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let x = await Async.Succeed(42)
    x + 1

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 43
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await let binding");
}

#[test]
fn await_multiple() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let a = await Async.Succeed(10)
    let b = await Async.Succeed(20)
    a + b

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 30
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await multiple");
}

#[test]
fn await_in_sub_expression() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Bool, Never> =
    let x = (await Async.Succeed(42)) + 1
    x == 43

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == true
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await in sub expression");
}

#[test]
fn await_with_error_type() {
    common::compile_and_run_async(
        r#"
package a

async function helper(): Async<Int32, String> = 42

async function f(): Async<Int32, String> =
    let x = await helper()
    x

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await with error type");
}

// ── Await error tests ───────────────────────────────────────────────

#[test]
fn await_outside_async_function_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function f(): Unit =
    let x = await Async.Succeed(1)
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("await can only be used inside an async function")),
        "expected 'await can only be used inside an async function' error, got: {:?}",
        errors
    );
}

#[test]
fn await_on_non_awaitable_type_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function f(): Async<Int32, Never> = await 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement Awaitable")),
        "expected 'does not implement Awaitable' error, got: {:?}",
        errors
    );
}

// ── No-await async function (succeed wrapping) ──────────────────────

#[test]
fn async_no_await_succeed_wrapping() {
    common::compile_and_run_async(
        r#"
package a

async function pure(): Async<Int32, Never> = 42

function main(): Unit =
    let result = pure()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("no-await async function succeed wrapping");
}

// ── Await fail propagation ──────────────────────────────────────────

#[test]
fn await_fail_propagation() {
    common::compile_and_run_async(
        r#"
package a

function failingOp(): Async<Int32, String> = Async.Fail("oops")

async function failing(): Async<Int32, String> =
    let x = await failingOp()
    x + 1

function main(): Unit =
    let result = failing()
    match (result).evaluate() with
    case Async.Succeed(_) => panic "expected Fail"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(msg) => assert msg == "oops"
"#,
    )
    .expect("await fail propagation");
}

// ── Chained awaits with compile_and_run ─────────────────────────────

#[test]
fn await_chained_operations() {
    common::compile_and_run_async(
        r#"
package a

async function chained(): Async<Int32, Never> =
    let a = await Async.Succeed(10)
    let b = await Async.Succeed(20)
    let c = await Async.Succeed(30)
    a + b + c

function main(): Unit =
    let result = chained()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 60
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("chained await operations");
}

// ── Tail await (last statement is bare await) ───────────────────────

#[test]
fn await_tail_position() {
    common::compile_and_run_async(
        r#"
package a

async function inner(): Async<Int32, Never> = 42

async function outer(): Async<Int32, Never> =
    await inner()

function main(): Unit =
    let result = outer()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("tail await returns operand directly");
}

#[test]
fn await_tail_after_let() {
    common::compile_and_run_async(
        r#"
package a

async function add(a: Int32, b: Int32): Async<Int32, Never> = a + b

async function compute(): Async<Int32, Never> =
    let x = await Async.Succeed(10)
    await add(x, 20)

function main(): Unit =
    let result = compute()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 30
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("tail await after let binding");
}

// ── Non-await statements before first await ─────────────────────────

#[test]
fn await_with_preceding_statements() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let y = 5
    let z = 10
    let x = await Async.Succeed(42)
    x + y + z

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 57
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await with preceding non-await statements");
}

// ── Await inside module method ──────────────────────────────────────

#[test]
fn await_in_module_method() {
    common::compile_and_run_async(
        r#"
package a

module Api =
    async function fetchAndDouble(): Async<Int32, Never> =
        let x = await Async.Succeed(21)
        x * 2

function main(): Unit =
    let result = Api.fetchAndDouble()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await in module method");
}

// ── Await inside extension method ───────────────────────────────────

#[test]
fn await_in_extension_method() {
    common::compile_and_run_async(
        r#"
package a

import a.ReqExt

record Req =
    value: Int32

extension ReqExt for Req =
    async function process(self): Async<Int32, Never> =
        let x = await Async.Succeed(self.value)
        x + 1

function main(): Unit =
    let r = Req { value = 10 }
    let result = r.process()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 11
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await in extension method");
}

// ── Await inside implement method ───────────────────────────────────

#[test]
fn await_in_implement_method() {
    common::compile_and_run_async(
        r#"
package a

trait AsyncOp =
    function run(self: Self): Async<Int32, Never>

record Job =
    base: Int32

implement AsyncOp for Job =
    async function run(self: Job): Async<Int32, Never> =
        let x = await Async.Succeed(self.base)
        x * 3

function main(): Unit =
    let j = Job { base = 7 }
    let result = j.run()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 21
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await in implement method");
}

// ── Incompatible error types ────────────────────────────────────────

#[test]
fn await_incompatible_error_types_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function getOp(): Async<Int32, Int32> = Async.Succeed(1)

async function f(): Async<Int32, String> =
    let x = await getOp()
    x

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot await")),
        "expected 'cannot await' error for incompatible error types, got: {:?}",
        errors
    );
}

// ── Error type mismatch — map path (single await) ───────────────────

#[test]
fn await_error_type_mismatch_single_await() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function getOp(): Async<Int32, Int32> = Async.Succeed(1)

async function f(): Async<Int32, String> =
    await getOp()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot await")),
        "expected 'cannot await' error for single await with mismatched error types, got: {:?}",
        errors
    );
}

// ── Second of multiple awaits has incompatible error type ────────────

#[test]
fn await_second_await_incompatible_error_type() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function goodOp(): Async<Int32, Never> = Async.Succeed(1)
function badOp(): Async<Int32, String> = Async.Succeed(2)

async function f(): Async<Int32, Never> =
    let x = await goodOp()
    let y = await badOp()
    x + y

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot await")),
        "expected 'cannot await' error for second await with String error in Never-returning fn, got: {:?}",
        errors
    );
}

// ── Body type mismatch — no awaits ──────────────────────────────────

#[test]
fn async_body_type_mismatch_no_awaits() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function f(): Async<Int32, Never> = "hello"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected 'type mismatch' error for String body in Int32 async fn, got: {:?}",
        errors
    );
}

// ── Body type mismatch — after await ────────────────────────────────

#[test]
fn async_body_type_mismatch_after_await() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let x = await Async.Succeed(42)
    "not an int"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected 'type mismatch' error for String body in Int32 async fn after await, got: {:?}",
        errors
    );
}

// ── Body type mismatch — tail await value type ──────────────────────

#[test]
fn await_tail_value_type_mismatch() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function getStringOp(): Async<String, Never> = Async.Succeed("hi")

async function f(): Async<Int32, Never> =
    await getStringOp()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type mismatch") || e.contains("cannot await")),
        "expected type error for tail await with mismatched value types, got: {:?}",
        errors
    );
}

// ── Await in sub-expression type error ──────────────────────────────

#[test]
fn await_sub_expression_type_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function getStringOp(): Async<String, Never> = Async.Succeed("hi")

async function f(): Async<Int32, Never> =
    (await getStringOp()) + 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("binary '+' requires operands of the same type")),
        "expected binary '+' type error for String + Int32 in await sub-expression, got: {:?}",
        errors
    );
}

// ── Nested Awaitable (double await) ─────────────────────────────────

#[test]
fn await_nested_awaitable() {
    common::compile_and_run_async(
        r#"
package a

async function f(): Async<Int32, Never> =
    let inner = await Async.Succeed(Async.Succeed(42))
    await inner

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("nested awaitable double await");
}

// ── Async body cannot return Async.Fail directly (type mismatch) ────

#[test]
fn async_function_cannot_return_fail_directly() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function failing(): Async<Int32, String> = Async.Fail("boom")

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for Async.Fail body in async function (body checked against T), got: {:?}",
        errors
    );
}

// ── Non-async function returning Fail directly ──────────────────────

#[test]
fn non_async_function_returns_fail() {
    common::compile_and_run_async(
        r#"
package a

function failing(): Async<Int32, String> = Async.Fail("boom")

function main(): Unit =
    let result = failing()
    match (result).evaluate() with
    case Async.Succeed(_) => panic "expected Fail"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(msg) => assert msg == "boom"
"#,
    )
    .expect("non-async function returning Fail directly");
}

// ── Await Fail propagation skips subsequent code ────────────────────

#[test]
fn await_fail_skips_subsequent_awaits() {
    common::compile_and_run_async(
        r#"
package a

function failFirst(): Async<Int32, String> = Async.Fail("first failed")
function succeedSecond(): Async<Int32, String> = Async.Succeed(99)

async function f(): Async<Int32, String> =
    let x = await failFirst()
    let y = await succeedSecond()
    x + y

function main(): Unit =
    let result = f()
    match (result).evaluate() with
    case Async.Succeed(_) => panic "expected Fail"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(msg) => assert msg == "first failed"
"#,
    )
    .expect("await Fail propagation skips subsequent code");
}

// ══════════════════════════════════════════════════════════════════════
// Phase 7: Async Closures
// ══════════════════════════════════════════════════════════════════════

// ── Async closure with no await (succeed wrapping) ──────────────────

#[test]
fn async_closure_no_await() {
    common::compile_and_run_async(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Async<Int32, Never> = async x => x + 1
    let result = f(41)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure no await");
}

// ── Async closure with await ────────────────────────────────────────

#[test]
fn async_closure_with_await() {
    common::compile_and_run_async(
        r#"
package a

function getNumber(): Async<Int32, Never> = Async.Succeed(10)

function main(): Unit =
    let f: (Int32) => Async<Int32, Never> = async x =>
        let y = await getNumber()
        x + y
    let result = f(5)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 15
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure with await");
}

// ── Async closure as andThen argument ───────────────────────────────

#[test]
fn async_closure_as_and_then_arg() {
    common::compile_and_run_async(
        r#"
package a

function getNumber(): Async<Int32, Never> = Async.Succeed(5)
function doubleAsync(x: Int32): Async<Int32, Never> = Async.Succeed(x + 10)

function main(): Unit =
    let f: (Int32) => Async<Int32, Never> = async v => await doubleAsync(v)
    let result = getNumber().andThen(f)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 15
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure as andThen arg");
}

// ── Async closure captures outer variable ───────────────────────────

#[test]
fn async_closure_captures() {
    common::compile_and_run_async(
        r#"
package a

function main(): Unit =
    let offset = 100
    let f: (Int32) => Async<Int32, Never> = async x => x + offset
    let result = f(42)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 142
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure captures");
}

// ── Async closure inside async function ─────────────────────────────

#[test]
fn async_closure_inside_async_function() {
    common::compile_and_run_async(
        r#"
package a

function getNumber(): Async<Int32, Never> = Async.Succeed(7)

async function compute(): Async<Int32, Never> =
    let f: (Int32) => Async<Int32, Never> = async x =>
        let y = await getNumber()
        x + y
    await f(3)

function main(): Unit =
    let result = compute()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 10
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure inside async function");
}

// ── Async closure with annotated params ─────────────────────────────

#[test]
fn async_closure_annotated_params() {
    common::compile_and_run_async(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Async<Int32, Never> = async (x: Int32) => x * 2
    let result = f(21)
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure annotated params");
}

// ── Error: async closure without expected type context ───────────────

#[test]
fn async_closure_no_expected_type_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let f = async (x: Int32) => x + 1
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("async closure requires expected type context")),
        "expected 'async closure requires expected type context' error, got: {:?}",
        errors
    );
}

// ── Error: async closure with non-Awaitable return type ─────────────

#[test]
fn async_closure_non_awaitable_return_error() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Int32 = async (x: Int32) => x + 1
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement Awaitable")),
        "expected 'does not implement Awaitable' error, got: {:?}",
        errors
    );
}

// ── Async.Fail with wrong error type ────────────────────────────────

#[test]
fn async_fail_wrong_error_type() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function f(): Async<Int32, String> = Async.Fail(42)

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for Async.Fail(42) in Async<Int32, String> function, got: {:?}",
        errors
    );
}

// ── Async + try/orReturn (Phase 8) ─────────────────────────────────

#[test]
fn async_try_result_success() {
    common::compile_and_run_async(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

async function process(): Async<Int32, String> =
    let x = try getResult()
    x

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async try result success path");
}

#[test]
fn async_try_result_error() {
    common::compile_and_run_async(
        r#"
package a

function getResult(): Result<Int32, String> = Error("oops")

async function process(): Async<Int32, String> =
    let x = try getResult()
    x

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(_) => panic "expected Fail"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(e) => assert e == "oops"
"#,
    )
    .expect("async try result error path");
}

#[test]
fn async_or_return_result() {
    common::compile_and_run_async(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(10)

async function process(): Async<Int32, String> =
    let x = getResult().orReturn
    x * 2

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 20
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async orReturn result");
}

#[test]
fn async_try_multiple() {
    common::compile_and_run_async(
        r#"
package a

function getA(): Result<Int32, String> = Ok(10)
function getB(): Result<Int32, String> = Ok(20)

async function process(): Async<Int32, String> =
    let a = try getA()
    let b = try getB()
    a + b

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 30
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async try multiple");
}

#[test]
fn async_try_and_await_combined() {
    common::compile_and_run_async(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(5)

async function inner(): Async<Int32, String> = 10

async function process(): Async<Int32, String> =
    let x = try getResult()
    let y = await inner()
    x + y

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(v) => assert v == 15
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async try and await combined");
}

#[test]
fn async_try_early_return_first() {
    common::compile_and_run_async(
        r#"
package a

function failFirst(): Result<Int32, String> = Error("early")
function succeedSecond(): Result<Int32, String> = Ok(99)

async function process(): Async<Int32, String> =
    let a = try failFirst()
    let b = try succeedSecond()
    a + b

function main(): Unit =
    let result = process()
    match (result).evaluate() with
    case Async.Succeed(_) => panic "expected Fail"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(e) => assert e == "early"
"#,
    )
    .expect("async try early return on first");
}

#[test]
fn async_try_incompatible_error_type() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function getResult(): Result<Int32, Int32> = Ok(42)

async function process(): Async<Int32, String> =
    let x = try getResult()
    x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not assignable to async return type") || e.contains("no From")),
        "expected incompatible error type diagnostic, got: {:?}",
        errors
    );
}

// ── Method type param count mismatch ────────────────────────────────

#[test]
fn async_method_wrong_type_param_count() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

async function doWork(): Async<Int32, Never> =
    let x: Async<Int32, Never> = Async.Succeed(42)
    let result = await x.andThen<Int32, String>((v: Int32) => Async.Succeed(v))
    result

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expects 1 type parameter") && e.contains("2 were provided")),
        "expected type param count mismatch error, got: {:?}",
        errors
    );
}

// ── Async closure infers method type param from body ────────────────

#[test]
fn async_closure_infers_type_param_from_body() {
    common::compile_and_run_async(
        r#"
package a

async function doWork(): Async<Int32, Never> =
    let x: Async<Int32, Never> = Async.Succeed(42)
    let result = await x.andThen(async v => v + 1)
    result

function main(): Unit =
    let r = doWork()
    match (r).evaluate() with
    case Async.Succeed(v) => assert v == 43
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure should infer U from body return type");
}

// ── Async closure with await infers method type param ────────────────

#[test]
fn async_closure_with_await_infers_type_param() {
    common::compile_and_run_async(
        r#"
package a

async function doWork(): Async<Int32, Never> =
    let x: Async<Int32, Never> = Async.Succeed(42)
    let result = await x.andThen(async v =>
        let y = await Async.Succeed(v + 1)
        let z = await Async.Succeed(y + 1)
        z
    )
    result

function main(): Unit =
    let r = doWork()
    match (r).evaluate() with
    case Async.Succeed(v) => assert v == 44
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("async closure with await should infer U from body");
}

// ── No cascading await errors when async closure fails ──────────────

#[test]
fn async_closure_no_cascading_await_errors() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function doWork(): Unit =
    let x: Async<Int32, Never> = Async.Succeed(42)
    let result = x.andThen<Int32, String>(async v =>
        let y = await Async.Succeed(v + 1)
        y
    )
    ()

function main(): Unit = ()
"#,
    );
    // Should have the type param count mismatch error but NOT cascading "await can only be used inside async" errors
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expects 1 type parameter")),
        "expected type param count error, got: {:?}",
        errors
    );
    let await_errors: Vec<_> = errors
        .iter()
        .filter(|e| e.contains("await can only be used inside"))
        .collect();
    assert!(
        await_errors.is_empty(),
        "should not have cascading await errors, got: {:?}",
        await_errors
    );
}

// ── Explicit type params on method resolve async closure ────────────

#[test]
fn async_closure_explicit_type_param_resolves() {
    common::compile_and_run_async(
        r#"
package a

async function doWork(): Async<Int32, Never> =
    let x: Async<Int32, Never> = Async.Succeed(42)
    let result = await x.andThen<Int32>((v: Int32) => Async.Succeed(v + 1))
    result

function main(): Unit =
    let r = doWork()
    match (r).evaluate() with
    case Async.Succeed(v) => assert v == 43
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("explicit type param on andThen should resolve closure");
}

// ── await `Async<T, Never>` inside a function returning `Async<U, E>` ──────
// The error type widens via covariance — no `let x: Async<_, E> = ...` workaround
// needed at the call site. Verifies the await-desugar substitutes the operand's
// Never error type to match the function's error type before resolving andThen.

#[test]
fn await_never_widens_to_function_error_type() {
    common::compile_and_run_async(
        r#"
package a

newtype MyError = String

implement Display for MyError =
    public function format(self: MyError): String = self.value

// `pure` returns an Async with error type Never (no error possible).
function pure(): Async<Int32, Never> = Async.Succeed(7)

async function compute(): Async<Int32, MyError> =
    // `pure()` is Async<Int32, Never>; awaited in an Async<_, MyError> context.
    // The compiler must widen Never to MyError automatically.
    let x = await pure()
    x + 1

function main(): Unit =
    let r = compute()
    match (r).evaluate() with
    case Async.Succeed(v) => assert v == 8
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await of Async<T, Never> should widen to function error type");
}

// ── SPIKE: abstract async method overridden by async function, virtually dispatched ──
// De-risks the abstract-class refactor of AsyncInputStream/AsyncOutputStream: a base
// concrete `async` default method calls an abstract `async` method that the subclass
// overrides, invoked through a base-typed binding (vtable dispatch).
#[test]
fn async_abstract_method_virtual_dispatch() {
    common::compile_and_run_async(
        r#"
package a

abstract class Src<E>() =
    public abstract function read(self): Async<Int32, E>
    // Base concrete async default that self-calls the abstract async method.
    public async function readTwice(self): Async<Int32, E> =
        let a = await self.read()
        let b = await self.read()
        a + b

class Const<E>(public v: Int32) extends Src<E>() =
    public override async function read(self): Async<Int32, E> =
        let x = await Async.Succeed(self.v)
        x

function getTwice(s: Src<Never>): Async<Int32, Never> = s.readTwice()

function main(): Unit =
    let c = Const<Never>(21)
    let r: Async<Int32, Never> = getTwice(c)
    match (r).evaluate() with
    case Async.Succeed(v) => assert v == 42
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("virtual dispatch of abstract async method");
}

// ── Dependent awaits inside a `let` value block ─────────────────────
// `let x = <multi-statement block with awaits>` where a later await's operand
// reads a `let` bound EARLIER in the same block. `lower_expr_to_async` used to
// hoist the second await's operand out to become the andThen stem while the
// preceding statements moved into the continuation closure — evaluating
// `g(inner)` before `let inner = ...` existed ("undefined local: inner" at
// codegen). Blocks are now lowered statement-wise, like function bodies.

#[test]
fn async_dependent_awaits_in_let_value_block() {
    common::compile_and_run_async(
        r#"
package a

function lift(v: Int32): Async<Int32, Never> = Async.Succeed(v)

async function compute(): Async<Int32, Never> =
    let x: Int32 =
        let inner = await lift(3)
        let doubled = await lift(inner * 2)
        doubled + 1
    x + 10

function main(): Unit =
    match (compute()).evaluate() with
    case Async.Succeed(v) => assert v == 17
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("dependent awaits in a let value block");
}

// ── await inside a tuple literal (regression: extract_first_await had no
//    TupleLiteral arm, so a tuple with awaits reached lower_expr_to_async and
//    panicked with "contains_await=true but extract_first_await found none") ──

#[test]
fn await_in_tuple_literal_let_binding() {
    common::compile_and_run_async(
        r#"
package a

async function g(): Async<Int32, Never> = 10
async function h(): Async<Int32, Never> = 20

async function f(): Async<(Int32, Int32), Never> =
    let pair = (await g(), await h())
    pair

function main(): Unit =
    match (f()).evaluate() with
    case Async.Succeed((a, b)) =>
        assert a == 10
        assert b == 20
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a tuple literal (let binding)");
}

#[test]
fn await_in_tuple_literal_whole_body() {
    common::compile_and_run_async(
        r#"
package a

async function g(): Async<Int32, Never> = 10
async function h(): Async<Int32, Never> = 20

async function f(): Async<(Int32, Int32), Never> = (await g(), await h())

function main(): Unit =
    match (f()).evaluate() with
    case Async.Succeed((a, b)) =>
        assert a == 10
        assert b == 20
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a tuple literal (whole body)");
}

// ── await inside a control-flow expression nested UNDER an outer expression
//    (regression: If/Match nested below a BinaryOp/call-arg returned None from
//    extract_first_await and panicked). Now hoisted via the branching lift. ──

#[test]
fn await_in_if_nested_under_binary_op() {
    common::compile_and_run_async(
        r#"
package a

async function g(): Async<Int32, Never> = 10
async function h(): Async<Int32, Never> = 20

async function f(cond: Bool): Async<Int32, Never> =
    let x = 1 + (if cond then await g() else await h())
    x

function main(): Unit =
    match (f(true)).evaluate() with
    case Async.Succeed(v) =>
        assert v == 11
        match (f(false)).evaluate() with
        case Async.Succeed(w) => assert w == 21
        case Async.Deferred(_) => panic "unexpected deferred computation"
        case Async.Fail(_) => panic "expected Succeed"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside an if nested under a binary op");
}

#[test]
fn await_in_match_nested_under_binary_op() {
    common::compile_and_run_async(
        r#"
package a

async function g(): Async<Int32, Never> = 10
async function h(): Async<Int32, Never> = 20

async function f(n: Int32): Async<Int32, Never> =
    let x = 100 + match n with
        case 0 => await g()
        case _ => await h()
    x

function main(): Unit =
    match (f(0)).evaluate() with
    case Async.Succeed(v) =>
        assert v == 110
        match (f(1)).evaluate() with
        case Async.Succeed(w) => assert w == 120
        case Async.Deferred(_) => panic "unexpected deferred computation"
        case Async.Fail(_) => panic "expected Succeed"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a match nested under a binary op");
}

// ── `for` loops whose body awaits ───────────────────────────────────
//
// `for` desugars to a `while` driven by `Iterator.next()`. It used to end that
// loop with a generated `break`, which broke as soon as the body awaited:
// `desugar_await` lowers an awaiting `while` into `Async.whileLoop(cond, body)`,
// putting the body in a closure, and the lifted `break` no longer had a loop —
// codegen aborted with "break outside loop". The loop now ends by clearing a
// flag in its condition instead.

#[test]
fn await_inside_a_for_loop_over_a_list() {
    common::compile_and_run_async(
        r#"
package a

async function double(n: Int32): Async<Int32, Never> = n * 2

async function total(xs: List<Int32>): Async<Int32, Never> =
    let mutable sum = 0
    for x in xs do
        sum = sum + await double(x)
    sum

function main(): Unit =
    match (total([1, 2, 3])).evaluate() with
    case Async.Succeed(v) => assert v == 12
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a for loop over a list");
}

#[test]
fn await_inside_a_for_loop_over_an_array() {
    common::compile_and_run_async(
        r#"
package a

async function double(n: Int32): Async<Int32, Never> = n * 2

async function total(xs: Array<Int32>): Async<Int32, Never> =
    let mutable sum = 0
    for x in xs do
        sum = sum + await double(x)
    sum

function main(): Unit =
    match (total([| 1, 2, 3 |])).evaluate() with
    case Async.Succeed(v) => assert v == 12
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a for loop over an array");
}

/// The empty case still has to terminate — the flag is set before the first
/// `next()` and cleared by it.
#[test]
fn await_inside_a_for_loop_over_an_empty_list() {
    common::compile_and_run_async(
        r#"
package a

async function double(n: Int32): Async<Int32, Never> = n * 2

async function total(xs: List<Int32>): Async<Int32, Never> =
    let mutable sum = 0
    for x in xs do
        sum = sum + await double(x)
    sum

function main(): Unit =
    let empty: List<Int32> = []
    match (total(empty)).evaluate() with
    case Async.Succeed(v) => assert v == 0
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("await inside a for loop over an empty list");
}

#[test]
fn nested_for_loops_that_await() {
    common::compile_and_run_async(
        r#"
package a

async function add(a: Int32, b: Int32): Async<Int32, Never> = a + b

async function total(xs: List<Int32>, ys: List<Int32>): Async<Int32, Never> =
    let mutable sum = 0
    for x in xs do
        for y in ys do
            sum = await add(sum, x * y)
    sum

function main(): Unit =
    match (total([1, 2], [3, 4])).evaluate() with
    case Async.Succeed(v) => assert v == 21
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "expected Succeed"
"#,
    )
    .expect("nested awaiting for loops");
}

#[test]
fn awaited_destructuring_keeps_bindings_in_continuation_scope() {
    common::compile_and_run_async(
        r#"
package a

async function calculate(): Async<Int32, Never> =
    let (first, second) = await Async.Succeed((10, 20))
    let (third, fourth) =
        let next = await Async.Succeed(first + second)
        (next, next + 1)
    let offset = await Async.Succeed(1)
    first + second + third + fourth + offset

function main(): Unit =
    match calculate().evaluate() with
    case Async.Succeed(value) => assert value == 92
    case _ => panic "expected success"
"#,
    )
    .expect("awaited destructuring keeps bindings available across later awaits");
}

#[test]
fn annotated_async_closures_receive_method_argument_context() {
    common::compile_and_run_async(
        r#"
package a

module Operations =
    function apply(value: Int32, f: (Int32) => Async<Int32, Never>): Async<Int32, Never> = f(value)

function main(): Unit =
    let first = Operations.apply(20, async (value: Int32) => value + 1)
    let second = first.andThen(async (value: Int32) =>
        let increment = await Async<Int32, Never>.Succeed(1)
        value + increment
    )
    match second.evaluate() with
    case Async.Succeed(value) => assert value == 22
    case _ => panic "expected success"
"#,
    )
    .expect("typed async closure arguments use static and instance method context");
}

#[test]
fn async_closures_defer_work_before_the_first_await() {
    common::compile_and_run_async(
        r#"
package a
function main(): Unit =
    let mutable calls = 0
    let pure: () => Async<Int32, Never> = async () =>
        calls = calls + 1
        calls
    let effect = pure()
    assert calls == 0
    effect.evaluate()
    assert calls == 1
    effect.evaluate()
    assert calls == 2
    let suspending: () => Async<Int32, Never> = async () =>
        calls = calls + 1
        await Async.Succeed(())
        calls
    let second = suspending()
    assert calls == 2
    second.evaluate()
    assert calls == 3
"#,
    )
    .unwrap();
}
