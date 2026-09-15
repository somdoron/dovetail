mod common;

#[test]
fn awaiting_loops_report_unsupported_break_and_continue() {
    for keyword in ["break", "continue"] {
        let errors = common::compile_expecting_errors_async(&format!(
            r#"
package a
function main(): Unit =
    let program: Async<Unit, Never> = async do
        while true do
            await Async.Succeed(())
            {keyword}
    ()
"#
        ));
        assert!(
            errors
                .iter()
                .any(|error| error.contains(&format!("{keyword} in a loop containing await"))),
            "{errors:?}"
        );
    }
}

#[test]
fn awaiting_for_loops_report_unsupported_control() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a
function main(): Unit =
    let program: Async<Unit, Never> = async do
        for value in [1, 2] do
            await Async.Succeed(value)
            break
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("break in a loop containing await")),
        "{errors:?}"
    );
}

#[test]
fn nested_awaiting_loop_also_prevents_outer_break() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a
function main(): Unit =
    let program: Async<Unit, Never> = async do
        while true do
            for value in [1, 2] do
                await Async.Succeed(value)
                ()
            break
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("break in a loop containing await")),
        "{errors:?}"
    );
}

#[test]
fn awaiting_loop_condition_reports_a_diagnostic() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a
function main(): Unit =
    let program: Async<Unit, Never> = async do
        while await Async.Succeed(false) do
            ()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("await in a loop condition is not supported")),
        "{errors:?}"
    );
}

#[test]
fn resource_loop_condition_reports_a_diagnostic() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a
newtype Condition = Bool
implement Usable<Bool, Never> for Condition =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Condition, f: Bool => U, errorF: Never => E2): U = f(self.value)
function main(): Unit =
    let program: Async<Unit, Never> = async do
        while use Condition(false) do
            ()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("use in a loop condition is not supported")),
        "{errors:?}"
    );
}
