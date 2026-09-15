//! `[ ... ]` list literals.

mod common;

#[test]
fn list_literal_basics() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [1, 2, 3]
    assert xs.head == 1
    assert xs.tail.head == 2
    assert xs.length == 3
"#,
    )
    .expect("list literal");
}

/// Unlike an array literal, `[]` needs no annotation: `List<out T>` is
/// covariant, so `List<Never>` is assignable to every `List<T>`.
#[test]
fn empty_list_infers_without_annotation() {
    common::compile_and_run(
        r#"
package a

function takesList(xs: List<Int32>): Int32 = xs.length

function main(): Unit =
    assert takesList([]) == 0
    let xs: List<String> = []
    assert xs.isEmpty
"#,
    )
    .expect("empty list literal");
}

/// The element type is the lowest common type of the elements, exactly as for
/// array literals — not the first element's type.
#[test]
fn elements_join_to_a_common_supertype() {
    common::compile_and_run(
        r#"
package a

class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function main(): Unit =
    let dog = Dog("rex")
    let animal = Animal("generic")
    let xs: List<Animal> = [dog, animal]
    assert xs.length == 2
    assert xs.head.name == "rex"
"#,
    )
    .expect("element join");
}

/// A mismatch is reported against the offending element, not the whole literal.
#[test]
fn element_type_mismatch_reports_the_element() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [1, "two", 3]
    assert true
"#,
    )
    .expect_err("mixed element types");
    let text = format!("{err:?}");
    assert!(
        text.contains("type mismatch") && text.contains("Int32") && text.contains("String"),
        "unexpected: {text}"
    );
}

#[test]
fn nesting_and_trailing_comma() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [1, 2,]
    assert xs.length == 2
    let nested: List<List<Int32>> = [[1], [2, 3]]
    assert nested.length == 2
    assert nested.tail.head.tail.head == 3
"#,
    )
    .expect("nesting");
}

#[test]
fn list_literal_in_argument_position() {
    common::compile_and_run(
        r#"
package a

function total(xs: List<Int32>): Int32 = xs.foldLeft(0, (a, b) => a + b)

function main(): Unit = assert total([1, 2, 3]) == 6
"#,
    )
    .expect("argument position");
}

/// A statement starting with `[` after another statement must parse as a list
/// literal, not as an index into the previous expression.
#[test]
fn statement_initial_bracket_is_a_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = [1, 2]
    let b = [3, 4]
    assert a.length + b.length == 4
"#,
    )
    .expect("statement-initial bracket");
}
