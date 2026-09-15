mod common;

// Explicit disambiguation calls per trait-design-appendix §2.3 / §3:
// `TraitName.method(receiver, args...)` and `ExtName.method(receiver, args...)`
// select a specific implementation when implicit resolution is ambiguous or
// when priority would pick another source.

// ── Explicit trait call selects one of two ambiguous impls ──────────

#[test]
fn test_implicit_call_ambiguous_across_traits() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

trait Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag() == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous call") && e.contains("'Alpha'") && e.contains("'Beta'")),
        "expected cross-trait ambiguity naming both traits, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_trait_call_selects_impl() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

trait Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert Alpha.tag(r) == 1
    assert Beta.tag(r) == 2
"#,
    )
    .expect("explicit trait call selects the named trait's impl");
}

// ── Explicit trait call overrides extension priority ────────────────

#[test]
fn test_explicit_trait_call_overrides_priority() {
    common::compile_and_run(
        r#"
package a

import a.RecExt

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

extension RecExt for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag() == 2
    assert Alpha.tag(r) == 1
"#,
    )
    .expect("explicit trait call forces the trait impl past extension priority");
}

// ── Explicit extension call selects one of two ambiguous extensions ─

#[test]
fn test_explicit_extension_call_selects() {
    common::compile_and_run(
        r#"
package a

import a.FirstExt
import a.SecondExt

record Rec =
    x: Int32

extension FirstExt for Rec =
    function tag(self): Int32 = 1

extension SecondExt for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert FirstExt.tag(r) == 1
    assert SecondExt.tag(r) == 2
"#,
    )
    .expect("explicit extension call selects the named extension");
}

#[test]
fn test_two_extensions_ambiguous_names_both() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.FirstExt
import a.SecondExt

record Rec =
    x: Int32

extension FirstExt for Rec =
    function tag(self): Int32 = 1

extension SecondExt for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag() == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous call")
            && e.contains("'FirstExt'")
            && e.contains("'SecondExt'")),
        "expected cross-extension ambiguity naming both extensions, got: {:?}",
        errors
    );
}

// ── Explicit trait call on a bounded type parameter ─────────────────

#[test]
fn test_type_param_two_bounds_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

trait Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function pick<T>(v: T): Int32 where T: Alpha + Beta = v.tag()

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous method 'tag' on type parameter 'T'")),
        "expected type-param bound ambiguity, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_trait_call_on_type_param() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

trait Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function pickAlpha<T>(v: T): Int32 where T: Alpha + Beta = Alpha.tag(v)
function pickBeta<T>(v: T): Int32 where T: Alpha + Beta = Beta.tag(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert pickAlpha(r) == 1
    assert pickBeta(r) == 2
"#,
    )
    .expect("explicit trait call resolves via the named bound");
}

// ── Explicit trait call on an interface object (intersection) ───────

#[test]
fn test_explicit_trait_call_on_interface_object() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function tag(self): Int32

interface Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function pickAlpha(v: Alpha and Beta): Int32 = Alpha.tag(v)
function pickBeta(v: Alpha and Beta): Int32 = Beta.tag(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert pickAlpha(r) == 1
    assert pickBeta(r) == 2
"#,
    )
    .expect("explicit trait call dispatches through the named intersection component");
}

// ── Explicit static through the trait ───────────────────────────────

#[test]
fn test_trait_static_explicit_unique() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Maker =
    function make(seed: Int32): Self

implement Maker for Rec =
    function make(seed: Int32): Rec = Rec { x = seed }

function main(): Unit =
    let r = Maker.make(41)
    assert r.x == 41
"#,
    )
    .expect("explicit trait static resolves via the unique impl");
}

#[test]
fn test_trait_static_explicit_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record RecA =
    x: Int32

record RecB =
    x: Int32

trait Maker =
    function make(seed: Int32): Self

implement Maker for RecA =
    function make(seed: Int32): RecA = RecA { x = seed }

implement Maker for RecB =
    function make(seed: Int32): RecB = RecB { x = seed }

function main(): Unit =
    let r = Maker.make(41)
    assert r.x == 41
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous call to 'Maker.make'")),
        "expected trait-static ambiguity, got: {:?}",
        errors
    );
}

// ── Error quality ───────────────────────────────────────────────────

#[test]
fn test_plain_trait_bad_receiver_keeps_good_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

record Other =
    y: Int32

trait Alpha =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

function main(): Unit =
    let o = Other { y = 0 }
    assert Alpha.tag(o) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait 'Alpha'")),
        "expected 'does not implement trait', got: {:?}",
        errors
    );
    assert!(
        !errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "the misleading type-position error must not leak for call receivers: {:?}",
        errors
    );
}

