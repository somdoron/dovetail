mod common;

// --- Happy path tests ---

#[test]
fn test_record_define_construct_and_access_fields() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    assert p.x == 10
    assert p.y == 20
"#,
    )
    .expect("record define, construct and access fields");
}

#[test]
fn test_record_as_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function makePoint(x: Int32, y: Int32): Point = Point { x = x; y = y }

function getX(p: Point): Int32 = p.x

function main(): Unit =
    let p = makePoint(3, 4)
    assert getX(p) == 3
    assert p.y == 4
"#,
    )
    .expect("record as function param and return");
}

#[test]
fn test_multiple_records() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

record Size =
    w: Int32
    h: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let s = Size { w = 10; h = 20 }
    assert p.x + s.w == 11
    assert p.y + s.h == 22
"#,
    )
    .expect("multiple records");
}

#[test]
fn test_record_with_bool_field() {
    common::compile_and_run(
        r#"
package a

record Config =
    enabled: Bool
    count: Int32

function main(): Unit =
    let c = Config { enabled = true; count = 42 }
    assert c.enabled == true
    assert c.count == 42
"#,
    )
    .expect("record with bool field");
}

#[test]
fn test_record_with_various_numeric_types() {
    common::compile_and_run(
        r#"
package a

record Numbers =
    a: Int8
    b: Int64
    c: Float64

function main(): Unit =
    let n = Numbers { a = 5i8; b = 100i64; c = 3.14 }
    assert n.a == 5i8
    assert n.b == 100i64
"#,
    )
    .expect("record with various numeric types");
}

#[test]
fn test_record_referencing_another_record() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

record Line =
    start: Point
    end: Point

function main(): Unit =
    let p1 = Point { x = 0; y = 0 }
    let p2 = Point { x = 10; y = 20 }
    let line = Line { start = p1; end = p2 }
    assert line.start.x == 0
    assert line.end.x == 10
    assert line.end.y == 20
"#,
    )
    .expect("record referencing another record");
}

#[test]
fn test_record_construction_multiline() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point {
        x = 10
        y = 20
    }
    assert p.x == 10
    assert p.y == 20
"#,
    )
    .expect("record construction multiline");
}

#[test]
fn test_record_field_in_expression() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    let sum = p.x + p.y
    assert sum == 7
"#,
    )
    .expect("record field in expression");
}

#[test]
fn test_record_passed_to_multiple_functions() {
    common::compile_and_run(
        r#"
package a

record Pair =
    first: Int32
    second: Int32

function sum(p: Pair): Int32 = p.first + p.second

function diff(p: Pair): Int32 = p.first - p.second

function main(): Unit =
    let p = Pair { first = 10; second = 3 }
    assert sum(p) == 13
    assert diff(p) == 7
"#,
    )
    .expect("record passed to multiple functions");
}

// --- Error tests ---

#[test]
fn test_error_missing_field_in_construction() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 10 }
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("missing") || e.contains("Missing")),
        "expected missing field error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_extra_field_in_construction() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 10; y = 20; z = 30 }
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("z")),
        "expected unknown field 'z' error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_wrong_field_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = true; y = 20 }
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type") || e.contains("mismatch") || e.contains("expected")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_access_nonexistent_field() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    let z = p.z
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("z")),
        "expected unknown field 'z' error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_unknown_record_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let p = Unknown { x = 10 }
    ()
"#,
    );
    assert!(!errors.is_empty(), "expected error for unknown record type");
}

// --- With expression tests ---

#[test]
fn test_record_with_single_field_override() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with x = 10
    assert p2.x == 10
    assert p2.y == 2
"#,
    )
    .expect("record with single field override");
}

#[test]
fn test_record_with_multiple_overrides_same_line() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with x = 10; y = 20
    assert p2.x == 10
    assert p2.y == 20
"#,
    )
    .expect("record with multiple overrides same line");
}

