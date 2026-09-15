mod common;

// ── Basic Any assignment ────────────────────────────────────────────────────

#[test]
fn any_assignment_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = 42
"#,
    )
    .expect("Any assignment from Int32");
}

#[test]
fn any_assignment_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = "hello"
"#,
    )
    .expect("Any assignment from String");
}

#[test]
fn any_assignment_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = true
"#,
    )
    .expect("Any assignment from Bool");
}

#[test]
fn any_assignment_float64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = 3.14
"#,
    )
    .expect("Any assignment from Float64");
}

#[test]
fn any_assignment_unit() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = ()
"#,
    )
    .expect("Any assignment from Unit");
}

// ── Any function parameters ─────────────────────────────────────────────────

#[test]
fn any_function_parameter() {
    common::compile_and_run(
        r#"
package a

function take_any(x: Any): Unit = ()

function main(): Unit =
    take_any(42)
    take_any("hello")
    take_any(true)
"#,
    )
    .expect("Any function parameter");
}

// ── Any return type ─────────────────────────────────────────────────────────

#[test]
fn any_return_type_int() {
    common::compile_and_run(
        r#"
package a

function return_any(): Any = 42

function main(): Unit =
    let a: Any = return_any()
"#,
    )
    .expect("Any return type from Int32");
}

#[test]
fn any_return_type_string() {
    common::compile_and_run(
        r#"
package a

function return_any(): Any = "hello"

function main(): Unit =
    let a: Any = return_any()
"#,
    )
    .expect("Any return type from String");
}

// ── Any in record fields ────────────────────────────────────────────────────

#[test]
fn any_record_field() {
    common::compile_and_run(
        r#"
package a

record Container = value: Any

function main(): Unit =
    let c = Container { value = 42 }
"#,
    )
    .expect("Any in record field");
}

#[test]
fn any_record_field_string() {
    common::compile_and_run(
        r#"
package a

record Container = value: Any

function main(): Unit =
    let c = Container { value = "hello" }
"#,
    )
    .expect("Any in record field with string");
}

// ── Reference type assigned to Any ──────────────────────────────────────────

#[test]
fn any_from_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let a: Any = p
"#,
    )
    .expect("Record assigned to Any");
}

#[test]
fn any_from_enum() {
    common::compile_and_run(
        r#"
package a

enum Color = Red, Green, Blue

function main(): Unit =
    let c = Color.Red
    let a: Any = c
"#,
    )
    .expect("Enum assigned to Any");
}

// ── Any with generics ───────────────────────────────────────────────────────

#[test]
fn any_as_type_arg() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Option<Any> = Some(42)
"#,
    )
    .expect("Any as type argument");
}

#[test]
fn covariant_to_any() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = Some(42)
    let b: Option<Any> = a
"#,
    )
    .expect("Covariant assignment to Option<Any>");
}

// ── Mutable Any variable ────────────────────────────────────────────────────

#[test]
fn any_mutable_variable() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable a: Any = 42
    a = "hello"
    a = true
"#,
    )
    .expect("Mutable Any variable with reassignment");
}

// ── Contravariant default to Any ────────────────────────────────────────────

#[test]
fn contravariant_default_to_any() {
    // ContraBox<in T> — T only appears in contravariant position (function param type)
    // is not directly expressible in enum payloads, so use a record with a dummy field
    common::check_no_errors(
        r#"
package a

record ContraBox<in T> =
    dummy: Int32

function take(b: ContraBox<Int32>): Unit = ()

function main(): Unit =
    let x = ContraBox<Any> { dummy = 1 }
    take(x)
"#,
    );
}

// ── Type test (is) ─────────────────────────────────────────────────────────

#[test]
fn type_test_int32_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    assert x is Int32
"#,
    )
    .expect("is Int32 on Int32 value");
}

#[test]
fn type_test_int32_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    assert !(x is Int32)
"#,
    )
    .expect("is Int32 on String value should be false");
}

#[test]
fn type_test_string_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    assert x is String
"#,
    )
    .expect("is String on String value");
}

#[test]
fn type_test_bool_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = true
    assert x is Bool
"#,
    )
    .expect("is Bool on Bool value");
}

#[test]
fn type_test_float64_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 3.14
    assert x is Float64
"#,
    )
    .expect("is Float64 on Float64 value");
}

#[test]
fn type_test_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let a: Any = p
    assert a is Point
    assert !(a is Int32)
