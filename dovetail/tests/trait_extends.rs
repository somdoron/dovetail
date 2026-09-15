mod common;

// Trait/interface `extends` (trait-design-appendix §1, interface-objects-design §6).
// Model: flattened member sets — `implement B for T` must implement the FULL
// flattened set inline; a separate `implement A for T` is independent.

// ── Checkpoint 1: parsing, flattening, inline completeness ──────────

#[test]
fn test_extends_inline_impl_and_bound_calls() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

function useBeta<T>(v: T): Int32 where T: Beta = v.alpha() + v.beta()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useBeta(r) == 3
"#,
    )
    .expect("inline impl of the flattened set + T: Beta bound calls both members");
}

#[test]
fn test_extends_missing_inherited_member_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function beta(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("missing implementation of method 'alpha' from trait 'Beta'")),
        "the inherited member must be required inline, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_separate_super_impl_does_not_cure() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function alpha(self): Int32 = 1

implement Beta for Rec =
    function beta(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("missing implementation of method 'alpha' from trait 'Beta'")),
        "a separate `implement Alpha` must not satisfy Beta's flattened set, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_generic_super() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T

trait IntProducer extends Producer<Int32> =
    function bonus(self): Int32

implement IntProducer for Rec =
    function produce(self): Int32 = self.x
    function bonus(self): Int32 = 1

function main(): Unit =
    let r = Rec { x = 41 }
    assert r.produce() + r.bonus() == 42
"#,
    )
    .expect("generic super's member types substitute through the extends clause");
}

#[test]
fn test_extends_generic_chain_substitution() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T

trait Chain<U> extends Producer<U> =
    function chained(self): U

implement Chain<Int32> for Rec =
    function produce(self): Int32 = self.x
    function chained(self): Int32 = self.x + 1

function main(): Unit =
    let r = Rec { x = 10 }
    assert r.produce() == 10
    assert r.chained() == 11
"#,
    )
    .expect("super args in terms of the extender's params substitute correctly");
}

#[test]
fn test_extends_multiple_supers_and_transitive() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta =
    function beta(self): Int32

trait Gamma extends Alpha and Beta =
    function gamma(self): Int32

trait Delta extends Gamma =
    function delta(self): Int32

implement Delta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
    function gamma(self): Int32 = 3
    function delta(self): Int32 = 4

function useDelta<T>(v: T): Int32 where T: Delta = v.alpha() + v.beta() + v.gamma() + v.delta()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useDelta(r) == 10
"#,
    )
    .expect("multiple supers and a transitive chain flatten fully");
}

#[test]
fn test_extends_diamond_dedupes() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Root =
    function root(self): Int32

trait LeftSide extends Root =
    function leftSide(self): Int32

trait RightSide extends Root =
    function rightSide(self): Int32

trait Bottom extends LeftSide and RightSide =
    function bottom(self): Int32

implement Bottom for Rec =
    function root(self): Int32 = 1
    function leftSide(self): Int32 = 2
    function rightSide(self): Int32 = 3
    function bottom(self): Int32 = 4

function useBottom<T>(v: T): Int32 where T: Bottom = v.root() + v.leftSide() + v.rightSide() + v.bottom()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useBottom(r) == 10
"#,
    )
    .expect("diamond inheritance flattens the shared root once");
}

#[test]
fn test_class_implements_extended_trait() {
    common::compile_and_run(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

class Thing(value: Int32) implements Beta =
    public function alpha(self: Thing): Int32 = self.value
    public function beta(self: Thing): Int32 = self.value * 2

function main(): Unit =
    let t = Thing(5)
    assert t.alpha() + t.beta() == 15
"#,
    )
    .expect("class implements checks the flattened member set");
}

#[test]
fn test_class_missing_inherited_member_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

class Thing(value: Int32) implements Beta =
    public function beta(self: Thing): Int32 = self.value

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must implement method 'alpha' from trait 'Beta'")),
        "class must implement inherited members too, got: {:?}",
        errors
    );
}

// ── Declaration-site negatives ──────────────────────────────────────

#[test]
fn test_interface_extends_trait_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

interface Bad extends Alpha =
    function bad(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("an interface may extend only interfaces; 'Alpha' is a trait")),
        "expected the interface-extends-only-interfaces error, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_cycle_detected_and_errors_accumulate() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha extends Beta =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

function main(): Unit =
    let x: Int32 = "not an int"
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("'extends' cycle detected")),
        "expected a cycle error, got: {:?}",
        errors
    );
    assert!(
        errors.iter().any(|e| e.contains("expected 'Int32'") || e.contains("mismatch") || e.contains("found")),
        "unrelated errors must still accumulate, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_redundant_redeclaration_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function alpha(self): Int32
    function beta(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("method 'alpha' with the same signature is already inherited from trait 'Alpha'")),
        "redeclaring an inherited signature without a body is redundant, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_return_conflict_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function alpha(self): String
    function beta(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("method 'alpha' conflicts with member inherited from trait 'Alpha'")),
        "same params + different return must conflict, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_cross_super_conflict_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function pick(self): Int32

trait Beta =
    function pick(self): String

trait Gamma extends Alpha and Beta =
    function gamma(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("inherited members 'pick' from traits 'Alpha' and 'Beta' conflict")),
        "cross-super same-params different-return must conflict, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_unknown_super_and_arity() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Producer<T> =
    function produce(self): T

trait BadOne extends Missing =
    function one(self): Int32

trait BadTwo extends Producer =
    function two(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown trait: 'Missing'")),
        "unknown super must error, got: {:?}",
        errors
    );
    assert!(
        errors.iter().any(|e| e.contains("trait 'Producer' expects 1 type argument(s), but 0 were provided")),
        "super arity must be validated, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_duplicate_super_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha and Alpha =
    function beta(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate super trait 'Alpha' in extends clause")),
        "duplicate supers must error, got: {:?}",
        errors
    );
}

// ── Checkpoint 2: B satisfies A everywhere (bounds + dispatch routing) ──

#[test]
fn test_bound_on_super_satisfied_by_sub_impl() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 40
    function beta(self): Int32 = 2

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useAlpha(r) + r.beta() == 42
"#,
    )
    .expect("T: Alpha bound satisfied by the Beta impl; alpha dispatches to Beta's inline member");
}