#[test]
fn test_unknown_trait_member_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

function main(): Unit =
    let r = Rec { x = 0 }
    assert Alpha.missing(r) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("trait 'Alpha' has no function 'missing'")),
        "expected member-not-found error, got: {:?}",
        errors
    );
}

// ── Interface statics still resolve through the type-name path ──────

#[test]
fn test_interface_static_fallthrough_regression() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Greeter =
    function greet(self): Int32

implement Greeter for Rec =
    function greet(self): Int32 = self.x

function useIt(g: Greeter): Int32 = g.greet()

function main(): Unit =
    let r = Rec { x = 5 }
    assert useIt(r) == 5
    assert Greeter.greet(r) == 5
"#,
    )
    .expect("interface member calls work both dynamically and explicitly");
}

// ── Review-round regressions ────────────────────────────────────────

#[test]
fn test_explicit_super_trait_call_on_sub_bound() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function foo(self): Int32 = 7
    function beta(self): Int32 = 1

function genericCall<T>(v: T): Int32 where T: Beta = Alpha.foo(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert genericCall(r) == 7
"#,
    )
    .expect("an explicit super-trait call resolves through a sub-trait bound");
}

#[test]
fn test_trait_static_generic_only_honest_message() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

trait Maker =
    function makeIt(n: Int32): Int32

implement <T> Maker for Box<T> =
    function makeIt(n: Int32): Int32 = n

function main(): Unit =
    assert Maker.makeIt(2) == 2
"#,
    );
    assert!(
        errors.iter().any(|e| e
            .contains("the only implementations are generic; call it on a concrete type instead")),
        "generic-only static impls get an honest diagnostic, got: {:?}",
        errors
    );
}

// ── Round-3 review regressions ──────────────────────────────────────

#[test]
fn test_generic_ext_property_ambiguity() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.ExtA
import a.ExtB

record Box<T> =
    v: T

extension ExtA<T> for Box<T> =
    property p(self): Int32 = 1

extension ExtB<T> for Box<T> =
    property p(self): Int32 = 2

function main(): Unit =
    let b = Box { v = 1 }
    assert b.p == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous property 'p'")
            && e.contains("'ExtA'")
            && e.contains("'ExtB'")),
        "generic extension property ambiguity must be reported, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_generic_ext_property_call() {
    common::compile_and_run(
        r#"
package a

import a.BoxExt

record Box<T> =
    v: T

extension BoxExt<T> for Box<T> =
    property tag(self): Int32 = 5

function main(): Unit =
    let b = Box { v = 1 }
    assert BoxExt.tag(b) == 5
"#,
    )
    .expect("explicit extension property calls work through generic blocks");
}

#[test]
fn test_explicit_trait_property_on_interface_object() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    property p(self): Int32

implement Alpha for Rec =
    property p(self): Int32 = 5

function main(): Unit =
    let o: Alpha = Rec { x = 0 }
    assert Alpha.p(o) == 5
"#,
    )
    .expect("explicit trait property calls dispatch on interface objects");
}

#[test]
fn test_ambiguous_bound_ref_message() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function tag(self): Int32

trait Beta =
    function tag(self): Int32

implement Alpha for Rec =
    function tag(self): Int32 = 1

implement Beta for Rec =
    function tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    let f = r.tag
    assert f() == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous reference to 'tag'")),
        "ambiguous bound refs must say so (not 'no field'), got: {:?}",
        errors
    );
    assert!(
        !errors.iter().any(|e| e.contains("no field 'tag'")),
        "the 'no field' denial must not appear, got: {:?}",
        errors
    );
}

#[test]
fn test_bare_trait_static_property_access() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Versioned =
    property version: Int32

implement Versioned for Rec =
    property version: Int32 = 7

function main(): Unit =
    assert Versioned.version == 7
"#,
    )
    .expect("bare TraitName.prop resolves the unique impl's static property");
}

#[test]
fn test_generic_ext_static_honest_message() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.BoxExt

record Box<T> =
    v: T

extension BoxExt<T> for Box<T> =
    function makeIt(v: T): Box<T> = Box { v = v }

function main(): Unit =
    let b = BoxExt.makeIt(3)
    assert b.v == 3
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("the only matching members are on generic blocks")),
        "generic-only extension statics get an honest diagnostic, got: {:?}",
        errors
    );
}

// ── Round-4 review regressions ──────────────────────────────────────