"#,
    )
    .expect("is Record on record value");
}

#[test]
fn type_test_enum() {
    common::compile_and_run(
        r#"
package a

enum Color = Red, Green, Blue

function main(): Unit =
    let c = Color.Red
    let a: Any = c
    assert a is Color
    assert !(a is Int32)
"#,
    )
    .expect("is Enum on enum value");
}

// ── Type cast (as) ─────────────────────────────────────────────────────────

#[test]
fn type_cast_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let y: Int32 = x as Int32
    assert y == 42
"#,
    )
    .expect("as Int32 extracts value");
}

#[test]
fn type_cast_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    let y: String = x as String
"#,
    )
    .expect("as String extracts value");
}

#[test]
fn type_cast_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = true
    let y: Bool = x as Bool
    assert y == true
"#,
    )
    .expect("as Bool extracts value");
}

#[test]
fn type_cast_float64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 3.14
    let y: Float64 = x as Float64
    assert y == 3.14
"#,
    )
    .expect("as Float64 extracts value");
}

#[test]
fn type_cast_float32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 2.5f32
    let y: Float32 = x as Float32
    assert y == 2.5f32
"#,
    )
    .expect("as Float32 extracts value");
}

#[test]
fn type_cast_int8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42i8
    let y: Int8 = x as Int8
    assert y == 42i8
"#,
    )
    .expect("as Int8 extracts value");
}

#[test]
fn type_cast_int16() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 1000i16
    let y: Int16 = x as Int16
    assert y == 1000i16
"#,
    )
    .expect("as Int16 extracts value");
}

#[test]
fn type_cast_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 123456789i64
    let y: Int64 = x as Int64
    assert y == 123456789i64
"#,
    )
    .expect("as Int64 extracts value");
}

#[test]
fn type_cast_uint8() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 200u8
    let y: Uint8 = x as Uint8
    assert y == 200u8
"#,
    )
    .expect("as Uint8 extracts value");
}

#[test]
fn type_cast_uint16() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 50000u16
    let y: Uint16 = x as Uint16
    assert y == 50000u16
"#,
    )
    .expect("as Uint16 extracts value");
}

#[test]
fn type_cast_uint32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 100000u32
    let y: Uint32 = x as Uint32
    assert y == 100000u32
"#,
    )
    .expect("as Uint32 extracts value");
}

#[test]
fn type_cast_uint64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 999999u64
    let y: Uint64 = x as Uint64
    assert y == 999999u64
"#,
    )
    .expect("as Uint64 extracts value");
}

#[test]
fn type_cast_unit() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = ()
    let y: Unit = x as Unit
"#,
    )
    .expect("as Unit extracts value");
}

#[test]
fn type_cast_record() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let a: Any = p
    let p2 = a as Point
    assert p2.x == 1
    assert p2.y == 2
"#,
    )
    .expect("as Record extracts record and accesses fields");
}

// ── is/as with generics ────────────────────────────────────────────────────
#[test]
fn type_test_generic_option() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = Some(42)
    assert x is Option<Int32>
    assert !(x is Option<String>)
"#,
    )
    .expect("is Option<Int32> on Some(42)");
}

#[test]
fn type_cast_generic_option() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = Some(42)
    let opt = x as Option<Int32>
    match opt with
        case Some(v) => assert v == 42
        case None => assert false
"#,
    )
    .expect("as Option<Int32> extracts and matches");
}

#[test]
fn type_test_generic_result() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = Ok(99)
    assert x is Result<Int32, Never>
    assert !(x is Result<String, Never>)
"#,
    )
    .expect("is Result<Int32, Never> on Ok(99)");
}

#[test]
fn type_cast_generic_result() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = Ok(99)
    let r = x as Result<Int32, Never>
    match r with
        case Ok(v) => assert v == 99
        case Error(e) => assert false
"#,
    )
    .expect("as Result<Int32, Never> extracts and matches");
}

#[test]
fn type_test_generic_record() {
    common::compile_and_run(
        r#"
package a

record Box<out T> = value: T

function main(): Unit =
    let b = Box<Int32> { value = 7 }
    let x: Any = b
    assert x is Box<Int32>
    assert !(x is Box<String>)
"#,
    )
    .expect("is Box<Int32> on generic record");
}

