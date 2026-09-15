mod common;

#[test]
fn async_do_without_await_requires_context() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let program = async do 42
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("async do requires an Awaitable type context")),
        "{errors:?}"
    );
}

#[test]
fn nested_async_do_does_not_supply_outer_context() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let program = async do
        let inner = async do await Async.Succeed(42)
        1
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("async do requires an Awaitable type context")),
        "{errors:?}"
    );
}

#[test]
fn async_do_checks_expected_success_type() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let program: Async<Bool, Never> = async do 42
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("not assignable") && error.contains("Bool")),
        "{errors:?}"
    );
}

#[test]
fn async_do_does_not_enable_await_after_its_body() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let program: Async<Int32, Never> = async do 42
    await program
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("await can only be used")),
        "{errors:?}"
    );
}

#[test]
fn async_do_cannot_break_an_enclosing_loop() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    while true do
        let program: Async<Unit, Never> = async do
            break
        break
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("break") && error.contains("loop")),
        "{errors:?}"
    );
}

#[test]
fn synchronous_closure_inside_async_do_cannot_await() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let program: Async<Unit, Never> = async do
        let body: () => Int32 = () => await Async.Succeed(42)
        ()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("await can only be used")),
        "{errors:?}"
    );
}

#[test]
fn async_do_rejects_incompatible_context_parameters() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a

function main(): Unit =
    let first: Async<Int32, String> = Async.Succeed(1)
    let second: Async<Int32, Bool> = Async.Succeed(2)
    let program = async do
        let left = await first
        let right = await second
        left + right
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("incompatible Awaitable contexts")),
        "{errors:?}"
    );
}
