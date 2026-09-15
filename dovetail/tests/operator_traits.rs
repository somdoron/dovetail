mod common;

fn rejects(source: &str, message: &str) {
    let result = dovetail::check(source, "test.dove");
    let messages: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(
        messages.iter().any(|m| m.contains(message)),
        "expected {message}: {messages:?}"
    );
}

#[test]
fn generic_operators_infer_associated_outputs() {
    common::compile_and_run(
        r#"
package a
newtype Money = Int32
implement Add<Money> for Money =
    type Output = Money
    function add(self: Money, rhs: Money): Money = Money(self.value + rhs.value)
implement Sub<Money> for Money =
    type Output = Int32
    function sub(self: Money, rhs: Money): Int32 = self.value - rhs.value
implement Mul<Int32> for Money =
    type Output = Money
    function mul(self: Money, rhs: Int32): Money = Money(self.value * rhs)
function add<L, R, O>(a: L, b: R): O where L: Add<R, Output = O> = a + b
function subtract<L, R, O>(a: L, b: R): O where L: Sub<R, Output = O> = a - b
function multiply<L, R, O>(a: L, b: R): O where L: Mul<R, Output = O> = a * b
function divide<L, R, O>(a: L, b: R): O where L: Div<R, Output = O> = a / b
function join<L, R, O>(a: L, b: R): O where L: Concat<R, Output = O> = a ++ b
function main(): Unit =
    assert add(Money(2), Money(3)).value == 5
    assert subtract(Money(7), Money(3)) == 4
    assert multiply(Money(4), 3).value == 12
    assert add(2, 3) == 5
    assert divide(8, 2) == 4
    assert join("a", "b") == "ab"
    assert "a".concat("b") == "ab"
    assert 2.add(3) == 5
"#,
    )
    .unwrap();
}

#[test]
fn output_constraints_work_for_ordinary_methods_and_forwarded_bounds() {
    common::compile_and_run(
        r#"
package a
record Value = n: Int32
trait Produce =
    type Output
    function produce(self: Self): Option<Output>
implement Produce for Value =
    type Output = Int32
    function produce(self: Value): Option<Int32> = Some(self.n)
function get<T, O>(value: T): Option<O> where T: Produce<Output = O> = value.produce()
function forward<T, O>(value: T): Option<O> where T: Produce<Output = O> = get(value)
function main(): Unit = assert forward(Value { n = 9 }).require == 9
"#,
    )
    .unwrap();
}

#[test]
fn wrong_rhs_and_output_constraints_are_rejected() {
    rejects(
        r#"
package a
newtype N = Int32
implement Div<Int32> for N =
    type Output = N
    function div(self: N, rhs: Int32): N = N(self.value / rhs)
function main(): Unit =
    N(6) / "oops"
    ()
"#,
        "requires operands of the same type",
    );
    rejects(
        r#"
package a
function add<T>(a: T, b: T): String where T: Add<T, Output = String> = a + b
function main(): Unit =
    add(1, 2)
    ()
"#,
        "does not implement trait",
    );
    rejects(
        r#"
package a
function add<T>(a: T, b: T): T where T: Add<T> = a + b
function main(): Unit = ()
"#,
        "associated type constraint",
    );
}

#[test]
fn generic_division_preserves_rhs_selection() {
    common::compile_and_run(
        r#"
package a
newtype N = Int32
implement Div<N> for N =
    type Output = N
    function div(self: N, rhs: N): N = N(99)
implement Div<Int32> for N =
    type Output = N
    function div(self: N, rhs: Int32): N = N(self.value / rhs)
function half<T>(value: T): T where T: Div<Int32, Output = T> = value / 2
function main(): Unit = assert half(N(6)).value == 3
"#,
    )
    .unwrap();
}

#[test]
fn newtypes_have_no_implicit_equality() {
    rejects(
        r#"
package a
newtype N = Int32
function main(): Unit = assert N(1) == N(1)
"#,
        "consider implementing 'Equatable'",
    );
    rejects(
        r#"
package a
newtype N<T> = T
function main(): Unit = assert N(1) != N(2)
"#,
        "consider implementing 'Equatable'",
    );
}

#[test]
fn string_plus_is_rejected_and_interpolation_uses_concat() {
    rejects(
        r#"
package a
function main(): Unit =
    "a" + "b"
    ()
"#,
        "String concatenation uses '++'",
    );
    common::compile_and_run(
        r#"
package a
function main(): Unit =
    let n = 3
    assert "hello $n!" == "hello 3!"
    assert "${n + 1}" == "4"
"#,
    )
    .unwrap();
}

