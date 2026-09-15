mod common;

// ── Named generic extension, specialized (Array<Int32>) ─────────────

#[test]
fn test_generic_extension_first_on_int32_array() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt

extension ArrayExt<T> for Array<T> =
    function first(self): T = self.get(0)

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr.first() == 10
"#,
    )
    .expect("generic extension first on int32 array");
}

// ── Named generic extension, shared (Array<Record>) ─────────────────

#[test]
fn test_generic_extension_first_on_record_array() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt

record Box =
    value: Int32

extension ArrayExt<T> for Array<T> =
    function first(self): T = self.get(0)

function main(): Unit =
    let boxes = [|Box { value = 42 }, Box { value = 99 }|]
    let b = boxes.first()
    assert b.value == 42
"#,
    )
    .expect("generic extension first on record array");
}

// ── Named generic extension ───────────────────────────────────────────

#[test]
fn test_named_generic_extension_with_import() {
    common::compile_and_run(
        r#"
package a

import a.ArrayHelper

extension ArrayHelper<T> for Array<T> =
    function first(self): T = self.get(0)

function main(): Unit =
    let arr = [|5, 10, 15|]
    assert arr.first() == 5
"#,
    )
    .expect("named generic extension with import");
}

// ── Named generic extension not imported → error ──────────────────────

#[test]
fn test_named_generic_extension_not_imported_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

extension ArrayHelper<T> for Array<T> =
    function first(self): T = self.get(0)

function main(): Unit =
    let arr = [|1, 2, 3|]
    arr.first()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method, got: {:?}",
        errors
    );
}

// ── Generic extension on generic record ───────────────────────────────

#[test]
fn test_generic_extension_on_generic_record() {
    common::compile_and_run(
        r#"
package a

import a.BoxExt

record Box<T> =
    value: T

extension BoxExt<T> for Box<T> =
    function unwrap(self): T = self.value

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.unwrap() == 42
"#,
    )
    .expect("generic extension on generic record");
}

// ── Multiple methods in one generic extension ─────────────────────────

#[test]
fn test_multiple_methods_in_generic_extension() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt

extension ArrayExt<T> for Array<T> =
    function first(self): T = self.get(0)
    function last(self): T = self.get(self.length - 1)

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr.first() == 10
    assert arr.last() == 30
"#,
    )
    .expect("multiple methods in generic extension");
}

// ── Coexistence: generic ext on Array + non-generic ext on Int32 ──────

#[test]
fn test_generic_and_nongeneric_extension_coexistence() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt
import a.Int32Ext

extension ArrayExt<T> for Array<T> =
    function first(self): T = self.get(0)

extension Int32Ext for Int32 =
    function double(self): Int32 = self * 2

function main(): Unit =
    let arr = [|5, 10, 15|]
    assert arr.first().double() == 10
"#,
    )
    .expect("generic and nongeneric extension coexistence");
}

// ── Method with extra params ──────────────────────────────────────────

#[test]
fn test_generic_extension_method_with_extra_params() {
    common::compile_and_run(
        r#"
package a

import a.ArrayExt

extension ArrayExt<T> for Array<T> =
    function getOrDefault(self, index: Int32, default: T): T =
        if index >= 0 then
            if index < self.length then self.get(index)
            else default
        else default

function main(): Unit =
    let arr = [|10, 20, 30|]
    assert arr.getOrDefault(1, 0) == 20
    assert arr.getOrDefault(5, 99) == 99
"#,
    )
    .expect("generic extension method with extra params");
}

// ── Extension method-level type params ────────────────────────────────

#[test]
fn test_generic_extension_method_with_own_type_params() {
    common::compile_and_run(
        r#"
package a

import a.BoxExt

record Box<T> =
    value: T

extension BoxExt<T> for Box<T> =
    function replaceWith<U>(self, newValue: U): Box<U> = Box<U> { value = newValue }

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    let b2 = b.replaceWith(true)
    assert b2.value == true
    let b3 = b.replaceWith(10)
    assert b3.value == 10
"#,
    )
    .expect("generic extension method with own type params");
}
