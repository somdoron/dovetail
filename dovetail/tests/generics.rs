mod common;

#[test]
fn test_generic_identity_explicit_int32() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit = assert identity<Int32>(42) == 42
"#,
    )
    .expect("identity<Int32>(42) should return 42");
}

#[test]
fn test_generic_identity_inferred_int32() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit = assert identity(42) == 42
"#,
    )
    .expect("identity(42) should infer T=Int32 and return 42");
}

#[test]
fn test_generic_identity_bool() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit = assert identity(true)
"#,
    )
    .expect("identity(true) should return true");
}

#[test]
fn test_generic_identity_float64() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit = assert identity(3.14f64) == 3.14f64
"#,
    )
    .expect("identity(3.14f64) should return 3.14f64");
}

#[test]
fn test_generic_two_type_params() {
    common::compile_and_run(
        r#"
package a

function first<A, B>(a: A, b: B): A = a

function main(): Unit = assert first(10, true) == 10
"#,
    )
    .expect("first(10, true) should return 10");
}

#[test]
fn test_generic_two_type_params_second() {
    common::compile_and_run(
        r#"
package a

function second<A, B>(a: A, b: B): B = b

function main(): Unit = assert second(10, true)
"#,
    )
    .expect("second(10, true) should return true");
}

#[test]
fn test_generic_multiple_specializations() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    assert identity(42) == 42
    assert identity(true)
    assert identity(100i64) == 100i64
"#,
    )
    .expect("multiple specializations of identity should all work");
}

#[test]
fn test_generic_calling_generic() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function wrap<T>(x: T): T = identity(x)

function main(): Unit = assert wrap(42) == 42
"#,
    )
    .expect("wrap(42) calling identity(x) should return 42");
}

#[test]
fn test_generic_explicit_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit = identity<Int32, Bool>(42)
"#,
    );
    assert!(!errors.is_empty(), "should error on wrong type arg count");
}

#[test]
fn test_generic_with_non_generic_overload() {
    common::compile_and_run(
        r#"
package a

function double(x: Int32): Int32 = x + x

function main(): Unit = assert double(21) == 42
"#,
    )
    .expect("non-generic overload should still work");
}

#[test]
fn test_generic_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    assert identity<Bool>(true)
    assert identity<Int32>(42) == 42
"#,
    )
    .expect("explicit type args should work");
}

#[test]
fn test_generic_with_computation() {
    common::compile_and_run(
        r#"
package a

function addAndReturn<T>(x: T, y: Int32): T = x

function main(): Unit = assert addAndReturn(true, 42)
"#,
    )
    .expect("generic function with mixed params should work");
}

// ── Generic Records ──────────────────────────────────────────────────

#[test]
fn test_generic_record_box_int32() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.value == 42
"#,
    )
    .expect("Box<Int32> construction and field access");
}

#[test]
fn test_generic_record_box_bool() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Bool> { value = true }
    assert b.value
"#,
    )
    .expect("Box<Bool> construction and field access");
}

#[test]
fn test_generic_record_box_float64() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Float64> { value = 3.14f64 }
    assert b.value == 3.14f64
"#,
    )
    .expect("Box<Float64> construction and field access");
}

#[test]
fn test_generic_record_pair() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function main(): Unit =
    let p = Pair<Int32, Bool> { first = 1; second = true }
    assert p.first == 1
    assert p.second
"#,
    )
    .expect("Pair<Int32, Bool> construction and field access");
}

#[test]
fn test_generic_record_multiple_specializations() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b1 = Box<Int32> { value = 42 }
    let b2 = Box<Bool> { value = true }
    let b3 = Box<Int64> { value = 100i64 }
    assert b1.value == 42
    assert b2.value
    assert b3.value == 100i64
"#,
    )
    .expect("multiple specializations of Box");
}

#[test]
fn test_generic_record_with_expression() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function main(): Unit =
    let p = Pair<Int32, Bool> { first = 1; second = true }
    let p2 = p with first = 99
    assert p2.first == 99
    assert p2.second
"#,
    )
    .expect("with expression on generic record");
}

