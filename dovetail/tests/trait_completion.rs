mod common;

#[test]
fn abstract_resource_use_preserves_its_wrapped_continuation() {
    common::compile_and_run(
        r#"
package a
newtype Holder = Int32
implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self, f: (Int32) => U, errorF: (Never) => E2): U = f(self.value)
function scoped<R>(resource: R, finish: (Int32) => R.Wrapped<Int32, Never>): R.Wrapped<Int32, Never>
    where R: Usable<Int32, Never> =
    let value = use resource
    finish(value)
function main(): Unit = assert scoped(Holder(40), value => value + 2) == 42
"#,
    )
    .expect("generic use carries the resource's associated wrapper");
}

#[test]
fn inherited_default_overloads_work_through_parent_bounds() {
    common::compile_and_run(
        r#"
package a
trait TextValue =
    function value(self, input: String): Int32 = 10
trait Value extends TextValue =
    function value(self, input: Int32): Int32 = input + self.value("text")
record Reader = id: Int32
implement Value for Reader
function readText<R>(reader: R): Int32 where R: TextValue = reader.value("text")
function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.value(20) == 30
    assert readText(reader) == 10
"#,
    )
    .expect("overloaded defaults retain their selected declaration when routed");
}

#[test]
fn generic_default_method_checks_its_bound() {
    common::compile_and_run(
        r#"
package a
trait Comparison =
    function same<T>(self, left: T, right: T): Bool where T: Equatable = left == right
record Reader = id: Int32
implement Comparison for Reader
function compare<C, T>(comparison: C, left: T, right: T): Bool
    where C: Comparison, T: Equatable = comparison.same(left, right)
function main(): Unit =
    assert compare(Reader { id = 0 }, "text", "text")
"#,
    )
    .expect("default bodies can use the method's declared evidence");

    let errors = common::compile_expecting_errors(
        r#"
package a
trait Comparison =
    function same<T>(self, left: T, right: T): Bool where T: Equatable = left == right
record Reader = id: Int32
implement Comparison for Reader
record Uncomparable = id: Int32
function compare<C>(comparison: C, left: Uncomparable): Bool where C: Comparison =
    comparison.same(left, left)
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|error| error.contains("Equatable")),
        "{errors:?}"
    );
}

#[test]
fn associated_outputs_work_in_default_bodies_and_properties() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
    property output(self): Output
    function produce(self): Output = self.output
record NumberProducer = value: Int32
implement Producer for NumberProducer =
    type Output = Int32
    property output(self): Int32 = self.value
function read<P>(producer: P): P.Output where P: Producer = producer.produce()
function main(): Unit = assert read(NumberProducer { value = 42 }) == 42
"#,
    )
    .expect("default signatures and bodies share associated-output identity");
}

#[test]
fn inherited_interface_overloads_preserve_super_dispatch() {
    common::compile_and_run(
        r#"
package a

interface TextValue =
    function value(self, input: String): Int32

interface Value extends TextValue =
    function value(self, input: Int32): Int32

record Reader = id: Int32
implement Value for Reader =
    function value(self, input: String): Int32 = 10
    function value(self, input: Int32): Int32 = input + 1

function main(): Unit =
    let reader: Value = Reader { id = 0 }
    let text: TextValue = reader
    assert reader.value(20) == 21
    assert reader.value("text") == 10
    assert text.value("text") == 10
"#,
    )
    .expect("interface overload slots retain their declaring member");
}

#[test]
fn generic_class_can_inherit_a_generic_method_default() {
    common::compile_and_run(
        r#"
package a

trait Picker =
    function pick<T>(self, value: T): T = value

class Reader<T>(id: T) implements Picker =
    public function get(self: Reader<T>): T = self.id

function choose<P, T>(picker: P, value: T): T where P: Picker = picker.pick(value)

function main(): Unit =
    assert choose(Reader(1), "bound") == "bound"
    let reader = Reader(1)
    assert reader.pick("text") == "text"
    assert reader.pick(42) == 42
"#,
    )
    .expect("class and method parameters remain independent");
}

