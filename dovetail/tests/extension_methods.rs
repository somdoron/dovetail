mod common;

// ── Basic extension method ──────────────────────────────────────────

#[test]
fn test_basic_extension_method_on_int32() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

function main(): Unit = assert 5.double() == 10
"#,
    )
    .expect("basic extension method on Int32");
}

// ── Multiple methods in one extension ───────────────────────────────

#[test]
fn test_multiple_methods_in_extension() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2
    function isZero(self): Bool = self == 0

function main(): Unit =
    assert 5.double() == 10
    assert 0.isZero() == true
"#,
    )
    .expect("multiple methods in one extension");
}

// ── Method with additional parameters ───────────────────────────────

#[test]
fn test_extension_method_with_extra_params() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function add(self, other: Int32): Int32 = self + other

function main(): Unit = assert 3.add(4) == 7
"#,
    )
    .expect("extension method with additional parameters");
}

// ── Chained method calls ────────────────────────────────────────────

#[test]
fn test_chained_extension_method_calls() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

function main(): Unit = assert 3.double().double() == 12
"#,
    )
    .expect("chained extension method calls");
}

// ── Extension method on Bool ────────────────────────────────────────

#[test]
fn test_extension_method_on_bool() {
    common::compile_and_run(
        r#"
package a

import a.BoolExt

extension BoolExt for Bool =
    function toInt(self): Int32 = if self then 1 else 0

function main(): Unit = assert true.toInt() == 1
"#,
    )
    .expect("extension method on Bool");
}

// ── Extension with variable receiver ────────────────────────────────

#[test]
fn test_extension_method_with_variable_receiver() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

function main(): Unit =
    let x = 7
    assert x.double() == 14
"#,
    )
    .expect("extension method with variable receiver");
}

// ── Static extension methods ─────────────────────────────────────────

#[test]
fn test_static_extension_method_no_params() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function zero(): Int32 = 0

function main(): Unit = assert Int32.zero() == 0
"#,
    )
    .expect("static extension method with no params");
}

#[test]
fn test_static_extension_method_with_params() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function fromBool(b: Bool): Int32 = if b then 1 else 0

function main(): Unit = assert Int32.fromBool(true) == 1
"#,
    )
    .expect("static extension method with params");
}

#[test]
fn test_mixed_instance_and_static_extension_methods() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2
    function zero(): Int32 = 0

function main(): Unit =
    assert Int32.zero() == 0
    assert 5.double() == 10
"#,
    )
    .expect("mixed instance and static extension methods");
}

#[test]
fn test_static_extension_method_on_record() {
    common::compile_and_run(
        r#"
package a

import a.PointExt

record Point =
    x: Int32
    y: Int32

function makeOrigin(): Point = Point { x = 0; y = 0 }

extension PointExt for Point =
    function origin(): Point = makeOrigin()

function main(): Unit =
    let p = Point.origin()
    assert p.x == 0
"#,
    )
    .expect("static extension method on record type");
}

#[test]
fn test_static_factory_chained_with_instance_method() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function ten(): Int32 = 10
    function double(self): Int32 = self * 2

function main(): Unit = assert Int32.ten().double() == 20
"#,
    )
    .expect("static factory chained with instance method");
}

#[test]
fn test_error_static_method_called_on_instance() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function zero(): Int32 = 0

function main(): Unit =
    let x = 5
    x.zero()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method, got: {:?}",
        errors
    );
}

#[test]
fn test_error_instance_method_called_on_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

function main(): Unit = Int32.double()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no static method")),
        "expected error about no static method, got: {:?}",
        errors
    );
}

// ── Named extension methods ─────────────────────────────────────────

#[test]
fn test_named_extension_same_package_with_import() {
    common::compile_and_run(
        r#"
package a

import a.IntMath

extension IntMath for Int32 =
    function cube(self): Int32 = self * self * self

function main(): Unit = assert 2.cube() == 8
"#,
    )
    .expect("named extension with import");
}

#[test]
fn test_named_extension_not_imported_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

extension IntMath for Int32 =
    function cube(self): Int32 = self * self * self

function main(): Unit = 2.cube()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method, got: {:?}",
        errors
    );
}

#[test]
fn test_named_extension_static_method() {
    common::compile_and_run(
        r#"
package a

import a.IntFactory

extension IntFactory for Int32 =
    function zero(): Int32 = 0

function main(): Unit = assert Int32.zero() == 0
"#,
    )
    .expect("named extension with static method");
}

#[test]
fn test_two_named_extensions_on_same_type() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext
import a.IntExtra

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

extension IntExtra for Int32 =
    function triple(self): Int32 = self * 3

function main(): Unit =
    assert 5.double() == 10
    assert 5.triple() == 15
"#,
    )
    .expect("two named extensions on same type");
}

#[test]
fn test_named_extension_mixed_instance_and_static() {
    common::compile_and_run(
        r#"
package a

import a.IntOps

extension IntOps for Int32 =
    function negate(self): Int32 = 0 - self
    function one(): Int32 = 1

function main(): Unit =
    assert 5.negate() == -5
    assert Int32.one() == 1
"#,
    )
    .expect("named extension mixed instance and static");
}

// ── Overloaded extension methods ──────────────────────────────────────

#[test]
fn test_overloaded_extension_methods_different_params() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    function apply(self, x: Int32): Int32 = self + x
    function apply(self, x: Bool): Int32 = if x then self else 0 - self

function main(): Unit =
    assert 5.apply(3) == 8
    assert 5.apply(true) == 5
    assert 5.apply(false) == -5
"#,
    )
    .expect("overloaded extension methods with different params");
}

