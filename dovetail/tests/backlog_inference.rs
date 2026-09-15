mod common;

#[test]
fn fold_left_infers_closure_parameters_from_initial_value() {
    common::compile_and_run(r#"
package a
function main(): Unit =
    let total = [1, 2, 3].foldLeft(0, (acc, item) => acc + item)
    assert total == 6
"#).unwrap();
}

#[test]
fn generic_impl_instance_property() {
    common::compile_and_run(r#"
package a
record Box<T> = value: T
trait Tagged =
    property tag: Int32
implement <T> Tagged for Box<T> =
    property tag: Int32 = 7
function main(): Unit = assert Box { value = "x" }.tag == 7
"#).unwrap();
}

#[test]
fn closure_literal_argument_to_concrete_impl_method() {
    common::compile_and_run(r#"
package a
record Value = n: Int32
trait Apply =
    function apply(self, f: Int32 => Int32): Int32
implement Apply for Value =
    function apply(self, f: Int32 => Int32): Int32 = f(self.n)
function main(): Unit = assert Value { n = 3 }.apply(x => x + 2) == 5
"#).unwrap();
}

#[test]
fn list_literal_coerces_elements_to_expected_interface() {
    common::compile_and_run(r#"
package a
interface Read =
    function read(self): Int32
record Value = n: Int32
implement Read for Value =
    function read(self): Int32 = self.n
function main(): Unit =
    let values: List<Read> = [Value { n = 4 }]
    assert values.foldLeft(0, (acc, value) => acc + value.read()) == 4
"#).unwrap();
}

#[test]
fn module_wrong_arity_falls_through_to_trait_method() {
    common::compile_and_run(r#"
package a
record Value = n: Int32
module Value =
    function read(self: Value, x: Int32): Int32 = x
trait Read =
    function read(self): Int32
implement Read for Value =
    function read(self): Int32 = self.n
function main(): Unit = assert Value { n = 4 }.read() == 4
"#).unwrap();
}

#[test]
fn static_extension_on_generic_instantiation() {
    common::compile_and_run(r#"
package a
import a.E
record Wrap<T> = value: T
extension E for Wrap<Int32> =
    function make(): Int32 = 7
function main(): Unit = assert Wrap<Int32>.make() == 7
"#).unwrap();
}

#[test]
fn generic_extension_method_rejects_wrong_arity() {
    for call in ["value.dup(1, 2)", "value.dup()", "E.dup(value, 1, 2)", "E.dup(value)"] {
        let source = format!(r#"
package a
import a.E
record Box<T> = value: T
extension E<T> for Box<T> =
    function dup(self, n: Int32): Int32 = n
function main(): Unit =
    let value = Box {{ value = 7 }}
    let result = {call}
"#);
        assert!(!common::compile_expecting_errors(&source).is_empty(), "accepted {call}");
    }
}

#[test]
fn bound_references_capture_generic_extension_impl_and_interface_receivers() {
    for source in [
        r#"
package a
import a.E
record Box<T> = value: T
extension E<T> for Box<T> =
    function read(self): T = self.value
function main(): Unit =
    let value = Box { value = 7 }
    let read = value.read
    assert read() == 7
"#,
        r#"
package a
record Box<T> = value: T
trait Read =
    function read(self): Int32
implement <T> Read for Box<T> =
    function read(self): Int32 = 7
function main(): Unit =
    let value = Box { value = "x" }
    let read = value.read
    assert read() == 7
"#,
        r#"
package a
record Value = n: Int32
interface Read =
    function read(self): Int32
implement Read for Value =
    function read(self): Int32 = self.n
function main(): Unit =
    let value: Read = Value { n = 7 }
    let read = value.read
    assert read() == 7
"#,
    ] {
        common::compile_and_run(source).unwrap();
    }
}

#[test]
fn unbound_extension_property_reference() {
    common::compile_and_run(r#"
package a
import a.E
record Value = n: Int32
extension E for Value =
    property read(self): Int32 = self.n
function main(): Unit =
    let read = Value.read
    assert read(Value { n = 7 }) == 7
"#).unwrap();
}

#[test]
fn trait_static_property_honors_explicit_application() {
    common::compile_and_run(r#"
package a
record Value = n: Int32
trait Conv<T> =
    property zero(): Int32
implement Conv<Int32> for Value =
    property zero(): Int32 = 7
implement Conv<String> for Value =
    property zero(): Int32 = 9
function main(): Unit = assert Conv<Int32>.zero == 7
"#).unwrap();
}

#[test]
fn bare_interface_and_ambiguous_static_references_have_specific_diagnostics() {
    let errors = common::compile_expecting_errors(r#"
package a
interface Read =
    function read(self): Int32
function main(): Unit =
    let f = Read.read
"#);
    assert!(errors.iter().any(|e| e.contains("interface 'Read'") && e.contains("requires a receiver")), "{errors:?}");
    let errors = common::compile_expecting_errors(r#"
package a
record Value = n: Int32
trait A =
    function make(): Int32
trait B =
    function make(): Int32
implement A for Value =
    function make(): Int32 = 1
implement B for Value =
    function make(): Int32 = 2
function main(): Unit =
    let f = Value.make
"#);
    assert!(errors.iter().any(|e| e.contains("ambiguous reference")), "{errors:?}");
    assert!(!errors.iter().any(|e| e.contains("undefined variable")), "{errors:?}");
}

#[test]
fn bound_instantiation_diagnostic_uses_call_span() {
    let source = "package a\ntrait Required =\n    function value(self): Int32\nfunction read<T>(value: T): Int32 where T: Required = value.value()\nfunction main(): Unit =\n    let result = read(7)\n";
    let checked = dovetail::check(source, "test.dove");
    let bound = checked.diagnostics.iter().find(|d| d.message.contains("does not implement trait")).expect("bound error");
    assert_eq!(bound.span.line, 6);
    assert!(bound.span.column > 1);
}

#[test]
fn inapplicable_generic_extension_reference_falls_back_to_inherited_impl() {
    common::compile_and_run(r#"
package a
import a.E
record Box<T> = value: T
trait Required =
    function required(self): Int32
extension E<T> for Box<T> where T: Required =
    function read(self): Int32 = 1
trait Read =
    function read(self): Int32
trait Extended extends Read =
    function extra(self): Int32
implement <T> Extended for Box<T> =
    function read(self): Int32 = 7
    function extra(self): Int32 = 0
function main(): Unit =
    let value = Box { value = 3 }
    let read = value.read
    assert read() == 7
"#).unwrap();
}

#[test]
fn generic_method_reference_infers_output_from_expected_function_type() {
    common::compile_and_run(r#"
package a
import a.E
record Box<T> = value: T
extension E<T> for Box<T> =
    function empty<U>(self): Option<U> = None
function main(): Unit =
    let value = Box { value = 3 }
    let empty: () => Option<Int32> = value.empty
    assert empty().isNone
"#).unwrap();
}

#[test]
fn module_static_wrong_arity_falls_through_to_trait() {
    common::compile_and_run(r#"
package a
record Value = n: Int32
module Value =
    function make(n: Int32): Int32 = n
trait Make =
    function make(): Int32
implement Make for Value =
    function make(): Int32 = 7
function main(): Unit = assert Value.make() == 7
"#).unwrap();
}

#[test]
fn bound_reference_prefers_direct_origin_and_supports_intersections() {
    common::compile_and_run(r#"
package a
record Box<T> = value: T
trait Read =
    function read(self): Int32
trait Extended extends Read =
    function extra(self): Int32
implement <T> Read for Box<T> =
    function read(self): Int32 = 1
implement <T> Extended for Box<T> =
    function read(self): Int32 = 2
    function extra(self): Int32 = 3
interface Left =
    function left(self): Int32
interface Right =
    function right(self): Int32
implement <T> Left for Box<T> =
    function left(self): Int32 = 4
implement <T> Right for Box<T> =
    function right(self): Int32 = 5
function main(): Unit =
    let box = Box { value = 7 }
    let read = box.read
    assert read() == 1
    let both: Left and Right = box
    let right = both.right
    assert right() == 5
"#).unwrap();
}
