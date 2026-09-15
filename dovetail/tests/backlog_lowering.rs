mod common;

#[test]
fn inline_class_implementation_wins_over_an_external_provider() {
    common::compile_and_run(
        r#"
package a
interface Read =
    function read(self): Int32
    function again(self): Self
interface Extended extends Read =
    function extra(self): Int32
class C(value: Int32) implements Read =
    public function read(self: C): Int32 = 1
    public function again(self: C): C = self
implement Extended for C =
    function read(self: C): Int32 = 2
    function again(self: C): C = self
    function extra(self: C): Int32 = 3
function main(): Unit =
    let direct: Read = C(0)
    assert direct.read() == 1
    let provided: Extended = C(0)
    assert provided.read() == 2
    assert provided.again().read() == 1
"#,
    )
    .unwrap();
}

#[test]
fn class_implementation_can_be_boxed_as_interface() {
    common::compile_and_run(
        r#"
package a
interface Read =
    function read(self): Int32
class C(public value: Int32)
implement Read for C =
    function read(self: C): Int32 = self.value
interface Extended extends Read =
    function extra(self): Int32
class Generic<T>(public value: T)
implement <T> Extended for Generic<T> =
    function read(self: Generic<T>): Int32 = 9
    function extra(self: Generic<T>): Int32 = 1
function main(): Unit =
    let value: Read = C(7)
    assert value.read() == 7
    let generic: Read = Generic<String>("ok")
    assert generic.read() == 9
"#,
    )
    .unwrap();
}

#[test]
fn partially_erased_tuple_payloads_preserve_each_leaf() {
    common::compile_and_run(
        r#"
package a
enum Holder<T> =
    Wrap((T, Int32))
record Field<T> =
    pair: (T, Int32)
function main(): Unit =
    let field = Field<(Int32, Bool)> { pair = ((3, true), 4) }
    assert field.pair._0._0 == 3
    assert field.pair._0._1
    assert field.pair._1 == 4
    let scalar = Field<Int32> { pair = (5, 6) }
    assert scalar.pair._0 + scalar.pair._1 == 11
    let h: Holder<Int64> = Holder.Wrap((7i64, 9))
    match h with
    case Holder.Wrap((item, n)) => assert item == 7i64 && n == 9
"#,
    )
    .unwrap();
}

#[test]
fn tuple_globals_initialize_and_read() {
    common::compile_and_run(
        r#"
package a
function makePair(): (Int32, Bool) = (7, true)
let pair: (Int32, Bool) = makePair()
function main(): Unit =
    let (a, b) = pair
    assert a == 7 && b
"#,
    )
    .unwrap();
}

#[test]
fn class_initializers_elaborate_interface_coercions() {
    common::compile_and_run(
        r#"
package a
interface Read =
    function read(self): Int32
record Item =
    value: Int32
implement Read for Item =
    function read(self: Item): Int32 = self.value
class C(value: Int32) =
    public let item: Read = Item { value = value }
class Generic<T>(value: Int32, extra: T) =
    public let item: Read = Item { value = value }
class Base(public item: Read)
class Derived(value: Int32) extends Base(Item { value = value })
function main(): Unit =
    assert C(7).item.read() == 7
    assert Generic<String>(8, "ok").item.read() == 8
    assert Derived(9).item.read() == 9
"#,
    )
    .unwrap();
}

#[test]
fn interface_type_patterns_downcast_the_concrete_value() {
    common::compile_and_run(
        r#"
package a
interface Read =
    function read(self): Int32
record Item =
    value: Int32
implement Read for Item =
    function read(self: Item): Int32 = self.value
implement Read for Int32 =
    function read(self: Int32): Int32 = self
function extract(item: Read): Int32 =
    match item with
    case value: Item => value.value
    case value: Int32 => value
    case _ => 0
function main(): Unit =
    assert extract(Item { value = 7 }) == 7
    assert extract(8) == 8
"#,
    )
    .unwrap();
}

#[test]
fn use_materializes_generic_method_on_instantiated_receiver() {
    common::compile_and_run(r#"
package a
record Wrap<T> =
    value: T
implement Usable<Int32, Never> for Wrap<Int32> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Wrap<Int32>, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)
function main(): Unit =
    let value = use Wrap<Int32> { value = 7 }
    assert value == 7
"#).unwrap();
}