#[test]
fn test_direct_impl_wins_over_provider() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function alpha(self): Int32 = 100

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useAlpha(r) == 100
"#,
    )
    .expect("a direct Alpha impl wins over the Beta provider in Alpha contexts");
}

#[test]
fn test_provider_ambiguity_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

trait Gamma extends Alpha =
    function gamma(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

implement Gamma for Rec =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useAlpha(r) == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous implementations of trait 'Alpha'") && e.contains("'Beta'") && e.contains("'Gamma'")),
        "two distinct providers with no direct impl must be ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_class_implements_sub_satisfies_super_bound() {
    common::compile_and_run(
        r#"
package a

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

class Thing(value: Int32) implements Beta =
    public function alpha(self: Thing): Int32 = self.value
    public function beta(self: Thing): Int32 = self.value * 2

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let t = Thing(7)
    assert useAlpha(t) == 7
"#,
    )
    .expect("class implements Beta satisfies a T: Alpha bound");
}

#[test]
fn test_generic_super_bound_via_sub_impl() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T

trait IntProducer extends Producer<Int32> =
    function bonus(self): Int32

implement IntProducer for Rec =
    function produce(self): Int32 = self.x
    function bonus(self): Int32 = 1

function drain<T>(v: T): Int32 where T: Producer<Int32> = v.produce()

function main(): Unit =
    let r = Rec { x = 9 }
    assert drain(r) == 9
"#,
    )
    .expect("T: Producer<Int32> satisfied via the IntProducer impl with substituted args");
}

#[test]
fn test_generic_super_bound_wrong_args_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T

trait IntProducer extends Producer<Int32> =
    function bonus(self): Int32

implement IntProducer for Rec =
    function produce(self): Int32 = self.x
    function bonus(self): Int32 = 1

function drain<T>(v: T): String where T: Producer<String> = v.produce()

function main(): Unit =
    let r = Rec { x = 9 }
    assert drain(r) == "s"
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("does not implement trait 'Producer<String>'")),
        "closure substitution must reject mismatched super args, got: {:?}",
        errors
    );
}

// ── Checkpoint 3: interface objects with extends ────────────────────

#[test]
fn test_interface_object_extends_calls() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 40
    function beta(self): Int32 = 2

function main(): Unit =
    let b: Beta = Rec { x = 0 }
    assert b.alpha() + b.beta() == 42
"#,
    )
    .expect("B-object dispatches own and inherited members");
}

#[test]
fn test_interface_object_upcast() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 7
    function beta(self): Int32 = 2

function useAlpha(v: Alpha): Int32 = v.alpha()

function main(): Unit =
    let b: Beta = Rec { x = 0 }
    let viaParam = useAlpha(b)
    let a: Alpha = b
    assert viaParam + a.alpha() == 14
"#,
    )
    .expect("B-object upcasts to A-object via param and let coercion");
}

#[test]
fn test_interface_object_concrete_to_super_coercion() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 5
    function beta(self): Int32 = 2

function main(): Unit =
    let a: Alpha = Rec { x = 0 }
    assert a.alpha() == 5
"#,
    )
    .expect("concrete coerces straight to a super interface via the provider block");
}

#[test]
fn test_interface_object_transitive_upcast() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

interface Gamma extends Beta =
    function gamma(self): Int32

implement Gamma for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
    function gamma(self): Int32 = 3

function main(): Unit =
    let g: Gamma = Rec { x = 0 }
    let a: Alpha = g
    assert g.alpha() + g.beta() + g.gamma() == 6
    assert a.alpha() == 1
"#,
    )
    .expect("transitive upcast navigates the nested super chain");
}

#[test]
fn test_intersection_with_extended_component() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

interface Other =
    function other(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

implement Other for Rec =
    function other(self): Int32 = 4

function useBoth(v: Beta and Other): Int32 = v.alpha() + v.beta() + v.other()

function toAlpha(v: Beta and Other): Alpha = v

function main(): Unit =
    let r = Rec { x = 0 }
    assert useBoth(r) == 7
    let a: Alpha = toAlpha(r)
    assert a.alpha() == 1
"#,
    )
    .expect("intersections containing an extended component dispatch inherited members and upcast");
}

#[test]
fn test_bound_dispatch_on_extended_object() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 40
    function beta(self): Int32 = 2

function callAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let b: Beta = Rec { x = 0 }
    assert callAlpha(b) == 40
"#,
    )
    .expect("a B-object satisfies bound T: Alpha and dispatches dynamically");
}

#[test]
fn test_inherited_bare_self_return() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function bump(self): Self
    function value(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function bump(self): Rec = Rec { x = self.x + 1 }
    function value(self): Int32 = self.x
    function beta(self): Int32 = 100

function main(): Unit =
    let b: Beta = Rec { x = 1 }
    let bumped = b.bump()
    assert bumped.value() == 2
"#,
    )
    .expect("an inherited bare-Self member re-boxes as the origin interface");
}

#[test]
fn test_provider_ambiguity_at_coercion() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

interface Gamma extends Alpha =
    function gamma(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

implement Gamma for Rec =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4

function main(): Unit =
    let a: Alpha = Rec { x = 0 }
    assert a.alpha() == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous implementations of trait 'Alpha'")),
        "coercing with two providers and no direct impl must be ambiguous, got: {:?}",
        errors
    );
}

// ── Review-round regressions ────────────────────────────────────────

