mod common;

#[test]
fn bare_type_parameter_implementation_reports_unsupported() {
    let errors = common::compile_expecting_errors(r#"
package a
trait Tag =
    function tag(self): Int32
implement <T> Tag for T =
    function tag(self): Int32 = 1
function main(): Unit = ()
"#);
    assert!(errors.iter().any(|error| error.contains("implement blocks for a bare type parameter are not supported")), "{errors:?}");
}

#[test]
fn generic_impl_parameter_does_not_capture_trait_parameter() {
    common::compile_and_run(r#"
package a
record Box<T> =
    value: T
trait Conv<T> =
    function conv(self, value: T): T
implement <T> Conv<Int32> for Box<T> =
    function conv(self, value: Int32): Int32 = value + 1
function main(): Unit =
    let b = Box { value = "hello" }
    assert b.conv(41) == 42
"#).expect("the trait's T substitution must not capture the impl's unrelated T");
}

#[test]
fn inherited_generic_method_bounds_substitute_trait_parameters() {
    common::compile_and_run(r#"
package a
trait Wanted<T>
record Arg =
    value: Int32
implement Wanted<Int32> for Arg
trait Chooser<T> =
    function choose<U>(self, value: U): Int32 where U: Wanted<T>
trait Child extends Chooser<Int32>
record Rec =
    value: Int32
implement Child for Rec =
    function choose<U>(self, value: U): Int32 where U: Wanted<Int32> = 42
function choose<R, U>(receiver: R, value: U): Int32 where R: Child, U: Wanted<Int32> = receiver.choose(value)
function main(): Unit =
    assert choose(Rec { value = 0 }, Arg { value = 0 }) == 42
"#).expect("inherited generic method bounds use the chosen supertrait application");
}

#[test]
fn class_overloads_by_arity_dispatch_correctly() {
    common::compile_and_run(r#"
package a
class K() =
    public function value(self): Int32 = 1
    public function value(self, extra: Int32): Int32 = extra + 1
function main(): Unit =
    let k = K()
    assert k.value() == 1
    assert k.value(41) == 42
"#).expect("class overloads select their matching arity");
}

#[test]
fn class_overloads_keep_inherited_virtual_slots() {
    common::compile_and_run(r#"
package a
class Base() =
    public function value(self): Int32 = 1
    public function value(self, extra: Int32): Int32 = extra + 1
class Derived() extends Base() =
    public override function value(self): Int32 = 7
function main(): Unit =
    let k: Base = Derived()
    assert k.value() == 7
    assert k.value(41) == 42
"#).expect("overriding one arity retains the sibling inherited slot");
}

#[test]
fn generic_class_overloads_select_matching_arity() {
    common::compile_and_run(r#"
package a
class K<T>(stored: T) =
    public function value(self): T = self.stored
    public function value(self, extra: Int32): Int32 = extra + 1
function main(): Unit =
    let k = K("hello")
    assert k.value() == "hello"
    assert k.value(41) == 42
"#).expect("generic class virtual calls keep each arity's parameter layout");
}

#[test]
fn class_default_materialization_selects_matching_overload() {
    common::compile_and_run(r#"
package a
trait Greeter =
    function greet(self): Int32 = 42
class K() implements Greeter =
    public function greet(self, extra: Int32): Int32 = extra + 1
function main(): Unit =
    let k = K()
    assert k.greet() == 42
    assert k.greet(1) == 2
"#).expect("an existing overload does not hide the omitted defaulted member");
}

#[test]
fn class_defaults_call_the_matching_virtual_arity() {
    common::compile_and_run(r#"
package a
trait Counted =
    function value(self): Int32
    function answer(self): Int32 = self.value()
class K() implements Counted =
    public function value(self): Int32 = 42
    public function value(self, extra: Int32): Int32 = extra + 1
function main(): Unit =
    assert K().answer() == 42
"#).expect("default self calls select their declared virtual arity");
}

#[test]
fn interface_coercion_selects_matching_class_overload_arity() {
    common::compile_and_run(r#"
package a
interface Valued =
    function value(self, extra: Int32): Int32
class K() implements Valued =
    public function value(self): Int32 = 1
    public function value(self, extra: Int32): Int32 = extra + 1
function main(): Unit =
    let k: Valued = K()
    assert k.value(41) == 42
"#).expect("an interface wrapper selects the class overload matching its declaration");
}

#[test]
fn class_sibling_trait_applications_do_not_miscompile() {
    let errors = common::compile_expecting_errors(r#"
package a
interface A<T> =
    function value(self, argument: T): Int32
class K() implements A<Int32> and A<String> =
    public function value(self, argument: Int32): Int32 = argument
    public function value(self, argument: String): Int32 = argument.length
function main(): Unit =
    let k = K()
    let integers: A<Int32> = k
    let strings: A<String> = k
    assert integers.value(42) == 42
    assert strings.value("hi") == 2
"#);
    assert!(errors.iter().any(|error| error.contains("cannot implement different applications of trait 'A' that require overloaded members")), "{errors:?}");
}
