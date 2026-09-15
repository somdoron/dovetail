mod common;

#[test]
fn trait_method_infers_operands_after_extension_argument() {
    common::compile_and_run(r#"
package a
trait Checker =
    function check<T, U>(self: Self, value: T ~ U, left: T, right: U): T ~ U
record Probe = id: Int32
implement Checker for Probe =
    function check<T, U>(self: Probe, value: T ~ U, left: T, right: U): T ~ U = value
function main(): Unit =
    let probe = Probe {id = 0}
    assert probe.check((1, true), 1, true)._1
    assert probe.check((1, true, "x"), (1, true), "x")._2 == "x"
    assert probe.check<Int32, Bool>((1, true), 1, true)._0 == 1
"#).expect("trait method infers extension operands from later arguments");
}

#[test]
fn static_extension_infers_independent_nested_operands() {
    common::compile_and_run(r#"
package a
import a.ArrayExtension
extension ArrayExtension<T> for Array<T> =
    function check<U>(values: (T ~ U, T, U)): T ~ U = values._0
function main(): Unit =
    let pair = Array.check(((1, true), 1, true))
    assert pair._0 == 1 && pair._1
    let triple = Array.check(((1, true, "x"), (1, true), "x"))
    assert triple._2 == "x"
"#).expect("static extension infers operands nested after an extension");
}

#[test]
fn free_function_infers_independent_nested_operands() {
    common::compile_and_run(r#"
package a
function check<T, U>(values: (T ~ U, T, U)): T ~ U = values._0
function main(): Unit =
    assert check(((1, true), 1, true))._1
    assert check(((1, true, "x"), (1, true), "x"))._2 == "x"
"#).expect("free function infers operands nested after an extension");
}

#[test]
fn deferred_extension_arguments_are_validated_after_inference() {
    for source in [r#"
package a
trait Checker =
    function check<T, U>(self: Self, value: T ~ U, left: T, right: U): Unit
record Probe = id: Int32
implement Checker for Probe =
    function check<T, U>(self: Probe, value: T ~ U, left: T, right: U): Unit = ()
function main(): Unit = Probe {id = 0}.check((1, "wrong"), 1, true)
"#, r#"
package a
import a.ArrayExtension
extension ArrayExtension<T> for Array<T> =
    function check<U>(values: (T ~ U, T, U)): Unit = ()
function main(): Unit = Array.check(((1, "wrong"), 1, true))
"#, r#"
package a
function check<T, U>(values: (T ~ U, T, U)): Unit = ()
function main(): Unit = check(((1, "wrong"), 1, true))
"#, r#"
package a
import a.ArrayExtension
extension ArrayExtension<T> for Array<T> =
    function check<U>(value: T ~ U): Unit = ()
function main(): Unit = Array.check((1, true))
"#] {
        let result = dovetail::check(source, "test.dove");
        assert!(result.diagnostics.has_errors(), "unexpectedly accepted {source}");
    }
}

#[test]
fn instance_extension_infers_operands_inside_generic_record() {
    common::compile_and_run(r#"
package a
import a.ArrayExtension
record Inputs<A, B> = result: A; operand: B
extension ArrayExtension<T> for Array<T> =
    function check<U>(self, values: Inputs<T ~ U, U>): T ~ U = values.result
function main(): Unit =
    let values = Inputs {result = (1, true); operand = true}
    assert [|1|].check(values)._1
"#).expect("instance extension infers operands nested in generic record arguments");
}

#[test]
fn known_left_shape_infers_right_operand_in_either_argument_order() {
    common::compile_and_run(r#"
package a
function first<T, U>(left: T, value: T ~ U): T ~ U = value
function last<T, U>(value: T ~ U, left: T): T ~ U = value
function main(): Unit =
    assert first(1, (1, true))._1
    assert last((1, true), 1)._1
    assert first((1, true), (1, true, "x"))._2 == "x"
    assert last((1, true, "x"), (1, true))._2 == "x"
"#).expect("known left shapes permit forward normalization and right inference");
}

#[test]
fn nested_extension_dependencies_are_revisited_until_bound() {
    common::compile_and_run(r#"
package a
import a.ArrayExtension
extension ArrayExtension<T> for Array<T> =
    function check<U, V>(values: (U ~ V, T ~ U, T)): U ~ V = values._0
function main(): Unit =
    let pair = Array.check(((true, "x"), (1, true), 1))
    assert pair._0 && pair._1 == "x"
"#).expect("nested dependency resolves T, then U, then V without inverse inference");
}
