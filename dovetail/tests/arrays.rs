mod common;

#[test]
fn test_array_type_annotation_named() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x: Array<Int32> = [|1, 2, 3|]
    ()
"#).expect("array with named type annotation");
}

#[test]
fn test_array_literal_infers_type() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x = [|1, 2, 3|]
    ()
"#).expect("array literal type inference");
}

#[test]
fn test_array_literal_type_mismatch() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let x = [|1, "hello"|]
    ()
"#);
    assert!(!errors.is_empty(), "expected type mismatch error");
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch, got: {:?}",
        errors
    );
}

#[test]
fn test_empty_array_literal_error() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let x = [||]
    ()
"#);
    assert!(!errors.is_empty(), "expected error for empty array");
    assert!(
        errors.iter().any(|e| e.contains("cannot infer element type")),
        "expected 'cannot infer element type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_array_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let x: Array<Int32, Bool> = [|1|]
    ()
"#);
    assert!(!errors.is_empty(), "expected error for wrong type arg count");
    assert!(
        errors.iter().any(|e| e.contains("expected 1 type argument")),
        "expected '1 type argument' error, got: {:?}",
        errors
    );
}

#[test]
fn test_bare_array_without_type_args() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let x: Array = [|1|]
    ()
"#);
    assert!(!errors.is_empty(), "expected error for bare Array");
    assert!(
        errors.iter().any(|e| e.contains("expected 1 type argument")),
        "expected '1 type argument' error, got: {:?}",
        errors
    );
}

#[test]
fn test_nested_array_named() {
    common::check_no_errors(r#"
package a

function main(): Unit =
    let x: Array<Array<Int32>> = [|[|1, 2|], [|3, 4|]|]
    ()
"#);
}

#[test]
fn test_array_as_function_parameter() {
    common::compile_and_run(r#"
package a

function first(arr: Array<Int32>): Int32 = arr.get(0)

function main(): Unit =
    assert first([|10, 20, 30|]) == 10
"#).expect("array as function parameter");
}

#[test]
fn test_array_as_return_type() {
    common::compile_and_run(r#"
package a

function make_array(): Array<Int32> = [|10, 20, 30|]

function main(): Unit =
    let arr = make_array()
    assert arr.get(2) == 30
"#).expect("array as return type");
}

#[test]
fn test_array_of_bool() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x: Array<Bool> = [|true, false, true|]
    assert x.length == 3
    assert x.get(0) == true
    assert x.get(2) == true
"#).expect("array of bool");
}

#[test]
fn test_array_of_string() {
    common::check_no_errors(r#"
package a

function main(): Unit =
    let x: Array<String> = [|"hello", "world"|]
    ()
"#);
}

#[test]
fn test_array_of_char() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x: Array<Char> = [|'a', 'b', 'c'|]
    assert x.length == 3
    assert x.get(0) == 'a'
    assert x.get(2) == 'c'
"#).expect("array of char");
}

#[test]
fn test_array_of_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x: Array<Float64> = [|1.0, 2.5, 3.14|]
    assert x.length == 3
"#).expect("array of float64");
}

#[test]
fn test_array_literal_with_trailing_comma() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let x = [|1, 2, 3,|]
    assert x.length == 3
"#).expect("array with trailing comma");
}

// --- Codegen tests: array get ---

#[test]
fn test_array_get() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr.get(0) == 10
    assert arr.get(1) == 20
    assert arr.get(2) == 30
"#).expect("array get");
}

// --- Codegen tests: array set ---

#[test]
fn test_array_set() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    arr.set(1, 99)
    assert arr.get(1) == 99
"#).expect("array set");
}

// --- Codegen tests: array length ---

#[test]
fn test_array_length() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr.length == 3
"#).expect("array length");
}

// --- Codegen tests: single element ---

#[test]
fn test_single_element_array() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|42|].get(0) == 42
"#).expect("single element array");
}

// --- Codegen tests: chained ---

