mod common;

#[test]
fn concrete_extension_preserves_outer_shape() {
    common::compile_and_run(
        r#"
package a
type Pair = (Int32, Bool)
newtype Wrapped = (Int32, Bool)
function main(): Unit =
    let pair: Int32 ~ Bool = 1 ~ true
    let triple: Pair ~ String = pair ~ "hello"
    assert triple._0 == 1
    assert triple._1
    assert triple._2 == "hello"
    let nested = ((1, 2), true) ~ ("a", "b")
    assert nested._0._1 == 2
    assert nested._2._1 == "b"
    let wrapped = Wrapped((3, true)) ~ 4
    assert wrapped._0.value._0 == 3
    assert wrapped._1 == 4
    let chain = 1 ~ 2 ~ 3 ~ 4
    assert chain._3 == 4
"#,
    )
    .expect("concrete extension");
}

#[test]
fn generic_extension_and_forwarding() {
    common::compile_and_run(
        r#"
package a
function append<T, U>(left: T, right: U): T ~ U = left ~ right
function forward<T, U>(left: T, right: U): T ~ U = append(left, right)
function main(): Unit =
    let pair = forward(1, true)
    let triple = forward(pair, "hello")
    assert triple._0 == 1
    assert triple._1
    assert triple._2 == "hello"
"#,
    )
    .expect("generic extension and forwarding");
}

#[test]
fn generic_extension_inside_returned_closure() {
    common::compile_and_run(
        r#"
package a
function appender<T, U>(left: T): U => (T ~ U) = right => left ~ right
function main(): Unit =
    let pairFn = appender<Int32, Bool>(1)
    let pair = pairFn(true)
    let tripleFn = appender<(Int32, Bool), String>(pair)
    let result = Some(tripleFn("hello")).require
    assert result._0 == 1
    assert result._1
    assert result._2 == "hello"
"#,
    )
    .expect("extension in generic returned closure and Option");
}

#[test]
fn extension_evaluates_each_operand_once_in_order() {
    common::compile_and_run(
        r#"
package a
function append<T, U>(left: T, right: U): T ~ U = left ~ right
function main(): Unit =
    let mutable calls = 0
    let left: () => (Int32, Bool) = () =>
        calls = calls + 1
        assert calls == 1
        (10, true)
    let right: () => String = () =>
        calls = calls + 1
        assert calls == 2
        "right"
    let result = left() ~ right()
    assert calls == 2
    assert result._0 == 10 && result._1 && result._2 == "right"
    calls = 0
    let forwarded = append(left(), right())
    assert calls == 2 && forwarded._2 == "right"
"#,
    )
    .expect("extension evaluation order");
}

#[test]
fn extension_precedence_and_function_types() {
    common::compile_and_run(
        r#"
package a
function main(): Unit =
    let bits = ~1 ~ 2
    assert bits._0 == -2 && bits._1 == 2
    let comparison = 1 ~ 2 == 2
    assert comparison._0 == 1 && comparison._1
    let arrow: Int32 ~ Bool => Int32 = pair => pair._0
    assert arrow((3, true)) == 3
    let single: ((Int32, Bool)) => Bool = pair => pair._1
    let multiple: (Int32, Bool) => Bool = (a, b) => b
    assert single((4, true)) && multiple(4, true)
    let appendedFn: Int32 ~ (Bool => Bool) = 1 ~ ((x: Bool) => x)
    assert appendedFn._1(true)
    let typed: Option<(Int32 ~ Bool) ~ String> = Some(1 ~ true ~ "x")
    assert typed.require._2 == "x"
    match typed.require with
        case value: (Int32 ~ Bool) ~ String => assert value._0 == 1
"#,
    )
    .expect("extension and function type precedence");
}

#[test]
fn generic_module_and_concat_output_normalize() {
    common::compile_and_run(
        r#"
package a
newtype Accumulator<T> = T
module Accumulator<T> =
    function append<U>(self, other: Accumulator<U>): Accumulator<T ~ U> =
        Accumulator(self.value ~ other.value)
implement <T, U> Concat<Accumulator<U>> for Accumulator<T> =
    type Output = Accumulator<T ~ U>
    function concat(self, other: Accumulator<U>): Accumulator<T ~ U> = self.append(other)
function join<L, R, O>(left: L, right: R): O where L: Concat<R, Output = O> = left ++ right
function main(): Unit =
    let result = join(Accumulator(1), Accumulator(true)) ++ Accumulator("x")
    assert result.value._0 == 1
    assert result.value._1
    assert result.value._2 == "x"
"#,
    )
    .expect("generic module and associated output");
}