#[test]
fn test_record_with_original_unchanged() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with x = 10
    assert p.x == 1
    assert p.y == 2
    assert p2.x == 10
"#,
    )
    .expect("record with original unchanged");
}

#[test]
fn test_record_with_chained() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = (p with x = 10) with y = 20
    assert p2.x == 10
    assert p2.y == 20
"#,
    )
    .expect("record with chained");
}

#[test]
fn test_record_with_as_function_argument() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function getX(p: Point): Int32 = p.x

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert getX(p with x = 99) == 99
"#,
    )
    .expect("record with as function argument");
}

#[test]
fn test_record_with_multiline() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with
        x = 10
        y = 20
    assert p2.x == 10
    assert p2.y == 20
"#,
    )
    .expect("record with multiline");
}

#[test]
fn test_error_with_on_non_record_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = 42
    let y = x with z = 1
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("record")),
        "expected 'requires a record type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_with_nonexistent_field() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with z = 10
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("z")),
        "expected no field 'z' error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_with_wrong_field_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with x = true
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type") || e.contains("mismatch") || e.contains("expected")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_with_duplicate_field() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let p2 = p with x = 10; x = 20
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate")),
        "expected duplicate field error, got: {:?}",
        errors
    );
}

// --- Forward reference / dependency ordering tests ---

#[test]
fn test_record_forward_reference_ordering() {
    common::compile_and_run(
        r#"
package a

record Line =
    start: Point
    end: Point

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let line = Line { start = Point { x = 1; y = 2 }; end = Point { x = 3; y = 4 } }
    assert line.start.x == 1
    assert line.end.y == 4
"#,
    )
    .expect("forward reference ordering");
}

#[test]
fn test_mutually_recursive_records_compile() {
    common::compile_and_run(
        r#"
package a

record Foo =
    value: Int32
    bar: Bar

record Bar =
    value: Int32
    foo: Foo

function main(): Unit = ()
"#,
    )
    .expect("mutually recursive records compile");
}

#[test]
fn test_self_referential_record_compile() {
    common::compile_and_run(
        r#"
package a

record Node =
    value: Int32
    next: Node

function main(): Unit = ()
"#,
    )
    .expect("self-referential record compile");
}

// --- Pattern matching tests ---

#[test]
fn test_record_pattern_destructure_all_fields() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    let result = match p with
        case Point { x, y } => x + y
    assert result == 7
"#,
    )
    .expect("record pattern destructure all fields");
}

#[test]
fn test_record_pattern_explicit_bind() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    let result = match p with
        case Point { x = a, y = b } => a + b
    assert result == 7
"#,
    )
    .expect("record pattern explicit bind");
}

#[test]
fn test_record_pattern_literal_match() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 0; y = 0 }
    let result = match p with
        case Point { x = 0, y = 0 } => 1
        case Point { x, y } => 2
    assert result == 1
"#,
    )
    .expect("record pattern literal match");
}

#[test]
fn test_record_pattern_mixed_literal_and_bind() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 0; y = 42 }
    let result = match p with
        case Point { x = 0, y } => y
        case Point { x, y } => 0
    assert result == 42
"#,
    )
    .expect("record pattern mixed literal and bind");
}

#[test]
fn test_record_pattern_partial_fields() {
    common::compile_and_run(
        r#"
package a

record Triple =
    a: Int32
    b: Int32
    c: Int32

function main(): Unit =
    let t = Triple { a = 10; b = 20; c = 30 }
    let result = match t with
        case Triple { b } => b
    assert result == 20
"#,
    )
    .expect("record pattern partial fields");
}

#[test]
fn test_record_pattern_wildcard_field() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 5; y = 10 }
    let result = match p with
        case Point { x = _, y } => y
    assert result == 10
"#,
    )
    .expect("record pattern wildcard field");
}

