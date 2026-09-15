mod common;
#[test]
fn interface_any_coverage_must_not_discard_leaf() {
    let errors = common::compile_expecting_errors(
        r#"
package probe
interface Named =
    function name(self): String
sealed abstract class Root<out A, out B>()
final class Leaf<out T>() extends Root<Named, T>()
function inspect(value: Root<Any, Int32>): Int32 =
    match value with
        case x: Leaf<String> => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}
#[test]
fn interface_any_coverage_fallback_and_positive() {
    common::compile_and_run(
        r#"
package probe
interface Named =
    function name(self): String
sealed abstract class Root<out A, out B>()
final class Leaf<out T>() extends Root<Named, T>()
function inspect(value: Root<Any, Int32>): Int32 =
    match value with
        case x: Leaf<String> => 1
        case _ => 2
function exhaustive(value: Root<Any, Int32>): Int32 =
    match value with
        case x: Leaf<Int32> => 3
function main(): Unit =
    let value: Root<Any, Int32> = Leaf<Int32>()
    assert inspect(value) == 2
    assert exhaustive(value) == 3
"#,
    )
    .unwrap();
}

#[test]
fn contravariant_interface_any_coverage_must_not_discard_leaf() {
    let errors = common::compile_expecting_errors(
        r#"
package probe
interface Named =
    function name(self): String
sealed abstract class Root<in A, out B>()
final class Leaf<out T>() extends Root<Any, T>()
function inspect(value: Root<Named, Int32>): Int32 =
    match value with
        case x: Leaf<String> => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn interface_coverage_keeps_never_instantiations_of_invariant_leaves() {
    let errors = common::compile_expecting_errors(
        r#"
package probe
interface Named =
    function name(self): String
sealed abstract class Root<out T>()
final class Leaf<T>() extends Root<T>()
function inspect(value: Root<Named>): Int32 =
    match value with
        case x: Leaf<Named> => 1
function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn interface_coverage_fallback_handles_never_instantiation() {
    common::compile_and_run(
        r#"
package probe
interface Named =
    function name(self): String
sealed abstract class Root<out T>()
final class Leaf<T>() extends Root<T>()
function inspect(value: Root<Named>): Int32 =
    match value with
        case x: Leaf<Named> => 1
        case _ => 2
function main(): Unit =
    let value: Root<Named> = Leaf<Never>()
    assert inspect(value) == 2
"#,
    )
    .expect("Never instantiation remains in the subject domain and reaches fallback");
}
