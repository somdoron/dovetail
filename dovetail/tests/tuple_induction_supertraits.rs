#[test]
fn decreasing_tuple_supertrait_recursion_does_not_hit_the_depth_limit() {
    let elements = (1..=32).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = format!(r#"
package a
trait Base
trait Derived extends Base
implement <A, B> Derived for (A, B)
implement <T, U> Derived for T ~ U where T: Tuple, T: Base
function requireBase<T>(value: T): Unit where T: Base = ()
function main(): Unit = requireBase(({elements}))
"#);
    let checked = dovetail::check(&source, "test.dove");
    assert!(!checked.diagnostics.has_errors(), "{:?}", checked.diagnostics);
}

#[test]
fn ordinary_supertrait_recursion_still_spends_the_depth_budget() {
    let nested = format!("{}Int32{}", "Box<".repeat(32), ">".repeat(32));
    let source = format!(r#"
package a
trait Base
trait Derived extends Base
record Box<T> = value: T
implement Derived for Int32
implement <T> Derived for Box<T> where T: Base
function requireBase<T>(value: T): Unit where T: Base = ()
function check(value: {nested}): Unit = requireBase(value)
"#);
    let checked = dovetail::check(&source, "test.dove");
    assert!(checked.diagnostics.iter().any(|d| d.message.contains("does not implement trait")), "{:?}", checked.diagnostics);
}
