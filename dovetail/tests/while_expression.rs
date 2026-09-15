mod common;

#[test]
fn test_basic_while_loop() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable i = 0
    while i < 5 do
        i = i + 1
    assert i == 5
"#,
    )
    .expect("basic while loop counting to 5");
}

#[test]
fn test_while_loop_with_break() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable i = 0
    while true do
        if i == 3 then break
        i = i + 1
    assert i == 3
"#,
    )
    .expect("while loop with break");
}

#[test]
fn test_while_loop_with_continue() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    let mutable i = 0
    while i < 10 do
        i = i + 1
        if i % 2 == 0 then continue
        sum = sum + i
    assert sum == 25
"#,
    )
    .expect("while loop with continue skipping even numbers");
}

#[test]
fn test_while_body_must_be_unit_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    while true do 42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for non-Unit while body, got: {:?}",
        errors
    );
}

#[test]
fn test_while_condition_must_be_bool_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    while 42 do ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for non-Bool while condition, got: {:?}",
        errors
    );
}

#[test]
fn test_break_outside_loop_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = break
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("break outside of loop")),
        "expected 'break outside of loop' error, got: {:?}",
        errors
    );
}

#[test]
fn test_continue_outside_loop_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = continue
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("continue outside of loop")),
        "expected 'continue outside of loop' error, got: {:?}",
        errors
    );
}

#[test]
fn test_nested_while_loops_with_break() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable outer = 0
    let mutable inner_total = 0
    while outer < 3 do
        let mutable inner = 0
        while true do
            if inner == 2 then break
            inner = inner + 1
        inner_total = inner_total + inner
        outer = outer + 1
    assert outer == 3
    assert inner_total == 6
"#,
    )
    .expect("nested while loops with break in inner loop");
}

#[test]
fn test_while_false_never_executes() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable x = 0
    while false do
        x = 99
    assert x == 0
"#,
    )
    .expect("while false should never execute body");
}

#[test]
fn test_break_continue_inside_if_inside_loop() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    let mutable i = 0
    while i < 10 do
        i = i + 1
        if i == 5 then
            continue
        else if i == 8 then
            break
        sum = sum + i
    assert i == 8
    assert sum == 1 + 2 + 3 + 4 + 6 + 7
"#,
    )
    .expect("break and continue inside if-else inside loop");
}
