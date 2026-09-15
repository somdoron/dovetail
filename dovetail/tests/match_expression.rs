mod common;

// ── Basic match with Int32 literals + wildcard ────────────────────────

#[test]
fn test_match_int_literals_with_wildcard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 42
    let result = match x with
        case 0 => 0
        case 42 => 1
        case _ => 2
    assert result == 1
"#,
    )
    .expect("match on int32 with wildcard");
}

#[test]
fn test_match_int_wildcard_fallthrough() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 99
    let result = match x with
        case 0 => 0
        case 1 => 1
        case _ => 42
    assert result == 42
"#,
    )
    .expect("match wildcard should catch unmatched values");
}

// ── Match with Bool — exhaustive ────────────────────────────────────

#[test]
fn test_match_bool_exhaustive_no_wildcard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = true
    let result = match x with
        case true => 1
        case false => 0
    assert result == 1
"#,
    )
    .expect("bool match exhaustive without wildcard");
}

#[test]
fn test_match_bool_false_branch() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = false
    let result = match x with
        case true => 1
        case false => 0
    assert result == 0
"#,
    )
    .expect("bool match false branch");
}

// ── Match with variable pattern ──────────────────────────────────────

#[test]
fn test_match_variable_binding() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 42
    let result = match x with
        case n => n + 1
    assert result == 43
"#,
    )
    .expect("variable binding in match arm");
}

#[test]
fn test_match_variable_binding_with_literal_arms() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 10
    let result = match x with
        case 0 => 100
        case n => n * 2
    assert result == 20
"#,
    )
    .expect("variable binding after literal arms");
}

// ── Match with guard conditions ──────────────────────────────────────

#[test]
fn test_match_with_guard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 15
    let result = match x with
        case n if n > 10 => 1
        case _ => 0
    assert result == 1
"#,
    )
    .expect("match with guard condition");
}

#[test]
fn test_match_guard_falls_through() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 5
    let result = match x with
        case n if n > 10 => 1
        case _ => 0
    assert result == 0
"#,
    )
    .expect("guard condition fails, falls through to wildcard");
}

// ── Match as expression ──────────────────────────────────────────────

#[test]
fn test_match_as_expression_in_let() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 2
    let name = match x with
        case 1 => 10
        case 2 => 20
        case _ => 0
    assert name == 20
"#,
    )
    .expect("match result used in let binding");
}

// ── Match inside sub-expression ──────────────────────────────────────

#[test]
fn test_match_in_function_argument() {
    common::compile_and_run(
        r#"
package a

function double(n: Int32): Int32 = n * 2

function main(): Unit =
    let x = 3
    let result = double(match x with
        case 3 => 10
        case _ => 0
    )
    assert result == 20
"#,
    )
    .expect("match as function argument");
}

// ── Nested match ─────────────────────────────────────────────────────

#[test]
fn test_nested_match() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 1
    let y = true
    let result = match x with
        case 1 =>
            match y with
                case true => 100
                case false => 200
        case _ => 0
    assert result == 100
"#,
    )
    .expect("nested match expression");
}

// ── Match with negative literal ──────────────────────────────────────

#[test]
fn test_match_negative_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = -1
    let result = match x with
        case -1 => 1
        case 0 => 2
        case _ => 3
    assert result == 1
"#,
    )
    .expect("match with negative literal pattern");
}

// ── Match with Int64 ─────────────────────────────────────────────────

#[test]
fn test_match_int64() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 100i64
    let result = match x with
        case 100i64 => 1i64
        case _ => 0i64
    assert result == 1i64
"#,
    )
    .expect("match on Int64 values");
}

// ── Error tests ──────────────────────────────────────────────────────

#[test]
fn test_non_exhaustive_int_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = 42
    let result = match x with
        case 0 => 0
        case 1 => 1
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "expected non-exhaustive match error, got: {:?}",
        errors
    );
}

#[test]
fn test_non_exhaustive_bool_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = true
    let result = match x with
        case true => 1
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("non-exhaustive") && e.contains("false")),
        "expected non-exhaustive match error mentioning 'false', got: {:?}",
        errors
    );
}

#[test]
fn test_pattern_type_mismatch_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = 42
    let result = match x with
        case true => 0
        case _ => 1
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_type_annotated_pattern_mismatch_error() {
    // Non-generic function: match Int32 with case x: String => must report type mismatch.
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Int32 = 42
    let result = match x with
        case y: String => y
        case _ => "ok"
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type mismatch in type-annotated pattern")),
        "expected type-annotated pattern mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_arm_body_type_mismatch_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = 42
    let result = match x with
        case 0 => 1
        case _ => true
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected arm body type mismatch error, got: {:?}",
        errors
    );
}

// ── Match with all arm types ─────────────────────────────────────────

#[test]
fn test_match_all_arm_types() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 5
    let result = match x with
        case 0 => 100
        case 5 => 200
        case n => n
    assert result == 200