#[test]
fn test_array_chained_length() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2, 3|].length == 3
"#).expect("chained array length");
}

// --- Codegen tests: non-Int32 primitive element types ---

#[test]
fn test_array_of_uint32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Uint32> = [|10u32, 20u32, 30u32|]
    assert arr.length == 3
    assert arr.get(0) == 10u32
    assert arr.get(2) == 30u32
    arr.set(1, 99u32)
    assert arr.get(1) == 99u32
"#).expect("array of uint32");
}

#[test]
fn test_array_of_int64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Int64> = [|100i64, 200i64, 300i64|]
    assert arr.length == 3
    assert arr.get(0) == 100i64
    assert arr.get(2) == 300i64
    arr.set(1, 999i64)
    assert arr.get(1) == 999i64
"#).expect("array of int64");
}

#[test]
fn test_array_of_uint64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Uint64> = [|1u64, 2u64, 3u64|]
    assert arr.length == 3
    assert arr.get(0) == 1u64
    assert arr.get(2) == 3u64
    arr.set(0, 42u64)
    assert arr.get(0) == 42u64
"#).expect("array of uint64");
}

#[test]
fn test_array_of_float32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Float32> = [|1.0f32, 2.5f32, 3.14f32|]
    assert arr.length == 3
"#).expect("array of float32");
}

#[test]
fn test_array_of_int64_as_param() {
    common::compile_and_run(r#"
package a

function array_len(arr: Array<Int64>): Int32 = arr.length

function main(): Unit =
    let arr: Array<Int64> = [|10i64, 20i64|]
    assert array_len(arr) == 2
"#).expect("array of int64 as function parameter");
}

// --- Codegen tests: packed primitive array types (Int8, Uint8, Int16, Uint16) ---

#[test]
fn test_array_of_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Int8> = [|1i8, -1i8, 127i8|]
    assert arr.length == 3
    assert arr.get(0) == 1i8
    assert arr.get(1) == -1i8
    assert arr.get(2) == 127i8
    arr.set(0, 42i8)
    assert arr.get(0) == 42i8
"#).expect("array of int8");
}

#[test]
fn test_array_of_uint8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Uint8> = [|0u8, 128u8, 255u8|]
    assert arr.length == 3
    assert arr.get(0) == 0u8
    assert arr.get(1) == 128u8
    assert arr.get(2) == 255u8
    arr.set(1, 42u8)
    assert arr.get(1) == 42u8
"#).expect("array of uint8");
}

#[test]
fn test_array_of_int16() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Int16> = [|1i16, -1i16, 32767i16|]
    assert arr.length == 3
    assert arr.get(0) == 1i16
    assert arr.get(1) == -1i16
    assert arr.get(2) == 32767i16
    arr.set(0, 100i16)
    assert arr.get(0) == 100i16
"#).expect("array of int16");
}

#[test]
fn test_array_of_uint16() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Uint16> = [|0u16, 256u16, 65535u16|]
    assert arr.length == 3
    assert arr.get(0) == 0u16
    assert arr.get(1) == 256u16
    assert arr.get(2) == 65535u16
    arr.set(2, 1000u16)
    assert arr.get(2) == 1000u16
"#).expect("array of uint16");
}

#[test]
fn test_packed_array_as_param() {
    common::compile_and_run(r#"
package a

function sum_i8(arr: Array<Int8>): Int8 =
    arr.get(0) + arr.get(1) + arr.get(2)

function first_u16(arr: Array<Uint16>): Uint16 = arr.get(0)

function main(): Unit =
    assert sum_i8([|10i8, 20i8, 30i8|]) == 60i8
    assert first_u16([|1000u16, 2000u16|]) == 1000u16
"#).expect("packed array as function parameter");
}

// --- Codegen tests: Array.fill ---

#[test]
fn test_array_fill_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(5, 42)
    assert arr.length == 5
    assert arr.get(0) == 42
    assert arr.get(4) == 42
"#).expect("array fill int32");
}