#[test]
fn test_generic_function_with_generic_record() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function unbox<T>(b: Box<T>): T = b.value

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert unbox(b) == 42
"#,
    )
    .expect("generic function + generic record");
}

#[test]
fn test_generic_record_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Int32, Bool> { value = 42 }
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "should error on wrong type arg count for generic record"
    );
}

#[test]
fn test_generic_record_infer_type_args_from_fields() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box { value = 42 }
    assert b.value == 42
"#,
    )
    .expect("should infer T=Int32 from field value");
}

#[test]
fn test_generic_record_field_type_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Int32> { value = true }
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "should error on field type mismatch after specialization"
    );
}

// ── Shared Generic Functions ─────────────────────────────────────────

#[test]
fn test_shared_identity_with_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function identity<T>(x: T): T = x

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    let p2 = identity(p)
    assert p2.x == 10
    assert p2.y == 20
"#,
    )
    .expect("shared identity with record should work");
}

#[test]
fn test_shared_identity_with_string() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let s = identity("hello")
    assert s == "hello"
"#,
    )
    .expect("shared identity with String should work");
}

#[test]
fn test_mixed_shared_and_specialized() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function identity<T>(x: T): T = x

function main(): Unit =
    assert identity(42) == 42
    let p = identity(Point { x = 1; y = 2 })
    assert p.x == 1
"#,
    )
    .expect("mixed shared + specialized identity should work");
}

#[test]
fn test_shared_nested_forwarding() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function identity<T>(x: T): T = x
function wrap<T>(x: T): T = identity(x)

function main(): Unit =
    let p = Point { x = 5; y = 10 }
    let p2 = wrap(p)
    assert p2.x == 5
    assert p2.y == 10
"#,
    )
    .expect("nested shared forwarding should work");
}

#[test]
fn test_shared_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function identity<T>(x: T): T = x

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    let p2 = identity<Point>(p)
    assert p2.x == 3
"#,
    )
    .expect("shared explicit type args should work");
}

#[test]
fn test_shared_two_type_params() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32
record Color = r: Int32; g: Int32; b: Int32

function first<A, B>(a: A, b: B): A = a

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let c = Color { r = 255; g = 0; b = 0 }
    let result = first(p, c)
    assert result.x == 1
    assert result.y == 2
"#,
    )
    .expect("shared two type params should work");
}

// ── Type Parameter Shadowing ─────────────────────────────────────────

#[test]
fn test_shadow_type_param_with_let() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point = x: Int32; y: Int32

function foo<T>(x: T): String =
    let T = "hello"
    T

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    foo(p)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows type parameter")),
        "expected shadow error, got: {:?}",
        errors
    );
}

#[test]
fn test_shadow_type_param_case_sensitive() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function foo<T>(x: T): T =
    let t = x
    t

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = foo(p)
    assert p2.x == 1
"#,
    )
    .expect("lowercase t should not shadow T");
}

#[test]
fn test_shadow_second_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point = x: Int32; y: Int32
record Color = r: Int32; g: Int32; b: Int32

function foo<A, B>(a: A, b: B): A =
    let B = a
    B

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let c = Color { r = 0; g = 0; b = 0 }
    foo(p, c)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows type parameter")),
        "expected shadow error for B, got: {:?}",
        errors
    );
}

#[test]
fn test_shadow_type_param_in_match_arm() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point = x: Int32; y: Int32

function foo<T>(x: T): Bool =
    match true with
        case T => T

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    foo(p)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows type parameter")),
        "expected shadow error in match arm, got: {:?}",
        errors
    );
}

#[test]
fn test_shadow_type_param_in_match_arm_ok() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function foo<T>(x: T): Bool =
    match true with
        case t => t

function main(): Unit = assert foo(Point { x = 1; y = 2 })
"#,
    )
    .expect("lowercase t in match arm should not shadow T");
}

#[test]
fn test_shadow_type_param_in_record_pattern() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point = x: Int32; y: Int32

function foo<T>(p: T): Int32 =
    match Point { x = 1; y = 2 } with
        case Point { x = T, y } => T + y

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    foo(p)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows type parameter")),
        "expected shadow error in record pattern, got: {:?}",
        errors
    );
}

