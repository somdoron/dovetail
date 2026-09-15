fn assert_specialization_limit(source: &str) {
    let checked = dovetail::check(source, "growth.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
    let compiled = dovetail::compile(source, "growth.dove");
    assert!(compiled.wasm.is_none());
    let diagnostic = compiled
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.message.contains("specialization limit"))
        .unwrap_or_else(|| panic!("{:?}", compiled.diagnostics));
    assert_eq!(diagnostic.span.file.as_ref(), "growth.dove");
}

#[test]
fn alternating_helper_and_impl_growth_reports_a_diagnostic() {
    assert_specialization_limit(
        r#"
package a
trait Grow =
    function grow(self): Unit
implement <T> Grow for (T, Int32) =
    function grow(self: (T, Int32)): Unit = helper((self, 1))
function helper<T>(value: T): Unit where T: Grow = value.grow()
function main(): Unit = helper((1, 1))
"#,
    );
}

#[test]
fn recursive_tuple_impl_growth_reports_a_diagnostic() {
    assert_specialization_limit(
        r#"
package a
trait Grow =
    function grow(self): Unit
implement <T, U> Grow for T ~ U where T: Tuple =
    function grow(self: T ~ U): Unit = (self ~ 1).grow()
function main(): Unit = (1, 2, 3).grow()
"#,
    );
}

#[test]
fn late_interface_coercion_growth_reports_a_diagnostic() {
    assert_specialization_limit(
        r#"
package a
interface Next =
    function next(self): Next
implement <T, U> Next for T ~ U where T: Tuple =
    function next(self: T ~ U): Next = self ~ 1
function main(): Unit =
    let value: Next = (1, 2, 3)
    let larger = value.next()
    ()
"#,
    );
}

#[test]
fn recursive_generic_helper_growth_reports_a_diagnostic() {
    assert_specialization_limit(
        r#"
package a
function grow<T>(value: T): Unit = grow((value, 1))
function main(): Unit = grow(1)
"#,
    );
}
