mod common;

// ── Basic extension property on a record type ─────────────────────────

#[test]
fn test_basic_extension_property_on_record() {
    common::compile_and_run(
        r#"
package a

import a.PointExt

record Point =
    x: Int32
    y: Int32

extension PointExt for Point =
    property magnitude(self): Int32 = self.x * self.x + self.y * self.y

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.magnitude == 25
"#,
    )
    .expect("basic extension property on record");
}

// ── Extension property on Int32 (non-record type) ─────────────────────

#[test]
fn test_extension_property_on_int32() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property isEven(self): Bool = self % 2 == 0

function main(): Unit =
    assert 4.isEven == true
    assert 3.isEven == false
"#,
    )
    .expect("extension property on Int32");
}

// ── Extension property with computation ──────────────────────────────

#[test]
fn test_extension_property_with_computation() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property doubled(self): Int32 = self * 2
    property tripled(self): Int32 = self * 3

function main(): Unit =
    assert 5.doubled == 10
    assert 5.tripled == 15
"#,
    )
    .expect("extension property with computation");
}

// ── Extension property alongside methods ─────────────────────────────

#[test]
fn test_extension_property_and_method_together() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property squared(self): Int32 = self * self
    function add(self, other: Int32): Int32 = self + other

function main(): Unit =
    assert 5.squared == 25
    assert 5.add(3) == 8
"#,
    )
    .expect("extension property and method together");
}

// ── Named extension with property ─────────────────────────────────────

#[test]
fn test_named_extension_with_property() {
    common::compile_and_run(
        r#"
package a

import a.IntProps

extension IntProps for Int32 =
    public property doubled(self): Int32 = self * 2

function main(): Unit = assert 5.doubled == 10
"#,
    )
    .expect("named extension with property");
}

// ── Named extension property not imported → error ────────────────────

#[test]
fn test_named_extension_property_not_imported_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

extension IntProps for Int32 =
    public property doubled(self): Int32 = self * 2

function main(): Unit = assert 5.doubled == 10
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no field")),
        "expected error about no field, got: {:?}",
        errors
    );
}

// ── Array.length as a property (confirms prelude migration) ───────────

#[test]
fn test_array_length_property() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3|]
    assert arr.length == 3
"#,
    )
    .expect("array length as property");
}

// ── Chained property access ──────────────────────────────────────────

#[test]
fn test_chained_property_and_method() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property abs(self): Int32 = if self < 0 then 0 - self else self

function main(): Unit =
    let arr = [|1, 2, 3, 4, 5|]
    let len = arr.length
    assert len == 5
    let neg = -3
    assert neg.abs == 3
"#,
    )
    .expect("chained property and method");
}

// ── Generic extension with property ──────────────────────────────────

#[test]
fn test_generic_extension_property() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt

extension ArrayExt<T> for Array<T> =
    property isEmpty(self): Bool = self.length == 0

function main(): Unit =
    let arr = [|1, 2, 3|]
    assert arr.isEmpty == false
    let one = [|42|]
    assert one.isEmpty == false
"#,
    )
    .expect("generic extension property");
}

// ── Private extension property visibility ─────────────────────────────

#[test]
fn test_private_extension_property_visible_in_same_file() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    private property doubled(self): Int32 = self * 2

function main(): Unit = assert 5.doubled == 10
"#,
    )
    .expect("private extension property visible in same file");
}

// ── Extension property on Bool ───────────────────────────────────────

#[test]
fn test_extension_property_on_bool() {
    common::compile_and_run(
        r#"
package a

import a.BoolExt

extension BoolExt for Bool =
    property toggled(self): Bool = if self then false else true

function main(): Unit =
    assert true.toggled == false
    assert false.toggled == true
"#,
    )
    .expect("extension property on Bool");
}

// ── Trait with property signature + implement block ──────────────────

#[test]
fn test_trait_property_with_implement() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait HasMagnitude =
    property magnitude(self): Int32

implement HasMagnitude for Point =
    property magnitude(self): Int32 = self.x * self.x + self.y * self.y

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.magnitude == 25
"#,
    )
    .expect("trait property with implement");
}

// ── Trait with both methods and properties ───────────────────────────

#[test]
fn test_trait_with_methods_and_properties() {
    common::compile_and_run(
        r#"
package a

record Rectangle =
    width: Int32
    height: Int32

trait Shape =
    property area(self): Int32
    function describe(self: Self): Int32

implement Shape for Rectangle =
    property area(self): Int32 = self.width * self.height
    function describe(self: Rectangle): Int32 = self.width + self.height

function main(): Unit =
    let r = Rectangle { width = 3; height = 4 }
    assert r.area == 12
    assert r.describe() == 7
"#,
    )
    .expect("trait with methods and properties");
}

// ── Missing property in implement block → error ─────────────────────

#[test]
fn test_missing_trait_property_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait HasMagnitude =
    property magnitude(self): Int32

implement HasMagnitude for Point =
    function dummy(self): Int32 = 0
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("missing implementation of property")),
        "expected error about missing property, got: {:?}",
        errors
    );
}

// ── Property name collision with method name in trait → error ────────

#[test]
fn test_trait_property_method_name_collision_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Bad =
    function foo(self: Self): Int32
    property foo(self): Int32
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate") || e.contains("conflicts")),
        "expected error about duplicate/conflict, got: {:?}",
        errors
    );
}

// ── Static property on extension ─────────────────────────────────────

#[test]
fn test_static_extension_property() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property maxValue: Int32 = 2147483647

function main(): Unit =
    assert Int32.maxValue == 2147483647
"#,
    )
    .expect("static extension property");
}

// ── Static property alongside instance property ──────────────────────

#[test]
fn test_static_and_instance_property() {
    common::compile_and_run(
        r#"
package a

import a.Int32Ext

extension Int32Ext for Int32 =
    property zero: Int32 = 0
    property doubled(self): Int32 = self * 2

function main(): Unit =
    assert Int32.zero == 0
    assert 5.doubled == 10
"#,
    )
    .expect("static and instance property");
}

// ── Multi-target named extensions: same name, different `for_type` ─────

#[test]
fn test_multi_target_named_extension_static_property() {
    common::compile_and_run(
        r#"
package a

import a.MathExt

extension MathExt for Int32 =
    property zero: Int32 = 0

extension MathExt for String =
    property empty: String = ""

function main(): Unit =
    assert Int32.zero == 0
    assert String.empty == ""
"#,
    )
    .expect("multi-target named extension static property");
}

#[test]
fn test_duplicate_extension_for_same_type_rejected() {
    let result = common::compile_and_run(
        r#"
package a

extension Foo for Int32 =
    property a: Int32 = 1

extension Foo for Int32 =
    property b: Int32 = 2

function main(): Unit = ()
"#,
    );
    let err = result.expect_err("duplicate (name, for_type) should be rejected");
    assert!(
        err.contains("duplicate named extension 'Foo'") && err.contains("Int32"),
        "error should mention name and for_type; got: {err}"
    );
}

#[test]
fn test_multi_target_generic_and_nongeneric_under_one_name() {
    common::compile_and_run(
        r#"
package a

import a.U

record Box<T> = value: T

extension U for Int32 =
    property tag: Int32 = 42

extension U<T> for Box<T> =
    function unwrap(self): T = self.value

function main(): Unit =
    let b = Box<Int32> { value = 7 }
    assert Int32.tag == 42
    assert b.unwrap() == 7
"#,
    )
    .expect("multi-target generic and non-generic under one name");
}