#[test]
fn test_array_fill_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(3, 1i8)
    assert arr.length == 3
    assert arr.get(0) == 1i8
    assert arr.get(2) == 1i8
"#).expect("array fill int8");
}

#[test]
fn test_array_fill_uint16() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(4, 100u16)
    assert arr.length == 4
    assert arr.get(0) == 100u16
    assert arr.get(3) == 100u16
"#).expect("array fill uint16");
}

#[test]
fn test_array_fill_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(2, 3.14)
    assert arr.length == 2
"#).expect("array fill float64");
}

#[test]
fn test_array_fill_bool() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(3, true)
    assert arr.length == 3
    assert arr.get(0) == true
    assert arr.get(2) == true
"#).expect("array fill bool");
}

#[test]
fn test_array_fill_zero_length() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(0, 42)
    assert arr.length == 0
"#).expect("array fill zero length");
}

#[test]
fn test_array_fill_then_set() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(3, 0)
    arr.set(1, 99)
    assert arr.get(0) == 0
    assert arr.get(1) == 99
    assert arr.get(2) == 0
"#).expect("array fill then set");
}

#[test]
fn test_array_fill_as_param() {
    common::compile_and_run(r#"
package a

function sum3(arr: Array<Int32>): Int32 =
    arr.get(0) + arr.get(1) + arr.get(2)

function main(): Unit =
    assert sum3(Array.fill(3, 10)) == 30
"#).expect("array fill as param");
}

#[test]
fn test_array_fill_wrong_arg_count() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let arr = Array.fill(5)
    ()
"#);
    assert!(!errors.is_empty(), "expected error for wrong arg count");
    assert!(
        errors.iter().any(|e| e.contains("no matching overload") || e.contains("undefined variable")),
        "expected 'no matching overload' error, got: {:?}",
        errors
    );
}

#[test]
fn test_array_fill_non_int32_length() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let arr = Array.fill(5i64, 42)
    ()
"#);
    assert!(!errors.is_empty(), "expected error for non-Int32 length");
    assert!(
        errors.iter().any(|e| e.contains("no matching overload") || e.contains("undefined variable")),
        "expected 'no matching overload' error, got: {:?}",
        errors
    );
}

// --- Array equality tests ---

#[test]
fn test_array_eq_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2, 3|] == [|1, 2, 3|]
    let a = [|1, 2, 3|]
    let b = [|1, 2, 4|]
    assert !(a == b)
"#).expect("array eq int32");
}

#[test]
fn test_array_ne_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2|] != [|1, 3|]
    assert !([|1, 2|] != [|1, 2|])
"#).expect("array ne int32");
}

#[test]
fn test_array_eq_different_lengths() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert !([|1, 2|] == [|1, 2, 3|])
    assert [|1, 2|] != [|1, 2, 3|]
"#).expect("array eq different lengths");
}

#[test]
fn test_array_eq_empty() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = Array.fill(0, 0)
    let b = Array.fill(0, 0)
    assert a == b
"#).expect("array eq empty");
}

#[test]
fn test_array_eq_bool() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|true, false|] == [|true, false|]
    assert [|true, false|] != [|false, true|]
"#).expect("array eq bool");
}

#[test]
fn test_array_eq_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a: Array<Int8> = [|1i8, 2i8, 3i8|]
    let b: Array<Int8> = [|1i8, 2i8, 3i8|]
    assert a == b
    let c: Array<Int8> = [|1i8, 2i8, 4i8|]
    assert a != c
"#).expect("array eq int8");
}

#[test]
fn test_array_eq_uint16() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a: Array<Uint16> = [|100u16, 200u16|]
    let b: Array<Uint16> = [|100u16, 200u16|]
    assert a == b
    let c: Array<Uint16> = [|100u16, 201u16|]
    assert a != c
"#).expect("array eq uint16");
}