#[test]
fn test_global_shadows_trait_static_property() {
    common::compile_and_run(
        r#"
package a

record Holder =
    mark: Int32

trait Marked =
    property mark: Int32

implement Marked for Holder =
    property mark: Int32 = 7

let Marked: Holder = Holder { mark = 99 }

function main(): Unit =
    assert Marked.mark == 99
"#,
    )
    .expect("a global sharing a trait's name wins the bare member-access form");
}

#[test]
fn test_explicit_generic_ext_property_rejects_extra_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.BoxExt

record Box<T> =
    v: T

extension BoxExt<T> for Box<T> =
    property first(self): T = self.v

function main(): Unit =
    let b = Box { v = 1 }
    assert BoxExt.first(b, 42) == 1
"#,
    );
    assert!(
        !errors.is_empty(),
        "extra args on an explicit property call must not be silently dropped"
    );
}

// ── Round-5 review regressions ──────────────────────────────────────

#[test]
fn test_explicit_sibling_instantiation_args_honored() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 10

implement Conv<Bool> for Rec =
    function tag(self): Int32 = 20

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv<Int32>.tag(r) == 10
    assert Conv<Bool>.tag(r) == 20
"#,
    )
    .expect("explicit trait type args select the sibling instantiation");
}

#[test]
fn test_explicit_bare_sibling_call_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 10

implement Conv<Bool> for Rec =
    function tag(self): Int32 = 20

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv.tag(r) == 10
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous")),
        "a bare explicit call over sibling instantiations must be ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_extension_trait_name_clash_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import a.Pick

record Rec =
    x: Int32

trait Pick =
    function m(self): Int32

extension Pick for Rec =
    function m(self): Int32 = 100

implement Pick for Rec =
    function m(self): Int32 = 200

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("named extension 'Pick' conflicts with a trait of the same name")),
        "a trait/extension name clash must be diagnosed, got: {:?}",
        errors
    );
}

// ── Round-6 review regressions ──────────────────────────────────────

#[test]
fn test_explicit_args_reject_wrong_provider_application() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

trait Prov extends Conv<Int32> =
    function prov(self): Int32

implement Prov for Rec =
    function tag(self): Int32 = 10
    function prov(self): Int32 = 1

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv<Bool>.tag(r) == 10
"#,
    );
    assert!(
        !errors.is_empty(),
        "explicit args of an unprovided application must not silently route to another provider"
    );
}

#[test]
fn test_explicit_args_select_among_providers() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

trait ProvInt extends Conv<Int32> =
    function provInt(self): Int32

trait ProvBool extends Conv<Bool> =
    function provBool(self): Int32

implement ProvInt for Rec =
    function tag(self): Int32 = 10
    function provInt(self): Int32 = 1

implement ProvBool for Rec =
    function tag(self): Int32 = 20
    function provBool(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv<Int32>.tag(r) == 10
    assert Conv<Bool>.tag(r) == 20
"#,
    )
    .expect("explicit args uniquely select among providers of different applications");
}

#[test]
fn test_is_interface_target_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Sq =
    side: Int32

interface Shape =
    function area(self): Int32

implement Shape for Sq =
    function area(self): Int32 = self.side * self.side

function main(): Unit =
    let a: Any = Sq { side = 5 }
    assert a is Shape
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("'is' cannot test interface type")),
        "an interface target in 'is' must be rejected, got: {:?}",
        errors
    );
}

#[test]
fn test_sibling_iterable_for_loop_honest_message() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Both =
    x: Int32

trait Fake =
    function unused(self): Int32

implement Iterable<Int32> for Both =
    function iterator(self): Iterator<Int32> = [1].iterator()

implement Iterable<String> for Both =
    function iterator(self): Iterator<String> = ["a"].iterator()

function main(): Unit =
    let b = Both { x = 0 }
    for v in b do
        ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("implements 'Iterable' more than once")),
        "sibling Iterable impls get the honest ambiguity message, got: {:?}",
        errors
    );
}

// ── Round-7 review regressions ──────────────────────────────────────

#[test]
fn test_match_type_pattern_interface_target_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Sq =
    side: Int32

interface Shape =
    function area(self): Int32

implement Shape for Sq =
    function area(self): Int32 = self.side * self.side

function main(): Unit =
    let a: Any = Sq { side = 5 }
    let mutable hit = 0
    match a with
        case p: Shape => hit = 1
        case other => hit = 2
    assert hit == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type pattern cannot test interface type")),
        "interface targets in type patterns must be rejected, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_args_route_past_wrong_application_direct() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

trait Prov extends Conv<Bool> =
    function p(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 1

implement Prov for Rec =
    function tag(self): Int32 = 2
    function p(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv<Int32>.tag(r) == 1
    assert Conv<Bool>.tag(r) == 2
"#,
    )
    .expect("a wrong-application direct impl must not hide the requested application's provider");
}