#[test]
fn test_concrete_generic_super_coercion() {
    // Sub extends Super<ConcreteArgs>: the provider block's functions are
    // mangled with the provider's own (empty) trait args, not the super's
    // substituted ones.
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Producer<T> =
    function produce(self): T

interface IntProducer extends Producer<Int32> =
    function bonus(self): Int32

implement IntProducer for Rec =
    function produce(self): Int32 = 41
    function bonus(self): Int32 = 1

function main(): Unit =
    let ip: IntProducer = Rec { x = 41 }
    assert ip.produce() + ip.bonus() == 42
"#,
    )
    .expect("coercion to a sub-interface of a concretely-applied generic super");
}

#[test]
fn test_direct_super_impl_wins_regardless_of_coercion_order() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function alpha(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function alpha(self): Int32 = 100

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    let b: Beta = r
    assert b.alpha() == 1
    let a: Alpha = r
    assert a.alpha() == 100
"#,
    )
    .expect("direct super impl owns direct-super contexts independent of coercion order");
}

#[test]
fn test_default_calling_member_through_vtable_generic_impl() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

interface Tagger =
    function tag(self): Int32
    function tagPlus(self): Int32 = self.tag() + 1

implement <T> Tagger for Box<T> =
    function tag(self): Int32 = 10

function main(): Unit =
    let t: Tagger = Box { value = 5 }
    assert t.tagPlus() == 11
"#,
    )
    .expect("a default calling another member dispatches via vtable on a generic impl");
}

#[test]
fn test_cross_namespace_inherited_collision_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function thing(self): Int32

trait Beta =
    property thing(self): String

trait Gamma extends Alpha and Beta =
    function gamma(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("conflicts with method of the same name inherited from trait")
            || e.contains("conflicts with property of the same name inherited from trait")),
        "inherited method/property name collisions across supers must error, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_missing_overload_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self): Int32

trait Beta extends Alpha =
    function foo(self, y: Int32): Int32

implement Beta for Rec =
    function foo(self, y: Int32): Int32 = y

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("missing implementation of method")),
        "implementing one overload must not satisfy another, got: {:?}",
        errors
    );
}

#[test]
fn test_extends_cross_super_same_name_distinct_params_allowed() {
    common::check_no_errors(
        r#"
package a

trait Alpha =
    function foo(self): Int32

trait Beta =
    function foo(self, y: Int32): Int32

trait Gamma extends Alpha and Beta =
    function gamma(self): Int32

function main(): Unit = ()
"#,
    );
}

// ── Round-2 review regressions ──────────────────────────────────────

#[test]
fn test_generic_impl_interface_coercion() {
    common::compile_and_run(
        r#"
package a

record Wrap<T> =
    item: T

interface Alpha<T> =
    function first(self): T

implement <T> Alpha<T> for Wrap<T> =
    function first(self): T = self.item

function main(): Unit =
    let ai: Alpha<Int32> = Wrap { item = 5 }
    assert ai.first() == 5
"#,
    )
    .expect("generic impl block boxes into an interface object");
}

#[test]
fn test_sibling_interface_instantiation_coercion() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T

implement Alpha<Int32> for Rec =
    function get(self): Int32 = self.x

implement Alpha<String> for Rec =
    function get(self): String = "str"

function main(): Unit =
    let r = Rec { x = 9 }
    let asx: Alpha<String> = r
    let aix: Alpha<Int32> = r
    assert asx.get() == "str"
    assert aix.get() == 9
"#,
    )
    .expect("each sibling coercion uses its own block's members");
}

#[test]
fn test_providers_of_distinct_super_instantiations_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T

interface IntSide extends Alpha<Int32> =
    function intSide(self): Int32

interface StrSide extends Alpha<String> =
    function strSide(self): Int32

implement IntSide for Rec =
    function get(self): Int32 = self.x
    function intSide(self): Int32 = 1

implement StrSide for Rec =
    function get(self): String = "s"
    function strSide(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 4 }
    let ai: Alpha<Int32> = r
    let astr: Alpha<String> = r
    assert ai.get() == 4
    assert astr.get() == "s"
"#,
    )
    .expect("providers of different super instantiations never compete");
}

#[test]
fn test_via_only_self_rebox_uses_own_provider() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Growable =
    function grow(self): Self
    function who(self): Int32

interface LeftG extends Growable =
    function leftG(self): Int32

interface RightG extends Growable =
    function rightG(self): Int32

implement LeftG for Rec =
    function grow(self): Rec = Rec { x = self.x }
    function who(self): Int32 = 1
    function leftG(self): Int32 = 0

implement RightG for Rec =
    function grow(self): Rec = Rec { x = self.x }
    function who(self): Int32 = 2
    function rightG(self): Int32 = 0

function main(): Unit =
    let li: LeftG = Rec { x = 0 }
    let ri: RightG = Rec { x = 0 }
    assert li.grow().who() == 1
    assert ri.grow().who() == 2
"#,
    )
    .expect("an inherited Self re-box carries its own provider's members when no direct impl exists");
}

#[test]
fn test_trait_static_via_provider() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function zero(): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function zero(): Int32 = 99
    function beta(self): Int32 = 1

function main(): Unit =
    assert Alpha.zero() == 99
    assert Beta.zero() == 99
"#,
    )
    .expect("explicit super-trait statics route through the provider block");
}

// ── Round-3 review regressions ──────────────────────────────────────

#[test]
fn test_sibling_direct_super_does_not_hijack_provider() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    v: T

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Box<String> =
    function alpha(self): Int32 = 3
    function beta(self): Int32 = 4

implement Alpha for Box<Int32> =
    function alpha(self): Int32 = 10

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let bs = Box { v = "s" }
    let bi = Box { v = 1 }
    assert useAlpha(bs) == 3
    assert Alpha.alpha(bs) == 3
    assert useAlpha(bi) == 10
"#,
    )
    .expect("a sibling instantiation's direct super block must not hijack provider routing");
}

#[test]
fn test_sibling_self_return_rebox() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T
    function dup(self): Self

implement Alpha<Int32> for Rec =
    function get(self): Int32 = self.x
    function dup(self): Rec = Rec { x = self.x + 1 }

implement Alpha<String> for Rec =
    function get(self): String = "str"
    function dup(self): Rec = Rec { x = self.x + 10 }