#[test]
fn associated_output_does_not_gain_unproven_capabilities() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Producer =
    type Output
    function produce(self): Output
function invalid<P>(producer: P): Int32 where P: Producer =
    producer.produce()
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|error| error.contains("type mismatch")),
        "{errors:?}"
    );
}

#[test]
fn inherited_overloads_dispatch_by_argument_type() {
    common::compile_and_run(
        r#"
package a

trait TextValue =
    function value(self, input: String): Int32

trait Value extends TextValue =
    function value(self, input: Int32): Int32

record Reader = id: Int32

implement Value for Reader =
    function value(self, input: String): Int32 = 10
    function value(self, input: Int32): Int32 = input + 1

function read<R>(reader: R): Int32 where R: Value =
    reader.value("text") + reader.value(20)

function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.value("text") == 10
    assert reader.value(20) == 21
    assert read(reader) == 31
    assert Value.value(reader, 20) == 21
    assert TextValue.value(reader, "text") == 10
"#,
    )
    .expect("inherited overloads select distinct implementations");
}

#[test]
fn generic_method_default_specializes_for_each_call() {
    common::compile_and_run(
        r#"
package a

trait Picker =
    function pick<T>(self, value: T): T = value

record Reader = id: Int32

implement Picker for Reader

function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.pick(42) == 42
    assert reader.pick("text") == "text"
"#,
    )
    .expect("generic method defaults specialize independently");
}

#[test]
fn generic_default_works_through_a_bound_and_generic_implementor() {
    common::compile_and_run(
        r#"
package a

trait Picker =
    function pick<T>(self, value: T): T = value

record Reader<R> = id: R
implement <R> Picker for Reader<R>

function select<P, T>(picker: P, value: T): T where P: Picker =
    picker.pick(value)

function main(): Unit =
    let reader = Reader { id = 1 }
    assert select(reader, "text") == "text"
    assert select(reader, 42) == 42
"#,
    )
    .expect("method arguments survive generic bound dispatch");
}

#[test]
fn generic_class_default_calls_virtual_override() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function value(self): Int32
    function greet(self): Int32 = self.value() + 1

class Box<T>(item: T) implements Greeter =
    public function value(self: Box<T>): Int32 = 1

class Child extends Box<Int32>(42) =
    public override function value(self: Child): Int32 = 10

function greet<G>(greeter: G): Int32 where G: Greeter = greeter.greet()
function value<G>(greeter: G): Int32 where G: Greeter = greeter.value()
function main(): Unit =
    let child: Box<Int32> = Child()
    assert greet(child) == 11
    assert value(child) == 10
    assert child.greet() == 11
"#,
    )
    .expect("generic default bodies preserve virtual dispatch");
}

#[test]
fn generic_class_inherits_trait_default() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self): Int32 = 1

class Box<T>(value: T) implements Greeter =
    public function get(self: Box<T>): T = self.value

function main(): Unit =
    assert Box(42).greet() == 1
    assert Box("text").greet() == 1
"#,
    )
    .expect("generic classes inherit default members");
}

#[test]
fn ordinary_associated_output_flows_through_generic_helper() {
    common::compile_and_run(
        r#"
package a

trait Producer =
    type Output
    function produce(self): Output

record NumberProducer = value: Int32

implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value

function read<P>(producer: P): P.Output where P: Producer =
    producer.produce()

function forward<P>(producer: P): P.Output where P: Producer =
    read(producer)

function main(): Unit =
    assert forward(NumberProducer { value = 42 }) == 42
"#,
    )
    .expect("ordinary associated outputs retain their receiver identity");
}