#[test]
fn test_array_eq_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1.0, 2.5, 3.14|] == [|1.0, 2.5, 3.14|]
    assert [|1.0, 2.5|] != [|1.0, 2.6|]
"#).expect("array eq float64");
}

#[test]
fn test_array_eq_char() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|'a', 'b', 'c'|] == [|'a', 'b', 'c'|]
    assert [|'a', 'b'|] != [|'a', 'c'|]
"#).expect("array eq char");
}

// --- Array comparison tests ---

#[test]
fn test_array_lt_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2|] < [|1, 3|]
    assert !([|1, 3|] < [|1, 2|])
    assert !([|1, 2|] < [|1, 2|])
"#).expect("array lt int32");
}

#[test]
fn test_array_gt_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 3|] > [|1, 2|]
    assert !([|1, 2|] > [|1, 3|])
    assert !([|1, 2|] > [|1, 2|])
"#).expect("array gt int32");
}

#[test]
fn test_array_le_ge_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2|] <= [|1, 2|]
    assert [|1, 2|] >= [|1, 2|]
    assert [|1, 2|] <= [|1, 3|]
    assert [|1, 3|] >= [|1, 2|]
"#).expect("array le ge int32");
}

#[test]
fn test_array_cmp_prefix() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1, 2|] < [|1, 2, 3|]
    assert [|1, 2, 3|] > [|1, 2|]
    assert !([|1, 2, 3|] < [|1, 2|])
"#).expect("array cmp prefix");
}

#[test]
fn test_array_cmp_uint32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a: Array<Uint32> = [|1u32, 2u32|]
    let b: Array<Uint32> = [|1u32, 3u32|]
    assert a < b
    assert b > a
"#).expect("array cmp uint32");
}

#[test]
fn test_array_cmp_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a: Array<Int8> = [|-1i8, 0i8|]
    let b: Array<Int8> = [|0i8, 0i8|]
    assert a < b
    assert b > a
"#).expect("array cmp int8");
}

#[test]
fn test_array_cmp_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    assert [|1.0, 2.0|] < [|1.0, 3.0|]
    assert [|1.0, 3.0|] > [|1.0, 2.0|]
    assert [|1.0, 2.0|] <= [|1.0, 2.0|]
"#).expect("array cmp float64");
}

// --- Array error tests ---

#[test]
fn test_array_bool_no_comparison() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let a = [|true, false|]
    let b = [|false, true|]
    let r = a < b
    ()
"#);
    assert!(!errors.is_empty(), "expected error for bool array comparison");
    assert!(
        errors.iter().any(|e| e.contains("not supported")),
        "expected 'not supported' error, got: {:?}",
        errors
    );
}

// --- Array.clone tests ---

#[test]
fn test_array_clone_independent_copy() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = [|1, 2, 3|]
    let b = a.clone()
    b.set(0, 99)
    assert a.get(0) == 1
    assert b.get(0) == 99
"#).expect("array clone independent copy");
}

#[test]
fn test_array_clone_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a: Array<Int8> = [|10i8, 20i8, 30i8|]
    let b = a.clone()
    b.set(0, 99i8)
    assert a.get(0) == 10i8
    assert b.get(0) == 99i8
    assert b.length == 3
"#).expect("array clone int8");
}

#[test]
fn test_array_clone_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = [|1.0, 2.5, 3.14|]
    let b = a.clone()
    assert b.length == 3
"#).expect("array clone float64");
}

#[test]
fn test_array_clone_empty() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = Array.fill(0, 0)
    let b = a.clone()
    assert b.length == 0
"#).expect("array clone empty");
}

// --- Reference-element array tests ---

#[test]
fn test_array_string_literal_and_get() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|"a", "b", "c"|]
    assert arr.get(1) == "b"
"#).expect("array string literal and get");
}

#[test]
fn test_array_string_set() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|"a", "b", "c"|]
    arr.set(1, "z")
    assert arr.get(1) == "z"
    assert arr.get(0) == "a"
"#).expect("array string set");
}

