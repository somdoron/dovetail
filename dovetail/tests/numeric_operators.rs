mod common;

// ── Basic Int32 arithmetic ──────────────────────────────────────

#[test]
fn test_int32_addition() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 + 2 == 3
"#,
    )
    .expect("1 + 2 == 3");
}

#[test]
fn test_int32_subtraction() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 10 - 3 == 7
"#,
    )
    .expect("10 - 3 == 7");
}

#[test]
fn test_int32_multiplication() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 3 * 4 == 12
"#,
    )
    .expect("3 * 4 == 12");
}

#[test]
fn test_int32_division() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 10 / 3 == 3
"#,
    )
    .expect("10 / 3 == 3");
}

#[test]
fn test_int32_remainder() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 10 % 3 == 1
"#,
    )
    .expect("10 % 3 == 1");
}

// ── Operator precedence ─────────────────────────────────────────

#[test]
fn test_precedence_mul_over_add() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 2 + 3 * 4 == 14
"#,
    )
    .expect("2 + 3 * 4 == 14");
}

#[test]
fn test_precedence_parens_override() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (2 + 3) * 4 == 20
"#,
    )
    .expect("(2 + 3) * 4 == 20");
}

#[test]
fn test_nested_parens() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (1 + 2) * (3 + 4) == 21
"#,
    )
    .expect("(1+2)*(3+4) == 21");
}

#[test]
fn test_precedence_comparison_lower_than_arithmetic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 2 + 3 > 4
"#,
    )
    .expect("2 + 3 > 4");
}

// ── Unary operators ─────────────────────────────────────────────

#[test]
fn test_unary_negation() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert -5 + 5 == 0
"#,
    )
    .expect("-5 + 5 == 0");
}

#[test]
fn test_unary_not() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert !false
"#,
    )
    .expect("!false");
}

#[test]
fn test_unary_not_true_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit = assert !true
"#,
    );
}

#[test]
fn test_bitwise_not() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert ~0 == -1
"#,
    )
    .expect("~0 == -1");
}

// ── Comparison operators ────────────────────────────────────────

#[test]
fn test_less_than() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 < 2
"#,
    )
    .expect("1 < 2");
}

#[test]
fn test_greater_than() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 2 > 1
"#,
    )
    .expect("2 > 1");
}

#[test]
fn test_less_equal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 <= 1
"#,
    )
    .expect("1 <= 1");
}

#[test]
fn test_greater_equal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 >= 1
"#,
    )
    .expect("1 >= 1");
}

#[test]
fn test_not_equal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1 != 2
"#,
    )
    .expect("1 != 2");
}

// ── Suffixed integer types ──────────────────────────────────────

#[test]
fn test_int64_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42i64 == 42i64
"#,
    )
    .expect("42i64 == 42i64");
}

#[test]
fn test_uint8_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 255u8 == 255u8
"#,
    )
    .expect("255u8 == 255u8");
}

#[test]
fn test_int8_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 127i8 == 127i8
"#,
    )
    .expect("127i8 == 127i8");
}

#[test]
fn test_int64_arithmetic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1000000000i64 + 2000000000i64 == 3000000000i64
"#,
    )
    .expect("i64 arithmetic");
}

// ── Float literals ──────────────────────────────────────────────

#[test]
fn test_float64_addition() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1.0 + 2.0 == 3.0
"#,
    )
    .expect("1.0 + 2.0 == 3.0");
}

#[test]
fn test_float32_addition() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1.0f32 + 2.0f32 == 3.0f32
"#,
    )
    .expect("f32 addition");
}

#[test]
fn test_float64_comparison() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 3.14 > 2.71
"#,
    )
    .expect("3.14 > 2.71");
}

// ── Sub-32-bit wrapping ─────────────────────────────────────────

#[test]
fn test_uint8_overflow_wraps() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 200u8 + 100u8 == 44u8
"#,
    )
    .expect("u8 wrapping: 200+100=44");
}

#[test]
fn test_int8_overflow_wraps() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100i8 + 100i8 == -56i8
"#,
    )
    .expect("i8 wrapping: 100+100=-56");
}

// ── Hex, binary, octal literals ─────────────────────────────────

#[test]
fn test_hex_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 0xFF == 255
"#,
    )
    .expect("0xFF == 255");
}

#[test]
fn test_binary_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 0b1010 == 10
"#,
    )
    .expect("0b1010 == 10");
}

#[test]
fn test_octal_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 0o77 == 63
"#,
    )
    .expect("0o77 == 63");
}

// ── Type error tests ────────────────────────────────────────────

#[test]
fn test_type_mismatch_int_plus_float() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = 1 + 3.14
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("same type")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_bool_add_not_supported() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = true + false
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not supported")),
        "expected unsupported operator error, got: {:?}",
        errors
    );
}

#[test]
fn test_not_on_int_not_supported() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = !42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not supported")),
        "expected unsupported operator error, got: {:?}",
        errors
    );
}

#[test]
fn test_rem_on_float_not_supported() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = 3.14 % 1.0
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not supported")),
        "expected unsupported operator error, got: {:?}",
        errors
    );
}

#[test]
fn test_negate_unsigned_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = -5u8
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("negate unsigned")),
        "expected negate unsigned error, got: {:?}",
        errors
    );
}

#[test]
fn test_u8_overflow_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = 257u8
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("out of range")),
        "expected out of range error, got: {:?}",
        errors
    );
}

#[test]
fn test_i8_overflow_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = 128i8
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("out of range")),
        "expected out of range error, got: {:?}",
        errors
    );
}

// ── Bool equality ───────────────────────────────────────────────

#[test]
fn test_bool_equality() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true == true
"#,
    )
    .expect("true == true");
}

#[test]
fn test_bool_inequality() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert true != false
"#,
    )
    .expect("true != false");
}

// ── Negative literals ───────────────────────────────────────────

#[test]
fn test_negative_int_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert -1 + 1 == 0
"#,
    )
    .expect("-1 + 1 == 0");
}

#[test]
fn test_negative_float_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert -1.0 + 1.0 == 0.0
"#,
    )
    .expect("-1.0 + 1.0 == 0.0");
}

#[test]
fn test_negative_i8_min() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert -128i8 == -128i8
"#,
    )
    .expect("-128i8");
}
