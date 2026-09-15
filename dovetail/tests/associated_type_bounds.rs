mod common;

const STATIC_ASSOCIATED_METHOD: &str = r#"
package a
record Value = n: Int32
trait Consume =
    type Output
    function consume(value: Option<Output>): Output
implement Consume for Value =
    type Output = Int32
    function consume(value: Option<Int32>): Int32 = value.require + 1
"#;

#[test]
fn static_trait_methods_substitute_associated_parameters_and_results() {
    common::compile_and_run(&format!(
        r#"
{STATIC_ASSOCIATED_METHOD}
function consume<T, O>(receiver: T, value: Option<O>): O where T: Consume<Output = O> = T.consume(value)
function main(): Unit = assert consume(Value {{ n = 0 }}, Some(2)) == 3
"#,
    ))
    .unwrap();
}

#[test]
fn static_trait_methods_reject_wrong_associated_arguments_and_arity() {
    for call in [
        r#"T.consume(Some("bad"))"#,
        "T.consume()",
        "T.consume(Some(2), Some(3))",
    ] {
        let source = format!(
            r#"
{STATIC_ASSOCIATED_METHOD}
function consume<T>(receiver: T): Int32 where T: Consume<Output = Int32> = {call}
function main(): Unit = ()
"#
        );
        let result = dovetail::check(&source, "test.dove");
        assert!(result.diagnostics.has_errors(), "accepted {call}");
    }
}

#[test]
fn static_trait_methods_cannot_assume_an_abstract_output_type() {
    let source = format!(
        r#"
{STATIC_ASSOCIATED_METHOD}
function consume<T>(receiver: T): Int32 where T: Consume = T.consume(Some(2))
function main(): Unit = ()
"#
    );
    let result = dovetail::check(&source, "test.dove");
    assert!(
        result.diagnostics.has_errors(),
        "an abstract output cannot be assumed to be Int32"
    );
}

#[test]
fn class_methods_infer_associated_outputs() {
    common::compile_and_run(
        r#"
package a
class Calculator =
    public function add<L, R, O>(self: Calculator, left: L, right: R): O where L: Add<R, Output = O> = left + right
    public function join<L, R, O>(left: L, right: R): O where L: Concat<R, Output = O> = left ++ right
function main(): Unit =
    let number = Calculator().add(1, 2)
    let text = Calculator.join("a", "b")
    assert number == 3
    assert text == "ab"
"#,
    )
    .unwrap();
}

#[test]
fn static_trait_methods_select_the_matching_associated_parameter() {
    common::compile_and_run(
        r#"
package a
record Value = n: Int32
trait Consume<R> =
    type Input
    function consume(value: Input): R
implement Consume<Int32> for Value =
    type Input = Int32
    function consume(value: Int32): Int32 = value + 1
implement Consume<String> for Value =
    type Input = String
    function consume(value: String): String = value ++ "!"
function consume<T>(receiver: T): String where T: Consume<Int32, Input = Int32> + Consume<String, Input = String> = T.consume("ok")
function main(): Unit = assert consume(Value { n = 0 }) == "ok!"
"#,
    )
    .unwrap();
}
