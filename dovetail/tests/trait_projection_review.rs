mod common;

#[test]
fn projections_in_where_bounds_resolve_independently_of_order() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
    function produce(self): Output
trait Consumer<T> =
    function consume(self, value: T): Int32
record NumberProducer = value: Int32
record NumberConsumer = offset: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
implement Consumer<Int32> for NumberConsumer =
    function consume(self, value: Int32): Int32 = value + self.offset
function pipe<P, C>(producer: P, consumer: C): Int32
    where P: Producer, C: Consumer<P.Output> = consumer.consume(producer.produce())
function reversed<P, C>(producer: P, consumer: C): Int32
    where C: Consumer<P.Output>, P: Producer = consumer.consume(producer.produce())
type OutputAlias<P> where P: Producer = P.Output
function throughAlias<P, C>(producer: P, consumer: C): Int32
    where C: Consumer<OutputAlias<P>>, P: Producer = consumer.consume(producer.produce())
function sameOutput<P, Q>(producer: P, other: Q): P.Output
    where Q: Producer<Output = P.Output>, P: Producer = other.produce()
function main(): Unit =
    let producer = NumberProducer { value = 40 }
    let consumer = NumberConsumer { offset = 2 }
    assert pipe(producer, consumer) == 42
    assert reversed(producer, consumer) == 42
    assert throughAlias(producer, consumer) == 42
    assert sameOutput(producer, producer) == 40
"#,
    )
    .expect("projection bounds use all declared evidence, independent of order");
}

#[test]
fn generic_default_method_bounds_can_reference_another_parameters_output() {
    common::compile_and_run(r#"
package a
trait Producer =
    type Output
    function produce(self): Output
trait Consumer<T> =
    function consume(self, value: T): Int32
trait Pipe =
    function pipe<P, C>(self, producer: P, consumer: C): Int32
        where C: Consumer<P.Output>, P: Producer = consumer.consume(producer.produce())
record Runner = id: Int32
implement Pipe for Runner
record ExplicitRunner = id: Int32
implement Pipe for ExplicitRunner =
    function pipe<P, C>(self, producer: P, consumer: C): Int32
        where C: Consumer<P.Output> = consumer.consume(producer.produce())
record NumberProducer = value: Int32
record NumberConsumer = offset: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
implement Consumer<Int32> for NumberConsumer =
    function consume(self, value: Int32): Int32 = value + self.offset
function main(): Unit =
    assert Runner { id = 0 }.pipe(NumberProducer { value = 40 }, NumberConsumer { offset = 2 }) == 42
    assert ExplicitRunner { id = 0 }.pipe(NumberProducer { value = 40 }, NumberConsumer { offset = 2 }) == 42
"#).expect("generic default contracts can carry associated outputs in method bounds");
}

#[test]
fn invalid_projection_bound_evidence_still_reports_diagnostics() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Consumer<T> =
    function consume(self, value: T): Unit
function invalid<P, C>(producer: P, consumer: C): Unit
    where C: Consumer<P.Output> = ()
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("no bound on 'P' declares associated type 'Output'")),
        "{errors:?}"
    );
}

#[test]
fn abstract_resource_continuations_can_choose_a_different_error_type() {
    common::compile_and_run(r#"
package a
newtype Holder = Int32
implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self, f: (Int32) => U, errorF: (Never) => E2): U = f(self.value)
function scoped<R>(resource: R, finish: (Int32) => R.Wrapped<Int32, String>): R.Wrapped<Int32, String>
    where R: Usable<Int32, Never> =
    let value = use resource
    finish(value)
function main(): Unit = assert scoped(Holder(40), value => value + 2) == 42
"#).expect("the declared symbolic wrapper supplies the continuation error type");
}

#[test]
fn class_method_bounds_preserve_enclosing_projection_evidence() {
    common::compile_and_run(
        r#"
package a
import a.PipelineExtension
trait Producer =
    type Output
    function produce(self): Output
trait Consumer<T> =
    function consume(self, value: T): Int32
record NumberProducer = value: Int32
record NumberConsumer = offset: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
implement Consumer<Int32> for NumberConsumer =
    function consume(self, value: Int32): Int32 = value + self.offset
class Pipeline<P>(producer: P) where P: Producer =
    public function send<C>(self: Pipeline<P>, consumer: C): Int32
        where C: Consumer<P.Output> = consumer.consume(self.producer.produce())
extension PipelineExtension<P> for Pipeline<P> where P: Producer =
    function sendAgain<C>(self: Pipeline<P>, consumer: C): Int32
        where C: Consumer<P.Output> = self.send(consumer)
record Batch<P> where P: Producer =
    producer: P
module Batch<P> =
    function send<C>(self: Batch<P>, consumer: C): Int32
        where C: Consumer<P.Output> = consumer.consume(self.producer.produce())
function main(): Unit =
    let producer = NumberProducer { value = 40 }
    let consumer = NumberConsumer { offset = 2 }
    let pipeline = Pipeline(producer)
    assert pipeline.send(consumer) == 42
    assert pipeline.sendAgain(consumer) == 42
    assert Batch { producer = producer }.send(consumer) == 42
"#,
    )
    .expect("method-local bounds can reference an enclosing parameter's associated output");
}

#[test]
fn implementation_parameter_projections_use_the_selected_method_contract() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
    function produce(self): Output
trait Acceptor =
    function accept<P>(self, producer: P, value: P.Output): Unit where P: Producer
record NumberProducer = value: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
record Reader = id: Int32
implement Acceptor for Reader =
    function accept<P>(self, producer: P, value: P.Output): Unit where P: Producer = ()
function main(): Unit = Reader { id = 0 }.accept(NumberProducer { value = 42 }, 42)
"#,
    )
    .expect("implementation signatures resolve projections using their generic method contract");
}

#[test]
fn implementation_gat_parameters_preserve_method_bounds() {
    common::compile_and_run(
        r#"
package a
trait Wrapper =
    type Wrapped<T>
    function unwrap<T>(self, value: Wrapped<T>): T where T: Display
record Reader = id: Int32
implement Wrapper for Reader =
    type Wrapped<T> = Option<T>
    function unwrap<T>(self, value: Option<T>): T where T: Display = value.require
function main(): Unit = assert Reader { id = 0 }.unwrap<Int32>(Some(42)) == 42
"#,
    )
    .expect("GAT parameter matching retains the selected generic method bounds");
}

#[test]
fn implementation_matching_does_not_capture_enclosing_parameters() {
    common::compile_and_run(
        r#"
package a
trait Equal =
    function mix<A>(self, left: A, right: A): A
trait Mixed<V> extends Equal =
    function mix<T>(self, left: V, right: T): T
record Box<X> = value: X
implement<T> Mixed<T> for Box<T> =
    function mix<U>(self: Box<T>, left: U, right: U): U = right
    function mix<U>(self: Box<T>, left: T, right: U): U = right
function equal<R>(reader: R): String where R: Equal = reader.mix("left", "right")
function mixed<R>(reader: R): String where R: Mixed<Int32> = reader.mix(42, "right")
function main(): Unit =
    let reader = Box { value = 1 }
    assert equal(reader) == "right"
    assert mixed(reader) == "right"
"#,
    )
    .expect("method substitution preserves names inserted by enclosing substitution");
}
