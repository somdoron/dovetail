mod common;

#[test]
fn test_if_else_true_branch() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = if true then 1 else 2
    assert result == 1
"#,
    )
    .expect("if true should take then branch");
}

#[test]
fn test_if_else_false_branch() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = if false then 1 else 2
    assert result == 2
"#,
    )
    .expect("if false should take else branch");
}

#[test]
fn test_if_else_with_variable_condition() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 10
    let y = if x > 5 then 1 else 0
    assert y == 1
"#,
    )
    .expect("if with variable condition");
}

#[test]
fn test_if_without_else_unit() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    if true then ()
"#,
    )
    .expect("if without else should work when then-branch is Unit");
}

#[test]
fn test_if_without_else_assert() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    if true then assert 1 == 1
"#,
    )
    .expect("if without else with assert body");
}

#[test]
fn test_else_if_chain() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 2
    let result = if x == 1 then 10 else if x == 2 then 20 else 30
    assert result == 20
"#,
    )
    .expect("else-if chain");
}

#[test]
fn test_if_else_with_blocks() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 5
    let result = if x > 0 then
        let y = x + 1
        y
    else
        let y = x - 1
        y
    assert result == 6
"#,
    )
    .expect("if-else with multi-line blocks");
}

#[test]
fn test_if_with_panic_in_else() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = if true then 42 else panic "unreachable"
    assert x == 42
"#,
    )
    .expect("if with panic in else branch (Never type)");
}

#[test]
fn test_if_with_panic_in_then() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = if false then panic "unreachable" else 42
    assert x == 42
"#,
    )
    .expect("if with panic in then branch (Never type)");
}

#[test]
fn test_if_else_bool_result() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 5
    let is_positive = if x > 0 then true else false
    assert is_positive
"#,
    )
    .expect("if-else returning Bool");
}

#[test]
fn test_if_condition_not_bool_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = if 42 then 1 else 2
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error for non-Bool condition, got: {:?}",
        errors
    );
}

#[test]
fn test_if_branch_type_mismatch_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = if true then 1 else true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error for branch types, got: {:?}",
        errors
    );
}

#[test]
fn test_if_without_else_non_unit_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = if true then 42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error for non-Unit then-branch without else, got: {:?}",
        errors
    );
}
