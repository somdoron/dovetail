mod common;

#[test]
fn interface_coercion_checks_direct_impl_bounds_before_selecting_a_provider() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Required =
    function required(self): Int32
interface Alpha =
    function alpha(self): Int32
interface Beta extends Alpha =
    function beta(self): Int32
implement <T> Alpha for Wrap<T> where T: Required =
    function alpha(self): Int32 = 1
implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3
implement Required for String =
    function required(self): Int32 = 0
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    let a: Alpha = w
    assert a.alpha() == 2
    let direct: Alpha = Wrap<String> { item = "x" }
    assert direct.alpha() == 1
"#,
    )
    .expect("only applicable implementations may supply an interface vtable");
}

#[test]
fn class_bounds_apply_to_direct_and_provided_interface_implementations() {
    common::compile_and_run(
        r#"
package a
class Base(public x: Int32)
class Child(x: Int32) extends Base(x)
record Wrap<T> =
    item: T
interface Alpha =
    function alpha(self): Int32
interface Beta extends Alpha =
    function beta(self): Int32
implement <T> Alpha for Wrap<T> where T: Child =
    function alpha(self): Int32 = 10
implement <T> Beta for Wrap<T> where T: Base =
    function alpha(self): Int32 = 20
    function beta(self): Int32 = 2
function bound<T>(v: T): Int32 where T: Alpha = v.alpha()
function main(): Unit =
    let child = Wrap<Child> { item = Child(1) }
    let base = Wrap<Base> { item = Base(1) }
    assert Alpha.alpha(child) == 10
    assert Alpha.alpha(base) == 20
    assert bound(child) == 10
    assert bound(base) == 20
    let childObject: Alpha = child
    let baseObject: Alpha = base
    assert childObject.alpha() == 10
    assert baseObject.alpha() == 20
"#,
    )
    .expect("class bounds use subtype relationships in inference and lowering");
}

#[test]
fn implicit_method_call_accepts_a_class_bounded_impl() {
    common::compile_and_run(
        r#"
package a
class Base(public x: Int32)
record Wrap<T> =
    item: T
trait Alpha =
    function alpha(self): Int32
implement <T> Alpha for Wrap<T> where T: Base =
    function alpha(self): Int32 = 7
function generic<T>(w: Wrap<T>): Int32 where T: Base = w.alpha()
function main(): Unit =
    let w = Wrap<Base> { item = Base(1) }
    assert w.alpha() == 7
    assert generic(w) == 7
"#,
    )
    .expect("a class-bounded impl remains applicable during monomorphization");
}

#[test]
fn unrelated_classes_do_not_satisfy_impl_class_bounds() {
    let errors = common::compile_expecting_errors(
        r#"
package a
class Base(public x: Int32)
class Other(public x: Int32)
record Wrap<T> =
    item: T
interface Alpha =
    function alpha(self): Int32
interface Beta extends Alpha =
    function beta(self): Int32
implement <T> Beta for Wrap<T> where T: Base =
    function alpha(self): Int32 = 20
    function beta(self): Int32 = 2
function main(): Unit =
    let value: Alpha = Wrap<Other> { item = Other(1) }
"#,
    );
    assert!(
        !errors.is_empty(),
        "unrelated classes cannot supply the interface"
    );
}

#[test]
fn interface_vtables_infer_hidden_impl_parameters_from_associated_type_bounds() {
    for implemented_trait in ["Read", "Extended"] {
        let extra = if implemented_trait == "Extended" {
            "    function extra(self): Int32 = 1"
        } else {
            ""
        };
        let source = format!(
            r#"
package a
record Wrap<T> =
    item: T
trait HasOutput =
    type Output
implement HasOutput for Int32 =
    type Output = String
interface Read =
    function read(self): Int32
interface Extended extends Read =
    function extra(self): Int32
implement <T, O> {implemented_trait} for Wrap<T> where T: HasOutput<Output = O> =
    function read(self): Int32 = 7
{extra}
function main(): Unit =
    let value: Read = Wrap<Int32> {{ item = 1 }}
    assert value.read() == 7
"#
        );
        common::compile_and_run(&source).unwrap_or_else(|error| {
            panic!("{implemented_trait} hidden parameter coercion: {error}")
        });
    }
}
