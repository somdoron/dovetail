mod common;

#[test]
fn named_arguments_bind_and_evaluate_in_written_order() {
    common::compile_and_run(
        r#"
package a
function subtract(left: Int32, right: Int32): Int32 = left - right
function main(): Unit =
    assert subtract(right = 2, left = 9) == 7
    assert subtract(9, right = 2) == 7
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    assert subtract(right = next(), left = next()) == 1
    assert count == 2
"#,
    )
    .expect("named arguments preserve bindings and written evaluation order");
}

#[test]
fn named_arguments_support_generic_functions_and_overloads() {
    common::compile_and_run(
        r#"
package a
function choose<T>(first: T, second: T): T = first
function convert(number: Int32): Int32 = number + 1
function convert(text: String): Int32 = 8
function apply(value: Int32, transform: Int32 => Int32): Int32 = transform(value)
function main(): Unit =
    assert choose(second = "right", first = "left") == "left"
    assert convert(number = 4) == 5
    assert convert(text = "a") == 8
    assert apply(transform = x => x + 2, value = 3) == 5
"#,
    )
    .expect("named generic and overloaded calls");
}

#[test]
fn named_arguments_support_classes_and_methods() {
    common::compile_and_run(
        r#"
package a
class Counter(public value: Int32) =
    public function add(self: Counter, amount: Int32): Int32 = self.value + amount
record Point = x: Int32
module Point =
    function shift(self, distance: Int32): Int32 = self.x + distance
function main(): Unit =
    let counter = Counter(value = 3)
    assert counter.add(amount = 4) == 7
    assert (Point { x = 2 }).shift(distance = 5) == 7
"#,
    )
    .expect("named constructors and methods");
}

#[test]
fn invalid_named_bindings_are_diagnosed() {
    for (call, expected) in [
        ("subtract(left = 1, 2)", "positional arguments must precede"),
        ("subtract(1, left = 2)", "supplied more than once"),
        ("subtract(left = 1)", "missing argument"),
        ("subtract(wrong = 1, right = 2)", "unknown argument name"),
    ] {
        let source = format!(
            r#"
package a
function subtract(left: Int32, right: Int32): Int32 = left - right
function main(): Unit =
    {call}
"#
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors.iter().any(|error| error.contains(expected)),
            "{call}: {errors:?}"
        );
    }
}

#[test]
fn contracts_and_overrides_use_only_the_static_declarations_names() {
    common::compile_and_run(
        r#"
package a
trait Adder =
    function add(self, amount: Int32): Int32
record Value = number: Int32
implement Adder for Value =
    function add(self, increment: Int32): Int32 = self.number + increment
function throughTrait<T>(value: T): Int32 where T: Adder = value.add(amount = 4)
abstract class Base() =
    public abstract function add(self: Base, amount: Int32): Int32
class Concrete() extends Base() =
    public override function add(self: Concrete, increment: Int32): Int32 = increment + 1
function throughBase(value: Base): Int32 = value.add(amount = 4)
function main(): Unit =
    let value = Value { number = 3 }
    assert throughTrait(value = value) == 7
    assert value.add(increment = 4) == 7
    assert Adder.add(value, amount = 4) == 7
    assert throughBase(value = Concrete()) == 5
    assert Concrete().add(increment = 4) == 5
"#,
    )
    .expect("static declarations determine labels, independently of runtime implementation");
}

#[test]
fn named_arguments_preserve_byname_laziness() {
    common::compile_and_run(
        r#"
package a
function fail(): Int32 = panic "must remain lazy"
function choose(condition: Bool, fallback: ByName<Int32>): Int32 =
    if condition then 4 else fallback.get
function main(): Unit =
    assert choose(fallback = fail(), condition = true) == 4
"#,
    )
    .expect("named lazy arguments are not evaluated eagerly");
}

#[test]
fn named_super_constructor_arguments_preserve_source_order() {
    common::compile_and_run(
        r#"
package a
class Parent(public first: Int32, public second: Int32)
class Child(next: () => Int32) extends Parent(second = next(), first = next())
class GenericParent<T>(public first: T, public second: T)
class GenericChild<T>(value: T) extends GenericParent<T>(second = value, first = value)
function main(): Unit =
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    let child = Child(next = next)
    assert child.first == 2
    assert child.second == 1
    assert count == 2
    assert GenericChild(value = "value").first == "value"
"#,
    )
    .expect("super constructor arguments retain source order");
}

#[test]
fn function_values_and_wrong_contract_names_are_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a
function add(amount: Int32): Int32 = amount
function main(): Unit =
    let functionValue: Int32 => Int32 = add
    functionValue(amount = 2)