// ── Shared Generic Records ──────────────────────────────────────────

#[test]
fn test_shared_record_box_string() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert b.value == "hello"
"#,
    )
    .expect("Box<String> construction and field access");
}

#[test]
fn test_shared_record_box_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32
record Box<T> = value: T

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    let b = Box<Point> { value = p }
    assert b.value.x == 10
    assert b.value.y == 20
"#,
    )
    .expect("Box<Point> construction and nested field access");
}

#[test]
fn test_shared_record_pair_strings() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function main(): Unit =
    let p = Pair<String, String> { first = "hello"; second = "world" }
    assert p.first == "hello"
    assert p.second == "world"
"#,
    )
    .expect("Pair<String, String> with multiple type-info fields");
}

#[test]
fn test_shared_record_mixed_fields() {
    common::compile_and_run(
        r#"
package a

record Tagged<T> = tag: Int32; value: T

function main(): Unit =
    let t = Tagged<String> { tag = 42; value = "hello" }
    assert t.tag == 42
    assert t.value == "hello"
"#,
    )
    .expect("Tagged<String> with concrete + TypeParameter fields");
}

#[test]
fn test_shared_record_with() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<String> { value = "hello" }
    let b2 = b with value = "world"
    assert b2.value == "world"
    assert b.value == "hello"
"#,
    )
    .expect("with expression on shared record Box<String>");
}

#[test]
fn test_shared_function_with_shared_record() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function unbox<T>(b: Box<T>): T = b.value

function main(): Unit =
    let b = Box<String> { value = "hello" }
    let v = unbox(b)
    assert v == "hello"
"#,
    )
    .expect("shared function unbox with shared record");
}

#[test]
fn test_shared_function_constructing_shared_record() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function wrap<T>(x: T): Box<T> = Box<T> { value = x }

function main(): Unit =
    let b = wrap("hello")
    assert b.value == "hello"
"#,
    )
    .expect("shared function constructing shared record");
}

#[test]
fn test_shared_function_with_on_shared_record() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function rewrap<T>(b: Box<T>, x: T): Box<T> = b with value = x

function main(): Unit =
    let b = Box<String> { value = "hello" }
    let b2 = rewrap(b, "world")
    assert b2.value == "world"
"#,
    )
    .expect("shared function with-expression on shared record");
}

#[test]
fn test_mixed_shared_and_specialized_records() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b1 = Box<Int32> { value = 42 }
    let b2 = Box<String> { value = "hello" }
    assert b1.value == 42
    assert b2.value == "hello"
"#,
    )
    .expect("both Box<Int32> (specialized) and Box<String> (shared) in same program");
}

#[test]
fn test_generic_record_infer_pair() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function main(): Unit =
    let p = Pair { first = 1; second = 2 }
    assert p.first == 1
    assert p.second == 2
"#,
    )
    .expect("should infer A=Int32, B=Int32 from field values");
}

#[test]
fn test_generic_record_infer_shared_path() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box { value = "hello" }
    assert b.value == "hello"
"#,
    )
    .expect("should infer T=String (shared path) from field value");
}

#[test]
fn test_generic_record_infer_mixed_types() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function main(): Unit =
    let p = Pair { first = 10; second = true }
    assert p.first == 10
    assert p.second == true
"#,
    )
    .expect("should infer A=Int32, B=Bool from mixed field types");
}

#[test]
fn test_generic_record_infer_nested() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T
record Wrapper<T> = inner: Box<T>

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    let w = Wrapper { inner = b }
    assert w.inner.value == 42
"#,
    )
    .expect("should infer Wrapper<Int32> from nested Box<Int32>");
}

// ── Reified Match ──────────────────────────────────────────────────

#[test]
fn test_reified_match_type_annotated_string() {
    common::compile_and_run(
        r#"
package a

function check<T>(x: T): String =
    match x with
        case s: String => s
        case _ => "other"

function main(): Unit =
    assert check("hello") == "hello"
    assert check("world") == "world"
"#,
    )
    .expect("reified match on String type annotation");
}

