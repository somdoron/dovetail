mod common;

#[test]
fn doc_comment_on_function() {
    common::compile_and_run(r#"
package a

/// A documented function
function main(): Unit = assert 1 + 1 == 2
"#)
    .expect("doc comments should not affect compilation");
}

#[test]
fn doc_comment_on_record() {
    common::compile_and_run(r#"
package a

/// A documented record
record Point =
    /// The x coordinate
    x: Int32
    /// The y coordinate
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert p.x == 1
"#)
    .expect("doc comments on records should not affect compilation");
}

#[test]
fn doc_comment_on_enum() {
    common::compile_and_run(r#"
package a

/// A color enum
enum Color =
    /// Red color
    Red
    /// Green color
    Green
    Blue

function main(): Unit =
    let c = Color.Red
    match c with
        case Color.Red => assert true
        case _ => assert false
"#)
    .expect("doc comments on enums should not affect compilation");
}

#[test]
fn doc_comment_on_trait() {
    common::compile_and_run(r#"
package a

/// A describable trait
trait Describable =
    /// Get a description
    function describe(self): String

record Foo

implement Describable for Foo =
    function describe(self): String = "foo"

function main(): Unit =
    let f = Foo {}
    assert f.describe() == "foo"
"#)
    .expect("doc comments on traits should not affect compilation");
}

#[test]
fn doc_comment_on_class() {
    common::compile_and_run(r#"
package a

/// A counter class
class Counter(private mutable count: Int32)

function main(): Unit =
    let c = Counter(0)
    assert true
"#)
    .expect("doc comments on classes should not affect compilation");
}

#[test]
fn doc_comment_on_module() {
    common::compile_and_run(r#"
package a

/// Math utilities
module Math =
    /// Add two numbers
    function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
    assert Math.add(1, 2) == 3
"#)
    .expect("doc comments on modules should not affect compilation");
}

#[test]
fn doc_comment_on_extension() {
    common::compile_and_run(r#"
package a

import a.FooExt

record Foo

/// Extension for Foo
extension FooExt for Foo =
    /// A helper method
    function hello(self): Int32 = 42

function main(): Unit =
    let f = Foo {}
    assert f.hello() == 42
"#)
    .expect("doc comments on extensions should not affect compilation");
}

#[test]
fn doc_comment_on_newtype() {
    common::compile_and_run(r#"
package a

/// Cents is a newtype for Int32
newtype Cents = Int32

function main(): Unit =
    let c = Cents(100)
    assert c.value == 100
"#)
    .expect("doc comments on newtypes should not affect compilation");
}

#[test]
fn doc_comment_on_global_variable() {
    common::compile_and_run(r#"
package a

/// The maximum value
let MAX: Int32 = 100

function main(): Unit = assert MAX == 100
"#)
    .expect("doc comments on globals should not affect compilation");
}

#[test]
fn doc_comment_on_type_alias() {
    common::compile_and_run(r#"
package a

/// An alias for Int32
type Number = Int32

function main(): Unit =
    let n: Number = 42
    assert n == 42
"#)
    .expect("doc comments on type aliases should not affect compilation");
}

#[test]
fn multiline_doc_comment() {
    common::compile_and_run(r#"
package a

/// This function does something.
/// It takes no arguments.
/// It returns Unit.
function main(): Unit = assert true
"#)
    .expect("multiline doc comments should not affect compilation");
}

#[test]
fn doc_comment_with_visibility() {
    common::compile_and_run(r#"
package a

/// A public function
public function helper(): Int32 = 42

function main(): Unit = assert helper() == 42
"#)
    .expect("doc comments with visibility should not affect compilation");
}
