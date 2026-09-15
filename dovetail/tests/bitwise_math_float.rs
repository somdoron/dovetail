mod common;

// ── Bitwise AND ──────────────────────────────────────────────────

#[test]
fn test_bitwise_and_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (5 & 3) == 1
"#,
    )
    .expect("5 & 3 == 1");
}

#[test]
fn test_bitwise_and_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (255i64 & 15i64) == 15i64
"#,
    )
    .expect("255i64 & 15i64 == 15i64");
}

// ── Bitwise OR ───────────────────────────────────────────────────

#[test]
fn test_bitwise_or_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (5 | 3) == 7
"#,
    )
    .expect("5 | 3 == 7");
}

#[test]
fn test_bitwise_or_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (5i64 | 3i64) == 7i64
"#,
    )
    .expect("5i64 | 3i64 == 7i64");
}

// ── Bitwise XOR ──────────────────────────────────────────────────

#[test]
fn test_bitwise_xor_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (5 ^ 3) == 6
"#,
    )
    .expect("5 ^ 3 == 6");
}

#[test]
fn test_bitwise_xor_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (0i64 ^ 0i64) == 0i64
"#,
    )
    .expect("0i64 ^ 0i64 == 0i64");
}

// ── Shift left ───────────────────────────────────────────────────

#[test]
fn test_shift_left_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (1 << 3) == 8
"#,
    )
    .expect("1 << 3 == 8");
}

#[test]
fn test_shift_left_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (1i64 << 32i64) == 4294967296i64
"#,
    )
    .expect("1i64 << 32i64 == 4294967296i64");
}

// ── Shift right ──────────────────────────────────────────────────

#[test]
fn test_shift_right_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (8 >> 2) == 2
"#,
    )
    .expect("8 >> 2 == 2");
}

#[test]
fn test_shift_right_signed() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 0 - 8
    assert (x >> 1) == 0 - 4
"#,
    )
    .expect("signed right shift preserves sign");
}

#[test]
fn test_shift_right_unsigned() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 128u8
    assert (x >> 1u8) == 64u8
"#,
    )
    .expect("unsigned right shift");
}

// ── Bitwise NOT ──────────────────────────────────────────────────

#[test]
fn test_bitwise_not_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (~0) == 0 - 1
"#,
    )
    .expect("~0 == -1");
}

// ── Bitwise ops on smaller integer types ─────────────────────────

#[test]
fn test_bitwise_and_uint8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (0xFFu8 & 0x0Fu8) == 15u8
"#,
    )
    .expect("0xFF & 0x0F == 15 for Uint8");
}

#[test]
fn test_bitwise_or_uint16() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (0u16 | 255u16) == 255u16
"#,
    )
    .expect("0 | 255 == 255 for Uint16");
}

// ── Bitwise error: Bool not supported ────────────────────────────

#[test]
fn test_bitwise_and_bool_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = true & false
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not supported")),
        "expected error for & on Bool"
    );
}

// ── Bitwise precedence with generics ─────────────────────────────

#[test]
fn test_shift_right_does_not_break_nested_generics() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr: Array<Array<Int32>> = [|[|1, 2|], [|3, 4|]|]
    assert arr.get(0).get(1) == 2
"#,
    )
    .expect("nested generics still work with >> token");
}

// ── Float toBits / bitsToFloat ───────────────────────────────────

#[test]
fn test_float64_to_bits() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1.0.toBits() == 4607182418800017408i64
"#,
    )
    .expect("1.0.toBits() == IEEE 754 for 1.0");
}

#[test]
fn test_float64_bits_roundtrip() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 3.14
    assert x.toBits().bitsToFloat64() == x
"#,
    )
    .expect("Float64 toBits/bitsToFloat64 roundtrip");
}

#[test]
fn test_float32_to_bits() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1.0f32.toBits() == 1065353216
"#,
    )
    .expect("1.0f32.toBits() == IEEE 754 for 1.0f32");
}

#[test]
fn test_float32_bits_roundtrip() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 3.14f32
    assert x.toBits().bitsToFloat32() == x
"#,
    )
    .expect("Float32 toBits/bitsToFloat32 roundtrip");
}