#[test]
fn test_reified_match_type_annotated_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function isPoint<T>(x: T): Bool =
    match x with
        case p: Point => true
        case _ => false

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert isPoint(p)
    assert isPoint("hello") == false
"#,
    )
    .expect("reified match on record type annotation");
}

#[test]
fn test_reified_match_generic_record() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function isBoxString<T>(x: T): Bool =
    match x with
        case b: Box<String> => true
        case _ => false

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert isBoxString(b)
    let b2 = Box<String> { value = "world" }
    assert isBoxString(b2)
"#,
    )
    .expect("reified match on generic record with type args");
}

#[test]
fn test_reified_match_record_destructure() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function unboxString<T>(x: T): String =
    match x with
        case Box<String> { value = v } => v
        case _ => "not a box"

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert unboxString(b) == "hello"
"#,
    )
    .expect("reified match with record destructuring and type args");
}

#[test]
fn test_reified_match_wildcard_fallback() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function describe<T>(x: T): String =
    match x with
        case s: String => "string"
        case p: Point => "point"
        case _ => "unknown"

function main(): Unit =
    assert describe("hi") == "string"
    assert describe(Point { x = 1; y = 2 }) == "point"
"#,
    )
    .expect("reified match with multiple type checks and wildcard fallback");
}

#[test]
fn test_reified_match_forward_type_param() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function isBoxOf<T, U>(x: T, y: U): Bool =
    match x with
        case b: Box<U> => true
        case _ => false

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert isBoxOf(b, "test")
"#,
    )
    .expect("reified match with forwarded type parameter");
}

#[test]
fn test_reified_match_primitive_type() {
    // Primitive types in reified match are allowed in inference.
    // In the specialized path (e.g., foo(42)), the match is static.
    // In the shared path (e.g., foo("hello")), the primitive arm is unreachable.
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function describe<T>(x: T): String =
    match x with
        case s: String => "string"
        case p: Point => "point"
        case _ => "other"

function main(): Unit =
    assert describe("hello") == "string"
    assert describe(Point { x = 1; y = 2 }) == "point"
"#,
    )
    .expect("primitive type in reified match should be allowed");
}

#[test]
fn test_reified_match_exhaustiveness_per_specialization() {
    // With full monomorphization, exhaustiveness is checked per specialization.
    // Calling check(Point{...}) specializes T=Point, and matching Point against
    // `case p: Point` is exhaustive for that specialization.
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function check<T>(x: T): Bool =
    match x with
        case p: Point => true

function main(): Unit =
    assert check(Point { x = 1; y = 2 })
"#,
    )
    .expect("match on Point is exhaustive when T=Point");
}

#[test]
fn test_static_match_type_annotated() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    match p with
        case q: Point => assert q.x == 10
"#,
    )
    .expect("static type-annotated pattern should act like variable binding");
}

#[test]
fn test_static_match_record_with_type_args() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<String> { value = "hello" }
    match b with
        case Box<String> { value = v } => assert v == "hello"
"#,
    )
    .expect("static record pattern with type args");
}

#[test]
fn test_reified_match_non_generic_record_destructure() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function getX<T>(x: T): Int32 =
    match x with
        case Point { x = v } => v
        case _ => 0

function main(): Unit =
    let p = Point { x = 42; y = 10 }
    assert getX(p) == 42
"#,
    )
    .expect("reified match with non-generic record destructuring");
}

#[test]
fn test_static_match_generic_record_without_type_args() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function unwrap(x: Box<String>): String =
    match x with
        case Box { value = v } => v

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert unwrap(b) == "hello"
"#,
    )
    .expect("match generic record without type args in pattern");
}

#[test]
fn test_static_match_generic_record_without_type_args_literal() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function check(x: Box<String>): Bool =
    match x with
        case Box { value = "hello" } => true
        case _ => false

function main(): Unit =
    let b = Box<String> { value = "hello" }
    assert check(b)
"#,
    )
    .expect("match generic record without type args with literal field pattern");
}