#[test]
fn generic_associated_output_selects_the_implementors_wrapper() {
    common::compile_and_run(
        r#"
package a

trait Wrapper =
    type Wrapped<T>
    function wrap<T>(self, value: T): Wrapped<T>

record OptionalWrapper = name: String
record ArrayWrapper = name: String

implement Wrapper for OptionalWrapper =
    type Wrapped<T> = Option<T>
    function wrap<T>(self, value: T): Option<T> = Some(value)

implement Wrapper for ArrayWrapper =
    type Wrapped<T> = Array<T>
    function wrap<T>(self, value: T): Array<T> = [|value|]

function wrapValue<W, T>(wrapper: W, value: T): W.Wrapped<T>
    where W: Wrapper =
    wrapper.wrap(value)

function main(): Unit =
    let optional = OptionalWrapper { name = "optional" }
    let array = ArrayWrapper { name = "array" }
    let numberOption: Option<Int32> = wrapValue(optional, 42)
    let numberArray: Array<Int32> = wrapValue(array, 42)
    let textOption: Option<String> = wrapValue(optional, "hello")
    let textArray: Array<String> = wrapValue(array, "hello")
    assert numberOption.require == 42
    assert numberArray == [|42|]
    assert textOption.require == "hello"
    assert textArray == [|"hello"|]
"#,
    )
    .expect("generic associated outputs normalize per implementation");
}

#[test]
fn class_interface_overloads_select_full_signatures() {
    common::compile_and_run(
        r#"
package a
interface TextValue =
    function value(self, input: String): Int32
interface Value extends TextValue =
    function value(self, input: Int32): Int32
class Reader implements Value =
    public function value(self: Reader, input: String): Int32 = 10
    public function value(self: Reader, input: Int32): Int32 = input + 1
function main(): Unit =
    let reader: Value = Reader()
    let text: TextValue = reader
    assert reader.value(20) == 21
    assert reader.value("text") == 10
    assert text.value("text") == 10
"#,
    )
    .expect("class interface slots select signatures, including same-arity overloads");
}

#[test]
fn class_inherits_each_overloaded_default() {
    common::compile_and_run(
        r#"
package a
trait TextValue =
    function value(self, input: String): Int32 = 10
trait Value extends TextValue =
    function value(self, input: Int32): Int32 = input + self.value("text")
class Reader<T>(id: T) implements Value =
    public function get(self: Reader<T>): T = self.id
class Plain implements Value =
    public function get(self: Plain): Int32 = 0
function main(): Unit =
    assert Reader(true).value(20) == 30
    assert Reader(true).value("text") == 10
    assert Plain().value(20) == 30
    assert Plain().value("text") == 10
"#,
    )
    .expect("each class default retains its overload identity");
}

#[test]
fn explicit_and_inherited_class_methods_take_precedence_over_defaults() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: T): T = value
    function number(self): Int32 = 1
class Parent =
    public function number(self: Parent): Int32 = 20
class Reader<R>(id: R) extends Parent() implements Picker =
    public function pick<U>(self: Reader<R>, value: U): U = value
function main(): Unit =
    assert Reader(true).number() == 20
    assert Reader(true).pick("text") == "text"
"#,
    )
    .expect("explicit generic signatures match by renaming; inherited methods win");
}

#[test]
fn equality_bounds_and_nested_noninjective_projections_normalize() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
    function produce(self): Output
record NumberProducer = id: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.id
function read<P>(producer: P): P.Output where P: Producer = producer.produce()
function readNumber<P>(producer: P): Int32 where P: Producer<Output = Int32> = read(producer)
trait Wrapper =
    type Wrapped<T>
    function wrap<T>(self, value: T): Wrapped<T>
record Constant = id: Int32
implement Wrapper for Constant =
    type Wrapped<T> = Int32
    function wrap<T>(self, value: T): Int32 = self.id
function nested<W, T>(wrapper: W, value: T): W.Wrapped<W.Wrapped<T>> where W: Wrapper =
    wrapper.wrap(wrapper.wrap(value))
