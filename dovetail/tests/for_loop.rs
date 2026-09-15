mod common;

#[test]
fn test_basic_for_loop() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    for x in [|1, 2, 3|] do
        sum = sum + x
    assert sum == 6
"#,
    )
    .expect("basic for loop summing array elements");
}

#[test]
fn test_for_loop_sum() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    for x in [|10, 20, 30, 40|] do
        sum = sum + x
    assert sum == 100
"#,
    )
    .expect("for loop summing to 100");
}

#[test]
fn test_for_loop_with_break() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    for x in [|1, 2, 3, 4, 5|] do
        if x == 4 then break
        sum = sum + x
    assert sum == 6
"#,
    )
    .expect("for loop with early break");
}

#[test]
fn test_for_loop_with_continue() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    for x in [|1, 2, 3, 4, 5|] do
        if x % 2 == 0 then continue
        sum = sum + x
    assert sum == 9
"#,
    )
    .expect("for loop with continue skipping even numbers");
}

#[test]
fn test_nested_for_loops() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable sum = 0
    for x in [|1, 2, 3|] do
        for y in [|10, 20|] do
            sum = sum + x * y
    assert sum == 180
"#,
    )
    .expect("nested for loops");
}

#[test]
fn test_for_loop_body_must_be_unit() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    for x in [|1, 2, 3|] do x
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for non-Unit for body, got: {:?}",
        errors
    );
}

#[test]
fn test_non_iterable_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    for x in 42 do ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("does not implement Iterable")),
        "expected Iterable error, got: {:?}",
        errors
    );
}

#[test]
fn test_for_loop_empty_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable count = 0
    let empty: Array<Int32> = [||]
    for x in empty do
        count = count + 1
    assert count == 0
"#,
    )
    .expect("for loop over empty array does nothing");
}

#[test]
fn test_for_loop_wildcard_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable count = 0
    for _ in [|1, 2, 3|] do
        count = count + 1
    assert count == 3
"#,
    )
    .expect("for loop with wildcard pattern");
}