#[test]
fn test_reified_match_shared_array() {
    // Shared arrays (reference-element) use ARRAY_REF_STRUCT with type-info in field 0.
    // The reified match must compare element type-info to distinguish Array<String> from other shared arrays.
    common::compile_and_run(
        r#"
package a

function isStringArray<T>(x: T): Bool =
    match x with
        case arr: Array<String> => true
        case _ => false

function main(): Unit =
    let strings = [|"hello", "world"|]
    assert isStringArray(strings)
    assert isStringArray("not an array") == false
"#,
    )
    .expect("reified match on shared array (Array<String>)");
}

#[test]
fn test_reified_match_primitive_array() {
    // Primitive-element arrays have unique WASM array types, so ref.test alone suffices.
    common::compile_and_run(
        r#"
package a

function isInt32Array<T>(x: T): Bool =
    match x with
        case arr: Array<Int32> => true
        case _ => false

function main(): Unit =
    let nums = [|1, 2, 3|]
    assert isInt32Array(nums)
    assert isInt32Array("not an array") == false
"#,
    )
    .expect("reified match on primitive array (Array<Int32>)");
}

#[test]
fn test_reified_match_shared_array_multiple_types() {
    // Distinguish between different shared array types at runtime.
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function describeArray<T>(x: T): String =
    match x with
        case arr: Array<String> => "string-array"
        case arr: Array<Box<Int32>> => "box-array"
        case _ => "other"

function main(): Unit =
    let strings = [|"a", "b"|]
    let boxes = [|Box<Int32> { value = 1 }, Box<Int32> { value = 2 }|]
    assert describeArray(strings) == "string-array"
    assert describeArray(boxes) == "box-array"
    assert describeArray(42) == "other"
    assert describeArray(Box<Int32> { value = 99 }) == "other"
"#,
    )
    .expect("reified match distinguishing different shared array types");
}

#[test]
fn test_reified_match_primitive_array_multiple_types() {
    // Distinguish between different primitive array types at runtime.
    common::compile_and_run(
        r#"
package a

function describeArray<T>(x: T): String =
    match x with
        case arr: Array<Int32> => "int32-array"
        case arr: Array<Float64> => "float64-array"
        case _ => "other"

function main(): Unit =
    let ints = [|1, 2, 3|]
    let floats = [|1.0, 2.0, 3.0|]
    assert describeArray(ints) == "int32-array"
    assert describeArray(floats) == "float64-array"
    assert describeArray("hello") == "other"
"#,
    )
    .expect("reified match distinguishing different primitive array types");
}

#[test]
fn test_reified_match_skip_unreachable_arms_primitive() {
    // When T = Int32, arms for Array<String> and Array<Box<Int32>> should be
    // silently skipped (not produce type errors) and the wildcard should match.
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function describeArray<T>(x: T): String =
    match x with
        case arr: Array<String> => "string-array"
        case arr: Array<Box<Int32>> => "box-array"
        case _ => "other"

function main(): Unit =
    assert describeArray(42) == "other"
    assert describeArray(true) == "other"
"#,
    )
    .expect("reified match skips unreachable arms for primitive call-site type");
}

#[test]
fn test_reified_match_skip_unreachable_record_pattern() {
    // When T = Int32, the record pattern `case Box { value } =>` should be
    // silently skipped since Int32 can never match Box.
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function describe<T>(x: T): String =
    match x with
        case b: Box<Int32> => "box"
        case _ => "other"

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert describe(b) == "box"
    assert describe(42) == "other"
    assert describe("hello") == "other"
"#,
    )
    .expect("reified match skips unreachable record pattern for primitive");
}

#[test]
fn test_reified_match_mixed_specialization() {
    // Generic function with both T-typed and non-T-typed params.
    // Only the T-typed scrutinee should have unreachable arms skipped;
    // static errors on non-T scrutinee are still caught at compile time.
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function checkType<T>(x: T): String =
    match x with
        case s: String => "string"
        case n: Int32 => "int"
        case b: Box<Int32> => "box"
        case _ => "other"

function main(): Unit =
    assert checkType("hello") == "string"
    assert checkType(42) == "int"
    let b = Box<Int32> { value = 10 }
    assert checkType(b) == "box"
    assert checkType(true) == "other"
"#,
    )
    .expect("reified match with mixed specialization");
}

