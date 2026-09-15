mod common;

#[test]
fn generic_trait_call_does_not_select_a_concrete_class_overload() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: T): T = value
class Reader implements Picker =
    public function pick(self: Reader, value: Int32): Int32 = 99
function choose<P, T>(picker: P, value: T): T where P: Picker = picker.pick(value)
function main(): Unit = assert choose(Reader(), 42) == 42
"#,
    )
    .expect("a trait call preserves the generic declaration it selected");
}

#[test]
fn generic_default_with_unused_parameter_is_not_suppressed_by_ordinary_method() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: Int32): Int32 = value
class Reader implements Picker =
    public function pick(self: Reader, value: Int32): Int32 = 99
function choose<P>(picker: P): Int32 where P: Picker = picker.pick<String>(42)
function main(): Unit =
    assert Reader().pick<String>(42) == 42
    assert choose(Reader()) == 42
"#,
    )
    .expect("generic arity is part of explicit-member matching");
}

#[test]
fn generic_class_keeps_unused_generic_defaults_distinct() {
    common::compile_and_run(
        r#"
package a
trait Picker =
    function pick<T>(self, value: Int32): Int32 = value
class Reader<X>(id: X) implements Picker =
    public function pick(self: Reader<X>, value: Int32): Int32 = 99
function choose<P>(picker: P): Int32 where P: Picker = picker.pick<String>(42)
function main(): Unit =
    assert Reader(1).pick<String>(42) == 42
    assert choose(Reader("text")) == 42
    assert Reader(1).pick(42) == 99
"#,
    )
    .expect("class and method generic parameters retain separate template identities");
}

#[test]
fn explicit_generic_class_methods_keep_unused_parameters_in_their_identity() {
    common::compile_and_run(
        r#"
package a
class Reader =
    public function pick(self: Reader, value: Int32): Int32 = 99
    public function pick<T>(self: Reader, value: Int32): Int32 = value
    public function pick<T, U>(self: Reader, value: Int32): Int32 = value + 1
    public function choose(value: Int32): Int32 = 99
    public function choose<T>(value: Int32): Int32 = value
function main(): Unit =
    assert Reader().pick<String>(42) == 42
    assert Reader().pick<String, Bool>(42) == 43
    assert Reader().pick(42) == 99
    assert Reader.choose<String>(42) == 42
    assert Reader.choose(42) == 99
"#,
    )
    .expect("explicit generic instance and static methods retain their own templates");
}
