mod common;

#[test]
fn test_derive_equatable_simple_record() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p1 = Point { x = 1; y = 2 }
    let p2 = Point { x = 1; y = 2 }
    let p3 = Point { x = 1; y = 3 }
    assert p1 == p2
    assert (p1 == p3) == false
"#,
    )
    .expect("derived record equals");
}

#[test]
fn test_derive_equatable_single_field_record() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
record Wrap =
    n: Int32

function main(): Unit =
    let a = Wrap { n = 7 }
    let b = Wrap { n = 7 }
    let c = Wrap { n = 8 }
    assert a == b
    assert (a == c) == false
"#,
    )
    .expect("derived equals on single-field record");
}

#[test]
fn test_derive_equatable_nested_record() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
record Inner =
    n: Int32

@derive(Equatable)
record Outer =
    inner: Inner
    tag: Int32

function main(): Unit =
    let a = Outer { inner = Inner { n = 5 }; tag = 1 }
    let b = Outer { inner = Inner { n = 5 }; tag = 1 }
    let c = Outer { inner = Inner { n = 6 }; tag = 1 }
    assert a == b
    assert (a == c) == false
"#,
    )
    .expect("derived nested records");
}

#[test]
fn test_derive_equatable_enum_no_payload() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
enum Color =
    Red
    Green
    Blue

function main(): Unit =
    assert Color.Red == Color.Red
    assert (Color.Red == Color.Green) == false
"#,
    )
    .expect("derived equals on enum without payloads");
}

#[test]
fn test_derive_equatable_enum_tuple_payload() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
enum Shape =
    Circle(Int32)
    Rect(Int32, Int32)

function main(): Unit =
    assert Shape.Circle(5) == Shape.Circle(5)
    assert (Shape.Circle(5) == Shape.Circle(6)) == false
    assert (Shape.Circle(5) == Shape.Rect(3, 4)) == false
    assert Shape.Rect(3, 4) == Shape.Rect(3, 4)
    assert (Shape.Rect(3, 4) == Shape.Rect(3, 5)) == false
"#,
    )
    .expect("derived equals on tuple-payload enum");
}

#[test]
fn test_derive_equatable_generic_record() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
record Box<T> =
    value: T

function main(): Unit =
    let a = Box<Int32> { value = 1 }
    let b = Box<Int32> { value = 1 }
    let c = Box<Int32> { value = 2 }
    assert a == b
    assert (a == c) == false
"#,
    )
    .expect("derived equals on generic record");
}

#[test]
fn test_derive_equatable_on_non_record_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

@derive(Equatable)
function foo(): Int32 = 5

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("@derive(...) can only appear on a record or enum")),
        "expected @derive misuse error, got: {:?}",
        errors
    );
}

#[test]
fn test_derive_unknown_macro_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

@derive(NotAMacro)
record Foo = x: Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("unknown derive macro 'NotAMacro'")),
        "expected unknown derive macro error, got: {:?}",
        errors
    );
}