#[test]
fn test_reified_match_shared_array_called_with_primitive() {
    // Calling a function that matches on Array<String> with a primitive type.
    // The Array<String> arm should be silently skipped.
    common::compile_and_run(
        r#"
package a

function isStringArray<T>(x: T): Bool =
    match x with
        case arr: Array<String> => true
        case _ => false

function main(): Unit =
    assert isStringArray(42) == false
    assert isStringArray(true) == false
    let strings = [|"hello", "world"|]
    assert isStringArray(strings) == true
"#,
    )
    .expect("reified match shared array called with primitive");
}

#[test]
fn test_reified_match_primitive_array_called_with_primitive() {
    // Calling a function that matches on Array<Int32> with a non-array type.
    common::compile_and_run(
        r#"
package a

function isInt32Array<T>(x: T): Bool =
    match x with
        case arr: Array<Int32> => true
        case _ => false

function main(): Unit =
    assert isInt32Array("hello") == false
    assert isInt32Array(42) == false
    let nums = [|1, 2, 3|]
    assert isInt32Array(nums) == true
"#,
    )
    .expect("reified match primitive array called with non-array type");
}

#[test]
fn test_reified_match_array_type_param_shared() {
    // Match on the element type-parameter of Array<T> where T is a shared (reference) type.
    // When called with Array<String>, the Array<String> arm matches; when called with
    // Array<Box<Int32>>, the Array<String> arm is silently skipped.
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function describeElements<T>(arr: Array<T>): String =
    match arr with
        case strings: Array<String> => "string-elements"
        case boxes: Array<Box<Int32>> => "box-elements"
        case _ => "other"

function main(): Unit =
    let strings = [|"hello", "world"|]
    let boxes = [|Box<Int32> { value = 1 }, Box<Int32> { value = 2 }|]
    assert describeElements(strings) == "string-elements"
    assert describeElements(boxes) == "box-elements"
"#,
    )
    .expect("reified match on array type-parameter (shared element types)");
}

#[test]
fn test_reified_match_array_type_param_primitive() {
    // Match on the element type-parameter of Array<T> where T is a primitive type.
    // When called with Array<Int32>, the Array<Int32> arm matches; when called with
    // Array<Float64>, the Array<Int32> arm is silently skipped.
    common::compile_and_run(
        r#"
package a

function describeElements<T>(arr: Array<T>): String =
    match arr with
        case ints: Array<Int32> => "int32-elements"
        case floats: Array<Float64> => "float64-elements"
        case _ => "other"

function main(): Unit =
    let ints = [|1, 2, 3|]
    let floats = [|1.0, 2.0, 3.0|]
    assert describeElements(ints) == "int32-elements"
    assert describeElements(floats) == "float64-elements"
"#,
    )
    .expect("reified match on array type-parameter (primitive element types)");
}

// ── Type parameter rules ─────────────────────────────────────────────

#[test]
fn test_self_as_generic_function_type_param_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function identity<Self>(x: Int32): Int32 = x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("'Self' cannot be used as a type parameter name")),
        "expected Self type param error, got: {:?}",
        errors
    );
}

#[test]
fn test_self_as_generic_extension_type_param_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

extension ArrayExt<Self> for Array<Self> =
    function first(self): Self = self.get(0)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("'Self' cannot be used as a type parameter name")),
        "expected Self type param error, got: {:?}",
        errors
    );
}

// Note: test for extension method type param shadowing extension type param
// is not yet possible because the parser doesn't support method-level type params
// on extension methods. The rule is in place for when that feature is added.

// ── Nested generic types ────────────────────────────────────────────

#[test]
fn test_nested_generic_enum_in_function_return() {
    common::compile_and_run(
        r#"
package a

function wrap<T>(x: T): Result<Int32, Result<T, String>> = Error(Ok(x))

function unwrapInner(r: Result<Int32, Result<Bool, String>>): Bool =
    match r with
        case Ok(_) => false
        case Error(inner) =>
            match inner with
                case Ok(b) => b
                case Error(_) => false

function main(): Unit =
    let r = wrap<Bool>(true)
    assert unwrapInner(r)
"#,
    )
    .expect("nested generic enum types should work");
}

