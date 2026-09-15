mod common;

#[test]
fn test_assert_true_runs() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true
"#,
    )
    .expect("assert true should run successfully");
}

#[test]
fn test_assert_true_with_message_runs() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true, "ok"
"#,
    )
    .expect("assert true with message should run successfully");
}

#[test]
fn test_assert_false_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = assert false
"#,
    );
}

#[test]
fn test_assert_false_with_message_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = assert false, "fail"
"#,
    );
}

#[test]
fn test_panic_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = panic "oops"
"#,
    );
}

#[test]
fn test_panic_never_type_matches_unit() {
    // panic has type Never, which is compatible with Unit return type
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = panic "oops"
"#,
    );
}

#[test]
fn test_type_mismatch_bool_vs_unit() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

// ── Logical AND (&&) ─────────────────────────────────────────────────

#[test]
fn test_logical_and_true_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true && true
"#,
    )
    .expect("true && true");
}

#[test]
fn test_logical_and_true_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !(true && false)
"#,
    )
    .expect("true && false is false");
}

#[test]
fn test_logical_and_false_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !(false && true)
"#,
    )
    .expect("false && true is false");
}

#[test]
fn test_logical_and_false_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !(false && false)
"#,
    )
    .expect("false && false is false");
}

#[test]
fn test_logical_and_short_circuit() {
    common::compile_and_run(
        r#"
package a

function helper(): Bool = panic "should not reach"

function main(): Unit = assert !(false && helper())
"#,
    )
    .expect("short-circuit && skips right operand");
}

// ── Logical OR (||) ──────────────────────────────────────────────────

#[test]
fn test_logical_or_true_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true || true
"#,
    )
    .expect("true || true");
}

#[test]
fn test_logical_or_true_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true || false
"#,
    )
    .expect("true || false");
}

#[test]
fn test_logical_or_false_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert false || true
"#,
    )
    .expect("false || true");
}

#[test]
fn test_logical_or_false_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !(false || false)
"#,
    )
    .expect("false || false is false");
}

#[test]
fn test_logical_or_short_circuit() {
    common::compile_and_run(
        r#"
package a

function helper(): Bool = panic "should not reach"

function main(): Unit = assert true || helper()
"#,
    )
    .expect("short-circuit || skips right operand");
}

// ── Precedence and combinations ──────────────────────────────────────

#[test]
fn test_logical_and_binds_tighter_than_or() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true || false && false
"#,
    )
    .expect("&& binds tighter than ||");
}

#[test]
fn test_logical_combined_with_comparison() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 == 1 && 2 == 2
"#,
    )
    .expect("comparison combined with &&");
}

#[test]
fn test_logical_with_negation() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !false || false
"#,
    )
    .expect("!false || false => true");
}

// ── Type errors ──────────────────────────────────────────────────────

#[test]
fn test_logical_and_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = assert 1 && 2
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("requires Bool")),
        "expected Bool type error, got: {:?}",
        errors
    );
}

#[test]
fn test_logical_or_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = assert 1 || 2
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("requires Bool")),
        "expected Bool type error, got: {:?}",
        errors
    );
}

// ── Panic/Assert message printing ────────────────────────────────────

#[test]
fn test_panic_with_message_traps() {
    // panic with a string message should trap (message printed to stderr)
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = panic "something went wrong"
"#,
    );
}

#[test]
fn test_assert_false_auto_message_traps() {
    // assert false with no message should trap (auto-generated location message)
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = assert false
"#,
    );
}

#[test]
fn test_assert_false_custom_message_traps() {
    // assert with custom message should trap (custom message printed)
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = assert 1 == 2, "one is not two"
"#,
    );
}

#[test]
fn test_panic_in_function_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function fail(): Unit = panic "fail!"

function main(): Unit = fail()
"#,
    );
}

#[test]
fn test_assert_true_no_message_succeeds() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true
"#,
    )
    .expect("assert true should succeed");
}
