mod common;

#[test]
fn generic_ancestor_tests_and_casts_preserve_construction_identity() {
    common::compile_and_run(
        r#"
package runtimeIds
sealed abstract class Async<out T, out E>()
final class Succeed<out T>(public value: T) extends Async<T, Never>()
final class Fail<out E>(public error: E) extends Async<Never, E>()
final class Yield() extends Async<Unit, Never>()
function make<T>(value: T): Any = Succeed<T>(value)
function main(): Unit =
    let value: Any = make(42)
    assert value is Succeed<Int32>
    assert !(value is Succeed<String>)
    assert value is Async<Int32, String>
    assert !(value is Async<String, String>)
    let original = value as Succeed<Int32>
    let wide: Async<Int32, String> = original
    let again: Any = wide
    assert again is Succeed<Int32>
    assert ClassIdentity.hash(original) == ClassIdentity.hash(wide)
    let empty: Any = Yield()
    assert empty is Async<Unit, String>
    assert !(empty is Async<Int32, String>)
    assert empty is Yield
    let result = match value with
        case wrong: Succeed<String> => 0
        case good: Succeed<Int32> => good.value
        case _ => -1
    assert result == 42
"#,
    )
    .expect("precise generic ancestor checks");
}

#[test]
fn generic_descendant_shares_header_with_non_generic_root_and_sibling() {
    common::compile_and_run(
        r#"
package runtimeIds
class Base(public mutable counter: Int32)
final class Child<T>(public value: T) extends Base(7)
final class Sibling() extends Base(9)
function main(): Unit =
    let child = Child<Int32>(42)
    let parent: Base = child
    let hash = ClassIdentity.hash(parent)
    parent.counter = 11
    assert child.counter == 11
    assert child.value == 42
    assert hash == ClassIdentity.hash(child)
    let any: Any = parent
    assert any is Child<Int32>
    assert !(any is Child<String>)
    assert any is Base
    let sibling: Any = Sibling()
    assert sibling is Base
    assert !(sibling is Child<Int32>)
    let root = Base(3)
    assert root.counter == 3
"#,
    )
    .expect("consistent mixed-hierarchy header");
}

#[test]
fn wrong_argument_cast_traps_without_payload_access() {
    let error = common::compile_and_run(
        r#"
package runtimeIds
function main(): Unit =
    let value: Any = Some(42)
    let wrong = value as Option<String>
    ()
"#,
    )
    .expect_err("wrong instantiation must fail at the cast");
    assert!(
        !error.contains("compilation failed") && !error.contains("component load error"),
        "{error}"
    );
}

#[test]
fn nested_types_invariant_arguments_and_never() {
    common::compile_and_run(
        r#"
package runtimeIds
record Pair<A, B> = first: A; second: B
function main(): Unit =
    let nested: Any = Some(Some(42))
    assert nested is Option<Option<Int32>>
    assert !(nested is Option<Option<String>>)
    let pair: Any = Pair { first = 42; second = "hi" }
    assert pair is Pair<Int32, String>
    assert !(pair is Pair<Any, String>)
    let empty: Any = None
    assert empty is Option<Int32>
    assert empty is Option<String>
    let result: Any = Ok(42)
    assert result is Result<Int32, String>
    assert !(result is Result<String, String>)
"#,
    )
    .expect("nested arguments, invariance, and bottom type");
}

#[test]
fn generic_hierarchy_coverage_tracks_arguments() {
    common::compile_and_run(
        r#"
package runtimeIds
sealed abstract class Async<out T, out E>()
final class Succeed<out T>(public value: T) extends Async<T, Never>()
final class Fail<out E>(public error: E) extends Async<Never, E>()
function inspect<T, E>(a: Async<T, E>): Int32 =
    match a with
        case s: Succeed<T> => 1
        case f: Fail<E> => 2
function main(): Unit =
    let value: Async<Int32, String> = Succeed(42)
    assert inspect(value) == 1
"#,
    )
    .expect("symbolic generic exhaustiveness");
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
sealed abstract class Container<out T>()
final class Box<out T>(public value: T) extends Container<T>()
function inspect(value: Container<Any>): Int32 =
    match value with
        case b: Box<Int32> => b.value
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn static_ancestor_arguments_are_checked() {
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
class Base<out T>()
final class Child<out T>() extends Base<T>()
function main(): Unit =
    let wrong: Base<String> = Child<Int32>()
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "invalid generic ancestor assignment was accepted"
    );
}

