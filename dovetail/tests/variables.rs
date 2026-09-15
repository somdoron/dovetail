mod common;

#[test]
fn test_let_binding() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let x = 5
    assert x == 5
",
    )
    .expect("let binding should work");
}

#[test]
fn test_let_mutable_and_assign() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let mutable x = 0
    x = 5
    assert x == 5
",
    )
    .expect("mutable assignment should work");
}

#[test]
fn test_let_with_type_annotation() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let x: Int32 = 5
    assert x == 5
",
    )
    .expect("let with type annotation should work");
}

#[test]
fn test_let_shadowing() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let x = 5
    let x = 10
    assert x == 10
",
    )
    .expect("shadowing should work");
}

#[test]
fn test_let_in_computation() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let x = 3
    let y = 4
    assert x + y == 7
",
    )
    .expect("let in computation should work");
}

#[test]
fn test_let_bool_variable() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let flag = true
    assert flag
",
    )
    .expect("let with bool should work");
}

#[test]
fn test_mutable_increment() {
    common::compile_and_run(
        "\
package a

function main(): Unit =
    let mutable counter = 0
    counter = counter + 1
    counter = counter + 1
    assert counter == 2
",
    )
    .expect("mutable increment should work");
}

#[test]
fn test_assign_immutable_error() {
    let errors = common::compile_expecting_errors(
        "\
package a

function main(): Unit =
    let x = 5
    x = 10
    ()
",
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot assign to immutable variable")),
        "expected immutable assignment error, got: {:?}",
        errors
    );
}

#[test]
fn test_undefined_variable_error() {
    let errors = common::compile_expecting_errors(
        "\
package a

function main(): Unit =
    assert x == 5
",
    );
    assert!(
        errors.iter().any(|e| e.contains("undefined variable")),
        "expected undefined variable error, got: {:?}",
        errors
    );
}

#[test]
fn test_let_type_mismatch_error() {
    let errors = common::compile_expecting_errors(
        "\
package a

function main(): Unit =
    let x: Bool = 5
    ()
",
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}