#[test]
fn type_cast_generic_record() {
    common::compile_and_run(
        r#"
package a

record Box<out T> = value: T

function main(): Unit =
    let b = Box<Int32> { value = 7 }
    let x: Any = b
    let b2 = x as Box<Int32>
    assert b2.value == 7
"#,
    )
    .expect("as Box<Int32> extracts generic record");
}

#[test]
fn type_test_generic_enum() {
    common::compile_and_run(
        r#"
package a

enum Maybe<out T> =
    Just(T)
    Nothing

function main(): Unit =
    let m = Maybe.Just(42)
    let x: Any = m
    assert x is Maybe<Int32>
    assert !(x is Maybe<String>)
"#,
    )
    .expect("is Maybe<Int32> on generic enum");
}

#[test]
fn type_cast_generic_enum() {
    common::compile_and_run(
        r#"
package a

enum Maybe<out T> =
    Just(T)
    Nothing

function main(): Unit =
    let m = Maybe.Just(42)
    let x: Any = m
    let m2 = x as Maybe<Int32>
    match m2 with
        case Maybe.Just(v) => assert v == 42
        case Maybe.Nothing => assert false
"#,
    )
    .expect("as Maybe<Int32> extracts generic enum");
}

// ── is/as with arrays ──────────────────────────────────────────────────────

#[test]
fn type_test_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3|]
    let x: Any = arr
    assert x is Array<Int32>
    assert !(x is Array<String>)
"#,
    )
    .expect("is Array<Int32> on int array");
}

#[test]
fn type_cast_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    let x: Any = arr
    let arr2 = x as Array<Int32>
    assert arr2[0] == 10
    assert arr2[1] == 20
    assert arr2[2] == 30
"#,
    )
    .expect("as Array<Int32> extracts array");
}

#[test]
fn type_test_string_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|"hello", "world"|]
    let x: Any = arr
    assert x is Array<String>
    assert !(x is Array<Int32>)
"#,
    )
    .expect("is Array<String> distinguishes from Array<Int32>");
}

// ── is/as chaining ─────────────────────────────────────────────────────────

#[test]
fn type_test_then_cast() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    if x is Int32 then
        assert (x as Int32) == 42
    else
        assert false
"#,
    )
    .expect("is + as chaining");
}

// ── as trap on wrong type ──────────────────────────────────────────────────

#[test]
fn type_cast_wrong_type_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    let y: Int32 = x as Int32
"#,
    );
}

// ── Error tests ─────────────────────────────────────────────────────────────

#[test]
fn type_test_on_non_any_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Int32 = 42
    let b = x is Int32
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires subject of type Any")),
        "expected type error, got: {:?}",
        errors
    );
}

#[test]
fn type_cast_on_non_any_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Int32 = 42
    let b = x as String
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires subject of type Any")),
        "expected type error, got: {:?}",
        errors
    );
}

#[test]
fn type_test_any_target_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let b = x is Any
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must be a concrete type")),
        "expected type error, got: {:?}",
        errors
    );
}

#[test]
fn type_test_never_target_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let b = x is Never
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must be a concrete type")),
        "expected type error, got: {:?}",
        errors
    );
}

#[test]
fn any_not_assignable_to_concrete_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let a: Any = 42
    let b: Int32 = a
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expected 'Int32'")),
        "expected type error, got: {:?}",
        errors
    );
}

// ── Type-annotated match patterns on Any ────────────────────────────────────

#[test]
fn match_any_int32_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case n: Int32 => assert n == 42
        case _ => panic "expected Int32"
"#,
    )
    .expect("match Any with Int32 pattern");
}

#[test]
fn match_any_string_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    match x with
        case s: String => assert s == "hello"
        case _ => panic "expected String"
"#,
    )
    .expect("match Any with String pattern");
}

#[test]
fn match_any_bool_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = true
    match x with
        case b: Bool => assert b == true
        case _ => panic "expected Bool"
"#,
    )
    .expect("match Any with Bool pattern");
}

#[test]
fn match_any_float64_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 3.14
    match x with
        case f: Float64 => assert f == 3.14
        case _ => panic "expected Float64"
"#,
    )
    .expect("match Any with Float64 pattern");
}

#[test]
fn match_any_record_pattern() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let x: Any = Point { x = 1; y = 2 }
    match x with
        case p: Point => assert p.x == 1
        case _ => panic "expected Point"
"#,
    )
    .expect("match Any with record pattern");
}

