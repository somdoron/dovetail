mod common;

// ── Basic global variables ──────────────────────────────────────────

#[test]
fn test_global_with_explicit_type() {
    common::compile_and_run(
        r#"
package a

let x: Int32 = 42

function main(): Unit = assert x == 42
"#,
    )
    .expect("global with explicit type");
}

#[test]
fn test_global_with_inferred_type() {
    common::compile_and_run(
        r#"
package a

let x = 42

function main(): Unit = assert x == 42
"#,
    )
    .expect("global with inferred type");
}

#[test]
fn test_global_referencing_another_global() {
    common::compile_and_run(
        r#"
package a

let x = 10
let y = x + 5

function main(): Unit = assert y == 15
"#,
    )
    .expect("global referencing another global");
}

#[test]
fn test_global_bool() {
    common::compile_and_run(
        r#"
package a

let flag: Bool = true

function main(): Unit = assert flag
"#,
    )
    .expect("global bool");
}

#[test]
fn test_global_used_in_function() {
    common::compile_and_run(
        r#"
package a

let x: Int32 = 42

function check(): Bool = x == 42

function main(): Unit = assert check()
"#,
    )
    .expect("global used in function");
}

#[test]
fn test_global_with_function_call_initializer() {
    common::compile_and_run(
        r#"
package a

function compute(): Int32 = 7 * 6

let result: Int32 = compute()

function main(): Unit = assert result == 42
"#,
    )
    .expect("global with function call initializer");
}

#[test]
fn test_reverse_declaration_order() {
    common::compile_and_run(
        r#"
package a

let y = x + 1
let x = 5

function main(): Unit = assert y == 6
"#,
    )
    .expect("reverse declaration order");
}

#[test]
fn test_multiple_globals() {
    common::compile_and_run(
        r#"
package a

let a: Int32 = 1
let b: Int32 = 2
let c: Int32 = 3

function main(): Unit = assert a + b + c == 6
"#,
    )
    .expect("multiple globals");
}

// ── Mutable globals ────────────────────────────────────────────────

#[test]
fn test_mutable_global_read_initial() {
    common::compile_and_run(
        r#"
package a

let mutable counter = 0

function main(): Unit = assert counter == 0
"#,
    )
    .expect("mutable global read initial");
}

#[test]
fn test_mutable_global_assignment() {
    common::compile_and_run(
        r#"
package a

let mutable counter: Int32 = 0

function main(): Unit =
    counter = counter + 1
    assert counter == 1
"#,
    )
    .expect("mutable global assignment");
}

#[test]
fn test_mutable_global_multiple_assignments() {
    common::compile_and_run(
        r#"
package a

let mutable x: Int32 = 0

function main(): Unit =
    x = 10
    x = x + 5
    assert x == 15
"#,
    )
    .expect("mutable global multiple assignments");
}

#[test]
fn test_mutable_global_with_explicit_type() {
    common::compile_and_run(
        r#"
package a

let mutable flag: Bool = false

function main(): Unit =
    flag = true
    assert flag
"#,
    )
    .expect("mutable global with explicit type");
}

#[test]
fn test_mutable_global_modified_in_function() {
    common::compile_and_run(
        r#"
package a

let mutable count: Int32 = 0

function increment(): Unit =
    count = count + 1

function main(): Unit =
    increment()
    increment()
    assert count == 2
"#,
    )
    .expect("mutable global modified in function");
}

// ── Record globals ────────────────────────────────────────────────

#[test]
fn test_global_record() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let origin: Point = Point { x = 0; y = 0 }

function main(): Unit =
    assert origin.x == 0
    assert origin.y == 0
"#,
    )
    .expect("global record");
}

#[test]
fn test_global_record_field_access_in_expression() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let p: Point = Point { x = 3; y = 4 }

function main(): Unit = assert p.x + p.y == 7
"#,
    )
    .expect("global record field access in expression");
}

#[test]
fn test_global_record_passed_to_function() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let p: Point = Point { x = 10; y = 20 }

function sum(pt: Point): Int32 = pt.x + pt.y

function main(): Unit = assert sum(p) == 30
"#,
    )
    .expect("global record passed to function");
}

#[test]
fn test_global_record_referencing_another_global() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let base: Point = Point { x = 1; y = 2 }
let shifted: Point = Point { x = base.x + 10; y = base.y + 10 }

function main(): Unit =
    assert shifted.x == 11
    assert shifted.y == 12
"#,
    )
    .expect("global record referencing another global");
}

#[test]
fn test_global_record_with_expression() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let p: Point = Point { x = 1; y = 2 }
let q: Point = p with { x = 10 }

function main(): Unit =
    assert q.x == 10
    assert q.y == 2
"#,
    )
    .expect("global record with expression");
}

#[test]
fn test_mutable_global_record() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

let mutable pos: Point = Point { x = 0; y = 0 }

function main(): Unit =
    pos = Point { x = 5; y = 10 }
    assert pos.x == 5
    assert pos.y == 10
"#,
    )
    .expect("mutable global record");
}

#[test]
fn test_global_record_with_nested_record() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

record Line =
    start: Point
    end: Point