#[test]
fn generic_newtypes_and_global_initializers_use_declared_output() {
    common::compile_and_run(
        r#"
package a
newtype Wrapped<T> = T
implement <T> Add<Wrapped<T>> for Wrapped<T> where T: Add<T, Output = T> =
    type Output = Wrapped<T>
    function add(self: Wrapped<T>, rhs: Wrapped<T>): Wrapped<T> = Wrapped(self.value + rhs.value)
record Part = size: Int32
implement Concat<Part> for Part =
    type Output = Int32
    function concat(self: Part, rhs: Part): Int32 = self.size + rhs.size
let left: Part = Part { size = 2 }
let right: Part = Part { size = 3 }
let total = left ++ right
function main(): Unit =
    assert total == 5
    assert (Wrapped(2) + Wrapped(3)).value == 5
    let add: (Int32, Int32) => Int32 = Int32.add
    assert add(4, 5) == 9
"#,
    )
    .unwrap();
}

#[test]
fn invalid_associated_bindings_are_diagnosed() {
    rejects(
        r#"
package a
function f<T>(x: T): Unit where T: Add<T, Missing = T> = ()
function main(): Unit = ()
"#,
        "unknown associated type 'Missing'",
    );
    rejects(
        r#"
package a
function f<T>(x: T): Unit where T: Add<T, Output = T, Output = String> = ()
function main(): Unit = ()
"#,
        "duplicate associated type binding",
    );
    rejects(
        r#"
package a
trait Rebind =
    type Item<T>
function f<T>(x: T): Unit where T: Rebind<Item = Int32> = ()
function main(): Unit = ()
"#,
        "cannot bind a generic associated type",
    );
}

#[test]
fn conflicting_output_constraints_are_rejected_without_a_call() {
    rejects(
        r#"
package a
function f<T>(x: T): Unit where T: Add<Int32, Output = Int32> + Add<Int32, Output = String> = ()
function main(): Unit = ()
"#,
        "conflicting associated type binding 'Output'",
    );
}

#[test]
fn operator_resolution_ignores_unrelated_same_named_trait_methods() {
    common::compile_and_run(
        r#"
package a
record Item = n: Int32
newtype Wrapped<T> = T
trait Marker =
    function mark(self: Self): Int32
trait Other<R> =
    function add(self: Self, rhs: R): Int32
implement <T> Other<Int32> for Wrapped<T> where T: Marker =
    function add(self: Wrapped<T>, rhs: Int32): Int32 = self.value.mark() + rhs
implement <T> Add<Int32> for Wrapped<T> =
    type Output = Int32
    function add(self: Wrapped<T>, rhs: Int32): Int32 = rhs
function main(): Unit = assert Wrapped(Item { n = 1 }) + 2 == 2
"#,
    )
    .unwrap();
}

#[test]
fn operator_bounds_require_the_rhs_type() {
    rejects(
        r#"
package a
function f<T>(x: T): T where T: Add<Output = T> = x + x
function main(): Unit = ()
"#,
        "expects 1 type argument(s)",
    );
}

#[test]
fn associated_output_inference_is_independent_of_bound_order() {
    common::compile_and_run(
        r#"
package a
record Value = n: Int32
trait First =
    type Output
trait Second<R> =
    type Output
    function second(self: Self): Output
implement First for Value =
    type Output = Int32
implement Second<Int32> for Value =
    type Output = Int32
    function second(self: Value): Int32 = self.n
implement Second<String> for Value =
    type Output = String
    function second(self: Value): String = "other"
function chained<T, A, B>(x: T): B where T: Second<A, Output = B> + First<Output = A> = x.second()
function main(): Unit =
    let value = chained(Value { n = 2 })
    assert value == 2
"#,
    )
    .unwrap();
}

