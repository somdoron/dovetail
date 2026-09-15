mod common;

const PRODUCER: &str = r#"
package a
trait Producer =
    type Output
    function produce(self): Output
record NumberProducer = value: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
type Output<P> where P: Producer = P.Output
"#;

#[test]
fn concrete_associated_alias_in_an_ordinary_signature_normalizes() {
    let source = format!(
        "{PRODUCER}{}",
        r#"
function readNumber(producer: NumberProducer): Output<NumberProducer> = producer.produce()
function increment(value: Output<NumberProducer>): Int32 = value + 1
function main(): Unit = assert increment(readNumber(NumberProducer { value = 41 })) == 42
"#
    );
    common::compile_and_run(&source)
        .expect("concrete aliases normalize before ordinary signature use");
}

#[test]
fn concrete_associated_aliases_normalize_in_stored_fields() {
    let source = format!(
        "{PRODUCER}{}",
        r#"
record Stored = value: Output<NumberProducer>
newtype Wrapped = Output<NumberProducer>
enum Choice = Value(Output<NumberProducer>)
class Box(value: Output<NumberProducer>) =
    public function get(self: Box): Output<NumberProducer> = self.value
function main(): Unit =
    let stored = Stored { value = 42 }
    assert stored.value == 42
    assert Wrapped(42).value == 42
    assert Box(42).get() == 42
    let extracted = match Choice.Value(42) with
        case Value(value) => value
    assert extracted == 42
"#
    );
    common::compile_and_run(&source)
        .expect("all stored representations use normalized concrete types");
}

#[test]
fn concrete_projection_targets_unlock_further_implementations() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
trait Next =
    type Result
record First = id: Int32
record Second = id: Int32
record Third = id: Int32
implement Producer for First =
    type Output = Second
type Output<P> where P: Producer = P.Output
type ResultOf<N> where N: Next = N.Result
implement Next for Output<First> =
    type Result = Third
implement Producer for ResultOf<Second> =
    type Output = Int32
function identity(value: Output<Third>): Int32 = value
function main(): Unit = assert identity(42) == 42
"#,
    )
    .expect("each normalized target makes its associated definitions available to the next pass");
}

#[test]
fn unresolved_projection_targets_preserve_diagnostics() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Producer =
    type Output
trait Next =
    type Result
record First = id: Int32
type Output<P> where P: Producer = P.Output
implement Next for Output<First> =
    type Result = Int32
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cannot resolve associated type")),
        "{errors:?}"
    );
}

#[test]
fn recursive_projection_definitions_preserve_cycle_diagnostics() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Producer =
    type Output
record Recursive = id: Int32
type Output<P> where P: Producer = P.Output
implement Producer for Recursive =
    type Output = Output<Recursive>
function identity(value: Output<Recursive>): Unit = ()
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cyclic associated type")),
        "{errors:?}"
    );
}

#[test]
fn wrapped_recursive_projections_do_not_expand_across_collection_rounds() {
    let mut source = String::from(
        r#"
package a
trait Producer =
    type Output
record Recursive = id: Int32
type Output<P> where P: Producer = P.Output
implement Producer for Recursive =
    type Output = Array<Output<Recursive>>
function identity(value: Output<Recursive>): Unit = ()
function main(): Unit = ()
"#,
    );
    for index in 0..40 {
        source.push_str(&format!("\nrecord Unrelated{index} = value: Int32\n"));
    }
    let errors = common::compile_expecting_errors(&source);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cyclic associated type")),
        "{errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|error| error.contains("did not converge")),
        "{errors:?}"
    );
}

#[test]
fn concrete_associated_aliases_normalize_in_module_members() {
    let source = format!(
        "{PRODUCER}{}",
        r#"
module Numbers =
    public function increment(value: Output<NumberProducer>): Output<NumberProducer> = value + 1
function main(): Unit = assert Numbers.increment(41) == 42
"#
    );
    common::compile_and_run(&source)
        .expect("module signatures and names agree after normalization");
}

#[test]
fn concrete_associated_aliases_are_normalized_before_contract_validation() {
    let source = format!(
        "{PRODUCER}{}",
        r#"
trait Increment =
    function increment(self, value: Int32): Int32
record Reader = id: Int32
implement Increment for Reader =
    function increment(self, value: Output<NumberProducer>): Output<NumberProducer> = value + 1
function main(): Unit = assert Reader { id = 0 }.increment(41) == 42
"#
    );
    common::compile_and_run(&source)
        .expect("collection validates normalized concrete alias signatures");
}

#[test]
fn invalid_concrete_associated_alias_contract_is_still_rejected() {
    let source = format!(
        "{PRODUCER}{}",
        r#"
trait Text =
    function text(self): String
record Reader = id: Int32
implement Text for Reader =
    function text(self): Output<NumberProducer> = 42
function main(): Unit = ()
"#
    );
    let errors = common::compile_expecting_errors(&source);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("return type mismatch")),
        "{errors:?}"
    );
}