#[test]
fn test_record_pattern_multiple_arms() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p1 = Point { x = 0; y = 0 }
    let p2 = Point { x = 0; y = 5 }
    let p3 = Point { x = 3; y = 4 }

    let r1 = match p1 with
        case Point { x = 0, y = 0 } => 1
        case Point { x = 0, y } => 2
        case Point { x, y } => 3
    assert r1 == 1

    let r2 = match p2 with
        case Point { x = 0, y = 0 } => 1
        case Point { x = 0, y } => 2
        case Point { x, y } => 3
    assert r2 == 2

    let r3 = match p3 with
        case Point { x = 0, y = 0 } => 1
        case Point { x = 0, y } => 2
        case Point { x, y } => 3
    assert r3 == 3
"#,
    )
    .expect("record pattern multiple arms");
}

#[test]
fn test_record_pattern_with_guard() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 5; y = 10 }
    let result = match p with
        case Point { x, y } if x > 3 => x + y
        case Point { x, y } => 0
    assert result == 15
"#,
    )
    .expect("record pattern with guard");
}

#[test]
fn test_record_pattern_exhaustive_single_arm() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Point { x, y } => x * y
    assert result == 2
"#,
    )
    .expect("record pattern exhaustive single arm");
}

#[test]
fn test_record_pattern_field_in_body_expression() {
    common::compile_and_run(
        r#"
package a

record Rect =
    w: Int32
    h: Int32

function area(r: Rect): Int32 =
    match r with
        case Rect { w, h } => w * h

function main(): Unit =
    let r = Rect { w = 5; h = 3 }
    assert area(r) == 15
"#,
    )
    .expect("record pattern field in body expression");
}

// --- Pattern matching error tests ---

#[test]
fn test_error_record_pattern_unknown_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Unknown { x } => x
        case _ => 0
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("unknown") || e.contains("Unknown")),
        "expected unknown record type error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_record_pattern_nonexistent_field() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Point { z } => 0
        case _ => 0
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("z")),
        "expected no field 'z' error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_record_pattern_duplicate_field() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Point { x, x } => 0
        case _ => 0
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate")),
        "expected duplicate field error, got: {:?}",
        errors
    );
}

#[test]
fn test_record_pattern_type_mismatch_arm_skipped() {
    // When not in_instantiation (e.g. in main), a record pattern with wrong base type
    // emits a type mismatch error.
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

record Color =
    r: Int32
    g: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Color { r } => 0
        case _ => 1
    assert result == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch in record pattern")),
        "expected type mismatch in record pattern, got: {:?}",
        errors
    );
}

#[test]
fn test_error_record_pattern_non_exhaustive() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case Point { x = 0, y } => y
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("exhaustive") || e.contains("catch-all") || e.contains("covered")),
        "expected non-exhaustive error, got: {:?}",
        errors
    );
}

#[test]
fn test_record_pattern_bool_field_exhaustive() {
    // Two arms covering both Bool values for a field should be exhaustive
    // without needing a wildcard catch-all
    common::compile_and_run(
        r#"
package a

record Toggle =
    flag: Bool
    value: Int32

function main(): Unit =
    let t = Toggle { flag = true; value = 42 }
    let result = match t with
        case Toggle { flag = true, value } => value
        case Toggle { flag = false, value } => value + 1
    assert result == 42
"#,
    )
    .expect("bool field exhaustive match");
}

#[test]
fn test_error_record_pattern_bool_field_non_exhaustive() {
    // Only covering one Bool value for a field should be non-exhaustive
    let errors = common::compile_expecting_errors(
        r#"
package a

record Toggle =
    flag: Bool
    value: Int32

function main(): Unit =
    let t = Toggle { flag = true; value = 42 }
    let result = match t with
        case Toggle { flag = true, value } => value
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("exhaustive") || e.contains("covered")),
        "expected non-exhaustive error, got: {:?}",
        errors
    );
}