#[test]
fn generic_operator_interface_rhs_preserves_parameter_identity() {
    common::compile_and_run(r#"
package a
interface Value<R> =
    function get(self: Self): R
newtype N = Int32
implement Value<Int32> for N =
    function get(self: N): Int32 = self.value
implement <R> Add<Value<R>> for N =
    type Output = R
    function add(self: N, rhs: Value<R>): R = rhs.get()
function add<T, R>(left: T, right: Value<R>): R where T: Add<Value<R>, Output = R>, R: Equatable = left + right
function main(): Unit =
    let value: Value<Int32> = N(3)
    assert add(N(2), value) == 3
"#).unwrap();
}

#[test]
fn generic_implementation_outputs_follow_nested_associated_bounds() {
    common::compile_and_run(
        r#"
package a
newtype Wrapped<T> = T
implement <T, O> Add<Wrapped<T>> for Wrapped<T> where T: Add<T, Output = O> =
    type Output = O
    function add(self: Wrapped<T>, rhs: Wrapped<T>): O = self.value + rhs.value
function add<L, R, O>(left: L, right: R): O where L: Add<R, Output = O> = left + right
function main(): Unit = assert add(Wrapped(2), Wrapped(3)) == 5
"#,
    )
    .unwrap();
}

#[test]
fn cyclic_associated_implementation_bounds_are_rejected() {
    rejects(
        r#"
package a
newtype N = Int32
trait Loop<R> =
    type Output
trait Other<R> =
    type Output
implement <T> Loop<T> for N where T: Loop<N, Output = Int32> + Other<N, Output = Int32> =
    type Output = Int32
implement <T> Other<T> for N where T: Loop<N, Output = Int32> + Other<N, Output = Int32> =
    type Output = Int32
function f<T>(value: T): Unit where T: Loop<N, Output = Int32> = ()
function main(): Unit = f(N(1))
"#,
        "does not implement trait",
    );
}

#[test]
fn cyclic_unresolved_associated_outputs_are_rejected() {
    rejects(
        r#"
package a
newtype N = Int32
trait Loop<R> =
    type Output
trait Other<R> =
    type Output
implement <T, O> Loop<T> for N where T: Loop<N, Output = O> + Other<N, Output = O> =
    type Output = O
implement <T, O> Other<T> for N where T: Loop<N, Output = O> + Other<N, Output = O> =
    type Output = O
function f<T>(value: T): Unit where T: Loop<N, Output = Int32> = ()
function main(): Unit = f(N(1))
"#,
        "does not implement trait",
    );
}

#[test]
fn generic_operator_overloads_preserve_the_selected_output() {
    common::compile_and_run(
        r#"
package a
newtype Wrapped<T> = T
implement <T> Add<Int32> for Wrapped<T> =
    type Output = Int32
    function add(self: Wrapped<T>, rhs: Int32): Int32 = rhs + 1
implement <T> Add<String> for Wrapped<T> =
    type Output = String
    function add(self: Wrapped<T>, rhs: String): String = rhs ++ "!"
function add<L, R, O>(left: L, right: R): O where L: Add<R, Output = O> = left + right
function main(): Unit =
    let number = Wrapped(true) + 2
    let text = Wrapped(true) + "ok"
    assert number == 3
    assert text == "ok!"
    assert add(Wrapped(true), 4) == 5
    assert add(Wrapped(true), "generic") == "generic!"
"#,
    )
    .unwrap();
}

#[test]
fn generic_operator_rhs_can_contain_an_interface_type_parameter() {
    common::compile_and_run(
        r#"
package a
interface Value<T> =
    function get(self: Self): T
newtype N = Int32
implement Value<Int32> for N =
    function get(self: N): Int32 = self.value
implement <T> Add<Value<T>> for N =
    type Output = T
    function add(self: N, rhs: Value<T>): T = rhs.get()
function main(): Unit =
    let value: Value<Int32> = N(2)
    assert N(1) + value == 2
"#,
    )
    .unwrap();
}

#[test]
fn static_extensions_infer_associated_outputs() {
    common::compile_and_run(
        r#"
package a
import a.ArrayFactory
extension ArrayFactory<T, O> for Array<T> where T: Add<T, Output = O> =
    function add(left: T, right: T): O = left + right
function main(): Unit =
    let result = Array.add(1, 2)
    assert result == 3
"#,
    )
    .unwrap();
}

#[test]
fn superclass_arguments_resolve_operator_implementations() {
    common::compile_and_run(
        r#"
package a
newtype N = Int32
implement Add<N> for N =
    type Output = Int32
    function add(self: N, rhs: N): Int32 = self.value + rhs.value
class Base(public value: Int32)
class Child(left: N, right: N) extends Base(left + right)
function main(): Unit = assert Child(N(2), N(3)).value == 5
"#,
    )
    .unwrap();
}

#[test]
fn static_associated_parameters_coerce_to_interfaces() {
    common::compile_and_run(
        r#"
package a
interface Value =
    function get(self: Self): Int32
newtype N = Int32
implement Value for N =
    function get(self: N): Int32 = self.value
trait Consume =
    type Output
    function consume(value: Output): Int32
implement Consume for N =
    type Output = Value
    function consume(value: Value): Int32 = value.get()
function consume<T>(receiver: T): Int32 where T: Consume<Output = Value> = T.consume(N(2))
function main(): Unit = assert consume(N(1)) == 2
"#,
    )
    .unwrap();
}

#[test]
fn bound_operator_calls_complete_intermediate_impl_parameters() {
    common::compile_and_run(
        r#"
package a
trait Produce =
    type Output
    function produce(self: Self): Output
newtype N = Int32
implement Produce for N =
    type Output = String
    function produce(self: N): String = "done"
implement Produce for String =
    type Output = Int32
    function produce(self: String): Int32 = self.length
newtype Wrapped<T> = T
implement <T, U, O> Add<Wrapped<T>> for Wrapped<T> where T: Produce<Output = U>, U: Produce<Output = O> =
    type Output = O
    function add(self: Wrapped<T>, rhs: Wrapped<T>): O = self.value.produce().produce()
function add<L, R, O>(left: L, right: R): O where L: Add<R, Output = O> = left + right
function main(): Unit = assert add(Wrapped(N(1)), Wrapped(N(2))) == 4
"#,
    )
    .unwrap();
}

#[test]
fn lowering_selects_concrete_operator_over_disjoint_generic_shape() {
    common::compile_and_run(
        r#"
package a
newtype Wrapped<T> = T
trait Marker =
    function mark(self: Self): Int32
implement <T, R> Add<R> for Wrapped<Array<T>> where T: Marker =
    type Output = String
    function add(self: Wrapped<Array<T>>, rhs: R): String = "wrong"
implement Add<Int32> for Wrapped<Int32> =
    type Output = Int32
    function add(self: Wrapped<Int32>, rhs: Int32): Int32 = self.value + rhs
function main(): Unit = assert Wrapped(2) + 3 == 5
"#,
    )
    .unwrap();
}

#[test]
fn generic_output_binding_does_not_resolve_ambiguous_operands() {
    let source = r#"
package a
newtype Wrapped<T> = T
implement Add<Int32> for Wrapped<Int32> =
    type Output = Int32
    function add(self: Wrapped<Int32>, rhs: Int32): Int32 = self.value + rhs
implement <T, R> Add<R> for Wrapped<T> =
    type Output = String
    function add(self: Wrapped<T>, rhs: R): String = "wrong"
function chosen<T>(value: T): String where T: Add<Int32, Output = String> = value + 3
function main(): Unit = assert chosen(Wrapped(2)) == "wrong"
"#;
    let result = dovetail::check(source, "test.dove");
    assert!(
        result.diagnostics.has_errors(),
        "accepted ambiguous operator operands"
    );
}

#[test]
fn inferred_class_fields_use_same_package_operator_outputs() {
    common::compile_and_run(
        r#"
package a
newtype N = Int32
implement Concat<N> for N =
    type Output = N
    function concat(self: N, rhs: N): N = N(self.value + rhs.value)
implement Add<N> for N =
    type Output = Int32
    function add(self: N, rhs: N): Int32 = self.value + rhs.value
class Combined(a: N, b: N) =
    public let joined = a ++ b
    public let total = a + b
    public let doubled = total + total
function main(): Unit =
    let result = Combined(N(2), N(3))
    assert result.joined.value == 5
    assert result.total == 5
    assert result.doubled == 10
"#,
    )
    .unwrap();
}

#[test]
fn inferred_generic_class_fields_use_associated_output_bounds() {
    common::compile_and_run(
        r#"
package a
newtype N = Int32
implement Add<N> for N =
    type Output = Int32
    function add(self: N, rhs: N): Int32 = self.value + rhs.value
class Same<T>(a: T, b: T) where T: Add<T, Output = T> =
    public let total = a + b
class Different<T>(a: T, b: T) where T: Add<T, Output = Int32> =
    public let total = a + b
function main(): Unit =
    assert Same(2, 3).total == 5
    assert Different(N(2), N(3)).total == 5
"#,
    )
    .unwrap();
}

#[test]
fn collected_operator_outputs_ignore_disjoint_generic_implementations() {
    common::compile_and_run(
        r#"
package a
newtype Wrapped<T> = T
trait Marker =
    function mark(self: Self): Int32
implement Add<Int32> for Wrapped<Int32> =
    type Output = Int32
    function add(self: Wrapped<Int32>, rhs: Int32): Int32 = self.value + rhs
implement <T> Add<Int32> for Wrapped<Array<T>> where T: Marker =
    type Output = String
    function add(self: Wrapped<Array<T>>, rhs: Int32): String = "wrong"
let left: Wrapped<Int32> = Wrapped(2)
let total = left + 3
class Sum(value: Wrapped<Int32>) =
    public let total = value + 3
function main(): Unit =
    assert total == 5
    assert Sum(left).total == 5
"#,
    )
    .unwrap();
}

#[test]
fn inferred_generic_field_rhs_preserves_nested_parameter_bounds() {
    let result = dovetail::check(
        r#"
package a
class Sum<T, R>(a: T, b: Array<R>) where T: Add<Array<R>, Output = Int32>, R: Equatable =
    public let total = a + b
function main(): Unit = ()
"#,
        "test.dove",
    );
    assert!(!result.diagnostics.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn inferred_generic_field_rhs_preserves_interface_parameter_bounds() {
    let result = dovetail::check(
        r#"
package a
interface Value<R> =
    function get(self: Self): R
class Join<T, R>(a: T, b: Value<R>) where T: Add<Value<R>, Output = Int32>, R: Equatable =
    public let result = a + b
function main(): Unit = ()
"#,
        "test.dove",
    );
    assert!(!result.diagnostics.has_errors(), "{:?}", result.diagnostics);
}

#[test]
fn inferred_class_field_can_depend_on_inferred_operator_global() {
    common::compile_and_run(
        r#"
package a
newtype N = Int32
let n: N = N(2)
let total = n + n
implement Add<N> for N =
    type Output = Int32
    function add(self: N, rhs: N): Int32 = self.value + rhs.value
class Total =
    public let amount = total + total
function main(): Unit = assert Total().amount == 8
"#,
    )
    .unwrap();
}

#[test]
fn inferred_operator_global_completes_impl_associated_output_parameters() {
    common::compile_and_run(
        r#"
package a
newtype W<T> = T
implement <T, O> Add<W<T>> for W<T> where T: Add<T, Output = O> =
    type Output = O
    function add(self: W<T>, rhs: W<T>): O = self.value + rhs.value
let left: W<Int32> = W(2)
let total = left + left
function main(): Unit = assert total == 4
"#,
    )
    .unwrap();
}

#[test]
fn abstract_generic_descendants_need_no_constructor_vtable() {
    common::compile_and_run(
        r#"
package a
class Base<T>(public value: T)
abstract class Child(value: Int32) extends Base<Int32>(value)
class Concrete(value: Int32) extends Child(value)
function main(): Unit =
    Concrete(3)
    ()
"#,
    )
    .unwrap();
}

#[test]
fn generic_initializers_specialize_each_instantiation() {
    common::compile_and_run(
        r#"
package a
class Combined<T>(a: T, b: T) where T: Add<T, Output = T> =
    public let total = a + b
class Joined<T>(a: T, b: T) where T: Concat<T, Output = T> =
    public let total = a ++ b
function main(): Unit =
    assert Combined(2, 3).total == 5
    assert Combined(1.5, 2.5).total == 4.0
    assert Joined("a", "b").total == "ab"
"#,
    )
    .unwrap();
}

#[test]
fn inherited_initializers_keep_field_values_and_evaluation_order() {
    common::compile_and_run(
        r#"
package a
let mutable trace: String = ""
function step<T>(label: String, value: T): T =
    trace = trace ++ label
    value
class Base<T>(a: T, b: T) where T: Add<T, Output = T> =
    public let total: T = step("p", a + b)
class Child<T>(a: T, b: T) extends Base<T>(step("e", b), step("f", a)) where T: Add<T, Output = T> =
    public let own: T = step("c", a + b)
class Concrete(a: Int32, b: Int32) extends Base<Int32>(a, b)
function main(): Unit =
    let child = Child(step("a", 2), step("b", 3))
    assert trace == "abefpc"
    assert child.total == 5
    assert child.own == 5
    trace = ""
    let concrete = Concrete(4, 5)
    assert trace == "p"
    assert concrete.total == 9
"#,
    )
    .unwrap();
}

#[test]
fn generic_initializer_closures_capture_concrete_operator_results() {
    common::compile_and_run(
        r#"
package a
class Combined<T>(a: T, b: T) where T: Add<T, Output = T> =
    let total = a + b
    public let get: () => T = () => total
function main(): Unit =
    let integer = Combined(2, 3)
    let decimal = Combined(1.5, 2.5)
    assert integer.get() == 5
    assert decimal.get() == 4.0
"#,
    )
    .unwrap();
}
