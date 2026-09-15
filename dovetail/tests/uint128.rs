// Integration tests for the `Uint128` built-in integer primitive (lo/hi `[i64, i64]`
// flattened representation; dedicated `$Uint128` box). See docs/crypto-library-design.md §3
// and docs/tuple-multivalue-codegen-design.md (Phase 6).

mod common;

#[test]
fn test_uint128_literal_and_conversion_roundtrip() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Uint128 = 42u128
    assert a == 42u128
    let b = 5u64.toUint128()
    assert b.toUint64() == 5u64
    assert b == 5u128
"#,
    )
    .expect("uint128 literal + conversion roundtrip");
}

#[test]
fn test_uint128_add_carry_across_64_bit_boundary() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = 18446744073709551615u64.toUint128()
    let b = 1u64.toUint128()
    let c = a + b
    assert c.toUint64() == 0u64
    assert (c >> 64u128).toUint64() == 1u64
"#,
    )
    .expect("uint128 add carries into the high word");
}

#[test]
fn test_uint128_sub_borrow_across_64_bit_boundary() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let twoPow64 = 18446744073709551615u64.toUint128() + 1u64.toUint128()
    let c = twoPow64 - 1u128
    assert c.toUint64() == 18446744073709551615u64
    assert (c >> 64u128).toUint64() == 0u64
"#,
    )
    .expect("uint128 sub borrows from the high word");
}

#[test]
fn test_uint128_widening_multiply_intrinsic_and_peephole_agree() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 4000000000u64
    let y = 5000000000u64
    let viaIntrinsic = Uint128.multiply(x, y)
    let viaOperator = x.toUint128() * y.toUint128()
    assert viaIntrinsic == viaOperator
    // 4e9 * 5e9 = 2e19, which overflows u64 (max ~1.8e19) so the high word is non-zero.
    assert (viaIntrinsic >> 64u128).toUint64() == 1u64
"#,
    )
    .expect("widening multiply: intrinsic and peephole agree");
}

#[test]
fn test_uint128_general_multiply() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let twoPow64 = 18446744073709551615u64.toUint128() + 1u64.toUint128()
    // twoPow64 is not a widening of a u64, so `*` uses the composed 128x128 multiply.
    let c = twoPow64 * 3u64.toUint128()
    assert c.toUint64() == 0u64
    assert (c >> 64u128).toUint64() == 3u64
"#,
    )
    .expect("uint128 general 128x128 multiply");
}

#[test]
fn test_uint128_shifts_crossing_64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let one = 1u128
    let shifted = one << 100u128
    assert shifted.toUint64() == 0u64
    assert (shifted >> 64u128).toUint64() == 68719476736u64
    // shift right back down
    assert (shifted >> 100u128) == 1u128
    // a within-word shift
    assert (1u128 << 4u128) == 16u128
"#,
    )
    .expect("uint128 shifts crossing the 64-bit boundary");
}

#[test]
fn test_uint128_bitwise() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = 0xFF00FF00FF00FF00u64.toUint128()
    let b = 0x00FF00FF00FF00FFu64.toUint128()
    assert (a | b).toUint64() == 18446744073709551615u64
    assert (a & b).toUint64() == 0u64
    assert (a ^ b).toUint64() == 18446744073709551615u64
"#,
    )
    .expect("uint128 bitwise operators");
}

#[test]
fn test_uint128_comparisons() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let small = 5u128
    let big = 18446744073709551615u64.toUint128() + 1u64.toUint128()
    assert small < big
    assert big > small
    assert small <= small
    assert big >= big
    assert small != big
    assert (small == small)
    // tie on the high word, differ on the low word
    let lo1 = 10u128
    let lo2 = 20u128
    assert lo1 < lo2
    assert lo2 > lo1
"#,
    )
    .expect("uint128 comparisons");
}

#[test]
fn test_uint128_array_get_set() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = Array.fill(3, 0u128)
    arr.set(0, 7u128)
    arr.set(1, 18446744073709551615u64.toUint128() + 9u64.toUint128())
    assert arr.get(0) == 7u128
    let v = arr.get(1)
    assert v.toUint64() == 8u64
    assert (v >> 64u128).toUint64() == 1u64
    assert arr.get(2) == 0u128
"#,
    )
    .expect("Array<Uint128> get/set");
}

#[test]
fn test_uint128_param_and_return() {
    common::compile_and_run(
        r#"
package a

function addOne(x: Uint128): Uint128 = x + 1u128

function pair(): (Uint128, Bool) = (170141183460469231731687303715884105727u128, true)

function main(): Unit =
    assert addOne(41u128) == 42u128
    let p = pair()
    assert p._0 == 170141183460469231731687303715884105727u128
    assert p._1 == true
"#,
    )
    .expect("uint128 as param, return, and multi-value tuple return");
}

#[test]
fn test_uint128_boxed_tuple_element() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    // An array of tuples boxes the whole tuple; the Uint128 element boxes to its dedicated struct.
    let arr = Array.fill(2, (1u128, 0i32))
    arr.set(0, (18446744073709551615u64.toUint128() + 1u64.toUint128(), 7i32))
    let t = arr.get(0)
    assert t._0.toUint64() == 0u64
    assert (t._0 >> 64u128).toUint64() == 1u64
    assert t._1 == 7i32
"#,
    )
    .expect("Uint128 inside a boxed tuple element");
}

#[test]
fn test_uint128_high_word_extraction_via_shift() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let v = Uint128.multiply(18446744073709551615u64, 18446744073709551615u64)
    // (2^64 - 1)^2 = 2^128 - 2^65 + 1; low word = 1, high word = 2^64 - 2
    assert v.toUint64() == 1u64
    assert (v >> 64u128).toUint64() == 18446744073709551614u64
"#,
    )
    .expect("uint128 widening square, high/low extraction");
}

#[test]
fn test_uint128_make_low_high() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let lo = 18446744073709551615u64
    let hi = 12345678901234567890u64
    let v = Uint128.make(lo, hi)
    assert v.low == lo
    assert v.high == hi
    // make(lo, hi) == lo | (hi << 64)
    let composed = lo.toUint128() | (hi.toUint128() << 64u128)
    assert v == composed
"#,
    )
    .expect("Uint128.make + low/high accessors");
}

#[test]
fn test_uint128_low_high_agree_with_widening_multiply() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let v = Uint128.multiply(18446744073709551615u64, 18446744073709551615u64)
    // (2^64 - 1)^2 = 2^128 - 2^65 + 1; low word = 1, high word = 2^64 - 2
    assert v.low == 1u64
    assert v.high == 18446744073709551614u64
    // Reconstruct from the words and compare.
    assert Uint128.make(v.low, v.high) == v
"#,
    )
    .expect("low/high agree with widening multiply decomposition");
}