function main(): Unit =
    let r = Rec { x = 1 }
    let ai: Alpha<Int32> = r
    let astr: Alpha<String> = r
    assert ai.dup().get() == 2
    assert astr.dup().get() == "str"
"#,
    )
    .expect("Self-returns on sibling-tagged coercions re-box through their own vtables");
}

#[test]
fn test_annotated_branch_keeps_direct_dispatch() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T

interface Sub extends Alpha<Int32> =
    function extra(self): Int32

implement Alpha<Int32> for Rec =
    function get(self): Int32 = self.x

implement Sub for Rec =
    function get(self): Int32 = self.x + 1000
    function extra(self): Int32 = 5

function pick(n: Int32, r: Rec): Alpha<Int32> =
    match n with
    case 0 => r
    case _ =>
        let s: Sub = r
        s

function pickIf(flag: Bool, r: Rec): Alpha<Int32> =
    if flag then r
    else
        let s: Sub = r
        s

function main(): Unit =
    let r = Rec { x = 2 }
    assert pick(0, r).get() == 2
    assert pick(1, r).get() == 1002
    assert pickIf(true, r).get() == 2
"#,
    )
    .expect("an annotated interface-object context keeps direct-impl dispatch for concrete arms");
}

#[test]
fn test_explicit_super_call_on_sub_object() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function foo(self): Int32

interface Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function foo(self): Int32 = 3
    function beta(self): Int32 = 4

function main(): Unit =
    let b: Beta = Rec { x = 0 }
    assert Alpha.foo(b) == 3
"#,
    )
    .expect("explicit super-trait calls route through sub-interface objects");
}

// ── Round-4 review regressions ──────────────────────────────────────

#[test]
fn test_bound_provider_not_hijacked_by_sibling_super_block() {
    common::compile_and_run(
        r#"
package a

trait Alpha<T> =
    function produce(self): T
    function tag(self): Int32

trait Beta<T> extends Alpha<T> =
    function beta(self): Int32

record Rec =
    x: Int32

implement Alpha<String> for Rec =
    function produce(self): String = "s"
    function tag(self): Int32 = 200

implement Beta<Int32> for Rec =
    function produce(self): Int32 = 7
    function tag(self): Int32 = 100
    function beta(self): Int32 = 5

function viaBound<T>(v: T): Int32 where T: Alpha<Int32> = v.tag()

function main(): Unit =
    assert viaBound(Rec { x = 1 }) == 100
"#,
    )
    .expect("a sibling super instantiation's direct block must not hijack a bound routed to the provider");
}

#[test]
fn test_static_property_via_provider() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Base =
    property baseMark: Int32

trait Derived extends Base =
    function derived(self): Int32

implement Derived for Rec =
    property baseMark: Int32 = 7
    function derived(self): Int32 = 1

function main(): Unit =
    assert Base.baseMark == 7
    assert Derived.baseMark == 7
"#,
    )
    .expect("static properties route through sub-trait provider blocks");
}

// ── Round-8 review regressions ──────────────────────────────────────

#[test]
fn test_explicit_args_select_among_sibling_provider_instantiations() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

trait Prov<T> extends Conv<T> =
    function p(self): Int32

implement Prov<Int32> for Rec =
    function tag(self): Int32 = 1
    function p(self): Int32 = 0

implement Prov<Bool> for Rec =
    function tag(self): Int32 = 2
    function p(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert Conv<Int32>.tag(r) == 1
    assert Conv<Bool>.tag(r) == 2
"#,
    )
    .expect("explicit args must select among sibling instantiations of ONE provider trait");
}

#[test]
fn test_self_return_rebox_prefers_direct_impl_without_direct_coercion() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function bump(self): Self
    function tag(self): Int32

interface Beta extends Alpha =
    function extra(self): Int32

implement Alpha for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 1

implement Beta for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 2
    function extra(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    let b: Beta = r
    assert b.bump().tag() == 1
"#,
    )
    .expect("the direct impl owns the (type, super) re-box vtable even with no direct coercion in the program");
}

#[test]
fn test_self_return_rebox_prefers_direct_impl_with_direct_coercion() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function bump(self): Self
    function tag(self): Int32

interface Beta extends Alpha =
    function extra(self): Int32

implement Alpha for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 1

implement Beta for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 2
    function extra(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    let unrelated: Alpha = r
    assert unrelated.tag() == 1
    let b: Beta = r
    assert b.bump().tag() == 1
"#,
    )
    .expect("re-box dispatch must not flip when an unrelated direct coercion exists");
}

#[test]
fn test_static_call_wrong_provider_application_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Maker<T> =
    function make(v: Int32): Int32

trait ProvI extends Maker<Int32> =
    function q(self): Int32

implement ProvI for Rec =
    function make(v: Int32): Int32 = v + 1
    function q(self): Int32 = 0

function main(): Unit =
    let x = Maker<Bool>.make(10)
    assert x == 11
"#,
    );
    assert!(
        !errors.is_empty(),
        "Maker<Bool>.make must not run a Maker<Int32> provider's body"
    );
}

#[test]
fn test_static_call_explicit_args_select_among_providers() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Maker<T> =
    function make(v: Int32): Int32

trait ProvI extends Maker<Int32> =
    function qi(self): Int32

trait ProvB extends Maker<Bool> =
    function qb(self): Int32

implement ProvI for Rec =
    function make(v: Int32): Int32 = v + 1
    function qi(self): Int32 = 0

implement ProvB for Rec =
    function make(v: Int32): Int32 = v + 2
    function qb(self): Int32 = 0

function main(): Unit =
    assert Maker<Int32>.make(10) == 11
    assert Maker<Bool>.make(10) == 12
"#,
    )
    .expect("explicit args uniquely select among static providers of different applications");
}

