mod common;

#[test]
fn identity_survives_mutation_inheritance_and_generic_forwarding() {
    common::compile_and_run(
        r#"
package identity
class Base(public mutable value: Int32)
class Child(public pair: (Int32, Int32)) extends Base(7)
class Box<out T>(public value: T)
function same<T>(a: T, b: T): Bool where T: class = ClassIdentity.equals(a, b)
function hash<T>(a: T): Int64 where T: class = ClassIdentity.hash(a)
function baseHash<T>(a: T): Int64 where T: Base = hash(a)
let global: Child = Child((3, 4))
function main(): Unit =
    let a = Child((3, 4))
    let b = Child((3, 4))
    let base: Base = a
    assert same(a, a)
    assert !same(a, b)
    assert ClassIdentity.equals<Base>(base, a)
    let h = hash(a)
    assert h == baseHash(base)
    a.value = 9
    assert a.value == 9
    assert a.pair._0 == 3
    assert hash(a) == h
    assert global.pair._1 == 4
    assert hash(global) != 0i64
    let box = Box<Child>(a)
    let wide: Box<Base> = box
    assert ClassIdentity.equals<Box<Base>>(box, wide)
    assert hash(box) == hash(wide)
"#,
    )
    .expect("class identity and common header");
}

#[test]
fn identity_traits_and_function_values_need_no_element_bounds() {
    common::compile_and_run(
        r#"
package identity
class Value()
class Box<T>(public value: T)
implement <T> Equatable for Box<T> =
    public function equals(self, other: Box<T>): Bool = ClassIdentity.equals(self, other)
implement <T> Hashable for Box<T> =
    public function hash(self): Int64 = ClassIdentity.hash(self)
function forwardedHash<T>(value: T): Int64 where T: class =
    let hash: (T) => Int64 = ClassIdentity.hash
    hash(value)
function main(): Unit =
    let a = Box(Value())
    let b = Box(Value())
    assert a == a
    assert a != b
    let equal: (Box<Value>, Box<Value>) => Bool = ClassIdentity.equals<Box<Value>>
    let hash: (Box<Value>) => Int64 = ClassIdentity.hash<Box<Value>>
    let inferredHash: (Box<Value>) => Int64 = ClassIdentity.hash
    let inferredEqual: (Box<Value>, Box<Value>) => Bool = ClassIdentity.equals
    assert equal(a, a)
    assert !equal(a, b)
    assert hash(a) == a.hash()
    assert inferredHash(a) == hash(a)
    assert forwardedHash(a) == hash(a)
    assert inferredEqual(a, a)
    assert !inferredEqual(a, b)
"#,
    )
    .expect("explicit identity implementations and adapters");
}

#[test]
fn class_constraint_rejects_non_classes_and_unbounded_forwarding() {
    for source in [
        "function main(): Unit = assert ClassIdentity.equals(1, 1)",
        "function main(): Unit = assert ClassIdentity.hash(1) == 0i64",
        "record Value = x: Int32\nfunction main(): Unit = assert ClassIdentity.equals(Value { x = 1 }, Value { x = 1 })",
        "class Value()\nnewtype Wrapped = Value\nfunction main(): Unit = assert ClassIdentity.hash(Wrapped(Value())) == 0i64",
        "function bad<T>(a: T): Int64 = ClassIdentity.hash(a)\nfunction main(): Unit = ()",
        "function bad<T>(a: T): Int64 where T: Hashable = ClassIdentity.hash(a)\nfunction main(): Unit = ()",
        "function main(): Unit =\n    let a: Any = 1\n    assert ClassIdentity.hash(a) == 0i64",
        "function main(): Unit = assert ClassIdentity.hash(Array<Int32>.empty()) == 0i64",
        "enum Choice = A\nfunction main(): Unit = assert ClassIdentity.hash(Choice.A) == 0i64",
        "interface View = function view(self): Int32\nfunction bad(a: View): Int64 = ClassIdentity.hash(a)\nfunction main(): Unit = ()",
        "function main(): Unit =\n    let hash: (Int32) => Int64 = ClassIdentity.hash<Int32>\n    ()",
        "class Holder<T>(public value: T) where T: class\nfunction main(): Unit =\n    let a = Holder(1)\n    ()",
        "record Holder<T> where T: class = value: T\nfunction main(): Unit =\n    let a = Holder { value = 1 }\n    ()",
        "function hash<T>(a: T): Int64 where T: class = ClassIdentity.hash(a)\nfunction main(): Unit =\n    let bad: (Int32) => Int64 = hash\n    ()",
        "function main(): Unit =\n    let bad: (Int32) => Int64 = ClassIdentity.hash\n    ()",
        "function bad<T>(a: T): Int64 where T: Equatable = ClassIdentity.hash(a)\nfunction main(): Unit = ()",
        "function main(): Unit = assert ClassIdentity.hash((1, 2)) == 0i64",
        "record Holder<T> where T: class = value: T\nextension MissingBound<T> for Holder<T> =\n    function get(self): T = self.value\nfunction main(): Unit = ()",
        "class Base()\nfunction needsBase<T>(a: T): Unit where T: Base = ()\nfunction bad<T>(a: T): Unit where T: class = needsBase(a)\nfunction main(): Unit = ()",
    ] {
        let result = dovetail::check(&format!("package identity\n{source}\n"), "identity.dove");
        assert!(result.diagnostics.has_errors(), "accepted {source}");
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("class")),
            "missing class diagnostic for {source}"
        );
    }
}

