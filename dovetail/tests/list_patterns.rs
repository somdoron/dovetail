//! `[]` / `[a, b]` / `h :: t` patterns.

mod common;

#[test]
fn empty_and_cons_patterns() {
    common::compile_and_run(
        r#"
package a

function describe(xs: List<Int32>): String =
    match xs with
        case [] => "empty"
        case h :: t => "cons"

function main(): Unit =
    let empty: List<Int32> = []
    assert describe(empty) == "empty"
    assert describe([1]) == "cons"
"#,
    )
    .expect("empty and cons patterns");
}

#[test]
fn cons_pattern_binds_head_and_tail() {
    common::compile_and_run(
        r#"
package a

function sum(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case h :: t => h + sum(t)

function main(): Unit = assert sum([1, 2, 3, 4]) == 10
"#,
    )
    .expect("recursive sum");
}

#[test]
fn multi_element_cons_pattern() {
    common::compile_and_run(
        r#"
package a

function firstTwo(xs: List<Int32>): Int32 =
    match xs with
        case a :: b :: rest => a + b
        case _ => 0

function main(): Unit =
    assert firstTwo([10, 20, 30]) == 30
    assert firstTwo([1]) == 0
"#,
    )
    .expect("a :: b :: rest");
}

#[test]
fn fixed_length_list_patterns() {
    common::compile_and_run(
        r#"
package a

function size(xs: List<Int32>): String =
    match xs with
        case [] => "none"
        case [a] => "one"
        case [a, b] => "two"
        case _ => "many"

function main(): Unit =
    let empty: List<Int32> = []
    assert size(empty) == "none"
    assert size([1]) == "one"
    assert size([1, 2]) == "two"
    assert size([1, 2, 3]) == "many"
"#,
    )
    .expect("fixed length patterns");
}

#[test]
fn list_patterns_nest() {
    common::compile_and_run(
        r#"
package a

function f(x: Option<List<Int32>>): Int32 =
    match x with
        case Some(h :: t) => h
        case Some([]) => -1
        case None => -2

function main(): Unit =
    assert f(Some([7, 8])) == 7
    let empty: List<Int32> = []
    assert f(Some(empty)) == -1
    assert f(None) == -2
"#,
    )
    .expect("nested list patterns");
}

#[test]
fn list_pattern_inside_a_tuple() {
    common::compile_and_run(
        r#"
package a

function f(pair: (List<Int32>, Int32)): Int32 =
    match pair with
        case (h :: t, n) => h + n
        case ([], n) => n

function main(): Unit =
    assert f(([1, 2], 10)) == 11
    let empty: List<Int32> = []
    assert f((empty, 5)) == 5
"#,
    )
    .expect("list pattern in tuple");
}

#[test]
fn guards_apply_to_list_patterns() {
    common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): String =
    match xs with
        case h :: t if h > 10 => "big"
        case h :: t => "small"
        case [] => "empty"

function main(): Unit =
    assert f([20]) == "big"
    assert f([1]) == "small"
    let empty: List<Int32> = []
    assert f(empty) == "empty"
"#,
    )
    .expect("guards");
}

/// `[] | h :: t` covers every `List`, so no catch-all is required.
#[test]
fn empty_plus_cons_is_exhaustive() {
    common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case h :: t => h

function main(): Unit = assert f([1]) == 1
"#,
    )
    .expect("exhaustive");
}

/// A cons pattern alone leaves the empty list unmatched.
#[test]
fn cons_alone_is_not_exhaustive() {
    let err = common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): Int32 =
    match xs with
        case h :: t => h

function main(): Unit = assert f([1]) == 1
"#,
    )
    .expect_err("non-exhaustive");
    assert!(
        format!("{err:?}").contains("non-exhaustive"),
        "unexpected: {err:?}"
    );
}

/// A list pattern only matches a `List`.
#[test]
fn list_pattern_against_a_non_list_scrutinee() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = [| 1, 2 |]
    let n = match xs with
        case [] => 0
        case h :: t => h
    assert n == 1
"#,
    )
    .expect_err("array scrutinee");
    let text = format!("{err:?}");
    assert!(
        text.contains("List") || text.contains("enum"),
        "unexpected: {text}"
    );
}

// ── Nested-pattern exhaustiveness ─────────────────────────────────────
// The checker looks through payloads, so a fixed-length list pattern no
// longer counts as covering the whole `Cons` variant.

/// The motivating case: `[]` and `[a, b]` leave `[x]` unmatched.
#[test]
fn fixed_lengths_alone_are_not_exhaustive() {
    let err = common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case [a, b] => a + b

function main(): Unit = assert f([1, 2]) == 3
"#,
    )
    .expect_err("non-exhaustive");
    let text = format!("{err:?}");
    assert!(text.contains("non-exhaustive"), "{text}");
    // The witness is reported in list notation, not as `Cons(_, Nil)`.
    assert!(text.contains("[_]"), "{text}");
}

#[test]
fn a_longer_list_is_reported_as_the_witness() {
    let err = common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case [a] => a

function main(): Unit = assert f([1]) == 1
"#,
    )
    .expect_err("non-exhaustive");
    let text = format!("{err:?}");
    assert!(text.contains("non-exhaustive"), "{text}");
    assert!(text.contains("[_, "), "expected a two-or-more witness: {text}");
}

/// Covering every length up to a trailing `h :: t` is exhaustive.
#[test]
fn fixed_lengths_plus_a_cons_tail_is_exhaustive() {
    common::compile_and_run(
        r#"
package a

function f(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case [a] => a
        case a :: b :: rest => a + b

function main(): Unit =
    assert f([1, 2, 3]) == 3
    assert f([5]) == 5
"#,
    )
    .expect("exhaustive by length");
}

/// `[]` is the `Nil` pattern, so it composes with `::` to cover a list by
/// length. All three arms together are exhaustive — no wildcard needed.
#[test]
fn empty_literal_is_the_nil_pattern() {
    common::compile_and_run(
        r#"
package a

function describe(xs: List<Int32>): String =
    match xs with
        case [] => "empty"
        case head :: [] => "one"
        case head :: tail => "many"

function main(): Unit =
    let empty: List<Int32> = []
    assert describe(empty) == "empty"
    assert describe([7]) == "one"
    assert describe([7, 8]) == "many"
"#,
    )
    .expect("[] / head :: [] / head :: tail is exhaustive");
}

/// Matching via the literal and via the variant must agree.
#[test]
fn literal_and_variant_patterns_agree() {
    common::compile_and_run(
        r#"
package a

function viaLiteral(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case h :: t => h

function viaVariant(xs: List<Int32>): Int32 =
    match xs with
        case Nil => 0
        case Cons(h, t) => h

function main(): Unit =
    let empty: List<Int32> = []
    assert viaLiteral(empty) == viaVariant(empty)
    assert viaLiteral([5]) == viaVariant([5])
"#,
    )
    .expect("[] and Nil are the same pattern");
}
