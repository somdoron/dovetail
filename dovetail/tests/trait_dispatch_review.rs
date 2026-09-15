mod common;

#[test]
fn renamed_generic_default_overrides_the_inherited_member() {
    common::compile_and_run(
        r#"
package a
trait Base =
    function pick<T>(self, value: T): T = value
trait Child extends Base =
    function pick<U>(self, value: U): U = value
record Reader = id: Int32
implement Child for Reader
function choose<P, T>(picker: P, value: T): T where P: Base = picker.pick(value)
function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.pick(42) == 42
    assert choose(reader, "text") == "text"
"#,
    )
    .expect("renamed method parameters describe one overriding declaration");
}

#[test]
fn return_conflicts_are_checked_against_every_inherited_overload() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Number =
    function value(self, input: Int32): Int32
trait Text =
    function value(self, input: String): Int32
trait OtherText =
    function value(self, input: String): String
trait Combined extends Number and Text and OtherText
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("different return types")),
        "{errors:?}"
    );
}

#[test]
fn renamed_generic_default_conflicts_are_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Left =
    function pick<T>(self, value: T): T = value
trait Right =
    function pick<U>(self, value: U): U = value
trait Both extends Left and Right
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("different default implementations")),
        "{errors:?}"
    );
}

#[test]
fn overload_diamonds_preserve_each_default_override() {
    common::compile_and_run(
        r#"
package a
trait Text =
    function value(self, input: String): Int32 = 10
trait Both extends Text =
    function value(self, input: Int32): Int32 = 20
trait Left extends Both =
    function value(self, input: String): Int32 = 30
trait Right extends Both
trait Diamond extends Left and Right
record Reader = id: Int32
implement Diamond for Reader
function read<R>(reader: R): Int32 where R: Diamond = reader.value("text")
function main(): Unit =
    let reader = Reader { id = 0 }
    assert read(reader) == 30
    assert reader.value(0) == 20
"#,
    )
    .expect("every overload participates in diamond deduplication");
}

#[test]
fn generic_default_overrides_cannot_strengthen_the_inherited_contract() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Base =
    function pick<T>(self, value: T): T = value
trait Child extends Base =
    function pick<U>(self, value: U): U where U: Equatable = value
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("cannot strengthen")),
        "{errors:?}"
    );
}

#[test]
fn generic_default_override_retains_permitted_bounds() {
    common::compile_and_run(
        r#"
package a
trait Base =
    function pick<T>(self, value: T): T where T: Equatable = value
trait Child extends Base =
    function pick<U>(self, value: U): U where U: Equatable = value
record Reader = id: Int32
implement Child for Reader
function main(): Unit = assert Reader { id = 0 }.pick(42) == 42
"#,
    )
    .expect("an override may retain the inherited method contract");
}

#[test]
fn inherited_property_does_not_replace_a_trait_method_default() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick(self): Int32 = 42
class Parent =
    public property pick(self: Parent): Int32 = 10
class Child<X>(id: X) extends Parent() implements Picker
function choose<P>(p: P): Int32 where P: Picker = p.pick()
function main(): Unit = assert choose(Child(1)) == 42
"#,
    )
    .expect("a property cannot supply a method's implementation");
}

#[test]
fn inherited_generic_method_binders_do_not_capture_child_trait_parameters() {
    common::compile_and_run(
        r#"
package a
trait Source<A> =
    function pick<T>(self, input: A, value: T): T = value
trait Child<T> extends Source<T>
record Reader = id: Int32
implement Child<Int32> for Reader
function choose<P, U>(reader: P, value: U): U where P: Child<Int32> = reader.pick(1, value)
function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.pick(1, "text") == "text"
    assert choose(reader, "bound") == "bound"
"#,
    )
    .expect("the child's T and inherited method's T remain independent");
}

#[test]
fn inherited_generic_override_contract_keeps_enclosing_parameters_separate() {
    common::compile_and_run(
        r#"
package a
trait Source<A> =
    function pick<T>(self, input: A, value: T): T where T: Equatable = value
trait Child<T> extends Source<T> =
    function pick<U>(self, input: T, value: U): U where U: Equatable = value
record Reader = id: Int32
implement Child<Int32> for Reader
function main(): Unit = assert Reader { id = 0 }.pick(1, "text") == "text"
"#,
    )
    .expect("override matching does not capture an enclosing argument");
}

#[test]
fn classes_materialize_inherited_generic_trait_defaults() {
    common::compile_and_run(
        r#"
package a
trait Picker<A> =
    function pick<T>(self, input: A, value: T): T = value
trait Child<T> extends Picker<T>
class Reader implements Child<Int32>
class Box<A>(id: A) implements Child<A>
function choose<P, T>(picker: P, value: T): T where P: Picker<Int32> = picker.pick(1, value)
function main(): Unit =
    assert Reader().pick(1, "text") == "text"
    assert choose(Reader(), "bound") == "bound"
    assert Box(1).pick(1, "generic") == "generic"
    assert choose(Box(1), "generic bound") == "generic bound"
"#,
    )
    .expect("inherited templates use the class signature's method binder identities");
}

#[test]
fn same_class_method_and_property_defaults_cannot_share_one_implementation() {
    for declaration in [
        "class Box<T>(id: T) implements Getter and Picker",
        "class Box<T>(id: T) implements Getter =\n    public function pick(self: Box<T>): Int32 = 42",
        "class Box<T>(id: T) implements Picker =\n    public property pick(self: Box<T>): Int32 = 10",
    ] {
        let source = format!(
            r#"
package a
trait Getter =
    property pick(self): Int32 = 10
trait Picker =
    function pick(self): Int32 = 42
{declaration}
function main(): Unit = ()
"#
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("conflicting method and property")),
            "{errors:?}"
        );
    }
}

#[test]
fn default_property_does_not_hide_an_inherited_method_call() {
    common::compile_and_run(
        r#"
package a
trait Getter =
    property pick(self): Int32 = 10
class Parent =
    public function pick(self: Parent): Int32 = 42
class Box<T>(id: T) extends Parent() implements Getter
class Plain extends Parent() implements Getter
function main(): Unit =
    let box = Box(1)
    assert box.pick == 10
    assert box.pick() == 42
    assert Plain().pick == 10
    assert Plain().pick() == 42
"#,
    )
    .expect("property access and calls select members of their respective kinds");
}
