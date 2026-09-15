mod common;

// ── Basic function parameters ───────────────────────────────────────

#[test]
fn test_function_with_params() {
    common::compile_and_run(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit = assert add(2, 3) == 5
"#,
    )
    .expect("function with params");
}

#[test]
fn test_single_param() {
    common::compile_and_run(
        r#"
package a

function double(x: Int32): Int32 = x * 2

function main(): Unit = assert double(7) == 14
"#,
    )
    .expect("single param");
}

#[test]
fn test_unit_return_with_params() {
    common::compile_and_run(
        r#"
package a

function check(x: Int32) = assert x > 0

function main(): Unit = check(5)
"#,
    )
    .expect("unit return with params");
}

#[test]
fn test_bool_param() {
    common::compile_and_run(
        r#"
package a

function identity(b: Bool): Bool = b

function main(): Unit = assert identity(true)
"#,
    )
    .expect("bool param");
}

#[test]
fn test_multiple_typed_params() {
    common::compile_and_run(
        r#"
package a

function pick(condition: Bool, a: Int32, b: Int32): Int32 =
    let result = a + b
    result

function main(): Unit = assert pick(true, 3, 4) == 7
"#,
    )
    .expect("multiple typed params");
}

// ── Function calls ──────────────────────────────────────────────────

#[test]
fn test_nested_calls() {
    common::compile_and_run(
        r#"
package a

function inc(x: Int32): Int32 = x + 1

function double(x: Int32): Int32 = x * 2

function main(): Unit = assert double(inc(3)) == 8
"#,
    )
    .expect("nested calls");
}

#[test]
fn test_call_no_args() {
    common::compile_and_run(
        r#"
package a

function five(): Int32 = 5

function main(): Unit = assert five() == 5
"#,
    )
    .expect("call no args");
}

#[test]
fn test_call_in_expression() {
    common::compile_and_run(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit = assert add(1, 2) + add(3, 4) == 10
"#,
    )
    .expect("call in expression");
}

#[test]
fn test_function_calling_another() {
    common::compile_and_run(
        r#"
package a

function square(x: Int32): Int32 = x * x

function sum_of_squares(a: Int32, b: Int32): Int32 = square(a) + square(b)

function main(): Unit = assert sum_of_squares(3, 4) == 25
"#,
    )
    .expect("function calling another");
}

// ── Overloading ─────────────────────────────────────────────────────

#[test]
fn test_overload_different_types() {
    common::compile_and_run(
        r#"
package a

function negate(x: Int32): Int32 = 0 - x

function negate(x: Bool): Bool = !x

function main(): Unit =
    assert negate(5) == -5
    assert negate(true) == false
"#,
    )
    .expect("overload different types");
}

#[test]
fn test_overload_different_arity() {
    common::compile_and_run(
        r#"
package a

function compute(x: Int32): Int32 = x * 2

function compute(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
    assert compute(5) == 10
    assert compute(3, 4) == 7
"#,
    )
    .expect("overload different arity");
}

// ── Visibility ──────────────────────────────────────────────────────

#[test]
fn test_public_function() {
    common::compile_and_run(
        r#"
package a

public function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit = assert add(1, 2) == 3
"#,
    )
    .expect("public function");
}

#[test]
fn test_internal_function() {
    common::compile_and_run(
        r#"
package a

internal function helper(x: Int32): Int32 = x + 1

function main(): Unit = assert helper(4) == 5
"#,
    )
    .expect("internal function");
}

// ── Int64 params ────────────────────────────────────────────────────

#[test]
fn test_int64_params() {
    common::compile_and_run(
        r#"
package a

function add64(a: Int64, b: Int64): Int64 = a + b

function main(): Unit = assert add64(100i64, 200i64) == 300i64
"#,
    )
    .expect("int64 params");
}

// ── Float params ────────────────────────────────────────────────────

#[test]
fn test_float64_params() {
    common::compile_and_run(
        r#"
package a

function mul(a: Float64, b: Float64): Float64 = a * b

function main(): Unit = assert mul(2.5f64, 4.0f64) == 10.0f64
"#,
    )
    .expect("float64 params");
}

// ── Private functions ───────────────────────────────────────────────

#[test]
fn test_private_function_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

private function secret(): Int32 = 42

function main(): Unit = assert secret() == 42
"#,
    )
    .expect("private function visible in same file");
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn test_undefined_function_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = foo(1)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("undefined function")),
        "expected 'undefined function' error, got: {:?}",
        errors
    );
}

#[test]
fn test_wrong_argument_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit = add(1, true)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected 'no matching overload' error, got: {:?}",
        errors
    );
}

#[test]
fn test_wrong_arg_count_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit = add(1)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected 'no matching overload' error, got: {:?}",
        errors
    );
}

#[test]
fn test_duplicate_function_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(): Unit = ()
function foo(): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate function")),
        "expected 'duplicate function' error, got: {:?}",
        errors
    );
}