#[test]
fn test_nested_generic_record_in_enum() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> = first: A; second: B

function makePair<T>(x: T): Option<Pair<T, Int32>> =
    Some(Pair<T, Int32> { first = x; second = 42 })

function main(): Unit =
    let r = makePair<Bool>(true)
    match r with
        case Some(p) => assert p.first == true
        case None => panic "expected Some"
"#,
    )
    .expect("nested generic record in enum should work");
}

#[test]
fn test_generic_module_function_calls_another_generic() {
    common::compile_and_run(
        r#"
package a

public enum MyList<T> =
    Cons(T, MyList<T>)
    Nil

module MyList<T> =
    public property head(self): T =
        match self with
            case Cons(h, _) => h
            case Nil => panic "empty"

public record Queue<T> =
    front: MyList<T>
    size: Int32

module Queue<T> =
    public property peek(self): Option<T> =
        if false then None
        else Some(self.front.head)

function peekQueue<T>(q: Queue<T>): Option<T> =
    if false then None
    else Some(q.front.head)

function main(): Unit =
    let q = Queue<Int32> { front = MyList.Cons(10, MyList.Nil); size = 1 }
    assert peekQueue(q).isSome
    assert q.peek.isSome
"#,
    )
    .expect("generic module function calling another generic module function");
}

#[test]
fn test_generic_class_field_assign_in_module_function() {
    common::compile_and_run(
        r#"
package a

public enum MyState<T, E> =
    Pending
    Done(Result<T, E>)

public class MyBox<T, E>(public mutable state: MyState<T, E>)

module MyBox<T, E> =
    public function complete(self, value: T): Unit =
        self.state = MyState.Done(Ok(value))

function main(): Unit =
    let b = MyBox<Int32, String>(MyState.Pending)
    b.complete(42)
    match b.state with
        case MyState.Done(Ok(v)) => assert v == 42
        case _ => panic "expected Done(Ok(42))"
"#,
    )
    .expect("generic class field assignment in module function");
}

#[test]
fn test_generic_impl_block_tuple_return() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

module Wrapper<T> =
    public function map<U>(self, f: T => U): Wrapper<U> =
        Wrapper(f(self.value))

    public function zip<U>(self, other: Wrapper<U>): Wrapper<(T, U)> =
        Wrapper((self.value, other.value))

public trait FlatZip<T> =
    type Result
    function flatZip(self, other: Wrapper<T>): Result

implement <A, B, T> FlatZip<T> for Wrapper<(A, B)> =
    type Result = Wrapper<(A, B, T)>
    function flatZip(self, other: Wrapper<T>): Wrapper<(A, B, T)> =
        self.zip(other).map((((a,b), c)) => (a, b, c))

function main(): Unit =
    let w = Wrapper((1, 2))
    let r = w.flatZip(Wrapper(3))
    assert r.value == (1, 2, 3)
"#,
    )
    .expect("generic impl block with tuple return");
}

#[test]
fn error_generic_module_global_references_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

public record Foo<T> =
    value: T

module Foo<T> =
    let DEFAULT: Option<T> = None

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("references type parameter")),
        "expected type-parameter rejection, got: {:?}",
        errors,
    );
}

/// A method that does not exist is an error wherever it is written. It used to be
/// swallowed whenever the receiver or an argument still mentioned a type parameter,
/// on the theory that monomorphize substitution would resolve it later — but
/// monomorphize only substitutes, it never re-runs inference, so the unresolved node
/// was copied into every instantiation and crashed codegen. `check` passed and
/// `build` trapped.
#[test]
fn error_unknown_method_on_generic_receiver() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function bad<T>(items: Array<T>): Int32 = items.nope()

function main(): Unit = assert bad<Int32>(Array<Int32>.fill(1i32, 0i32)) == 0i32
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method 'nope' found")),
        "expected the missing method to be reported, got: {:?}",
        errors,
    );
}