"#,
    );
    assert!(
        errors
            .iter()
            .any(|message| message.contains("function values")),
        "{errors:?}"
    );
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Adder =
    function add(self, amount: Int32): Int32
record Value = number: Int32
implement Adder for Value =
    function add(self, increment: Int32): Int32 = increment
function throughTrait<T>(value: T): Int32 where T: Adder = value.add(increment = 4)
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|message| message.contains("unknown argument name 'increment'")),
        "{errors:?}"
    );
}

#[test]
fn named_arguments_with_await_preserve_written_order() {
    common::compile_and_run_async(
        r#"
package a
function subtract(left: Int32, right: Int32): Int32 = left - right
async function calculate(next: () => Int32): Async<Int32, Never> =
    subtract(right = await Async.Succeed(next()), left = await Async.Succeed(next()))
function main(): Unit =
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    match calculate(next = next).evaluate() with
    case Async.Succeed(value) => assert value == 1 && count == 2
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("named arguments containing await execute in written order");
}

#[test]
fn named_arguments_in_initializers_and_generic_methods() {
    common::compile_and_run(
        r#"
package a
function subtract(left: Int32, right: Int32): Int32 = left - right
class Value(public number: Int32) =
    let difference: Int32 = subtract(right = 2, left = number)
    public function get(self: Value): Int32 = self.difference
record Holder<T> = value: T
module Holder<T> =
    function choose(self, replacement: T, useReplacement: Bool): T =
        if useReplacement then replacement else self.value
function main(): Unit =
    assert Value(number = 9).get() == 7
    let holder = Holder { value = "original" }
    assert holder.choose(useReplacement = true, replacement = "new") == "new"
"#,
    )
    .expect("named calls work in initializers and generic methods");
}

#[test]
fn named_arguments_support_generic_contracts_interfaces_and_extensions() {
    common::compile_and_run(
        r#"
package a
import a.NumberHelpers
trait Picker =
    function pick<T>(self, value: T): T
interface Adder =
    function add(self, amount: Int32): Int32
record Worker = number: Int32
implement Picker for Worker =
    function pick<T>(self, item: T): T = item
implement Adder for Worker =
    function add(self, increment: Int32): Int32 = self.number + increment
extension NumberHelpers for Int32 =
    function increase(self, amount: Int32): Int32 = self + amount
    function start(value: Int32): Int32 = value
function throughTrait<P>(picker: P): Int32 where P: Picker = picker.pick(value = 7)
function throughInterface(value: Adder): Int32 = value.add(amount = 3)
function main(): Unit =
    let worker = Worker { number = 2 }
    assert throughTrait(picker = worker) == 7
    assert Picker.pick(worker, value = 8) == 8
    assert throughInterface(value = worker) == 5
    assert worker.pick(item = 9) == 9
    assert 2.increase(amount = 3) == 5
    assert NumberHelpers.increase(2, amount = 3) == 5
    assert Int32.start(value = 4) == 4
"#,
    )
    .expect("named generic contracts, interfaces, and extensions");
}

#[test]
fn named_overloads_do_not_accept_another_declarations_labels() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Reader =
    function read(self, number: Int32): Int32
    function read(self, text: String): Int32
record Worker = number: Int32
implement Reader for Worker =
    function read(self, value: Int32): Int32 = value
    function read(self, value: String): Int32 = 2
function main(): Unit =
    Reader.read(Worker { number = 1 }, text = 4)
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "a label belonging to another overload must not be accepted"
    );
    common::compile_and_run(
        r#"
package a
function select(left: Int32, right: String): Int32 = left
function select(right: String, left: Bool): Int32 = if left then 8 else 0
function main(): Unit =
    assert select(right = "text", left = 7) == 7
    assert select(left = true, right = "text") == 8
"#,
    )
    .expect("overloads bind names independently before type matching");
}

#[test]
fn explicit_named_interface_calls_evaluate_receiver_once() {
    common::compile_and_run(
        r#"
package a
interface Adder =
    function add(self, amount: Int32): Int32
record Worker = number: Int32
implement Adder for Worker =
    function add(self, increment: Int32): Int32 = self.number + increment
class Counter(public number: Int32) implements Adder =
    public function add(self, amount: Int32): Int32 = self.number + amount
function main(): Unit =
    let mutable count = 0
    let make: () => Adder = () =>
        count = count + 1
        Worker { number = 2 }
    assert Adder.add(make(), amount = 3) == 5
    assert count == 1
    let makeConcrete: () => Counter = () =>
        count = count + 1
        Counter(2)
    assert Adder.add(makeConcrete(), amount = 3) == 5
    assert count == 2
"#,
    )
    .expect("explicit interface receiver executes once");
}