#[test]
fn test_use_routes_through_subtrait_provider() {
    common::compile_and_run(
        r#"
package a

record Holder =
    value: Int32

trait MyRes extends Usable<Int32, Never> =
    function label(self): Int32

implement MyRes for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)
    function label(self): Int32 = 0

function main(): Unit =
    let h = Holder { value = 42 }
    let x: Int32 = use h
    assert x == 42
"#,
    )
    .expect("`use` must accept a type whose Usable comes from a sub-trait provider");
}

// ── Round-9 review regressions ──────────────────────────────────────

#[test]
fn test_generic_direct_other_application_does_not_hide_provider() {
    common::compile_and_run(
        r#"
package a

record Wrap<T> =
    v: T

trait Conv<T> =
    function tag(self): Int32

trait Prov extends Conv<Bool> =
    function p(self): Int32

implement <T> Conv<T> for Wrap<T> =
    function tag(self): Int32 = 1

implement Prov for Wrap<Int32> =
    function tag(self): Int32 = 2
    function p(self): Int32 = 0

function main(): Unit =
    let w = Wrap<Int32> { v = 5 }
    assert Conv<Int32>.tag(w) == 1
    assert Conv<Bool>.tag(w) == 2
"#,
    )
    .expect("a generic direct impl unifying to another application must not hide the requested provider");
}

#[test]
fn test_explicit_args_match_generic_provider() {
    common::compile_and_run(
        r#"
package a

record Wrap<T> =
    v: T

trait Conv<T> =
    function tag(self): Int32

trait Prov<X> extends Conv<X> =
    function p(self): Int32

implement <T> Prov<T> for Wrap<T> =
    function tag(self): Int32 = 4
    function p(self): Int32 = 0

function main(): Unit =
    let w = Wrap<Int32> { v = 5 }
    assert w.tag() == 4
    assert Conv<Int32>.tag(w) == 4
"#,
    )
    .expect("explicit trait args must unify with a generic provider's closure args");
}

#[test]
fn test_use_ambiguous_providers_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Holder =
    value: Int32

trait ResA extends Usable<Int32, Never> =
    function la(self): Int32

trait ResB extends Usable<Int32, Never> =
    function lb(self): Int32

implement ResA for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)
    function la(self): Int32 = 0

implement ResB for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value + 1)
    function lb(self): Int32 = 0

function main(): Unit =
    let h = Holder { value = 42 }
    let x: Int32 = use h
    assert x == 42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous implementations of trait 'Usable'")),
        "two distinct Usable providers must be a use-site ambiguity error, got: {:?}",
        errors
    );
}

#[test]
fn test_use_via_generic_subtrait_provider() {
    common::compile_and_run(
        r#"
package a

record Holder<T> =
    value: T

trait MyRes<T> extends Usable<T, Never> =
    function label(self): Int32

implement <T> MyRes<T> for Holder<T> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Holder<T>, f: (T) => U, _errorF: (Never) => E2): U = f(self.value)
    function label(self): Int32 = 0

function main(): Unit =
    let h = Holder<Int32> { value = 42 }
    let x: Int32 = use h
    assert x == 42
"#,
    )
    .expect("`use` through a GENERIC sub-trait provider block must monomorphize");
}

#[test]
fn test_default_body_explicit_call_on_local_not_retargeted() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function probe(self): Int32 =
        let r = Rec { x = 0 }
        Alpha.alpha(r)

implement Alpha for Rec =
    function alpha(self): Int32 = 1

implement Beta for Rec =
    function alpha(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert Alpha.alpha(r) == 1
    assert Beta.probe(r) == 1
    assert Beta.alpha(r) == 2
"#,
    )
    .expect("an explicit disambiguation on a LOCAL in a default body keeps its typecheck resolution");
}

#[test]
fn test_rebox_not_hijacked_by_other_application_direct() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Conv<T> =
    function bump(self): Self
    function tag(self): Int32

interface Prov extends Conv<Bool> =
    function p(self): Int32

implement Conv<Int32> for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 1

implement Prov for Rec =
    function bump(self): Rec = self
    function tag(self): Int32 = 2
    function p(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    let ci: Conv<Int32> = r
    assert ci.tag() == 1
    let pv: Prov = r
    assert pv.bump().tag() == 2
"#,
    )
    .expect("a direct impl of a DIFFERENT super application must not own a via wrapper's re-box");
}

// ── Round-10 review regressions ─────────────────────────────────────

#[test]
fn test_intersection_different_origin_applications_ambiguous_method() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T

interface Beta extends Alpha<Bool> =
    function b(self): Int32

interface Gamma extends Alpha<Int32> =
    function g(self): Int32

implement Beta for Rec =
    function get(self): Bool = true
    function b(self): Int32 = 0

implement Gamma for Rec =
    function get(self): Int32 = 5
    function g(self): Int32 = 0

function pick(v: Beta and Gamma): Bool = v.get()

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous method 'get'")),
        "different origin APPLICATIONS must stay ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_different_origin_applications_ambiguous_property() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    property size(self): T

interface Beta extends Alpha<Bool> =
    function b(self): Int32

interface Gamma extends Alpha<Int32> =
    function g(self): Int32

implement Beta for Rec =
    property size(self): Bool = true
    function b(self): Int32 = 0

implement Gamma for Rec =
    property size(self): Int32 = 5
    function g(self): Int32 = 0

function pick(v: Beta and Gamma): Bool = v.size

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous property 'size'")),
        "different origin APPLICATIONS must stay ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_same_origin_application_still_dedupes() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha<T> =
    function get(self): T

interface Beta extends Alpha<Int32> =
    function b(self): Int32

implement Alpha<Int32> for Rec =
    function get(self): Int32 = 7

implement Beta for Rec =
    function get(self): Int32 = 8
    function b(self): Int32 = 0

function pick(v: Alpha<Int32> and Beta): Int32 = v.get()

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 7
"#,
    )
    .expect("same origin application with the origin as a component dedupes to the origin's slots");
}

// ── Round-11 review regressions ─────────────────────────────────────

#[test]
fn test_provider_ambiguity_not_masked_by_other_application_direct() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Conv<T> =
    function tag(self): Int32