#[test]
fn symbolic_extension_implementation_head_is_rejected() {
    let checked = dovetail::check(
        r#"
package a
trait Marker =
    function mark(self: Self): Bool
implement <T, U> Marker for T ~ U =
    function mark(self: T ~ U): Bool = true
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|d| d
            .message
            .contains("symbolic tuple extension implementation heads")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn generic_extension_typechecks_without_specialization() {
    common::check_no_errors(
        r#"
package a
function append<T, U>(left: T, right: U): T ~ U = left ~ right
function forward<A, B>(left: A, right: B): A ~ B = append(left, right)
function deferred<A, B>(left: A): B => (A ~ B) = right => forward(left, right)
"#,
    );
}

#[test]
fn reverse_extension_inference_requires_explicit_arguments() {
    let checked = dovetail::check(
        r#"
package a
function consume<T, U>(value: T ~ U): Unit = ()
function main(): Unit = consume((1, true))
"#,
        "test.dove",
    );
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("provide explicit type arguments")),
        "{:?}",
        checked.diagnostics
    );
    common::compile_and_run(
        r#"
package a
function consume<T, U>(value: T ~ U): Unit = ()
function main(): Unit = consume<Int32, Bool>((1, true))
"#,
    )
    .expect("explicit extension arguments");
}

#[test]
fn expected_result_does_not_change_the_left_operand_shape() {
    common::compile_and_run(
        r#"
package a
function main(): Unit =
    let result: (Int32, Bool, String) = (1, true) ~ "x"
    assert result._2 == "x"
    let scalar: (Int32, Bool) = 1 ~ true
    assert scalar._0 == 1
"#,
    )
    .expect("expected extension result does not leak into operands");
}

#[test]
fn extension_heads_nested_in_types_or_trait_arguments_are_rejected() {
    for source in [
        r#"
package a
trait Marker =
    function mark(self: Self): Bool
newtype Holder<T> = T
implement <T, U> Marker for Holder<T ~ U> =
    function mark(self: Holder<T ~ U>): Bool = true
"#,
        r#"
package a
trait Marker<T> =
    function mark(self: Self): Bool
newtype Holder = Int32
implement <T, U> Marker<T ~ U> for Holder =
    function mark(self: Holder): Bool = true
"#,
    ] {
        let checked = dovetail::check(source, "test.dove");
        assert!(
            checked.diagnostics.iter().any(|d| d
                .message
                .contains("symbolic tuple extension implementation heads")),
            "{:?}",
            checked.diagnostics
        );
    }
}

#[test]
fn concrete_extension_implementation_head_is_an_ordinary_tuple() {
    common::compile_and_run(
        r#"
package a
trait Marker =
    function mark(self: Self): Int32
implement Marker for Int32 ~ Bool =
    function mark(self: Int32 ~ Bool): Int32 = self._0
function main(): Unit = assert (3 ~ true).mark() == 3
"#,
    )
    .expect("concrete extension implementation head");
}

#[test]
fn malformed_extension_does_not_introduce_empty_tuples() {
    for source in [
        r#"
package a
function main(): Unit =
    let value = 1 ~
"#,
        r#"
package a
type Invalid = () ~ Int32
"#,
        r#"
package a
type Invalid = Int32 ~ () => Bool
"#,
    ] {
        let checked = dovetail::check(source, "test.dove");
        assert!(checked.diagnostics.has_errors(), "{source}");
    }
}

#[test]
fn extension_in_generic_record_field_uses_erased_storage() {
    common::compile_and_run(
        r#"
package a
record Extended<T, U> = value: T ~ U
function make<T, U>(left: T, right: U): Extended<T, U> =
    Extended<T, U> {value = left ~ right}
function main(): Unit =
    let pair = make(1, true)
    let triple = make(pair.value, "x")
    assert triple.value._0 == 1
    assert triple.value._1
    assert triple.value._2 == "x"
"#,
    )
    .expect("extension in generic record field");
}