"#,
    )
    .expect("match with literal + variable arms");
}

// ── Match in statement context ───────────────────────────────────────

#[test]
fn test_match_in_statement_context() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 1
    match x with
        case 1 => assert true
        case _ => assert false
"#,
    )
    .expect("match used as a statement");
}

// ── Type-annotated pattern: same type (subject = expected) ─────────────

#[test]
fn test_type_annotated_pattern_same_type_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Int32 = 42
    let result = match x with
        case y: Int32 => y + 1
        case _ => 0
    assert result == 43
"#,
    )
    .expect("type-annotated pattern same type as subject");
}

#[test]
fn test_type_annotated_pattern_record_same_type_ok() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let result = match p with
        case q: Point => q.x + q.y
        case _ => 0
    assert result == 3
"#,
    )
    .expect("type-annotated pattern record same type as subject");
}

#[test]
fn test_type_annotated_pattern_unreachable_arm_error() {
    // Second arm (String) can never match Int32 subject → type error.
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Int32 = 42
    let result = match x with
        case a: Int32 => 1
        case b: String => 2
        case _ => 3
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type mismatch in type-annotated pattern")),
        "expected type-annotated pattern mismatch for unreachable String arm, got: {:?}",
        errors
    );
}

// ── Subject type Any: pattern type assignable to subject ──────────────

#[test]
fn test_match_subject_any_type_annotated_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    let result = match x with
        case s: String => s
        case _ => "other"
    assert result == "hello"
"#,
    )
    .expect("match Any with type-annotated String arm");
}

#[test]
fn test_match_subject_any_type_annotated_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    let result = match x with
        case n: Int32 => n + 1
        case _ => 0
    assert result == 43
"#,
    )
    .expect("match Any with type-annotated Int32 arm");
}

// ── Record pattern: same type (is_assignable_for_pattern) ──────────────

#[test]
fn test_record_pattern_same_type_ok() {
    common::compile_and_run(
        r#"
package a

record Point = x: Int32; y: Int32

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    let result = match p with
        case Point { x, y } => x + y
    assert result == 30
"#,
    )
    .expect("record pattern same type as subject");
}

#[test]
fn test_record_pattern_generic_same_fqn_ok() {
    common::compile_and_run(
        r#"
package a

record Box<T> = value: T

function main(): Unit =
    let b = Box<Int32> { value = 7 }
    let result = match b with
        case Box<Int32> { value = v } => v * 2
    assert result == 14
"#,
    )
    .expect("generic record pattern same type as subject");
}

// ── Regression: a `Never`-typed arm (try / early-return) in a match whose
// result type is a reference type. The divergent arm must emit `unreachable`
// so the match block's `br` validates against the (ref) result type instead of
// carrying Never's phantom i32 (which produced "expected (ref $type), found
// i32" at WASM translation time).

#[test]
fn test_match_arm_try_early_return_with_reference_result() {
    common::compile_and_run(
        r#"
package a

record Box =
    value: Int32

function firstOr(r: Result<Box, String>): Result<Box, String> =
    let b = match r with
        case Ok(box) => box
        case Error(_) => try Result.Error("fail")
    Ok(Box { value = b.value + 1 })

function main(): Unit =
    let ok = firstOr(Ok(Box { value = 41 }))
    assert ok.require.value == 42
    let bad = firstOr(Result.Error("nope"))
    assert bad.isError
"#,
    )
    .expect("Never-typed (try) match arm with a reference result type");
}

// ── Regression: a match arm with a closure in BOTH its guard and its body.
// The emitter emits the guard test first and the body inside it, but the
// closure prescan used to visit the body first — so the two closures were
// assigned each other's ids. Both capture exactly one variable here, so the
// capture-count check passed and the swap was silent: the guard ran the body's
// lambda and vice versa, producing a wrong (not crashing) result.

#[test]
fn test_match_arm_closure_in_both_guard_and_body() {
    common::compile_and_run(
        r#"
package a

function anyOf(xs: Array<Int32>, predicate: (Int32) => Bool): Bool =
    let mutable found = false
    for x in xs do
        if predicate(x) then
            found = true
    found

function countOf(xs: Array<Int32>, predicate: (Int32) => Bool): Int32 =
    let mutable total = 0
    for x in xs do
        if predicate(x) then
            total = total + 1
    total

function main(): Unit =
    let low = 2
    let high = 5
    let xs = [|1, 3, 7|]
    let result = match xs.length with
        case 3 if anyOf(xs, (a: Int32) => a > low) => countOf(xs, (a: Int32) => a > high)
        case _ => 0 - 1
    // Swapping the two closures makes the guard test `a > high` (still true)
    // and the body count `a > low`, which is 2 rather than 1.
    assert result == 1
"#,
    )
    .expect("closure in both the guard and the body of one match arm");
}
