mod common;

// Resolution priority for `receiver.name(args)` per trait-design-appendix §2.1:
// module → named extension → trait impl. These tests pin the order for both
// instance methods and static functions.

// ── Extension beats trait impl (instance) ───────────────────────────

#[test]
fn test_extension_beats_trait_impl_instance() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Named =
    function label(self): Int32

implement Named for Rec =
    function label(self): Int32 = 1

extension RecExt for Rec =
    function label(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.label() == 2
"#,
    )
    .expect("extension beats trait impl for instance methods");
}

// ── Module beats extension (instance) ───────────────────────────────

#[test]
fn test_module_beats_extension_instance() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

module Rec =
    function label(self): Int32 = 1

extension RecExt for Rec =
    function label(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.label() == 1
"#,
    )
    .expect("module beats extension for instance methods");
}

// ── Extension beats trait impl (static) ─────────────────────────────

#[test]
fn test_extension_beats_trait_impl_static() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Maker =
    function make(): Self

implement Maker for Rec =
    function make(): Rec = Rec { x = 1 }

extension RecExt for Rec =
    function make(): Rec = Rec { x = 2 }

function main(): Unit =
    let r = Rec.make()
    assert r.x == 2
"#,
    )
    .expect("extension beats trait impl for static functions");
}

// ── Trait impl still reachable when no extension exists ─────────────

#[test]
fn test_trait_impl_still_resolves_without_extension() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Named =
    function label(self): Int32

implement Named for Rec =
    function label(self): Int32 = 7

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.label() == 7
"#,
    )
    .expect("trait impl resolves when no extension competes");
}

// ── Closure-arg inference flows through the extension winner ────────

#[test]
fn test_closure_arg_inferred_through_extension() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Applier =
    function apply(self, f: Int32 => Int32): Int32

implement Applier for Rec =
    function apply(self, f: Int32 => Int32): Int32 = f(self.x)

extension RecExt for Rec =
    function apply(self, f: Int32 => Int32): Int32 = f(self.x) + 100

function main(): Unit =
    let r = Rec { x = 5 }
    assert r.apply(v => v * 2) == 110
"#,
    )
    .expect("closure arg types inferred through the extension winner");
}

// ── Review-round regressions: properties, statics, refs, fall-through ──

#[test]
fn test_extension_beats_trait_impl_property() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Named =
    property tag(self): Int32

implement Named for Rec =
    property tag(self): Int32 = 1

extension RecExt for Rec =
    property tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag == 2
"#,
    )
    .expect("extension beats trait impl for instance properties");
}

#[test]
fn test_extension_beats_trait_impl_static_property() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Zeroed =
    property zero: Int32

implement Zeroed for Rec =
    property zero: Int32 = 1

extension RecExt for Rec =
    property zero: Int32 = 2

function main(): Unit =
    assert Rec.zero == 2
"#,
    )
    .expect("extension beats trait impl for static properties");
}

#[test]
fn test_extension_beats_trait_impl_bound_method_ref() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Named =
    function label(self): Int32

implement Named for Rec =
    function label(self): Int32 = 1

extension RecExt for Rec =
    function label(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    let f = r.label
    assert f() == 2
"#,
    )
    .expect("a bound method reference resolves with the same priority as the call form");
}

#[test]
fn test_cross_trait_property_ambiguity() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property p(self): Int32

trait Beta =
    property p(self): Int32

implement Alpha for Rec =
    property p(self): Int32 = 1

implement Beta for Rec =
    property p(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.p == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous property 'p'") && e.contains("'Alpha'") && e.contains("'Beta'")),
        "cross-trait property ambiguity must be reported, got: {:?}",
        errors
    );
}

#[test]
fn test_sibling_instantiation_property_access() {
    common::compile_and_run(
        r#"
package a

trait Tagged =
    property tag(self): Int32

implement Tagged for List<Int32> =
    property tag(self): Int32 = 1

implement Tagged for List<String> =
    property tag(self): Int32 = 2

function main(): Unit =
    let xs: List<Int32> = [1]
    let ys: List<String> = ["a"]
    assert xs.tag == 1
    assert ys.tag == 2
"#,
    )
    .expect("property access picks the sibling block matching the receiver");
}

#[test]
fn test_sibling_instantiation_method_ref() {
    common::compile_and_run(
        r#"
package a

trait Show2 =
    function show(self): Int32

implement Show2 for List<Int32> =
    function show(self): Int32 = 1

implement Show2 for List<String> =
    function show(self): Int32 = 2

function main(): Unit =
    let xs: List<Int32> = [1]
    let ys: List<String> = ["a"]
    let f = xs.show
    let g = ys.show
    assert f() == 1
    assert g() == 2
"#,
    )
    .expect("bound method references pick the sibling block matching the receiver");
}

#[test]
fn test_local_variable_shadows_extension_name() {
    common::compile_and_run(
        r#"
package a

import a.Thing

record Counter =
    n: Int32

extension Thing for Counter =
    function unused(self): Int32 = 0

module Counter =
    function bump(self): Int32 = self.n + 1

function main(): Unit =
    let Thing = Counter { n = 4 }
    assert Thing.bump() == 5
"#,
    )
    .expect("a local variable sharing an extension's name shadows the explicit-call form");
}

#[test]
fn test_non_matching_extension_falls_through_to_trait_impl() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Adder =
    function foo(self, n: Int32): Int32

implement Adder for Rec =
    function foo(self, n: Int32): Int32 = n + 1

extension RecExt for Rec =
    function foo(self, s: String): Int32 = 100

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.foo(1) == 2
    assert r.foo("s") == 100
"#,
    )
    .expect("an extension whose overloads don't match the args does not block trait impls");
}
