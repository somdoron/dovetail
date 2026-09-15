mod common;

// ── bare self shorthand ──────────────────────────────────────────

#[test]
fn test_bare_self_in_trait_method() {
    common::compile_and_run(
        r#"
package a

trait Foo =
    function bar(self): Int32

class Baz() implements Foo =
    public function bar(self): Int32 = 42

function main(): Unit =
    let b = Baz()
    assert b.bar() == 42
"#,
    )
    .expect("bare self in trait method");
}

#[test]
fn test_bare_self_in_class_method() {
    common::compile_and_run(
        r#"
package a

class Counter(public value: Int32) =
    public function doubled(self): Int32 = self.value * 2

function main(): Unit =
    let c = Counter(5)
    assert c.doubled() == 10
"#,
    )
    .expect("bare self in class method");
}

#[test]
fn test_bare_self_in_abstract_class_method() {
    common::compile_and_run(
        r#"
package a

abstract class Base() =
    public abstract function value(self): Int32

class Child() extends Base() =
    public override function value(self): Int32 = 99

function main(): Unit =
    let c = Child()
    assert c.value() == 99
"#,
    )
    .expect("bare self in abstract class method");
}

// ── field mutation ───────────────────────────────────────────────

#[test]
fn test_field_mutation() {
    common::compile_and_run(
        r#"
package a

class Counter(public mutable value: Int32) =
    public function increment(self): Unit =
        self.value = self.value + 1

function main(): Unit =
    let c = Counter(0)
    c.increment()
    assert c.value == 1
    c.increment()
    assert c.value == 2
"#,
    )
    .expect("field mutation via self.field = value");
}

#[test]
fn test_field_mutation_immutable_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Pair(public x: Int32, public y: Int32) =
    public function setX(self): Unit =
        self.x = 10

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot assign to immutable field")),
        "expected immutable field error, got: {:?}",
        errors
    );
}

// ── ArrayIterator and Iterable ───────────────────────────────────

#[test]
fn test_array_iterator_not_accessible() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let arr = [|10, 20, 30|]
    let iter = ArrayIterator<Int32>(arr, 0)
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("undefined") || e.contains("unknown")),
        "expected error when referencing internal ArrayIterator, got: {:?}",
        errors
    );
}

#[test]
fn test_array_iterator_via_trait() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3|]
    let iter = arr.iterator()
    assert iter.next().require == 1
    assert iter.next().require == 2
    assert iter.next().require == 3
    let done = iter.next()
    match done with
        case None => ()
        case Some(_) => panic "expected None"
"#,
    )
    .expect("Array.iterator() via Iterable trait");
}

#[test]
fn test_while_loop_iteration() {
    common::compile_and_run(
        r#"
package a

function sum_array(arr: Array<Int32>): Int32 =
    let iter = arr.iterator()
    let mutable sum = 0
    let mutable item = iter.next()
    while item.isSome do
        sum = sum + item.require
        item = iter.next()
    sum

function main(): Unit =
    assert sum_array([|1, 2, 3, 4, 5|]) == 15
"#,
    )
    .expect("full iteration using while loop");
}