#[test]
fn test_static_property_explicit_args_select_sibling() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    property zero: Int32
    function tag(self): Int32

implement Conv<Int32> for Rec =
    property zero: Int32 = 10
    function tag(self): Int32 = 1

implement Conv<Bool> for Rec =
    property zero: Int32 = 20
    function tag(self): Int32 = 2

function main(): Unit =
    assert Conv<Int32>.zero == 10
    assert Conv<Bool>.zero == 20
"#,
    )
    .expect("Trait<Args>.staticProp must select the requested application");
}

#[test]
fn test_static_property_wrong_application_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    property zero: Int32
    function tag(self): Int32

implement Conv<Int32> for Rec =
    property zero: Int32 = 10
    function tag(self): Int32 = 1

function main(): Unit =
    let z = Conv<Bool>.zero
    assert z == 10
"#,
    );
    assert!(
        !errors.is_empty(),
        "Conv<Bool>.zero must not silently resolve through the Conv<Int32> impl"
    );
}

#[test]
fn test_explicit_args_on_generic_direct_impl() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    v: T

trait Conv<T> =
    function tag(self): Int32

implement <T> Conv<T> for Box<T> =
    function tag(self): Int32 = 7

function main(): Unit =
    let b = Box<Int32> { v = 1 }
    assert Conv<Int32>.tag(b) == 7
"#,
    )
    .expect("a generic direct impl must satisfy an explicit-args call");
}

#[test]
fn test_explicit_args_on_generic_class_impl() {
    common::compile_and_run(
        r#"
package a

trait Conv<T> =
    function tag(self): Int32

class Holder<T>(v: T) implements Conv<T> =
    public function tag(self: Holder<T>): Int32 = 9

function main(): Unit =
    let h = Holder<Int32>(1)
    assert Conv<Int32>.tag(h) == 9
"#,
    )
    .expect("a generic class impl must satisfy an explicit-args call");
}

// ── Round-14 review regressions ─────────────────────────────────────

#[test]
fn test_instance_property_sibling_applications_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    property tag(self): Int32

implement Conv<Int32> for Rec =
    property tag(self): Int32 = 1

implement Conv<Bool> for Rec =
    property tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous property 'tag'")),
        "two applications of one trait must be ambiguous for properties too, got: {:?}",
        errors
    );
}

#[test]
fn test_static_property_two_traits_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property zero: Int32

trait Beta =
    property zero: Int32

implement Alpha for Rec =
    property zero: Int32 = 1

implement Beta for Rec =
    property zero: Int32 = 2

function main(): Unit =
    assert Rec.zero == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous static property 'zero'")),
        "a static property provided by two traits must be ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_static_two_traits_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    v: T

trait Alpha =
    function make(): Int32

trait Beta =
    function make(): Int32

implement <T> Alpha for Box<T> =
    function make(): Int32 = 1

implement <T> Beta for Box<T> =
    function make(): Int32 = 2

function main(): Unit =
    assert Box<Int32>.make() == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous call to 'Box.make'")),
        "generic static impls from two traits must be ambiguous, got: {:?}",
        errors
    );
}

// ── Round-15 review regressions ─────────────────────────────────────

#[test]
fn test_two_applications_of_one_trait_in_bounds_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function conv(self): T

implement Conv<Int32> for Rec =
    function conv(self): Int32 = 1

implement Conv<Bool> for Rec =
    function conv(self): Bool = true

function pick<T>(v: T): Int32 where T: Conv<Int32> + Conv<Bool> = v.conv()

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous method 'conv'")
            && e.contains("Conv<Int32>")
            && e.contains("Conv<Bool>")),
        "two applications of one trait in the bound list must be ambiguous and named apart, got: {:?}",
        errors
    );
}

#[test]
fn test_two_applications_of_one_trait_property_in_bounds_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    property conv(self): T

implement Conv<Int32> for Rec =
    property conv(self): Int32 = 1

implement Conv<Bool> for Rec =
    property conv(self): Bool = true

function pick<T>(v: T): Int32 where T: Conv<Int32> + Conv<Bool> = v.conv

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous property 'conv'")),
        "the property mirror must be ambiguous too, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_static_different_arity_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

trait Dec =
    function makeIt(): Int32

trait Other =
    function makeIt(x: Int32): Int32

implement <T> Dec for Box<T> =
    function makeIt(): Int32 = 1

implement <T> Other for Box<T> =
    function makeIt(x: Int32): Int32 = x

function main(): Unit =
    assert Box<Int32>.makeIt() == 1
