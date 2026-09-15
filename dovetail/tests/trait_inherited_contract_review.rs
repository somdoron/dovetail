mod common;

#[test]
fn inherited_method_cannot_strengthen_a_new_trait_contract() {
    for declaration in [
        "class Child<X>(id: X) extends Parent() implements Picker",
        "class Child extends Parent() implements Picker",
    ] {
        let source = format!(
            r#"
package a
trait Picker =
    function pick<T>(self, value: T): T = value
class Parent =
    public function pick<T>(self: Parent, value: T): T where T: Equatable = value
{declaration}
"#
        );
        let checked = dovetail::check(&source, "test.dove");
        assert!(
            checked.diagnostics.iter().any(|diagnostic| diagnostic
                .message
                .contains("inherited method 'pick' cannot strengthen its trait contract")),
            "{:?}",
            checked.diagnostics
        );
    }
}

#[test]
fn inherited_enclosing_requirements_need_class_evidence() {
    let checked = dovetail::check(
        r#"
package a
trait Picker =
    function pick<T>(self, value: T): T = value
class Parent<A> =
    public function pick<B>(self: Parent<A>, value: B): B where A: Equatable = value
class Middle<V> extends Parent<V>()
class Child<X>(id: X) extends Middle<X>() implements Picker
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("inherited method 'pick' cannot strengthen its trait contract")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn inherited_contract_substitutes_enclosing_and_method_parameters_together() {
    let checked = dovetail::check(
        r#"
package a
trait Accept<A> =
    function accept(self, value: A): Unit
trait Picker<V> =
    function pick<T>(self, value: T): T where T: Accept<V> = value
class Parent<A> =
    public function pick<B>(self: Parent<A>, value: B): B where B: Accept<A> = value
class Middle<V> extends Parent<V>()
class Child<T>(id: T) extends Middle<T>() implements Picker<T>
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn inherited_method_cannot_change_a_relational_bound() {
    let checked = dovetail::check(
        r#"
package a
trait Accept<A> =
    function accept(self, value: A): Unit
trait Picker<V> =
    function pick<T>(self, value: T): T where T: Accept<V> = value
class Parent<A> =
    public function pick<B>(self: Parent<A>, value: B): B where B: Accept<B> = value
class Child<T>(id: T) extends Parent<T>() implements Picker<T>
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("inherited method 'pick' cannot strengthen its trait contract")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn inherited_method_can_use_trait_and_class_guarantees() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: T): T where T: Equatable = value
class Parent<A> =
    public function pick<B>(self: Parent<A>, value: B): B where A: Equatable, B: Equatable = value
class Child<X>(id: X) extends Parent<X>() implements Picker where X: Equatable
function choose<P, T>(picker: P, value: T): T where P: Picker, T: Equatable = picker.pick(value)
function main(): Unit = assert choose(Child(1), "text") == "text"
"#,
    )
    .expect("inherited requirements follow the trait contract and class guarantees");
}

#[test]
fn explicit_override_replaces_an_inherited_stronger_method() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: T): T = value
class Parent =
    public function pick<T>(self: Parent, value: T): T where T: Equatable = value
class Child<X>(id: X) extends Parent() implements Picker =
    public override function pick<U>(self: Child<X>, value: U): U = value
record Unknown = id: Int32
function choose<P, T>(picker: P, value: T): T where P: Picker = picker.pick(value)
function main(): Unit = assert choose(Child(1), Unknown { id = 42 }).id == 42
"#,
    )
    .expect("explicit weaker override supplies the trait implementation");
}
