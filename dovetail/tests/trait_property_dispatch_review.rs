mod common;

#[test]
fn method_default_preserves_inherited_property_slot() {
    common::compile_and_run(r#"
package a
trait Picker =
    function pick(self): Int32 = 42
class Parent =
    public property pick(self: Parent): Int32 = 10
class Child<X>(id: X) extends Parent() implements Picker
function choose<P>(p: P): Int32 where P: Picker = p.pick()
function read(parent: Parent): Int32 = parent.pick
function main(): Unit =
    let child = Child(1)
    assert choose(child) == 42
    assert read(child) == 10
"#).expect("trait method and inherited property keep separate virtual slots");
}