"#,
    )
    .expect("only candidates that can accept the call compete for ambiguity");
}

#[test]
fn test_tuple_static_different_arity_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

trait Dec =
    function makeIt(): Int32

trait Other =
    function makeIt(x: Int32): Int32

implement <A, B> Dec for (A, B) =
    function makeIt(): Int32 = 1

implement <A, B> Other for (A, B) =
    function makeIt(x: Int32): Int32 = x

function main(): Unit =
    assert (Int32, Bool).makeIt() == 1
"#,
    )
    .expect("tuple static path filters by arity before ambiguity");
}

// ── Round-16 review regressions ─────────────────────────────────────

#[test]
fn test_generic_static_different_param_types_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

trait Dec =
    function makeIt(x: Int32): Int32

trait Other =
    function makeIt(x: String): Int32

implement <T> Dec for Box<T> =
    function makeIt(x: Int32): Int32 = x

implement <T> Other for Box<T> =
    function makeIt(x: String): Int32 = 2

function main(): Unit =
    assert Box<Int32>.makeIt(5) == 5
"#,
    )
    .expect("generic statics differing by PARAM TYPE must not be ambiguous");
}

#[test]
fn test_tuple_static_different_param_types_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

trait Dec =
    function makeIt(x: Int32): Int32

trait Other =
    function makeIt(x: String): Int32

implement <A, B> Dec for (A, B) =
    function makeIt(x: Int32): Int32 = x

implement <A, B> Other for (A, B) =
    function makeIt(x: String): Int32 = 2

function main(): Unit =
    assert (Int32, Bool).makeIt(5) == 5
"#,
    )
    .expect("tuple statics differing by PARAM TYPE must not be ambiguous");
}

#[test]
fn test_explicit_bare_call_on_type_param_two_applications_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 1

implement Conv<Bool> for Rec =
    function tag(self): Int32 = 2

function pick<T>(v: T): Int32 where T: Conv<Int32> + Conv<Bool> = Conv.tag(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous call to 'Conv.tag'")
                && e.contains("Conv<Int32>")
                && e.contains("Conv<Bool>")),
        "bound order must not decide a bare explicit call's callee, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_bare_call_on_type_param_named_application_resolves() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 1

implement Conv<Bool> for Rec =
    function tag(self): Int32 = 2

function pickInt<T>(v: T): Int32 where T: Conv<Int32> + Conv<Bool> = Conv<Int32>.tag(v)

function pickBool<T>(v: T): Int32 where T: Conv<Int32> + Conv<Bool> = Conv<Bool>.tag(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert pickInt(r) == 1
    assert pickBool(r) == 2
"#,
    )
    .expect("naming the application disambiguates the bound-supplied explicit call");
}

#[test]
fn test_ambiguity_messages_distinguish_applications() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    property tag(self): Int32

implement Conv<Int32> for Rec =
    property tag(self): Int32 = 1

implement Conv<Bool> for Rec =
    property tag(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("Conv<Int32>") && e.contains("Conv<Bool>")),
        "an ambiguity between two applications must name them apart, got: {:?}",
        errors
    );
}

// ── Round-17 review regressions ─────────────────────────────────────

#[test]
fn test_explicit_property_on_type_param_receiver() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property tag(self): Int32

trait Beta =
    property tag(self): Int32

implement Alpha for Rec =
    property tag(self): Int32 = 1

implement Beta for Rec =
    property tag(self): Int32 = 2

function pickA<T>(v: T): Int32 where T: Alpha + Beta = Alpha.tag(v)

function pickB<T>(v: T): Int32 where T: Alpha + Beta = Beta.tag(v)

function main(): Unit =
    let r = Rec { x = 0 }
    assert pickA(r) == 1
    assert pickB(r) == 2
"#,
    )
    .expect("the explicit form is the documented disambiguator for properties on type params too");
}

#[test]
fn test_generic_static_ambiguity_message_is_actionable() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    v: T

trait Dec =
    function makeIt(x: Int32): Int32

trait Other =
    function makeIt(x: Int32): Int32

implement <T> Dec for Box<T> =
    function makeIt(x: Int32): Int32 = 1

implement <T> Other for Box<T> =
    function makeIt(x: Int32): Int32 = 2

function main(): Unit =
    assert Box<Int32>.makeIt(5) == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous call to 'Box.makeIt'")),
        "the call is genuinely ambiguous, got: {:?}",
        errors
    );
    assert!(
        !errors
            .iter()
            .any(|e| e.contains("use 'Dec.makeIt(...)' to choose one")),
        "the message must not suggest a form the compiler rejects, got: {:?}",
        errors
    );
}
