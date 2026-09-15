//! The `::` cons operator.

mod common;

#[test]
fn cons_prepends() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = 1 :: [2, 3]
    assert xs.length == 3
    assert xs.head == 1
    assert xs.tail.head == 2
"#,
    )
    .expect("cons");
}

#[test]
fn cons_is_right_associative() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = 1 :: 2 :: 3 :: []
    assert xs.length == 3
    assert xs.head == 1
    assert xs.tail.tail.head == 3
"#,
    )
    .expect("right associativity");
}

/// The flattening rule: `a :: b :: c :: []` must produce exactly the node
/// `[a, b, c]` does, so both share one element-type join.
#[test]
fn cons_chain_equals_list_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1 :: 2 :: 3 :: []) == [1, 2, 3]
"#,
    )
    .expect("cons chain == list literal");
}

/// `+` binds tighter than `::`; `::` binds tighter than `==`; and `++` binds
/// tighter than `::`, so `x :: xs ++ ys` is `x :: (xs ++ ys)`.
#[test]
fn cons_precedence() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1 + 2 :: []) == [3]
    let rest: List<Int32> = []
    assert (1 :: rest) == [1]
    assert (1 :: [2] ++ [3]) == [1, 2, 3]
"#,
    )
    .expect("precedence");
}

/// The regression that rules out desugaring `::` straight to `List.Cons`: the
/// enum-construction path would bind the element type from the head and reject
/// a widening tail.
#[test]
fn cons_widens_to_a_common_supertype() {
    common::compile_and_run(
        r#"
package a

class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function main(): Unit =
    let animals: List<Animal> = [Animal("generic")]
    let all: List<Animal> = Dog("rex") :: animals
    assert all.length == 2
    assert all.head.name == "rex"
"#,
    )
    .expect("cons widening");
}

#[test]
fn cons_tail_must_be_a_list() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = 1 :: 5
    assert true
"#,
    )
    .expect_err("non-list tail");
    assert!(
        format!("{err:?}").contains("expected a 'List' on the right of '::'"),
        "unexpected: {err:?}"
    );
}

#[test]
fn cons_in_a_generic_function() {
    common::compile_and_run(
        r#"
package a

function prepend<T>(x: T, xs: List<T>): List<T> = x :: xs

function main(): Unit =
    assert prepend(1, [2, 3]).length == 3
    assert prepend("a", []).head == "a"
"#,
    )
    .expect("generic cons");
}

/// `::` prepends one element; `++` joins two lists.
#[test]
fn concat_operator_on_lists() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert ([1, 2] ++ [3, 4]) == [1, 2, 3, 4]
    let empty: List<Int32> = []
    assert (empty ++ [1]) == [1]
    assert ([1] ++ empty) == [1]
"#,
    )
    .expect("++ on lists");
}
