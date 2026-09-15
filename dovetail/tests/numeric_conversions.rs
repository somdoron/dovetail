mod common;

// ── Widening conversions ─────────────────────────────────────────────

#[test]
fn test_int8_to_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42i8.toInt32() == 42
"#,
    )
    .expect("int8 to int32");
}

#[test]
fn test_uint8_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100u8.toInt64() == 100i64
"#,
    )
    .expect("uint8 to int64");
}

#[test]
fn test_int16_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1000i16.toInt64() == 1000i64
"#,
    )
    .expect("int16 to int64");
}

#[test]
fn test_uint16_to_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 5000u16.toInt32() == 5000
"#,
    )
    .expect("uint16 to int32");
}

#[test]
fn test_int32_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100000.toInt64() == 100000i64
"#,
    )
    .expect("int32 to int64");
}

#[test]
fn test_uint32_to_uint64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100000u32.toUint64() == 100000u64
"#,
    )
    .expect("uint32 to uint64");
}

// ── Narrowing conversions ────────────────────────────────────────────

#[test]
fn test_int32_to_int8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42.toInt8() == 42i8
"#,
    )
    .expect("int32 to int8");
}

#[test]
fn test_int64_to_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100000i64.toInt32() == 100000
"#,
    )
    .expect("int64 to int32");
}

#[test]
fn test_int32_to_uint8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 200.toUint8() == 200u8
"#,
    )
    .expect("int32 to uint8");
}

// ── Sign change conversions ──────────────────────────────────────────

#[test]
fn test_uint8_to_int8_reinterpret() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 255u8.toInt8() == -1i8
"#,
    )
    .expect("255u8 reinterpreted as int8 should be -1");
}

#[test]
fn test_int8_to_uint8_reinterpret() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (-1i8).toUint8() == 255u8
"#,
    )
    .expect("-1i8 reinterpreted as uint8 should be 255");
}

#[test]
fn test_int64_to_uint64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42i64.toUint64() == 42u64
"#,
    )
    .expect("int64 to uint64");
}

// ── Int → Float conversions ──────────────────────────────────────────

#[test]
fn test_int32_to_float64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42.toFloat64() == 42.0
"#,
    )
    .expect("int32 to float64");
}

#[test]
fn test_int64_to_float32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 100i64.toFloat32() == 100.0f32
"#,
    )
    .expect("int64 to float32");
}

#[test]
fn test_uint8_to_float64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 200u8.toFloat64() == 200.0
"#,
    )
    .expect("uint8 to float64");
}

#[test]
fn test_int32_to_float32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42.toFloat32() == 42.0f32
"#,
    )
    .expect("int32 to float32");
}

// ── Float → Int conversions (truncating) ─────────────────────────────

#[test]
fn test_float64_to_int32_truncates() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 3.14.toInt32() == 3
"#,
    )
    .expect("3.14 truncated to int32 should be 3");
}

#[test]
fn test_float32_to_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 7.9f32.toInt32() == 7
"#,
    )
    .expect("7.9f32 truncated to int32 should be 7");
}

#[test]
fn test_float64_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 1000.5.toInt64() == 1000i64
"#,
    )
    .expect("float64 to int64");
}

#[test]
fn test_float64_to_uint8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 200.9.toUint8() == 200u8
"#,
    )
    .expect("float64 to uint8");
}

// ── Float ↔ Float conversions ────────────────────────────────────────

#[test]
fn test_float32_to_float64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Float64 = 3.0f32.toFloat64()
    assert x == 3.0
"#,
    )
    .expect("float32 to float64");
}

#[test]
fn test_float64_to_float32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 3.0.toFloat32() == 3.0f32
"#,
    )
    .expect("float64 to float32");
}

// ── Negative value preservation ──────────────────────────────────────

#[test]
fn test_negative_int8_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (-5i8).toInt64() == -5i64
"#,
    )
    .expect("negative int8 to int64 preserves sign");
}

#[test]
fn test_negative_int32_to_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (-100).toInt64() == -100i64
"#,
    )
    .expect("negative int32 to int64 preserves sign");
}

#[test]
fn test_negative_float_to_int() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert (-3.7).toInt32() == -3
"#,
    )
    .expect("negative float truncated toward zero");
}

// ── Chaining conversions ─────────────────────────────────────────────

#[test]
fn test_chained_conversions() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 42.toInt64().toFloat64().toInt32() == 42
"#,
    )
    .expect("chained conversions");
}

#[test]
fn test_int8_to_int16_to_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 10i8.toInt16().toInt32() == 10
"#,
    )
    .expect("int8 → int16 → int32 chain");
}