#[test]
fn match_any_enum_pattern() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let x: Any = Color.Green
    let mutable matched = false
    match x with
        case c: Color => matched = true
        case _ => panic "expected Color"
    assert matched
"#,
    )
    .expect("match Any with enum pattern");
}

#[test]
fn match_any_multiple_arms() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = 42
    let mutable result = 0
    match a with
        case n: Int32 => result = 1
        case s: String => result = 2
        case b: Bool => result = 3
        case _ => result = 0
    assert result == 1
"#,
    )
    .expect("match Any with multiple type arms");
}

#[test]
fn match_any_multiple_arms_fallthrough() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: Any = "hi"
    let mutable result = 0
    match a with
        case n: Int32 => result = 1
        case s: String => result = 2
        case b: Bool => result = 3
        case _ => result = 0
    assert result == 2
"#,
    )
    .expect("match Any skips non-matching arms");
}

#[test]
fn match_any_value_context() {
    common::compile_and_run(
        r#"
package a

function classify(x: Any): Int32 =
    match x with
        case n: Int32 => 1
        case s: String => 2
        case _ => 0

function main(): Unit =
    let a: Any = 42
    assert classify(a) == 1
    let b: Any = "hi"
    assert classify(b) == 2
    let c: Any = 3.14
    assert classify(c) == 0
"#,
    )
    .expect("match Any in value context");
}

#[test]
fn match_any_wildcard_fallthrough() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 3.14
    let mutable result = 0
    match x with
        case n: Int32 => result = 1
        case s: String => result = 2
        case _ => result = 99
    assert result == 99
"#,
    )
    .expect("match Any wildcard catches non-matching types");
}

#[test]
fn match_any_with_guard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let mutable result = 0
    match x with
        case n: Int32 if n > 100 => result = 1
        case n: Int32 if n > 10 => result = 2
        case n: Int32 => result = 3
        case _ => result = 0
    assert result == 2
"#,
    )
    .expect("match Any with guard condition");
}

#[test]
fn match_any_generic_option() {
    common::compile_and_run(
        r#"
package a

enum Option<out T> =
    Some(T)
    None

function main(): Unit =
    let x: Any = Some(42)
    match x with
        case opt: Option<Int32> =>
            match opt with
                case Some(n) => assert n == 42
                case None => panic "expected Some"
        case _ => panic "expected Option<Int32>"
"#,
    )
    .expect("match Any with generic Option pattern");
}

#[test]
fn match_any_array_pattern() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = [|1, 2, 3|]
    match x with
        case arr: Array<Int32> => assert arr[0] == 1
        case _ => panic "expected Array<Int32>"
"#,
    )
    .expect("match Any with Array pattern");
}

// ── Error tests for type-annotated match patterns ───────────────────────────

#[test]
fn match_any_pattern_target_any_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case a: Any => ()
        case _ => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("concrete type") && e.contains("Any")),
        "expected error about concrete type, got: {:?}",
        errors
    );
}

#[test]
fn match_any_pattern_target_never_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case n: Never => ()
        case _ => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("concrete type") && e.contains("Never")),
        "expected error about concrete type, got: {:?}",
        errors
    );
}

// ── Unsafe cast warnings ────────────────────────────────────────────────────

#[test]
fn as_without_is_warns() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let n = x as Int32
    assert n == 42
"#,
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("as Int32") && w.contains("may panic")),
        "expected warning about unsafe cast, got: {:?}",
        warnings
    );
}

#[test]
fn as_with_is_no_warning() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    if x is Int32 then
        let n = x as Int32
        assert n == 42
"#,
    );
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );
}

#[test]
fn as_with_is_different_var_warns() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let y: Any = "hello"
    if x is Int32 then
        let s = y as String
        assert s == "hello"
"#,
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("as String") && w.contains("may panic")),
        "expected warning for different variable, got: {:?}",
        warnings
    );
}

#[test]
fn as_with_is_different_type_warns() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    if x is Int32 then
        let s = x as String
        assert s == "hello"
"#,
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("as String") && w.contains("may panic")),
        "expected warning for different type, got: {:?}",
        warnings
    );
}

#[test]
fn as_non_variable_warns() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function getAny(): Any = 42

function main(): Unit =
    let n = getAny() as Int32
    assert n == 42
"#,
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("as Int32") && w.contains("type pattern")),
        "expected warning for non-variable cast, got: {:?}",
        warnings
    );
}