#[test]
fn contravariance_and_reordered_nested_ancestors() {
    common::compile_and_run(
        r#"
package runtimeIds
class Animal()
final class Dog() extends Animal()
class Consumer<in T>()
class Root<out A, out B>()
class Middle<out X, out Y>() extends Root<Option<Y>, X>()
final class Leaf<out X, out Y>() extends Middle<X, Y>()
function main(): Unit =
    let consumer: Any = Consumer<Animal>()
    assert consumer is Consumer<Dog>
    let dogConsumer: Any = Consumer<Dog>()
    assert !(dogConsumer is Consumer<Animal>)
    let leaf: Any = Leaf<Int32, Dog>()
    assert leaf is Root<Option<Animal>, Int32>
    assert !(leaf is Root<Option<Int32>, Animal>)
"#,
    )
    .expect("variance through nested and reordered ancestor parameters");
}

#[test]
fn record_update_has_its_resolved_instantiation_and_checks_evaluate_once() {
    common::compile_and_run(
        r#"
package runtimeIds
class Animal()
final class Dog() extends Animal()
record Box<out T> = value: T
class Counter(public mutable count: Int32)
function get(counter: Counter): Any =
    counter.count = counter.count + 1
    Some(42)
function main(): Unit =
    let original: Box<Dog> = Box { value = Dog() }
    let wide: Box<Animal> = original
    let updated = wide with value = Animal()
    let any: Any = updated
    assert any is Box<Animal>
    assert !(any is Box<Dog>)
    let untouched: Any = original
    assert untouched is Box<Dog>
    let counter = Counter(0)
    assert get(counter) is Option<Int32>
    assert counter.count == 1
    let result = get(counter) as Option<Int32>
    assert counter.count == 2
    match result with
        case Some(v) => assert v == 42
        case None => assert false
"#,
    )
    .expect("new construction identity on record updates and single evaluation");
}

#[test]
fn static_redundancy_requires_generic_argument_proof() {
    common::compile_and_run(
        r#"
package runtimeIds
class Box<out T>()
function check(value: Box<Any>): Bool = value is Box<Int32>
function main(): Unit =
    assert check(Box<Int32>())
    assert !check(Box<String>())
"#,
    )
    .expect("same nominal class can require a runtime argument check");
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
class Box<out T>()
function check(value: Box<Int32>): Bool = value is Box<Any>
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("always true")),
        "{errors:?}"
    );
}

#[test]
fn any_match_requires_actual_coverage_and_guarded_arms_do_not_cover() {
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
function inspect(value: Any): Int32 =
    match value with
        case option: Option<Int32> => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
sealed abstract class Root<out T>()
final class Leaf<out T>() extends Root<T>()
function inspect(value: Root<Int32>): Int32 =
    match value with
        case leaf: Leaf<Int32> if false => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn coverage_preserves_correlations_and_substitutes_intermediate_classes() {
    common::compile_and_run(
        r#"
package runtimeIds
sealed abstract class Root<out A, out B>()
sealed abstract class Middle<out X, out Y>() extends Root<Option<Y>, X>()
final class Leaf<out X, out Y>() extends Middle<X, Y>()
final class Fixed() extends Root<Option<String>, Int32>()
function inspect(value: Root<Option<String>, Int32>): Int32 =
    match value with
        case leaf: Leaf<Int32, String> => 1
        case fixed: Fixed => 2
function main(): Unit =
    assert inspect(Leaf<Int32, String>()) == 1
    assert inspect(Fixed()) == 2
"#,
    )
    .expect("multi-level symbolic class coverage");
}

#[test]
fn recursive_generic_fields_do_not_generate_infinite_hypothetical_ids() {
    common::compile_and_run(
        r#"
package runtimeIds
record Nest<T> = value: T; next: Option<Nest<Array<T>>>
function main(): Unit =
    let nest: Nest<Int32> = Nest { value = 42; next = None }
    let any: Any = nest
    assert any is Nest<Int32>
    assert !(any is Nest<String>)
"#,
    )
    .expect("discovery follows emitted constructions, not unbounded field expansion");
}

#[test]
fn runtime_checks_inside_generic_functions_keep_dynamic_arms() {
    common::compile_and_run(
        r#"
package runtimeIds
class Checker<T>(value: Any) =
    let found = value is Option<T>
    public property matched(self): Bool = self.found
function inspect<T>(value: Any): Int32 =
    match value with
        case typed: Option<T> => 1
        case _ => 2
function main(): Unit =
    assert inspect<Int32>(Some(42)) == 1
    assert inspect<String>(Some(42)) == 2
    assert Checker<Int32>(Some(42)).matched
    assert !Checker<String>(Some(42)).matched
"#,
    )
    .expect("monomorphization specializes the target without deleting dynamic arms");
}

#[test]
fn class_bounds_preserve_ancestor_arguments() {
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
class Base<out T>()
final class Child<out T>() extends Base<T>()
function wrong<T>(value: T): Base<String> where T: Child<Int32> = value
function main(): Unit = ()
"#,
    );
    assert!(!errors.is_empty(), "class bound lost its generic arguments");
}