let line: Line = Line { start = Point { x = 0; y = 0 }; end = Point { x = 10; y = 20 } }

function main(): Unit =
    assert line.start.x == 0
    assert line.end.x == 10
    assert line.end.y == 20
"#,
    )
    .expect("global record with nested record");
}

// ── Error cases ────────────────────────────────────────────────────

#[test]
fn test_immutable_global_assignment_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

let x: Int32 = 5

function main(): Unit = x = 10
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot assign to immutable global")),
        "expected immutable global error, got: {:?}",
        errors
    );
}

#[test]
fn test_type_mismatch_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

let x: Bool = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_duplicate_global_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

let x: Int32 = 1
let x: Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate global")),
        "expected duplicate global error, got: {:?}",
        errors
    );
}

// ── Global types ───────────────────────────────────────────────────

#[test]
fn test_global_int64() {
    common::compile_and_run(
        r#"
package a

let big: Int64 = 1000000i64

function main(): Unit = assert big == 1000000i64
"#,
    )
    .expect("global int64");
}

#[test]
fn test_global_float64() {
    common::compile_and_run(
        r#"
package a

let pi: Float64 = 3.14

function main(): Unit = assert pi == 3.14
"#,
    )
    .expect("global float64");
}

#[test]
fn test_private_global_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

private let SECRET: Int32 = 42

function main(): Unit = assert SECRET == 42
"#,
    )
    .expect("private global visible in same file");
}

#[test]
fn test_global_if_else_initializer() {
    common::compile_and_run(
        r#"
package a

let x: Int32 = if true then 1 else 2

function main(): Unit = assert x == 1
"#,
    )
    .expect("global with if-else initializer");
}

// ── Initializer ordering through function calls ────────────────────
//
// The dependency edges for the initializer sort must follow FUNCTION CALLS,
// not only GlobalRefs syntactically present in the initializer: a global whose
// initializer reads another global through a helper still needs that global
// first. Ref-typed globals default to ref.null and reads emit ref.as_non_null,
// so a missed edge is a trap at component start. The names below are chosen so
// that, without call-following, the blind sort runs `derived` first — `base`
// sorts earlier in key order and the ready set was drained from the wrong end.

#[test]
fn test_global_initialized_through_function_call_reading_ref_global() {
    common::compile_and_run(
        r#"
package a

class Counter(public mutable next: Int32)

let base: Counter = Counter(7)

function readBase(): Int32 = base.next

let derived: Int32 = readBase()

function main(): Unit = assert derived == 7
"#,
    )
    .expect("global initialized through a call that reads a ref-typed global");
}

#[test]
fn test_global_initialized_through_call_chain() {
    common::compile_and_run(
        r#"
package a

class Holder(public mutable value: Int32)

let anchor: Holder = Holder(31)

function inner(): Int32 = anchor.value

function outer(): Int32 = inner() + 1

let zFollower: Int32 = outer()

function main(): Unit = assert zFollower == 32
"#,
    )
    .expect("global initialized through a two-deep call chain");
}

// ClassNew EMISSION inlines the whole class hierarchy at the construction
// site: extends-args, the parent chain, and every class-body `let` field
// initializer all execute during global initialization. The dependency walker
// must follow the same paths — constructor ARGS alone are not enough. As
// above, names are adversarial: the reading global sorts before the global it
// reads, so a missing edge means initializing from `ref.null` (trap) or a
// zeroed scalar.

#[test]
fn test_global_initialized_through_class_body_field() {
    common::compile_and_run(
        r#"
package a

class Holder() =
    let v: String = zSource
    public function get(self): String = self.v

let aReader: Holder = Holder()

let zSource: String = "hello"

function main(): Unit = assert aReader.get() == "hello"
"#,
    )
    .expect("global initialized through a class BODY field reading a ref-typed global");
}

#[test]
fn test_global_initialized_through_extends_args() {
    common::compile_and_run(
        r#"
package a

class Parent(name: String) =
    public function getName(self): String = self.name

class Child() extends Parent(zSource)

let aChild: Parent = Child()

let zSource: String = "world"

function main(): Unit = assert aChild.getName() == "world"
"#,
    )
    .expect("global initialized through a subclass's extends-args reading a ref-typed global");
}

#[test]
fn test_global_initialized_through_parent_body_field() {
    common::compile_and_run(
        r#"
package a

class Base() =
    let stored: String = zSource
    public function get(self): String = self.stored

class Derived() extends Base()

let aDerived: Derived = Derived()

let zSource: String = "deep"

function main(): Unit = assert aDerived.get() == "deep"
"#,
    )
    .expect("global initialized through a PARENT class's body field, two-level hierarchy");
}

#[test]
fn test_global_initialized_through_call_then_class_body_field() {
    common::compile_and_run(
        r#"
package a

class Wrapper() =
    let inner: String = zTail
    public function get(self): String = self.inner

function makeWrapper(): Wrapper = Wrapper()

let aHead: String = makeWrapper().get()

let zTail: String = "chain"

function main(): Unit = assert aHead == "chain"
"#,
    )
    .expect("global initialized through function call -> ClassNew -> body field -> global");
}
