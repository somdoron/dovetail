//! `[| ... |]` array literal syntax.

mod common;

#[test]
fn array_literal_and_empty() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [| 1, 2, 3 |]
    assert xs.length == 3
    assert xs.get(1) == 2
    let empty: Array<Int32> = [||]
    assert empty.length == 0
"#,
    )
    .expect("array literal");
}

#[test]
fn trailing_comma_and_nesting() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [| 1, 2, |]
    assert xs.length == 2
    let nested = [| [| 1 |], [| 2, 3 |] |]
    assert nested.length == 2
    assert nested.get(1).get(1) == 3
"#,
    )
    .expect("trailing comma and nesting");
}

/// `|]` is a closing delimiter, so layout must not insert a `Sep` before it.
#[test]
fn multiline_array_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [|
        1,
        2,
        3
    |]
    assert xs.length == 3
"#,
    )
    .expect("multiline array literal");
}

/// A bare `[` after an expression is still the index operator, not a literal.
#[test]
fn indexing_still_works() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [| 10, 20, 30 |]
    assert xs[0] == 10
    assert xs[2] == 30
"#,
    )
    .expect("indexing");
}

/// `|]` needs adjacency, so a trailing bitwise-or inside a literal is fine.
#[test]
fn bitwise_or_element() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = 1
    let b = 2
    let xs = [| a | b |]
    assert xs.length == 1
    assert xs.get(0) == 3
"#,
    )
    .expect("bitwise or element");
}

/// Array is invariant, so an empty array literal still needs an annotation.
#[test]
fn empty_array_literal_needs_a_type() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [||]
    assert true
"#,
    )
    .expect_err("empty array literal without a type");
    assert!(
        format!("{err:?}").contains("cannot infer element type for empty array literal"),
        "unexpected: {err:?}"
    );
}