trait ProvA extends Conv<Bool> =
    function pa(self): Int32

trait ProvB extends Conv<Bool> =
    function pb(self): Int32

implement Conv<Int32> for Rec =
    function tag(self): Int32 = 0

implement ProvA for Rec =
    function tag(self): Int32 = 1
    function pa(self): Int32 = 0

implement ProvB for Rec =
    function tag(self): Int32 = 2
    function pb(self): Int32 = 0

function pick<T>(v: T): Int32 where T: Conv<Bool> = v.tag()

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous implementations of trait 'Conv'")),
        "a direct impl of a DIFFERENT application must not mask provider ambiguity, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_dispatch_prefers_exact_component_regardless_of_name_order() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Zup =
    function m(self): Int32

interface Alpha extends Zup =
    function extra(self): Int32

implement Zup for Rec =
    function m(self): Int32 = 1

implement Alpha for Rec =
    function m(self): Int32 = 2
    function extra(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    let both: Alpha and Zup = r
    assert both.m() == 1
"#,
    )
    .expect("dispatch must go through the EXACT deduped component, not the first that can reach it");
}

#[test]
fn test_use_sibling_instantiation_providers_not_ambiguous() {
    // check-only: running this trips a PRE-EXISTING codegen bug (generic
    // method in a non-generic block over a generic-record instantiation via
    // `use` — see docs/Backlog.md); the regression under test is the
    // typecheck-level spurious ambiguity.
    common::check_no_errors(
        r#"
package a

record Wrap<T> =
    value: T

trait ResA extends Usable<Int32, Never> =
    function la(self): Int32

trait ResB extends Usable<Int32, Never> =
    function lb(self): Int32

implement ResA for Wrap<Int32> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Wrap<Int32>, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)
    function la(self): Int32 = 0

implement ResB for Wrap<String> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Wrap<String>, f: (Int32) => U, _errorF: (Never) => E2): U = f(7)
    function lb(self): Int32 = 0

function main(): Unit =
    let h = Wrap<Int32> { value = 42 }
    let x: Int32 = use h
    assert x == 42
"#,
    );
}

#[test]
fn test_bound_sibling_instantiation_providers_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

record Wrap<T> =
    value: T

trait Zup =
    function m(self): Int32

trait SubA extends Zup =
    function sa(self): Int32

trait SubB extends Zup =
    function sb(self): Int32

implement SubA for Wrap<Int32> =
    function m(self): Int32 = 11
    function sa(self): Int32 = 0

implement SubB for Wrap<String> =
    function m(self): Int32 = 22
    function sb(self): Int32 = 0

function callM<T>(v: T): Int32 where T: Zup = v.m()

function main(): Unit =
    let w = Wrap<Int32> { value = 1 }
    assert callM(w) == 11
"#,
    )
    .expect("sibling-instantiation providers must not fail a valid generic bound");
}

// ── Round-12 review regressions ─────────────────────────────────────

#[test]
fn test_provider_ambiguity_not_masked_by_sibling_for_type_direct() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Wrap<T> =
    value: T

trait Zup =
    function m(self): Int32

trait SubA extends Zup =
    function sa(self): Int32

trait SubB extends Zup =
    function sb(self): Int32

implement Zup for Wrap<Int32> =
    function m(self): Int32 = 0

implement SubA for Wrap<String> =
    function m(self): Int32 = 1
    function sa(self): Int32 = 0

implement SubB for Wrap<String> =
    function m(self): Int32 = 2
    function sb(self): Int32 = 0

function callM<T>(v: T): Int32 where T: Zup = v.m()

function main(): Unit =
    let w = Wrap<String> { value = "x" }
    assert callM(w) == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous implementations of trait 'Zup'")),
        "a direct impl for a SIBLING for_type must not mask provider ambiguity, got: {:?}",
        errors
    );
}

#[test]
fn test_explicit_call_on_wrong_sibling_for_type_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Wrap<T> =
    value: T

trait Conv =
    function tag(self): Int32

implement Conv for Wrap<Int32> =
    function tag(self): Int32 = 1

function main(): Unit =
    let w = Wrap<String> { value = "x" }
    assert Conv.tag(w) == 1
"#,
    );
    assert!(
        !errors.is_empty(),
        "an explicit call on a receiver of a DIFFERENT sibling for_type must be a type error"
    );
}

#[test]
fn test_use_provider_not_hidden_by_sibling_direct() {
    common::check_no_errors(
        r#"
package a

record Wrap<T> =
    value: T

trait MyRes extends Usable<Int32, Never> =
    function label(self): Int32

implement Usable<Int32, Never> for Wrap<Int32> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Wrap<Int32>, f: (Int32) => U, _errorF: (Never) => E2): U = f(self.value)

implement MyRes for Wrap<String> =
    type Wrapped<U, E2> = U
    function use<U, E2>(self: Wrap<String>, f: (Int32) => U, _errorF: (Never) => E2): U = f(9)
    function label(self): Int32 = 0

function main(): Unit =
    let h = Wrap<String> { value = "x" }
    let x: Int32 = use h
    assert x == 9
"#,
    );
}

// ── Round-13 review regressions ─────────────────────────────────────

#[test]
fn test_explicit_wrong_application_on_generic_class_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Conv<T> =
    function tag(self): Int32

class Holder<T>(v: T) implements Conv<T> =
    public function tag(self: Holder<T>): Int32 = 9

function main(): Unit =
    let h = Holder<Int32>(1)
    assert Conv<Bool>.tag(h) == 9
"#,
    );
    assert!(
        !errors.is_empty(),
        "Conv<Bool>.tag on Holder<Int32> implements Conv<Int32> must be rejected"
    );
}

#[test]
fn test_property_from_two_bounds_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property size(self): Int32

trait Beta =
    property size(self): Int32

implement Alpha for Rec =
    property size(self): Int32 = 1

implement Beta for Rec =
    property size(self): Int32 = 2

function pick<T>(v: T): Int32 where T: Alpha + Beta = v.size