#[test]
fn coverage_reduces_finite_sealed_argument_domains() {
    common::compile_and_run(
        r#"
package runtimeIds
sealed abstract class Color()
final class Red() extends Color()
final class Blue() extends Color()
sealed abstract class Root<out T>()
final class Box<T>() extends Root<T>()
function inspect(value: Root<Color>): Int32 =
    match value with
        case b: Box<Color> => 0
        case b: Box<Red> => 1
        case b: Box<Blue> => 2
        case b: Box<Never> => 3
function main(): Unit =
    assert inspect(Box<Red>()) == 1
    assert inspect(Box<Color>()) == 0
"#,
    )
    .expect("finite sealed argument domain includes abstract types and Never");
}

#[test]
fn typed_patterns_on_widened_generic_values_remain_dynamic() {
    common::compile_and_run(
        r#"
package runtimeIds
function inspect<T>(value: Option<Any>): Int32 =
    match value with
        case typed: Option<T> => 1
        case _ => 2
function main(): Unit =
    let value: Option<Any> = Some(42)
    assert inspect<Int32>(value) == 1
    assert inspect<String>(value) == 2
"#,
    )
    .expect("generic record/enum subjects retain construction identity in typed matches");
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
function inspect(value: Option<Any>): Int32 =
    match value with
        case typed: Option<Int32> => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn interface_arguments_widen_to_any_without_establishing_wrapping_subtypes() {
    common::compile_and_run(
        r#"
package runtimeIds
interface Named =
    function name(self): String
record Person = value: String
implement Named for Person =
    function name(self): String = self.value
class Box<out T>(public value: T)
function main(): Unit =
    let named: Named = Person { value = "Ada" } as Named
    let box = Box(named)
    let widened: Box<Any> = box
    let any: Any = box
    assert any is Box<Any>
    assert any is Box<Named>
    assert ClassIdentity.hash(box) == ClassIdentity.hash(widened)
    assert box.value.name() == "Ada"
    let concrete: Any = Box(Person { value = "Ada" })
    assert !(concrete is Box<Named>)
"#,
    )
    .expect("Any accepts existing wrappers without implicit interface wrapping");
    let errors = common::compile_expecting_errors(
        r#"
package runtimeIds
interface Named =
    function name(self): String
record Person = value: String
implement Named for Person =
    function name(self): String = self.value
class Box<out T>(public value: T)
function main(): Unit =
    let concrete = Box(Person { value = "Ada" })
    let invalid: Box<Named> = concrete
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "container widening must not wrap its payload"
    );
}

#[test]
fn tuple_argument_covariance_payload() {
    common::compile_and_run(
        r#"
package probe
record Box<out T> = value: T
function main(): Unit =
    let original = Box { value = (42, "hello") }
    let any: Any = original
    assert any is Box<(Any, Any)>
    let wide = any as Box<(Any, Any)>
    assert (wide.value._0 as Int32) == 42
    assert (wide.value._1 as String) == "hello"
"#,
    )
    .unwrap();
}

#[test]
fn function_argument_covariance_payload() {
    common::compile_and_run(
        r#"
package probe
record Box<out T> = value: T
function main(): Unit =
    let original = Box { value = (value: Any) => 42 }
    let any: Any = original
    assert any is Box<Int32 => Any>
    let wide = any as Box<Int32 => Any>
    assert (wide.value(3) as Int32) == 42
"#,
    )
    .unwrap();
}

#[test]
fn interface_subject_matches_remain_dynamic_inside_generic_functions() {
    common::compile_and_run(
        r#"
package runtimeIds
interface Marker =
    function marker(self): Int32
record Box<T> = value: T
class Cell<T>(public value: T)
implement <T> Marker for Box<T> =
    function marker(self): Int32 = 1
implement <T> Marker for Cell<T> =
    function marker(self): Int32 = 2
function extract<T>(value: Marker, fallback: T): T =
    match value with
        case box: Box<T> => box.value
        case cell: Cell<T> => cell.value
        case _ => fallback
function main(): Unit =
    let boxed: Marker = Box { value = 42 }
    let cell: Marker = Cell("hello")
    assert extract<Int32>(boxed, 0) == 42
    assert extract<String>(boxed, "fallback") == "fallback"
    assert extract<String>(cell, "fallback") == "hello"
    assert extract<Int32>(cell, 0) == 0
"#,
    )
    .expect("interface subjects preserve generic type checks and payload extraction");
}