#[test]
fn as_in_nested_if_with_guard_no_warning() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    if x is Int32 then
        let n = x as Int32
        if n > 0 then
            let m = x as Int32
            assert m == 42
"#,
    );
    assert!(
        warnings.is_empty(),
        "expected no warnings (guard active in nested scope), got: {:?}",
        warnings
    );
}

// ── Nominal type identity ───────────────────────────────────────────────────
//
// WASM-GC canonicalizes types structurally, so same-shape Dovetail types would be the *same*
// WASM type — and indistinguishable to `ref.test`/`ref.cast` — unless codegen emits them as
// members of one module-wide rec group. These pin that guarantee down.
// See `docs/nominal-type-identity.md`.

#[test]
fn same_shape_records_are_distinct() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32
record Vec2 = x: Int32; y: Int32

function main(): Unit =
    let p: Any = Point { x = 1; y = 2 }
    let v: Any = Vec2 { x = 3; y = 4 }
    assert p is Point
    assert !(p is Vec2)
    assert v is Vec2
    assert !(v is Point)
"#,
    )
    .expect("same-shape records stay distinct types");
}

#[test]
fn same_shape_enums_are_distinct() {
    common::compile_and_run(
        r#"
package a

enum Color = Red, Green
enum Switch = On, Off

function main(): Unit =
    let c: Any = Color.Red
    let s: Any = Switch.On
    assert c is Color
    assert !(c is Switch)
    assert s is Switch
    assert !(s is Color)
"#,
    )
    .expect("same-shape enums stay distinct types");
}

#[test]
fn same_shape_enums_with_payloads_are_distinct() {
    common::compile_and_run(
        r#"
package a

enum Reading =
    Empty
    Value(Int32)

enum Slot =
    Vacant
    Filled(Int32)

function main(): Unit =
    let r: Any = Reading.Value(7)
    let s: Any = Slot.Filled(7)
    assert r is Reading
    assert !(r is Slot)
    assert s is Slot
    assert !(s is Reading)
"#,
    )
    .expect("same-shape enums with payloads stay distinct types");
}

#[test]
fn same_shape_classes_in_unrelated_hierarchies_are_distinct() {
    common::compile_and_run(
        r#"
package a

abstract class Base1()
class Impl1(public n: Int32) extends Base1()

abstract class Base2()
class Impl2(public n: Int32) extends Base2()

function main(): Unit =
    let a: Any = Impl1(1)
    let b: Any = Impl2(1)
    assert a is Impl1
    assert !(a is Impl2)
    assert b is Impl2
    assert !(b is Impl1)
"#,
    )
    .expect("same-shape classes in unrelated hierarchies stay distinct types");
}

#[test]
fn single_field_record_is_not_a_boxed_primitive() {
    // A one-`Int32`-field record has the same WASM shape as the `$Box$Int32` struct used to
    // store an `Int32` in an `Any`. Reading one as the other would hand back the raw field.
    common::compile_and_run(
        r#"
package a

record Wrapper = n: Int32

function main(): Unit =
    let w: Any = Wrapper { n = 42 }
    let i: Any = 42
    assert w is Wrapper
    assert !(w is Int32)
    assert i is Int32
    assert !(i is Wrapper)
"#,
    )
    .expect("a single-field record is not a boxed Int32");
}

#[test]
fn boxed_primitives_of_the_same_wasm_repr_are_distinct() {
    // Unit, Bool, Char, Int8/16/32 and Uint8/16/32 all box into a `(struct (field i32))`.
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let b: Any = true
    let c: Any = 'x'
    let u: Any = 7u32
    let i: Any = 7

    assert b is Bool
    assert !(b is Int32)
    assert !(b is Char)

    assert c is Char
    assert !(c is Int32)
    assert !(c is Bool)

    assert u is Uint32
    assert !(u is Int32)

    assert i is Int32
    assert !(i is Bool)
    assert !(i is Uint32)
"#,
    )
    .expect("boxed primitives sharing a WASM representation stay distinct");
}

#[test]
fn string_is_not_a_byte_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s: Any = "hi"
    let bytes: Any = [|1i8, 2i8|]
    assert s is String
    assert !(s is Array<Int8>)
    assert bytes is Array<Int8>
    assert !(bytes is String)
"#,
    )
    .expect("String and Array<Int8> stay distinct");
}