#[test]
fn mixed_bounds_aliases_and_identity_independent_of_structural_equality() {
    common::compile_and_run(
        r#"
package identity
class Item(public mutable value: Int32)
type Alias = Item
implement Equatable for Item =
    public function equals(self, other: Item): Bool = self.value == other.value
function same<T>(a: T, b: T): Bool where T: class + Equatable =
    assert a == b
    ClassIdentity.equals(a, b)
function main(): Unit =
    let a: Alias = Item(1)
    let b = Item(1)
    assert !same(a, b)
    assert same(a, a)
    let mutable calls = 0
    let next: () => Item = () =>
        calls = calls + 1
        a
    assert ClassIdentity.equals(next(), next())
    assert calls == 2
    let h = ClassIdentity.hash(next())
    assert calls == 3
    assert h == ClassIdentity.hash(a)
    let ordered: (Int32) => Item = (expected: Int32) =>
        assert calls == expected
        calls = calls + 1
        a
    assert ClassIdentity.equals(ordered(3), ordered(4))
    assert calls == 5
"#,
    )
    .expect("mixed bounds and raw identity");
}

#[test]
fn class_bounds_on_types_modules_and_conditional_implementations() {
    common::compile_and_run(
        r#"
package identity
import identity.HolderIdentity
class Item()
record Holder<T> where T: class = value: T
enum Choice<T> where T: class = Some(T)
module Holder<T> =
    public function same(self, other: Holder<T>): Bool = ClassIdentity.equals(self.value, other.value)
extension HolderIdentity<T> for Holder<T> where T: class =
    function identityValue(self): Int64 = ClassIdentity.hash(self.value)
class Wrapper<T>(public value: T)
implement <T> Hashable for Wrapper<T> where T: class =
    public function hash(self): Int64 = ClassIdentity.hash(self.value)
function needsHash<T>(a: T): Int64 where T: Hashable = a.hash()
function main(): Unit =
    let a = Item()
    let held = Holder { value = a }
    assert held.same(held)
    assert held.identityValue() == ClassIdentity.hash(a)
    assert needsHash(Wrapper(a)) == ClassIdentity.hash(a)
    let choice: Choice<Item> = Choice.Some(a)
    match choice with
    case Choice.Some(value) => assert ClassIdentity.equals(a, value)
"#,
    )
    .expect("class category across generic declarations");
    let errors = common::compile_expecting_errors(
        r#"
package identity
class Wrapper<T>(public value: T)
implement <T> Hashable for Wrapper<T> where T: class =
    public function hash(self): Int64 = ClassIdentity.hash(self.value)
function needsHash<T>(a: T): Int64 where T: Hashable = a.hash()
function main(): Unit = assert needsHash(Wrapper(1)) == 0i64
"#,
    );
    assert!(errors.iter().any(|message| message.contains("Hashable")));
}

#[test]
fn unsupported_intrinsic_function_values_report_errors_instead_of_panicking() {
    for binding in [
        "let fill: (Int32, Int32) => Array<Int32> = Array<Int32>.fill",
        "let fill: (Int32, Int32) => Array<Int32> = Array.fill",
        "let empty: () => Array<Int32> = Array<Int32>.empty",
        "let empty: () => Array<Int32> = Array.empty",
    ] {
        let source = format!("package identity\nfunction main(): Unit =\n    {binding}\n    ()\n");
        let errors = common::compile_expecting_errors(&source);
        assert!(errors.iter().any(|message| {
            message.contains("intrinsic") && message.contains("wrap the call in a lambda")
        }), "missing intrinsic function-value diagnostic: {errors:?}");
    }
    common::compile_and_run(
        r#"
package identity
record Factory<T> = value: T
module Factory<T> =
    public function empty(): Array<T> = Array<T>.empty()
    public property count: Int32 = 7
function main(): Unit =
    let fill: (Int32, Int32) => Array<Int32> = (count: Int32, value: Int32) => Array.fill(count, value)
    assert fill(2, 5).get(0) == 5
    let empty: () => Array<Int32> = Factory<Int32>.empty
    assert empty().length == 0
    assert Factory<Int32>.count == 7
"#,
    ).expect("direct intrinsic calls can be wrapped in lambdas");
}

#[test]
fn new_book_examples_compile_and_run() {
    for (book, marker, main) in [
        (
            include_str!("../../book/09-classes.md"),
            "class IdentityCounter",
            "checkIdentity()",
        ),
        (
            include_str!("../../book/08-traits.md"),
            "class IdentityBox",
            "checkBoxIdentity()\n    let a = IdentityBox(1)\n    assert a == a\n    assert a.hash() == ClassIdentity.hash(a)",
        ),
        (
            include_str!("../../book/26-advanced-generics.md"),
            "function sameInstance",
            "let a = Item()\n    assert sameInstance(a, a)\n    assert instanceHash(a) == ClassIdentity.hash(a)",
        ),
    ] {
        let example = book
            .split("```dovetail\n")
            .skip(1)
            .find(|s| s.starts_with(marker))
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        common::compile_and_run(&format!(
            "package identity\nclass Item()\n{example}\nfunction main(): Unit =\n    {main}\n"
        ))
        .expect(marker);
    }
}