/// The same rule for the overload-selection half: an argument whose type is still a
/// type parameter cannot pick an overload, and that is a real error, not a deferral.
#[test]
fn error_no_overload_for_type_parameter_argument() {
    let errors = common::compile_expecting_errors(
        r#"
package a

public record Box<T> =
    value: T

module Box<T> =
    public function tag(self, label: String): Int32 = label.length
    public function tag(self, label: Int32): Int32 = label

function bad<T>(box: Box<T>, key: T): Int32 = box.tag(key)

function main(): Unit = assert bad<Bool>(Box<Bool> { value = true }, true) == 0i32
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("no matching overload") || e.contains("no method 'tag' found")),
        "expected the unselectable overload to be reported, got: {:?}",
        errors,
    );
}

/// The other side of the fix: a method call that a generic body *can* justify still
/// resolves from the template's own types. Pins that reporting the failures above did
/// not come at the cost of rejecting legitimately generic calls.
#[test]
fn method_on_generic_receiver_still_resolves() {
    common::compile_and_run(
        r#"
package a

public record Box<T> =
    value: T

module Box<T> =
    public function get(self): T = self.value

function unwrapAll<T>(boxes: Array<Box<T>>): Array<T> =
    boxes.map<T>((b: Box<T>) => b.get())

function main(): Unit =
    let boxes = Array<Box<Int32>>.fill(2, Box<Int32> { value = 7i32 })
    let values = unwrapAll<Int32>(boxes)
    assert values.length == 2
    assert values.get(0) == 7i32
"#,
    )
    .expect("a resolvable method call inside a generic function");
}

/// An `Any`-typed slot lowers to exactly the same `(ref any)` a bare type parameter
/// does, so a value stored into one has to be boxed on the way in and cast on the
/// way out just the same. `Codegen::is_erased_slot` used to answer `false` for
/// `Any`, which left every caller that gates on it either special-casing `Any` by
/// hand or storing a raw `i32` into an anyref slot — a module that fails WASM
/// validation ("expected (ref any), found i32") rather than one that computes the
/// wrong answer.
///
/// Reaching it needs the value's static type to be a type *parameter* at the call
/// site: a directly-written `Box(7i32)` goes through the `TypeCast`-to-`Any` path,
/// which always boxed. Only after monomorphization does the argument arrive as a
/// concrete primitive against a declared `Any` parameter.
#[test]
fn type_parameter_argument_into_an_any_field_is_boxed() {
    common::compile_and_run(
        r#"
package a

class Box(public value: Any)

function wrap<T>(v: T): Box = Box(v)

function main(): Unit =
    let boxed = wrap<Int32>(7i32)
    match boxed.value with
        case n: Int32 => assert n == 7i32
        case _ => assert false
    let alsoBoxed = wrap<Bool>(true)
    match alsoBoxed.value with
        case b: Bool => assert b
        case _ => assert false
"#,
    )
    .expect("a monomorphized type-parameter argument boxes into an Any field");
}

/// A generic module's static member falls back to variance defaults for its
/// type parameters — the same fallback an enum variant gets — so it resolves
/// without an annotation.
#[test]
fn generic_module_static_uses_variance_defaults() {
    common::compile_and_run(
        r#"
package a

function takes(xs: List<Int32>): Int32 = xs.length

function main(): Unit =
    // No context at all: `T` defaults by variance, giving `List<Never>`,
    // which covariance then admits wherever a `List<T>` is wanted.
    let x = List.empty
    assert takes(x) == 0
"#,
    )
    .expect("variance defaulting for a generic module static");
}

/// An ambient expected type is a hint, not a gate. An expectation belonging to
/// the *enclosing* expression (`Bool` from `assert`, `Int32` from the binding)
/// must not defeat an inference that succeeds on its own.
#[test]
fn mismatched_ambient_expected_type_does_not_block_resolution() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    // `assert` wants Bool; the receiver is a List. It must still resolve.
    assert List.empty.isEmpty
    // The binding wants Int32; the receiver is a List.
    let n: Int32 = List.empty.length
    assert n == 0
"#,
    )
    .expect("mismatched ambient expectation must not block resolution");
}

/// A matching expectation is still preferred over the default.
#[test]
fn matching_expected_type_is_still_used() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs: List<Int32> = List.empty
    assert xs.isEmpty
    assert (5 :: xs).head == 5
"#,
    )
    .expect("matching expectation still drives inference");
}
