mod common;

#[test]
fn named_arguments_only_consider_the_concrete_receiver_application() {
    common::compile_and_run(
        r#"
package a
trait Adder =
    function add(self, amount: Int32): Int32
record Box<T> = value: T
implement Adder for Box<Int32> =
    function add(self, increment: Int32): Int32 = self.value + increment
implement Adder for Box<String> =
    function add(self, count: Int32): Int32 = count
function main(): Unit =
    let number = Box { value = 1 }
    let text = Box { value = "text" }
    assert number.add(increment = 4) == 5
    assert text.add(count = 3) == 3
"#,
    )
    .expect("unrelated receiver applications do not contribute argument names");
}

#[test]
fn named_static_bound_calls_require_the_selected_contracts_labels() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait NumberReader =
    function read(number: Int32): Int32
trait TextReader =
    function read(text: String): Int32
record Worker = number: Int32
implement NumberReader for Worker =
    function read(value: Int32): Int32 = value
implement TextReader for Worker =
    function read(value: String): Int32 = 2
function throughTrait<T>(): Int32 where T: NumberReader + TextReader = T.read(text = 4)
function main(): Unit =
    assert throughTrait<Worker>() == 4
"#,
    );
    assert!(
        !errors.is_empty(),
        "a String contract label must not select an Int32 contract"
    );
}

#[test]
fn named_extension_calls_only_consider_applicable_receiver_types() {
    common::compile_and_run(
        r#"
package a
import a.NumberHelpers
import a.TextHelpers
record Box<T> = value: T
extension NumberHelpers for Box<Int32> =
    function add(self, increment: Int32): Int32 = self.value + increment
extension TextHelpers for Box<String> =
    function add(self, count: Int32): Int32 = count
function main(): Unit =
    assert (Box { value = 1 }).add(increment = 4) == 5
    assert (Box { value = "text" }).add(count = 3) == 3
"#,
    )
    .expect("unrelated extension receiver applications do not contribute argument names");
}

#[test]
fn named_contracts_compare_instantiated_parameter_types() {
    common::compile_and_run(
        r#"
package a
trait NumberReader<T> =
    function read(self, number: T): Int32
trait TextReader<T> =
    function read(self, text: T): Int32
record Worker = number: Int32
implement NumberReader<Int32> for Worker =
    function read(self, value: Int32): Int32 = value
implement TextReader<String> for Worker =
    function read(self, value: String): Int32 = 2
function throughTrait<T>(worker: T): Int32 where T: NumberReader<Int32> + TextReader<String> =
    worker.read(number = 4) + worker.read(text = "text")
function main(): Unit =
    assert throughTrait(Worker { number = 1 }) == 6
"#,
    )
    .expect("instantiated contract parameter types distinguish overloads");
}

#[test]
fn explicit_generic_method_arguments_override_inference() {
    for call in [
        "worker.pick<String>(3)",
        "worker.pick<String>(item = 3)",
        "worker.choose<String>(3)",
        "worker.choose<String>(item = 3)",
        "worker.select<String>(3)",
        "worker.select<String>(item = 3)",
    ] {
        let source = format!(
            r#"
package a
import a.Helpers
trait Picker =
    function pick<T>(self, item: T): T
record Worker = number: Int32
implement Picker for Worker =
    function pick<T>(self, item: T): T = item
extension Helpers for Worker =
    function choose<T>(self, item: T): T = item
module Worker =
    function select<T>(self, item: T): T = item
function main(): Unit =
    let worker = Worker {{ number = 1 }}
    {call}
    ()
"#
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            !errors.is_empty(),
            "explicit String must reject Int32: {call}"
        );
    }
}