function main(): Unit =
    let r = Rec { x = 0 }
    assert pick(r) == 1
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous property 'size'")),
        "a property declared by two bounds must be ambiguous, got: {:?}",
        errors
    );
}

#[test]
fn test_explicitly_typed_static_selects_sibling_block() {
    common::compile_and_run(
        r#"
package a

record Wrap<T> =
    value: T

trait Maker =
    function make(): Int32

implement Maker for Wrap<Int32> =
    function make(): Int32 = 1

implement Maker for Wrap<String> =
    function make(): Int32 = 2

function main(): Unit =
    assert Wrap<Int32>.make() == 1
    assert Wrap<String>.make() == 2
"#,
    )
    .expect("Wrap<Int32>.make() must select the exact sibling block");
}

// ── Round-14 review regressions ─────────────────────────────────────

#[test]
fn test_inherited_member_via_two_bounds_not_ambiguous() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32
    property tag(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function alpha(self): Int32 = 1
    property tag(self): Int32 = 7

implement Beta for Rec =
    function alpha(self): Int32 = 2
    property tag(self): Int32 = 8
    function beta(self): Int32 = 0

function callAlpha<T>(v: T): Int32 where T: Alpha + Beta = v.alpha()

function readTag<T>(v: T): Int32 where T: Alpha + Beta = v.tag

function main(): Unit =
    let r = Rec { x = 0 }
    assert callAlpha(r) == 1
    assert readTag(r) == 7
"#,
    )
    .expect("an inherited member reached through both bounds is one declaration, dispatched via its origin");
}

// ── Round-17 review regressions ─────────────────────────────────────

#[test]
fn test_concrete_call_direct_impl_wins_over_subtrait() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32
    property atag(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function alpha(self): Int32 = 100
    property atag(self): Int32 = 100

implement Beta for Rec =
    function alpha(self): Int32 = 1
    property atag(self): Int32 = 1
    function beta(self): Int32 = 2

function useAlpha<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useAlpha(r) == 100
    assert Alpha.alpha(r) == 100
    assert r.alpha() == 100
    assert r.atag == 100
"#,
    )
    .expect("all three resolution paths must agree: the direct impl of the origin wins");
}

#[test]
fn test_two_subtraits_of_one_super_still_ambiguous_on_concrete_receiver() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

trait Gamma extends Alpha =
    function gamma(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 0

implement Gamma for Rec =
    function alpha(self): Int32 = 2
    function gamma(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.alpha() == 1
"#,
    );
    assert!(
        !errors.is_empty(),
        "two sub-traits with no direct super impl must stay ambiguous on a concrete receiver"
    );
}

// ── Round-18 review regressions ─────────────────────────────────────

#[test]
fn test_sibling_for_type_does_not_discard_applicable_block() {
    common::compile_and_run(r#"
package a

record Wrap<T> =
    item: T

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Wrap<String> =
    function alpha(self): Int32 = 1

implement Beta for Wrap<Int32> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    let w = Wrap<Int32> { item = 5 }
    assert w.alpha() == 2
"#).expect("a sibling for_type block must not discard the applicable one");
}

#[test]
fn test_unrelated_same_name_trait_does_not_defeat_dedup() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self, x: Int32): Int32

trait Beta extends Alpha =
    function beta(self): Int32

trait Gamma =
    function foo(self, s: String): Int32

implement Alpha for Rec =
    function foo(self, x: Int32): Int32 = 1

implement Beta for Rec =
    function foo(self, x: Int32): Int32 = 2
    function beta(self): Int32 = 9

implement Gamma for Rec =
    function foo(self, s: String): Int32 = 3

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.foo(5) == 1
    assert r.foo("hi") == 3
"#).expect("an unrelated same-named member must not defeat the origin dedup");
}

#[test]
fn test_generic_origin_application_dedups() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T

trait IntProducer extends Producer<Int32> =
    function bonus(self): Int32

implement Producer<Int32> for Rec =
    function produce(self): Int32 = 1

implement IntProducer for Rec =
    function produce(self): Int32 = 2
    function bonus(self): Int32 = 3

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.produce() == 1
"#).expect("a GENERIC origin application dedups against its sub-trait");
}

#[test]
fn test_generic_blocks_direct_impl_wins() {
    common::compile_and_run(r#"
package a

record Wrap<T> =
    item: T

trait Alpha =
    function alpha(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement <T> Alpha for Wrap<T> =
    function alpha(self): Int32 = 1

implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    let w = Wrap<Int32> { item = 5 }
    assert w.alpha() == 1
"#).expect("direct-impl-wins holds for generic blocks too");
}

// ── Round-19 review regressions ─────────────────────────────────────

#[test]
fn test_static_property_origin_dedup() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property tag: Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    property tag: Int32 = 1

implement Beta for Rec =
    property tag: Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    assert Rec.tag == 1
"#).expect("static properties get the extends origin dedup too");
}

#[test]
fn test_generic_static_origin_dedup() {
    common::compile_and_run(r#"
package a

record Wrap<T> =
    item: T

trait Alpha =
    function mk(): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement <T> Alpha for Wrap<T> =
    function mk(): Int32 = 1

implement <T> Beta for Wrap<T> =
    function mk(): Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    assert Wrap<Int32>.mk() == 1
"#).expect("generic-block statics get the extends origin dedup too");
}

#[test]
fn test_method_reference_origin_dedup() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function foo(self): Int32 = 1

implement Beta for Rec =
    function foo(self): Int32 = 2
    function beta(self): Int32 = 3

function apply(f: () => Int32): Int32 = f()

function main(): Unit =
    let r = Rec { x = 0 }
    assert apply(r.foo) == 1
"#).expect("method REFERENCES get the extends origin dedup too");
}

// ── Round-20 review regressions ─────────────────────────────────────

#[test]
fn test_static_call_via_bounds_origin_wins() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function mk(): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function mk(): Int32 = 10

implement Beta for Rec =
    function mk(): Int32 = 20
    function beta(self): Int32 = 3

function getA<T>(): Int32 where T: Alpha + Beta = T.mk()
function getB<T>(): Int32 where T: Beta + Alpha = T.mk()

function main(): Unit =
    assert getA<Rec>() == 10
    assert getB<Rec>() == 10
"#).expect("bound ORDER must not decide a static call through bounds");
}