// ── Math intrinsics ──────────────────────────────────────────────

#[test]
fn test_math_floor() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.floor(3.7) == 3.0
    assert Math.floor(-2.3) == -3.0
    assert Math.floor(5.0) == 5.0
"#,
    )
    .expect("Math.floor");
}

#[test]
fn test_math_trunc() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.trunc(3.7) == 3.0
    assert Math.trunc(-2.3) == -2.0
    assert Math.trunc(5.0) == 5.0
"#,
    )
    .expect("Math.trunc");
}

#[test]
fn test_math_abs() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.abs(-5.0) == 5.0
    assert Math.abs(5.0) == 5.0
    assert Math.abs(0.0) == 0.0
"#,
    )
    .expect("Math.abs");
}

#[test]
fn test_math_fmod() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.fmod(5.0, 3.0) == 2.0
    assert Math.fmod(10.0, 3.0) == 1.0
    assert Math.fmod(7.5, 2.5) == 0.0
"#,
    )
    .expect("Math.fmod");
}

#[test]
fn test_math_is_nan() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.isNan(Math.nan())
    assert Math.isNan(0.0 / 0.0)
    let result = Math.isNan(1.0)
    assert result == false
"#,
    )
    .expect("Math.isNan");
}

#[test]
fn test_math_is_infinity() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.isInfinity(Math.infinity())
    assert Math.isInfinity(1.0 / 0.0)
    let result = Math.isInfinity(1.0)
    assert result == false
    assert Math.isInfinity(-1.0 / 0.0)
"#,
    )
    .expect("Math.isInfinity");
}

#[test]
fn test_math_nan_and_infinity_values() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let nan = Math.nan()
    let inf = Math.infinity()
    let result = nan == nan
    assert result == false
    assert inf > 1000000.0
"#,
    )
    .expect("Math.nan and Math.infinity values");
}

#[test]
fn test_math_floor_float32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert Math.floor(3.7f32) == 3.0f32
    assert Math.floor(-2.3f32) == -3.0f32
"#,
    )
    .expect("Math.floor Float32");
}

// ── Float Display ────────────────────────────────────────────────

#[test]
fn test_float64_display_simple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 3.14.format() == "3.14"
"#,
    )
    .expect("3.14 display");
}

#[test]
fn test_float64_display_integer_value() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 1.0.format() == "1.0"
    assert 100.0.format() == "100.0"
"#,
    )
    .expect("integer float display");
}

#[test]
fn test_float64_display_zero() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 0.0.format() == "0.0"
"#,
    )
    .expect("0.0 display");
}

#[test]
fn test_float64_display_negative() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (-1.0).format() == "-1.0"
    assert (-3.14).format() == "-3.14"
"#,
    )
    .expect("negative float display");
}

#[test]
fn test_float64_display_negative_zero() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (-0.0).format() == "-0.0"
"#,
    )
    .expect("-0.0 display");
}

#[test]
fn test_float64_display_nan() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (0.0 / 0.0).format() == "NaN"
"#,
    )
    .expect("NaN display");
}

#[test]
fn test_float64_display_infinity() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1.0 / 0.0).format() == "Infinity"
    assert (-1.0 / 0.0).format() == "-Infinity"
"#,
    )
    .expect("Infinity display");
}

#[test]
fn test_float64_display_small_decimal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 0.1.format() == "0.1"
    assert 0.01.format() == "0.01"
    assert 0.001.format() == "0.001"
"#,
    )
    .expect("small decimal display");
}

#[test]
fn test_float64_display_large_number() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 1000000.0.format() == "1000000.0"
"#,
    )
    .expect("large number display");
}

#[test]
fn test_float32_display() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 3.14f32.format() == "3.140000104904175"
"#,
    )
    .expect("Float32 display via Float64 promotion");
}

// ── Empty record ─────────────────────────────────────────────────

#[test]
fn test_empty_record_as_namespace() {
    common::compile_and_run(
        r#"
package a

import a.UtilExt

record Util

extension UtilExt for Util =
    public function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
    assert Util.add(3, 4) == 7
"#,
    )
    .expect("empty record as namespace");
}