#[test]
fn test_ambiguous_extension_method_call() {
    // When both extensions are imported, both `process` overloads match → ambiguous call
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.Int32Ext
import a.Int32Ext2

extension Int32Ext for Int32 =
    function process(self, x: Int32): Int32 = self + x

extension Int32Ext2 for Int32 =
    function process(self, x: Int32): Int32 = self * x

function main(): Unit = 5.process(3)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous")),
        "expected error about ambiguous method call, got: {:?}",
        errors
    );
}

// ── Private extension methods ────────────────────────────────────────

#[test]
fn test_private_extension_method_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    private function secret(self): Int32 = self * 42

function main(): Unit = assert 5.secret() == 210
"#,
    )
    .expect("private extension method visible in same file");
}

#[test]
fn test_private_named_extension_method_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

import a.IntSecrets

extension IntSecrets for Int32 =
    private function hidden(self): Int32 = self + 100

function main(): Unit = assert 5.hidden() == 105
"#,
    )
    .expect("private named extension method visible in same file");
}

#[test]
fn test_named_extension_static_not_imported_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

extension IntFactory for Int32 =
    function zero(): Int32 = 0

function main(): Unit = Int32.zero()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no static method")),
        "expected error about no static method, got: {:?}",
        errors
    );
}

// ── Phase 5: Extension methods on all type kinds ─────────────────────

#[test]
fn test_extension_method_on_float64() {
    common::compile_and_run(
        r#"
package a

import a.Float64Ext

extension Float64Ext for Float64 =
    function isPositive(self): Bool = self > 0.0

function main(): Unit =
    assert 3.14.isPositive() == true
    assert (-1.0).isPositive() == false
"#,
    )
    .expect("extension method on Float64");
}

#[test]
fn test_extension_method_on_int64() {
    common::compile_and_run(
        r#"
package a

import a.Int64Ext

extension Int64Ext for Int64 =
    function doubled(self): Int64 = self + self

function main(): Unit = assert 5i64.doubled() == 10i64
"#,
    )
    .expect("extension method on Int64");
}

#[test]
fn test_instance_extension_method_on_record() {
    common::compile_and_run(
        r#"
package a

import a.PointExt

record Point =
    x: Int32
    y: Int32

extension PointExt for Point =
    function sum(self): Int32 = self.x + self.y

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.sum() == 7
"#,
    )
    .expect("instance extension method on record");
}

#[test]
fn test_named_extension_on_record_instance() {
    common::compile_and_run(
        r#"
package a

import a.PointOps

record Point =
    x: Int32
    y: Int32

extension PointOps for Point =
    function manhattan(self): Int32 = self.x + self.y

function main(): Unit =
    let p = Point { x = 5; y = 3 }
    assert p.manhattan() == 8
"#,
    )
    .expect("named extension with instance method on record");
}

// ── Phase 5: Future type kinds (ignored until implemented) ───────────

#[test]
fn test_extension_on_string() {
    common::compile_and_run(
        r#"
package a

import a.StringExt

extension StringExt for String =
    function isEmpty(self): Bool = self == ""

function main(): Unit = assert "hello".isEmpty() == false
"#,
    )
    .expect("extension method on String");
}

#[test]
#[ignore]
fn test_extension_on_enum() {
    common::compile_and_run(
        r#"
package a

import a.ColorExt

enum Color =
    Red
    Green
    Blue

extension ColorExt for Color =
    function isRed(self): Bool = match self with
        Color.Red -> true
        _ -> false
    function default(): Color = Color.Red

function main(): Unit =
    let c = Color.Red
    assert c.isRed() == true
    assert Color.default().isRed() == true
"#,
    )
    .expect("extension method on enum");
}

#[test]
#[ignore]
fn test_extension_on_class() {
    common::compile_and_run(
        r#"
package a

import a.PersonExt

class Person =
    name: String
    age: Int32

extension PersonExt for Person =
    function isAdult(self): Bool = self.age >= 18

function main(): Unit =
    let p = Person { name = "Alice"; age = 25 }
    assert p.isAdult() == true
"#,
    )
    .expect("extension method on class");
}

#[test]
#[ignore]
fn test_extension_on_newtype() {
    common::compile_and_run(
        r#"
package a

import a.UserIdExt

newtype UserId = Int32

extension UserIdExt for UserId =
    function toInt(self): Int32 = self.value

function main(): Unit =
    let id = UserId(42)
    assert id.toInt() == 42
"#,
    )
    .expect("extension method on newtype");
}

#[test]
#[ignore]
fn test_extension_on_array() {
    common::compile_and_run(
        r#"
package a

import a.ArrayInt32Ext

extension ArrayInt32Ext for Array<Int32> =
    function first(self): Int32 = self[0]

function main(): Unit =
    let arr = [|1, 2, 3|]
    assert arr.first() == 1
"#,
    )
    .expect("extension method on Array");
}

#[test]
#[ignore]
fn test_named_extension_on_enum() {
    common::compile_and_run(
        r#"
package a

import a.ColorOps

enum Color =
    Red
    Green
    Blue

extension ColorOps for Color =
    function isWarm(self): Bool = match self with
        Color.Red -> true
        _ -> false

function main(): Unit =
    let c = Color.Red
    assert c.isWarm() == true
"#,
    )
    .expect("named extension on enum");
}

// ── Multi-target instance methods under one extension name ─────────────

#[test]
fn test_multi_target_named_extension_instance_method() {
    common::compile_and_run(
        r#"
package a

import a.Mixed

extension Mixed for Int32 =
    function describe(self): String = "int " ++ self.format()

extension Mixed for String =
    function describe(self): String = "str " ++ self

function main(): Unit =
    assert 7.describe() == "int 7"
    assert "hi".describe() == "str hi"
"#,
    )
    .expect("multi-target named extension instance method");
}
