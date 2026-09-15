mod common;

#[test]
fn newtype_generic_typed_patterns_check_membership_before_binding_and_guards() {
    common::compile_and_run(
        r#"
package runtimePatterns
newtype Wrapped = Option<Any>
newtype Outer = Wrapped
class Counter(public mutable count: Int32)
function guard(counter: Counter): Bool =
    counter.count = counter.count + 1
    true
function inspect(value: Wrapped): Int32 =
    match value with
        case Wrapped(x: Option<String>) => 1
        case Wrapped(x: Option<Int32>) =>
            match x with
                case Some(n) => n
                case None => 0
        case _ => 2
function guarded(value: Wrapped, counter: Counter): Int32 =
    match value with
        case Wrapped(x: Option<String>) if guard(counter) => 1
        case Wrapped(x: Option<Int32>) if guard(counter) => 42
        case _ => 2
function rejectedGuard(value: Wrapped): Int32 =
    match value with
        case Wrapped(x: Option<Int32>) if false => 1
        case _ => 2
function nested(value: Outer): Int32 =
    match value with
        case Outer(Wrapped(x: Option<String>)) => 1
        case Outer(Wrapped(x: Option<Int32>)) => 42
        case _ => 2
function main(): Unit =
    let value = Wrapped(Some(42))
    let counter = Counter(0)
    assert inspect(value) == 42
    assert guarded(value, counter) == 42
    assert counter.count == 1
    assert rejectedGuard(value) == 2
    assert nested(Outer(value)) == 42
    assert inspect(Wrapped(Some(true))) == 2
    assert guarded(Wrapped(Some(true)), counter) == 2
    assert counter.count == 1
    assert nested(Outer(Wrapped(Some(true)))) == 2
"#,
    )
    .expect("newtype inner type patterns preserve runtime fallthrough and binding");
}

#[test]
fn nested_non_generic_class_patterns_test_before_binding_and_guards() {
    common::compile_and_run(
        r#"
package runtimePatterns
sealed abstract class Root<out T>()
final class Zero() extends Root<Int32>()
final class Leaf<out T>() extends Root<T>()
record Holder<T> = value: T
class Counter(public mutable count: Int32)
function guard(counter: Counter): Bool =
    counter.count = counter.count + 1
    true
function inspect(value: Root<Int32>, counter: Counter): Int32 =
    match (value, true) with
        case (x: Zero, true) if guard(counter) => 1
        case _ => 2
function recordCase(value: Holder<Root<Int32>>): Int32 =
    match value with
        case Holder { value = x: Zero } => 1
        case _ => 2
function enumCase(value: Option<Root<Int32>>): Int32 =
    match value with
        case Some(x: Zero) => 1
        case _ => 2
function main(): Unit =
    let counter = Counter(0)
    let leaf: Root<Int32> = Leaf<Int32>()
    let zero: Root<Int32> = Zero()
    assert inspect(leaf, counter) == 2
    assert counter.count == 0
    assert inspect(zero, counter) == 1
    assert counter.count == 1
    assert recordCase(Holder { value = leaf }) == 2
    assert recordCase(Holder { value = zero }) == 1
    assert enumCase(Some(leaf)) == 2
    assert enumCase(Some(zero)) == 1
"#,
    )
    .expect("nested class checks retain source types and fall through before guards");
}

#[test]
fn newtype_tuple_patterns_keep_flattened_values() {
    common::compile_and_run(
        r#"
package runtimePatterns
newtype Wrapped = (Option<Any>, Bool)
newtype Outer = Wrapped
function inspect(value: Wrapped): Int32 =
    match value with
        case Wrapped((x: Option<String>, true)) => 1
        case Wrapped((x: Option<Int32>, true)) if false => 2
        case Wrapped((x: Option<Int32>, true)) => 3
        case _ => 4
function nested(value: Outer): Int32 =
    match value with
        case Outer(Wrapped((x: Option<Int32>, true))) => 3
        case _ => 4
function bind(value: Wrapped): Bool =
    match value with
        case Wrapped(pair) => pair._1
function payload(value: Option<Wrapped>): Int32 =
    match value with
        case Some(Wrapped((x: Option<Int32>, true))) => 3
        case _ => 4
function main(): Unit =
    let value = Wrapped((Some(42), true))
    assert inspect(value) == 3
    assert inspect(Wrapped((Some(42), false))) == 4
    assert nested(Outer(value)) == 3
    assert nested(Outer(Wrapped((Some("s"), true)))) == 4
    assert bind(value)
    assert payload(Some(value)) == 3
    assert payload(Some(Wrapped((Some("s"), true)))) == 4
"#,
    )
    .expect("transparent newtypes retain flattened tuple patterns and bindings");
}

#[test]
fn nested_interface_patterns_test_the_underlying_generic_value() {
    common::compile_and_run(
        r#"
package runtimePatterns
interface Named =
    function name(self): String
record Box<T> = value: T
record Holder<T> = value: T
implement <T> Named for Box<T> =
    function name(self): String = "box"
function tupleCase(value: Named): Int32 =
    match (value, true) with
        case (x: Box<String>, true) => 1
        case (x: Box<Int32>, true) => x.value
        case _ => 2
function recordCase(value: Holder<Named>): Int32 =
    match value with
        case Holder { value = x: Box<String> } => 1
        case Holder { value = x: Box<Int32> } => x.value
        case _ => 2
function enumCase(value: Option<Named>): Int32 =
    match value with
        case Some(x: Box<String>) => 1
        case Some(x: Box<Int32>) => x.value
        case _ => 2
function main(): Unit =
    let value: Named = Box { value = 42 } as Named
    assert tupleCase(value) == 42
    assert recordCase(Holder { value = value }) == 42
    assert enumCase(Some(value)) == 42
    let other: Named = Box { value = true } as Named
    assert tupleCase(other) == 2
    assert recordCase(Holder { value = other }) == 2
    assert enumCase(Some(other)) == 2
"#,
    )
    .expect("nested interface tests unwrap data before membership tests and binding");
}
