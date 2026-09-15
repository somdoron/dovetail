mod common;

// ── Parsing function types ──────────────────────────────────────────

#[test]
fn function_type_in_parameter_single_param() {
    // (Int32) => Int32 as a parameter type — passes typechecking
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32) => Int32): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_in_parameter_multi_param() {
    // (Int32, String) => Bool as a parameter type
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32, String) => Bool): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_parenthesized_single_param() {
    // (Int32) => Bool — parenthesized single param
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32) => Bool): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_nested_right_associative() {
    // (Int32) => (Int32) => Int32 — nested, right-associative
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32) => (Int32) => Int32): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_in_return_position() {
    // Function type as return type — check-only (no codegen, no function values)
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32) => Int32): (Int32) => Int32 = f

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_as_generic_arg() {
    // Function type as a type argument: Array<(Int32) => Bool>
    common::check_no_errors(
        r#"
package a

function foo(fs: Array<(Int32) => Bool>): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_in_let_annotation() {
    // Function type in a let annotation (will error on value, but type itself is valid)
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Int32 = 42
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch, got: {:?}",
        errors
    );
}

// ── Bare (non-parenthesized) function type syntax ───────────────────

#[test]
fn function_type_bare_single_param() {
    // Int32 => Bool — bare single-param function type
    common::check_no_errors(
        r#"
package a

function foo(f: Int32 => Bool): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_bare_right_associative() {
    // Int32 => Int32 => Int32 — right-associative: Int32 => (Int32 => Int32)
    common::check_no_errors(
        r#"
package a

function foo(f: Int32 => Int32 => Int32): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_bare_does_not_conflict_with_match() {
    // Ensure bare function type doesn't break match arm parsing
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case n: Int32 => assert n == 42
        case _ => panic "unexpected"
"#,
    )
    .expect("match with type-annotated pattern");
}

// ── Function types in match pattern type annotations ────────────────

#[test]
fn match_pattern_named_type_with_fat_arrow() {
    // case n: Int32 => ... — basic type-annotated pattern, => is match arm separator
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case n: Int32 => assert n == 42
        case _ => panic "unexpected"
"#,
    )
    .expect("named type in match pattern");
}

#[test]
fn match_pattern_generic_type_with_fat_arrow() {
    // case r: Option<Int32> => ... — generic type in pattern annotation
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x: Any = Option.Some(42)
    match x with
        case s: Option<Int32> => ()
        case _ => ()
"#,
    );
}

#[test]
fn match_pattern_parenthesized_function_type() {
    // case f: (Int32 => Bool) => ... — parenthesized function type in pattern
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case f: (Int32 => Bool) => ()
        case _ => ()
"#,
    );
}

#[test]
fn match_pattern_parenthesized_multi_param_function_type() {
    // case f: ((Int32, String) => Bool) => ... — multi-param function type in pattern
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x: Any = 42
    match x with
        case f: ((Int32, String) => Bool) => ()
        case _ => ()
"#,
    );
}

#[test]
fn match_pattern_multiple_typed_arms() {
    // Multiple type-annotated arms in a single match
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Any = "hello"
    let result = match x with
        case n: Int32 => 1
        case s: String => 2
        case b: Bool => 3
        case _ => 0
    assert result == 2
"#,
    )
    .expect("multiple typed match arms");
}

// ── Type checking: Display output ──────────────────────────────────

#[test]
fn function_type_display_in_error_single_param() {
    // Verify that function type Display is correct in error messages
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(f: (Int32) => Bool): Unit =
    let x: Int32 = f
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("Int32 => Bool")),
        "expected error mentioning function type, got: {:?}",
        errors
    );
}

#[test]
fn function_type_display_in_error_multi_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(f: (Int32, String) => Bool): Unit =
    let x: Int32 = f
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("(Int32, String) => Bool")),
        "expected error mentioning function type, got: {:?}",
        errors
    );
}

// ── Type checking: assignability ────────────────────────────────────

#[test]
fn function_type_mismatch_different_param_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(f: (Int32) => Bool): Unit =
    let g: (Int32, String) => Bool = f
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn function_type_mismatch_different_return_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(f: (Int32) => Bool): Unit =
    let g: (Int32) => Int32 = f
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn function_type_same_type_assignable() {
    // Same function type should be assignable
    common::check_no_errors(
        r#"
package a

function foo(f: (Int32) => Bool): Unit =
    let g: (Int32) => Bool = f
    ()

function main(): Unit = ()
"#,
    );
}

// ── Function type with generics ─────────────────────────────────────

#[test]
fn function_type_with_generic_type_params() {
    // Function type inside a generic function signature (check-only, no calling)
    common::check_no_errors(
        r#"
package a

function apply<T, U>(f: (T) => U, x: T): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_in_record_field() {
    // Function type as a record field type
    common::check_no_errors(
        r#"
package a

record Handler =
    callback: (Int32) => Bool

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_in_enum_variant() {
    // Function type as an enum variant payload
    common::check_no_errors(
        r#"
package a

enum Action =
    Run((Int32) => Bool)

function main(): Unit = ()
"#,
    );
}

// ── Zero-parameter function types and closures ──────────────────────

#[test]
fn function_type_zero_param() {
    common::check_no_errors(
        r#"
package a

function foo(f: () => Int32): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn function_type_zero_param_type_alias() {
    common::check_no_errors(
        r#"
package a

type Thunk<T> = () => T

function foo(f: Thunk<Int32>): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn zero_param_closure_expression() {
    common::compile_and_run(
        r#"
package a

function apply(f: () => Int32): Int32 = f()

function main(): Unit =
    let result = apply(() => 42)
    assert result == 42
"#,
    )
    .expect("zero-param closure");
}

#[test]
fn zero_param_closure_deferred_evaluation() {
    common::compile_and_run(
        r#"
package a

function runIf(condition: Bool, f: () => Int32): Int32 =
    if condition then f() else 0

function main(): Unit =
    let result = runIf(true, () => 10 + 5)
    assert result == 15
    let result2 = runIf(false, () => 10 + 5)
    assert result2 == 0
"#,
    )
    .expect("deferred evaluation");
}

#[test]
fn zero_param_closure_as_return_type() {
    common::compile_and_run(
        r#"
package a

function makeThunk(x: Int32): () => Int32 = () => x

function main(): Unit =
    let f = makeThunk(42)
    assert f() == 42
"#,
    )
    .expect("closure as return type");
}

#[test]
fn zero_param_closure_nested() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: () => () => Int32 = () => () => 99
    let g = f()
    assert g() == 99
"#,
    )
    .expect("nested zero-param closures");
}