#[test]
fn test_static_property_via_bounds_origin_wins() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    property tag: Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    property tag: Int32 = 1

implement Beta for Rec =
    property tag: Int32 = 2
    function beta(self): Int32 = 3

function getA<T>(): Int32 where T: Alpha + Beta = T.tag
function getB<T>(): Int32 where T: Beta + Alpha = T.tag

function main(): Unit =
    assert getA<Rec>() == 1
    assert getB<Rec>() == 1
"#).expect("bound ORDER must not decide a static property through bounds");
}

#[test]
fn test_generic_static_property_origin_not_declaration_order() {
    common::compile_and_run(r#"
package a

record Wrap<T> =
    item: T

trait Alpha =
    property tag: Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement <T> Beta for Wrap<T> =
    property tag: Int32 = 2
    function beta(self): Int32 = 3

implement <T> Alpha for Wrap<T> =
    property tag: Int32 = 1

function main(): Unit =
    assert Wrap<Int32>.tag == 1
"#).expect("declaration ORDER must not decide a generic static property");
}

#[test]
fn test_static_function_reference_origin_dedup() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function mk(): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function mk(): Int32 = 1

implement Beta for Rec =
    function mk(): Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    let g: () => Int32 = Rec.mk
    assert g() == 1
"#).expect("static function references get the origin dedup");
}

#[test]
fn test_instance_method_reference_via_type_name_origin_dedup() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self): Int32

trait Beta extends Alpha =
    function beta(self): Int32

implement Alpha for Rec =
    function foo(self): Int32 = 1

implement Beta for Rec =
    function foo(self): Int32 = 2
    function beta(self): Int32 = 3

function main(): Unit =
    let f: (Rec) => Int32 = Rec.foo
    assert f(Rec { x = 0 }) == 1
"#).expect("Type.method references get the origin dedup");
}

// ── Round-21 review regressions ─────────────────────────────────────

#[test]
fn test_static_vs_instance_member_across_extends_rejected() {
    let errors = common::compile_expecting_errors(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(): Int32

trait Beta extends Alpha =
    function foo(self): Int32 = 5
    function bar(self): Int32

implement Beta for Rec =
    function bar(self): Int32 = 1

function useAlpha<T>(): Int32 where T: Alpha = T.foo()

function main(): Unit =
    assert useAlpha<Rec>() == 5
"#);
    assert!(
        errors.iter().any(|e| e.contains("one takes 'self' and the other does not")),
        "a static/instance same-name pair across extends must be rejected, got: {:?}",
        errors
    );
}

#[test]
fn test_instance_vs_static_member_across_extends_rejected() {
    let errors = common::compile_expecting_errors(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function foo(self): Int32

trait Beta extends Alpha =
    function foo(): Int32 = 5
    function bar(self): Int32

implement Beta for Rec =
    function bar(self): Int32 = 1

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.foo() == 5
"#);
    assert!(
        errors.iter().any(|e| e.contains("one takes 'self' and the other does not")),
        "the mirror direction must be rejected too, got: {:?}",
        errors
    );
}

#[test]
fn test_static_via_bounds_different_arity_not_ambiguous() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function mk(): Int32

trait Beta =
    function mk(a: Int32, b: Int32): Int32

implement Alpha for Rec =
    function mk(): Int32 = 1

implement Beta for Rec =
    function mk(a: Int32, b: Int32): Int32 = a + b

function callIt<T>(): Int32 where T: Alpha + Beta = T.mk(2, 3)

function main(): Unit =
    assert callIt<Rec>() == 5
"#).expect("only candidates that can accept the call compete");
}

// ── Round-22 review regressions ─────────────────────────────────────

#[test]
fn test_cross_super_static_instance_method_rejected() {
    let errors = common::compile_expecting_errors(r#"
package a

record Rec =
    x: Int32

trait A =
    function f(v: Int32): Int32

trait B =
    function f(self, v: Int32): Int32

trait C extends A and B =
    function g(self): Int32

implement C for Rec =
    function f(v: Int32): Int32 = v + 100
    function g(self): Int32 = 3

function inst<T>(v: T): Int32 where T: C = v.f(1)

function main(): Unit =
    let r = Rec { x = 0 }
    assert inst(r) == 101
"#);
    assert!(
        errors.iter().any(|e| e.contains("one takes 'self' and the other does not")),
        "cross-super static/instance method clash must be rejected, got: {:?}",
        errors
    );
}

#[test]
fn test_cross_super_static_instance_property_rejected() {
    let errors = common::compile_expecting_errors(r#"
package a

record Rec =
    x: Int32

trait A =
    property tag: Int32

trait B =
    property tag(self): Int32

trait C extends A and B =
    function g(self): Int32

implement C for Rec =
    property tag: Int32 = 1
    function g(self): Int32 = 3

function inst<T>(v: T): Int32 where T: C = v.tag

function main(): Unit =
    let r = Rec { x = 0 }
    assert inst(r) == 1
"#);
    assert!(
        errors.iter().any(|e| e.contains("one takes 'self' and the other does not")),
        "cross-super static/instance property clash must be rejected, got: {:?}",
        errors
    );
}

#[test]
fn test_cross_super_identical_members_share_one_slot() {
    common::compile_and_run(r#"
package a

record Rec =
    x: Int32

trait A =
    function f(self): Int32

trait B =
    function f(self): Int32

trait C extends A and B =
    function g(self): Int32

implement C for Rec =
    function f(self): Int32 = 7
    function g(self): Int32 = 3

function useA<T>(v: T): Int32 where T: A = v.f()

function main(): Unit =
    let r = Rec { x = 0 }
    assert useA(r) == 7
"#).expect("two supers declaring an IDENTICAL member still share one slot");
}