function main(): Unit =
    assert readNumber(NumberProducer { id = 42 }) == 42
    assert nested(Constant { id = 20 }, "text") == 20
"#,
    )
    .expect("equalities and nested non-injective wrappers preserve their contracts");
}

#[test]
fn invalid_projection_references_and_unwrapped_resource_results_are_rejected() {
    for (index, source) in [
        r#"
package a
trait Wrapper =
    type Wrapped<T>
function invalid<W>(wrapper: W): W.Wrapped where W: Wrapper = panic("unused")
function main(): Unit = ()
"#,
        r#"
package a
trait Left =
    type Output
trait Right =
    type Output
function invalid<P>(producer: P): P.Output where P: Left + Right = panic("unused")
function main(): Unit = ()
"#,
        r#"
package a
function invalid<R>(resource: R): Int32 where R: Usable<Int32, Never> =
    let value = use resource
    value + 1
function main(): Unit = ()
"#,
    ]
    .into_iter()
    .enumerate()
    {
        let errors = common::compile_expecting_errors(source);
        let expected = ["type parameter count", "ambiguous", "generic use requires"][index];
        assert!(
            errors.iter().any(|error| error.contains(expected)),
            "{errors:?}"
        );
    }
}

#[test]
fn projected_aliases_and_generic_class_property_defaults() {
    common::compile_and_run(
        r#"
package a
trait Producer =
    type Output
    function produce(self): Output
record NumberProducer = value: Int32
implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value
type Output<P> where P: Producer = P.Output
function read<P>(producer: P): Output<P> where P: Producer = producer.produce()
trait Sized =
    property size(self): Int32
    property doubled(self): Int32 = self.size * 2
class Box<T>(item: T) implements Sized =
    public property size(self: Box<T>): Int32 = 3
function main(): Unit =
    assert read(NumberProducer { value = 42 }) == 42
    assert Box("text").doubled == 6
"#,
    )
    .expect("projection substitution reaches aliases and generic property defaults specialize");
}

#[test]
fn class_cannot_use_one_overloads_default_for_another() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait TextValue =
    function value(self, input: String): Int32 = 10
trait Value extends TextValue =
    function value(self, input: Int32): Int32
class Reader implements Value =
    public function get(self: Reader): Int32 = 0
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("must implement method")),
        "{errors:?}"
    );
}

#[test]
fn overloaded_generic_methods_keep_their_own_bounds() {
    common::compile_and_run(r#"
package a
trait Plain =
    function same<T>(self, marker: String, left: T, right: T): Bool = true
trait Comparison extends Plain =
    function same<T>(self, marker: Int32, left: T, right: T): Bool where T: Equatable = left == right
record Reader = id: Int32
implement Comparison for Reader =
    function same<U>(self, marker: Int32, left: U, right: U): Bool where U: Equatable = left == right
function compare<C, T>(comparison: C, left: T, right: T): Bool
    where C: Comparison, T: Equatable = comparison.same(0, left, right)
function main(): Unit =
    let reader = Reader { id = 0 }
    assert compare(reader, "text", "text")
    assert reader.same("marker", true, false)
"#).expect("overload selection preserves the selected method's contract");
}

#[test]
fn renamed_generic_defaults_still_conflict() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Left =
    function pick<T>(self, value: T): T = value
trait Right =
    function pick<U>(self, value: U): U = value
class Reader<R>(id: R) implements Left and Right =
    public function get(self: Reader<R>): R = self.id
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|error| error.contains("conflict")),
        "{errors:?}"
    );
}

#[test]
fn abstract_use_in_an_operand_cannot_escape_wrapper_validation() {
    let errors = common::compile_expecting_errors(
        r#"
package a
function invalid<R>(resource: R): Int32 where R: Usable<Int32, Never> = (use resource) + 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("generic use requires")),
        "{errors:?}"
    );
}
