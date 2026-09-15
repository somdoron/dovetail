//! `++` and the `Concat` trait.
//!
//! `++` is concatenation, kept distinct from `+` so arithmetic and joining
//! never share an overload. All concatenation uses `Concat<R>`, whose associated
//! `Output` decides the result type. String uses a native implementation.

mod common;

fn check_err_contains(source: &str, needle: &str) {
    let result = dovetail::check(source, "test.dove");
    let combined: String = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        combined.contains(needle),
        "expected error containing '{needle}'; got:\n{combined}"
    );
}

#[test]
fn test_concat_strings_natively() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "left" ++ "right" == "leftright"
    assert ("a" ++ "b" ++ "c") == "abc"
"#,
    )
    .expect("String ++ String concatenates");
}

#[test]
fn test_concat_overload_on_a_record() {
    common::compile_and_run(
        r#"
package a

record Note = text: String

implement Concat<Note> for Note =
    type Output = Note
    function concat(self: Note, other: Note): Note = Note { text = self.text ++ other.text }

function main(): Unit =
    let joined = Note { text = "a" } ++ Note { text = "b" }
    assert joined.text == "ab"
"#,
    )
    .expect("a record with a Concat impl supports ++");
}

/// `Output` need not be `Self` — the same freedom `Div` has.
#[test]
fn test_concat_output_may_differ_from_self() {
    common::compile_and_run(
        r#"
package a

record Chunk = size: Int32
record Total = size: Int32

implement Concat<Chunk> for Chunk =
    type Output = Total
    function concat(self: Chunk, other: Chunk): Total = Total { size = self.size + other.size }

function main(): Unit =
    let total: Total = Chunk { size = 2 } ++ Chunk { size = 3 }
    assert total.size == 5
"#,
    )
    .expect("Concat's Output type is used as the result type");
}

/// The right-hand type is a parameter, so the operands need not match.
#[test]
fn test_concat_accepts_a_different_right_hand_type() {
    common::compile_and_run(
        r#"
package a

record Line = text: String

implement Concat<String> for Line =
    type Output = Line
    function concat(self: Line, other: String): Line = Line { text = self.text ++ other }

function main(): Unit =
    let line = Line { text = "a" } ++ "b"
    assert line.text == "ab"
"#,
    )
    .expect("Concat<R> allows an asymmetric right-hand type");
}

#[test]
fn test_concat_is_left_associative() {
    common::compile_and_run(
        r#"
package a

record Trace = steps: String

implement Concat<Trace> for Trace =
    type Output = Trace
    function concat(self: Trace, other: Trace): Trace =
        Trace { steps = "(" ++ self.steps ++ "+" ++ other.steps ++ ")" }

function main(): Unit =
    let a = Trace { steps = "a" }
    let b = Trace { steps = "b" }
    let c = Trace { steps = "c" }
    assert (a ++ b ++ c).steps == "((a+b)+c)"
"#,
    )
    .expect("++ groups to the left");
}

#[test]
fn test_concat_without_an_impl_is_an_error() {
    check_err_contains(
        r#"
package a

record Note = text: String

function main(): Unit =
    let joined = Note { text = "a" } ++ Note { text = "b" }
    ()
"#,
        "operator '++' is not supported for type 'Note'; consider implementing 'Concat'",
    );
}

/// `++` is concatenation only — it never falls back to arithmetic.
#[test]
fn test_concat_is_not_arithmetic() {
    check_err_contains(
        r#"
package a

function main(): Unit =
    let n = 1 ++ 2
    ()
"#,
        "operator '++' is not supported for type 'Int32'",
    );
}

/// `+` keeps its own meaning: a Concat impl does not make `+` work.
#[test]
fn test_a_concat_impl_does_not_overload_plus() {
    check_err_contains(
        r#"
package a

record Note = text: String

implement Concat<Note> for Note =
    type Output = Note
    function concat(self: Note, other: Note): Note = Note { text = self.text ++ other.text }

function main(): Unit =
    let joined = Note { text = "a" } + Note { text = "b" }
    ()
"#,
        "operator '+' is not supported for type 'Note'",
    );
}