#[test]
fn test_array_string_length() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|"a", "b"|]
    assert arr.length == 2
"#).expect("array string length");
}

#[test]
fn test_array_string_fill() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array.fill(3, "x")
    assert arr.length == 3
    assert arr.get(0) == "x"
    assert arr.get(1) == "x"
    assert arr.get(2) == "x"
"#).expect("array string fill");
}

#[test]
fn test_array_string_clone() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = [|"hello", "world"|]
    let b = a.clone()
    b.set(0, "changed")
    assert a.get(0) == "hello"
    assert b.get(0) == "changed"
    assert b.length == 2
"#).expect("array string clone");
}

#[test]
fn test_array_string_clone_empty() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = Array.fill(0, "x")
    let b = a.clone()
    assert b.length == 0
"#).expect("array string clone empty");
}

#[test]
fn test_array_record() {
    common::compile_and_run(r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p1 = Point { x = 1; y = 2 }
    let p2 = Point { x = 3; y = 4 }
    let arr = [|p1, p2|]
    assert arr.length == 2
    assert arr.get(0).x == 1
    assert arr.get(1).y == 4
    let p3 = Point { x = 5; y = 6 }
    arr.set(0, p3)
    assert arr.get(0).x == 5
"#).expect("array of records");
}

#[test]
fn test_array_nested_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let inner1 = [|1, 2, 3|]
    let inner2 = [|4, 5, 6|]
    let outer: Array<Array<Int32>> = [|inner1, inner2|]
    assert outer.length == 2
    assert outer.get(0).get(0) == 1
    assert outer.get(1).get(2) == 6
    let inner3 = [|7, 8, 9|]
    outer.set(0, inner3)
    assert outer.get(0).get(0) == 7
"#).expect("nested array of int32");
}

#[test]
fn test_array_string_eq() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = [|"hello"|]
    let b = [|"hello"|]
    assert a == b
    assert a != [|"world"|]
"#).expect("array string equality via generic Equatable impl");
}

// --- Array indexing sugar tests ---

#[test]
fn test_array_index_get() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr[0] == 10
    assert arr[1] == 20
    assert arr[2] == 30
"#).expect("array index get");
}

#[test]
fn test_array_index_set() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    arr[1] = 99
    assert arr[1] == 99
"#).expect("array index set");
}

#[test]
fn test_array_index_chained() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let inner1 = [|1, 2, 3|]
    let inner2 = [|4, 5, 6|]
    let outer: Array<Array<Int32>> = [|inner1, inner2|]
    assert outer[0][1] == 2
    assert outer[1][2] == 6
"#).expect("array index chained");
}

#[test]
fn test_array_index_in_expr() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr[0] + arr[1] == 30
    assert arr[2] - arr[0] == 20
"#).expect("array index in expr");
}

#[test]
fn test_array_index_string() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|"hello", "world"|]
    assert arr[0] == "hello"
    assert arr[1] == "world"
"#).expect("array index string");
}

#[test]
fn test_array_index_set_string() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = [|"hello", "world"|]
    arr[0] = "new"
    assert arr[0] == "new"
"#).expect("array index set string");
}

#[test]
fn test_array_index_non_array_error() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let x = 42
    let r = x[0]
    ()
"#);
    assert!(!errors.is_empty(), "expected error for indexing non-array");
    assert!(
        errors.iter().any(|e| e.contains("index operator requires Index<Int32> for Int32")),
        "expected 'index operator requires Index<Int32> for Int32' error, got: {:?}",
        errors
    );
}

#[test]
fn test_array_index_non_int_error() {
    let errors = common::compile_expecting_errors(r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3|]
    let r = arr["hello"]
    ()
"#);
    assert!(!errors.is_empty(), "expected error for non-int index");
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

// --- Array<T>.empty tests ---

#[test]
fn test_array_empty_int32() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array<Int32>.empty()
    assert arr.length == 0
"#).expect("Array<Int32>.empty()");
}