#[test]
fn test_record_pattern_bool_field_with_wildcard_exhaustive() {
    // A wildcard for the Bool field makes one arm cover all remaining values
    common::compile_and_run(
        r#"
package a

record Toggle =
    flag: Bool
    value: Int32

function main(): Unit =
    let t = Toggle { flag = false; value = 10 }
    let result = match t with
        case Toggle { flag = true, value } => value
        case Toggle { flag, value } => value + 1
    assert result == 11
"#,
    )
    .expect("bool field with wildcard fallback");
}

#[test]
fn test_record_pattern_two_bool_fields_exhaustive() {
    // All 4 combinations of two Bool fields should be exhaustive
    common::compile_and_run(
        r#"
package a

record Flags =
    a: Bool
    b: Bool

function main(): Unit =
    let f = Flags { a = true; b = false }
    let result = match f with
        case Flags { a = true, b = true } => 1
        case Flags { a = true, b = false } => 2
        case Flags { a = false, b = true } => 3
        case Flags { a = false, b = false } => 4
    assert result == 2
"#,
    )
    .expect("two bool fields full cross-product");
}

#[test]
fn test_error_record_pattern_two_bool_fields_incomplete() {
    // Missing one combination of two Bool fields should be non-exhaustive
    let errors = common::compile_expecting_errors(
        r#"
package a

record Flags =
    a: Bool
    b: Bool

function main(): Unit =
    let f = Flags { a = true; b = true }
    let result = match f with
        case Flags { a = true, b = true } => 1
        case Flags { a = true, b = false } => 2
        case Flags { a = false, b = true } => 3
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("exhaustive") || e.contains("covered")),
        "expected non-exhaustive error for missing (false, false), got: {:?}",
        errors
    );
}

#[test]
fn test_record_pattern_two_bool_fields_with_wildcard_shortcut() {
    // { a = true, b = _ } + { a = false, b = _ } covers everything
    common::compile_and_run(
        r#"
package a

record Flags =
    a: Bool
    b: Bool

function main(): Unit =
    let f = Flags { a = false; b = true }
    let result = match f with
        case Flags { a = true, b } => 1
        case Flags { a = false, b } => 2
    assert result == 2
"#,
    )
    .expect("two bool fields with wildcard shortcut");
}

// ── Private records ─────────────────────────────────────────────────

#[test]
fn test_private_record_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

private record Secret =
    x: Int32

function main(): Unit =
    let s = Secret { x = 42 }
    assert s.x == 42
"#,
    )
    .expect("private record visible in same file");
}

// --- Nested pattern matching ---

#[test]
fn test_match_record_with_enum_field() {
    common::compile_and_run(
        r#"
package a

record Container =
    value: Option<Int32>

function main(): Unit =
    let c = Container { value = Some(42) }
    let result = match c with
        case Container { value = Some(x) } => x
        case Container { value = None } => -1
    assert result == 42
"#,
    )
    .expect("record with enum sub-pattern");
}

#[test]
fn test_match_record_with_record_field() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

record Line =
    start: Point
    end: Point

function main(): Unit =
    let l = Line { start = Point { x = 1; y = 2 }; end = Point { x = 3; y = 4 } }
    let result = match l with
        case Line { start = Point { x = sx, y = sy }, end = Point { x = ex, y = ey } } => sx + sy + ex + ey
    assert result == 10
"#,
    )
    .expect("record with record sub-pattern");
}

#[test]
fn test_match_record_with_literal_and_nested_enum() {
    common::compile_and_run(
        r#"
package a

record Tagged =
    tag: Int32
    value: Option<Int32>

function main(): Unit =
    let t = Tagged { tag = 1; value = Some(99) }
    let result = match t with
        case Tagged { tag = 1, value = Some(x) } => x
        case Tagged { tag = 1, value = None } => -1
        case Tagged { value = v } => -2
    assert result == 99
"#,
    )
    .expect("record with literal and nested enum");
}