#[test]
fn test_array_empty_bool() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array<Bool>.empty()
    assert arr.length == 0
"#).expect("Array<Bool>.empty()");
}

#[test]
fn test_array_empty_float64() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array<Float64>.empty()
    assert arr.length == 0
"#).expect("Array<Float64>.empty()");
}

#[test]
fn test_array_empty_string() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array<String>.empty()
    assert arr.length == 0
"#).expect("Array<String>.empty()");
}

#[test]
fn test_array_empty_int8() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr = Array<Int8>.empty()
    assert arr.length == 0
"#).expect("Array<Int8>.empty()");
}

#[test]
fn test_array_empty_multiple_types() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = Array<Int32>.empty()
    let b = Array<Bool>.empty()
    let c = Array<String>.empty()
    assert a.length == 0
    assert b.length == 0
    assert c.length == 0
"#).expect("multiple empty array types");
}

#[test]
fn test_array_empty_equality() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let a = Array<Int32>.empty()
    let b = Array<Int32>.empty()
    assert a == b
"#).expect("empty array equality");
}

#[test]
fn test_array_empty_with_type_annotation() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Int32> = Array<Int32>.empty()
    assert arr.length == 0
"#).expect("empty array with type annotation");
}

// --- Array.empty() with inferred T tests ---

#[test]
fn test_array_empty_inferred_from_let_binding() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let arr: Array<Int32> = Array.empty()
    assert arr.length == 0
"#).expect("Array.empty() inferred from let binding");
}

#[test]
fn test_array_empty_inferred_from_function_param() {
    common::compile_and_run(r#"
package a

function check_empty(arr: Array<Int32>): Unit =
    assert arr.length == 0

function main(): Unit =
    check_empty(Array.empty())
"#).expect("Array.empty() inferred from function parameter");
}

// --- Array.empty() safety: extend/concat on empty create new arrays ---

#[test]
fn test_array_empty_extend_creates_new_array() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let empty = Array<Int32>.empty()
    let grown = empty.extend(42, 3)
    assert empty.length == 0
    assert grown.length == 3
    assert grown[0] == 42
"#).expect("extend on empty creates new array");
}

#[test]
fn test_array_empty_concat_creates_new_array() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let empty = Array<Int32>.empty()
    let other = [|1, 2, 3|]
    let result = empty.concat(other)
    assert empty.length == 0
    assert result.length == 3
    assert result[0] == 1
"#).expect("concat on empty creates new array");
}

#[test]
fn test_array_copy_basic() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let src = [|10, 20, 30, 40, 50|]
    let dst = Array.fill(5, 0)
    src.copy(0, dst, 0, 5)
    assert dst[0] == 10
    assert dst[1] == 20
    assert dst[2] == 30
    assert dst[3] == 40
    assert dst[4] == 50
"#).expect("basic array copy");
}

#[test]
fn test_array_copy_with_offsets() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let src = [|10, 20, 30, 40, 50|]
    let dst = Array.fill(5, 0)
    src.copy(1, dst, 2, 3)
    assert dst[0] == 0
    assert dst[1] == 0
    assert dst[2] == 20
    assert dst[3] == 30
    assert dst[4] == 40
"#).expect("array copy with offsets");
}

#[test]
fn test_array_copy_returns_destination() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let src = [|1, 2, 3|]
    let dst = Array.fill(3, 0)
    let result = src.copy(0, dst, 0, 3)
    assert result[0] == 1
    assert result[1] == 2
    assert result[2] == 3
"#).expect("array copy returns destination");
}

#[test]
fn test_array_copy_partial() {
    common::compile_and_run(r#"
package a

function main(): Unit =
    let src = [|100, 200, 300|]
    let dst = Array.fill(5, 0)
    src.copy(0, dst, 0, 2)
    assert dst[0] == 100
    assert dst[1] == 200
    assert dst[2] == 0
    assert dst[3] == 0
    assert dst[4] == 0
"#).expect("partial array copy");
}
