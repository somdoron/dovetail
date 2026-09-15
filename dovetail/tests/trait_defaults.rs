mod common;

// Default method/property bodies in traits (trait-design-appendix §4).

#[test]
fn test_default_method_omitted() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

implement Greeter for Rec =
    function name(self): Int32 = 5

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.greet() == 105
"#,
    )
    .expect("omitted default method uses the trait body (which calls an abstract member)");
}

#[test]
fn test_default_method_overridden_by_impl() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Greeter =
    function greet(self): Int32 = 1

implement Greeter for Rec =
    function greet(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.greet() == 2
"#,
    )
    .expect("the impl's own body beats the default");
}

#[test]
fn test_default_property_omitted() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Sized =
    function label(self): Int32
    property size(self): Int32 = 7

implement Sized for Rec =
    function label(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.size == 7
"#,
    )
    .expect("omitted default property uses the trait body");
}

#[test]
fn test_default_calls_default() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Chain =
    function base(self): Int32
    function middle(self): Int32 = self.base() * 2
    function top(self): Int32 = self.middle() + 1

implement Chain for Rec =
    function base(self): Int32 = 10

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.top() == 21
"#,
    )
    .expect("a default body can call another default");
}

#[test]
fn test_default_reads_trait_property() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Shaped =
    property area(self): Int32
    function doubleArea(self): Int32 = self.area * 2

implement Shaped for Rec =
    property area(self): Int32 = self.x

function main(): Unit =
    let r = Rec { x = 21 }
    assert r.doubleArea() == 42
"#,
    )
    .expect("a default body reads self.property via the trait bound");
}

#[test]
fn test_default_on_generic_trait() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Producer<T> =
    function produce(self): T
    function produceTwice(self): (T, T) = (self.produce(), self.produce())

implement Producer<Int32> for Rec =
    function produce(self): Int32 = self.x

function main(): Unit =
    let r = Rec { x = 3 }
    let (a, b) = r.produceTwice()
    assert a + b == 6
"#,
    )
    .expect("defaults on generic traits substitute the trait's type params");
}

#[test]
fn test_default_omitted_in_generic_impl() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

trait Tagger =
    function tag(self): Int32
    function tagPlus(self): Int32 = self.tag() + 1

implement <T> Tagger for Box<T> =
    function tag(self): Int32 = 10

function main(): Unit =
    let a = Box { value = 5 }
    let b = Box { value = "s" }
    assert a.tagPlus() == 11
    assert b.tagPlus() == 11
"#,
    )
    .expect("generic impls omit defaults; each instantiation materializes its copy");
}

#[test]
fn test_default_via_interface_object() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

implement Greeter for Rec =
    function name(self): Int32 = 5

function main(): Unit =
    let g: Greeter = Rec { x = 0 }
    assert g.greet() == 105
"#,
    )
    .expect("a defaulted member dispatches through the interface-object vtable");
}

#[test]
fn test_default_static_method() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Maker =
    function label(self): Int32
    function base(): Int32 = 40

implement Maker for Rec =
    function label(self): Int32 = 0

function main(): Unit =
    assert Rec.base() == 40
"#,
    )
    .expect("self-less default members resolve as statics");
}

#[test]
fn test_missing_member_without_default_still_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = 1

implement Greeter for Rec =
    function greet(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("missing implementation of method 'name' from trait 'Greeter'")),
        "members without defaults must still be required, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_method_default_allowed() {
    common::check_no_errors(
        r#"
package a

trait Bad =
    function pick<T>(self, v: T): T = v

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_intrinsic_default_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Bad =
    function magic(self): Int32 = intrinsic

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("default body of 'magic' in trait 'Bad' cannot be intrinsic")),
        "intrinsic defaults are rejected, got: {:?}",
        errors
    );
}

// ── extends interplay ───────────────────────────────────────────────

#[test]
fn test_inherited_default_used() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32 = 1

trait Beta extends Alpha =
    function beta(self): Int32

implement Beta for Rec =
    function beta(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.alpha() + r.beta() == 3
"#,
    )
    .expect("an inherited default satisfies the flattened requirement");
}

#[test]
fn test_override_beats_inherited_default() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32 = 1

trait Beta extends Alpha =
    function alpha(self): Int32 = 10
    function beta(self): Int32

implement Beta for Rec =
    function beta(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.alpha() == 10
"#,
    )
    .expect("a same-signature redeclaration WITH a body overrides the super's default");
}

#[test]
fn test_impl_beats_all_defaults() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function alpha(self): Int32 = 1

trait Beta extends Alpha =
    function alpha(self): Int32 = 10
    function beta(self): Int32

implement Beta for Rec =
    function alpha(self): Int32 = 100
    function beta(self): Int32 = 2

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.alpha() == 100
"#,
    )
    .expect("the impl's own definition beats both defaults");
}

// ── Classes may omit defaulted members (non-generic classes) ────────

#[test]
fn test_class_omits_default_method() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Person(id: Int32) implements Greeter =
    public function name(self: Person): Int32 = self.id

function main(): Unit =
    let p = Person(5)
    assert p.greet() == 105
"#,
    )
    .expect("a class omits a defaulted method; direct call works");
}

#[test]
fn test_class_omits_default_via_interface_object() {
    common::compile_and_run(
        r#"
package a

interface Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Person(id: Int32) implements Greeter =
    public function name(self: Person): Int32 = self.id

function main(): Unit =
    let g: Greeter = Person(7)
    assert g.greet() == 107
"#,
    )
    .expect("a class-omitted default dispatches through the interface vtable");
}

#[test]
fn test_class_own_method_beats_default() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self): Int32 = 1

class Person(id: Int32) implements Greeter =
    public function greet(self: Person): Int32 = 2

function main(): Unit =
    let p = Person(0)
    assert p.greet() == 2
"#,
    )
    .expect("the class's own method beats the default");
}

#[test]
fn test_generic_class_inherits_defaults() {
    common::check_no_errors(
        r#"
package a

trait Greeter =
    function greet(self): Int32 = 1

class Box<T>(value: T) implements Greeter =
    public function ignored(self: Box<T>): T = self.value

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_inherited_default_from_generic_super() {
    // The default template is written in the origin trait's type params; the
    // materialization must bind THOSE (Wrap's T := Int32), not the block
    // trait's params.
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Wrap<T> =
    function base(self): T
    function twice(self, v: T): T = v

trait SubWrap extends Wrap<Int32> =
    function extra(self): Int32

implement SubWrap for Rec =
    function base(self): Int32 = 3
    function extra(self): Int32 = 4

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.twice(5) == 5
"#,
    )
    .expect("inherited default from a concretely-applied generic super");
}

#[test]
fn test_inherited_default_generic_chain_renamed_param() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Wrap<T> =
    function base(self): T
    function twice(self, v: T): T = v

trait SubWrap<U> extends Wrap<U> =
    function extra(self): U

implement SubWrap<Int32> for Rec =
    function base(self): Int32 = 3
    function extra(self): Int32 = 4

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.twice(7) == 7
"#,
    )
    .expect("inherited default binds the ORIGIN trait's param names through the chain");
}

// ── Round-2 review regressions ──────────────────────────────────────

#[test]
fn test_diamond_override_wins_both_orders() {
    for (supers, expected) in [("Beta and Ceta", 2), ("Ceta and Beta", 2)] {
        let source = format!(
            r#"
package a

record Rec =
    x: Int32

trait Alpha =
    function f(self): Int32 = 1

trait Beta extends Alpha =
    function f(self): Int32 = 2
    function beta(self): Int32

trait Ceta extends Alpha =
    function ceta(self): Int32

trait Delta extends {supers} =
    function delta(self): Int32

implement Delta for Rec =
    function beta(self): Int32 = 0
    function ceta(self): Int32 = 0
    function delta(self): Int32 = 0

function useIt<T>(v: T): Int32 where T: Delta = v.f()

function main(): Unit =
    let r = Rec {{ x = 0 }}
    assert useIt(r) == {expected}
"#
        );
        common::compile_and_run(&source).unwrap_or_else(|e| panic!("supers order '{supers}': {e}"));
    }
}

#[test]
fn test_distinct_origin_conflicting_defaults_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait LeftT =
    function f(self): Int32 = 5

trait RightT =
    function f(self): Int32 = 6

trait Both extends LeftT and RightT =
    function both(self): Int32

implement Both for Rec =
    function both(self): Int32 = 0

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e
            .contains("have different default implementations; override 'f' here to disambiguate")),
        "conflicting defaults from distinct origins must error, got: {:?}",
        errors
    );
}

#[test]
fn test_distinct_origin_defaults_own_override_disambiguates() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait LeftT =
    function f(self): Int32 = 5

trait RightT =
    function f(self): Int32 = 6

trait Both extends LeftT and RightT =
    function f(self): Int32 = 7
    function both(self): Int32

implement Both for Rec =
    function both(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.f() == 7
"#,
    )
    .expect("an own override resolves the distinct-origin default conflict");
}

#[test]
fn test_diamond_two_overrides_cured_by_own_override() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait AlphaD =
    function foo(self): Int32 = 1

trait MidOne extends AlphaD =
    function foo(self): Int32 = 10
    function midOne(self): Int32

trait MidTwo extends AlphaD =
    function foo(self): Int32 = 20
    function midTwo(self): Int32

trait Joined extends MidOne and MidTwo =
    function foo(self): Int32 = 30
    function joined(self): Int32

implement Joined for Rec =
    function midOne(self): Int32 = 0
    function midTwo(self): Int32 = 0
    function joined(self): Int32 = 0

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.foo() == 30
"#,
    )
    .expect("an own override cures a two-override diamond");
}

// ── Round-5 review regressions ──────────────────────────────────────

#[test]
fn test_injected_default_self_calls_use_own_block() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

interface Alpha =
    function v(self): Int32
    function d(self): Int32 = self.v() + 1

interface Beta extends Alpha =
    function w(self): Int32

implement Alpha for Rec =
    function v(self): Int32 = 100

implement Beta for Rec =
    function v(self): Int32 = 200
    function w(self): Int32 = 9

function viaBound<T>(v: T): Int32 where T: Beta = v.d()

function main(): Unit =
    let r = Rec { x = 0 }
    assert viaBound(r) == 201
    let b: Beta = r
    assert b.d() == 201
    let a: Alpha = r
    assert a.d() == 101
"#,
    )
    .expect("a materialized default's self-calls dispatch within its own provider block");
}

// ── Round-13 review regressions ─────────────────────────────────────

#[test]
fn test_default_body_nonexhaustive_match_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

record Rec =
    x: Int32

trait Tagger =
    function color(self): Color
    function tag(self): Int32 =
        match self.color() with
            case Color.Red => 1
            case Color.Green => 2

implement Tagger for Rec =
    function color(self): Color = Color.Blue

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.tag() == 3
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("exhaustive")
            || e.contains("not covered")
            || e.contains("missing")),
        "rules must check default bodies (non-exhaustive match), got: {:?}",
        errors
    );
}

#[test]
fn test_default_body_unsafe_cast_warns() {
    let warnings = common::compile_and_get_warnings(
        r#"
package a

record Rec =
    x: Int32

trait Tagger =
    function anyValue(self): Any
    function coerced(self): Int32 = self.anyValue() as Int32

implement Tagger for Rec =
    function anyValue(self): Any = 5

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.coerced() == 5
"#,
    );
    assert!(
        !warnings.is_empty(),
        "an unguarded `as` in a default body must warn like any other function body"
    );
}

// ── Round-23 review regressions ─────────────────────────────────────

#[test]
fn test_class_static_trait_default_diagnosed_not_miscompiled() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Maker =
    function tagOf(): Int32 = 9
    function id(self): Int32

class Person(idv: Int32) implements Maker =
    public function id(self: Person): Int32 = self.idv

function callTag<T>(): Int32 where T: Maker = T.tagOf()

function main(): Unit =
    assert callTag<Person>() == 9
"#,
    );
    assert!(
        !errors.is_empty(),
        "a class relying on a static trait default must be diagnosed, not miscompiled"
    );
}

#[test]
fn test_class_satisfies_static_trait_member_with_own_static() {
    common::compile_and_run(
        r#"
package a

trait Maker =
    function tagOf(): Int32
    function id(self): Int32

class Person(idv: Int32) implements Maker =
    public function tagOf(): Int32 = 9
    public function id(self: Person): Int32 = self.idv

function callTag<T>(): Int32 where T: Maker = T.tagOf()

function main(): Unit =
    assert callTag<Person>() == 9
"#,
    )
    .expect("a class CAN satisfy a static trait member with its own static function");
}

#[test]
fn test_class_instance_default_still_works() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Person(idv: Int32) implements Greeter =
    public function name(self: Person): Int32 = self.idv

function main(): Unit =
    let p = Person(5)
    assert p.greet() == 105
"#,
    )
    .expect("instance defaults on classes still work");
}

#[test]
fn test_default_body_passes_self_to_bounded_generic() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Sized =
    function size(self): Int32
    function viaHelper(self): Int32 = helper(self)

function helper<T>(v: T): Int32 where T: Sized = v.size() + 1

implement Sized for Rec =
    function size(self): Int32 = 4

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.viaHelper() == 5
"#,
    )
    .expect("a default body may pass self to a generic function");
}

#[test]
fn test_default_body_passes_self_to_unbounded_generic() {
    common::compile_and_run(
        r#"
package a

record Rec =
    x: Int32

trait Tagged =
    function tag(self): Int32
    function viaId(self): Int32 = idOf(self).tag()

function idOf<T>(v: T): T = v

implement Tagged for Rec =
    function tag(self): Int32 = 7

function main(): Unit =
    let r = Rec { x = 0 }
    assert r.viaId() == 7
"#,
    )
    .expect("an unbounded generic call from a default body works too");
}

// ── Round-24 review regressions ─────────────────────────────────────

#[test]
fn test_two_traits_same_named_defaults_on_class_diagnosed() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function tag(self): Int32 = 1

trait Beta =
    function tag(self): Int32 = 2

class C(k: Int32) implements Alpha and Beta =
    public function other(self: C): Int32 = 3

function ga<T>(v: T): Int32 where T: Alpha = v.tag()
function gb<T>(v: T): Int32 where T: Beta = v.tag()

function main(): Unit =
    let c = C(0)
    assert ga(c) == 1
    assert gb(c) == 2
"#,
    );
    assert!(
        !errors.is_empty(),
        "two traits' same-named defaults on one class must be diagnosed"
    );
}

#[test]
fn test_subclass_override_seen_by_materialized_default() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Base(k: Int32) implements Greeter =
    public function name(self: Base): Int32 = 1

class Derived(k: Int32) extends Base(k) implements Greeter =
    public override function name(self: Derived): Int32 = 2

function main(): Unit =
    assert Base(0).greet() == 101
    assert Derived(0).greet() == 102
"#,
    )
    .expect("a materialized default must see a subclass override");
}

#[test]
fn test_default_member_dispatches_virtually() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Base(k: Int32) implements Greeter =
    public function name(self: Base): Int32 = 1

class Derived(k: Int32) extends Base(k) implements Greeter =
    public override function name(self: Derived): Int32 = 2

function main(): Unit =
    let b: Base = Derived(0)
    assert b.greet() == 102
"#,
    )
    .expect("a default-supplied member dispatches virtually through a base-typed variable");
}

#[test]
fn test_class_static_trait_member_routing() {
    common::compile_and_run(
        r#"
package a

trait Maker =
    function tagOf(): Int32
    function id(self): Int32

class Person(idv: Int32) implements Maker =
    public function tagOf(): Int32 = 9
    public function id(self: Person): Int32 = self.idv

function callTag<T>(): Int32 where T: Maker = T.tagOf()

function main(): Unit =
    assert callTag<Person>() == 9
    assert Person(4).id() == 4
"#,
    )
    .expect("round-23 class static routing still works");
}

// ── Round-25 review regressions ─────────────────────────────────────

// F1: intermediate class redeclaring implements
#[test]
fn test_intermediate_class_redeclaring_implements() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class A1(k: Int32) implements Greeter =
    public function name(self: A1): Int32 = 1

class B1(k: Int32) extends A1(k) implements Greeter =
    public function other(self: B1): Int32 = 0

function main(): Unit =
    assert B1(0).greet() == 101
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// F2: explicit override in the middle of the chain must win
#[test]
fn test_mid_chain_explicit_override_wins_over_default() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class A1(k: Int32) implements Greeter =
    public function name(self: A1): Int32 = 1

class B1(k: Int32) extends A1(k) implements Greeter =
    public override function greet(self: B1): Int32 = 500

class C1(k: Int32) extends B1(k) implements Greeter =
    public override function name(self: C1): Int32 = 3

function main(): Unit =
    assert C1(0).greet() == 500
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// F3: defaulted PROPERTY sees subclass override
#[test]
fn test_defaulted_property_sees_subclass_override() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    property greeting(self): Int32 = self.name() + 100

class Base(k: Int32) implements Greeter =
    public function name(self: Base): Int32 = 1

class Derived(k: Int32) extends Base(k) implements Greeter =
    public override function name(self: Derived): Int32 = 2

function main(): Unit =
    assert Derived(0).greeting == 102
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// F4: one declaration reached through two applications is not a conflict
#[test]
fn test_one_declaration_through_two_applications_on_class() {
    common::compile_and_run(
        r#"
package a

trait Base0 =
    function tag(self): Int32 = 1

trait L extends Base0 =
    function l(self): Int32 = 10

class C(k: Int32) implements Base0 and L =
    public function z(self: C): Int32 = 3

function main(): Unit =
    assert C(0).tag() == 1
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// F5: abstract class + defaulted member
#[test]
fn test_abstract_class_with_defaulted_member() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

abstract class Base(k: Int32) implements Greeter =
    public abstract function name(self: Base): Int32

class Derived(k: Int32) extends Base(k) implements Greeter =
    public override function name(self: Derived): Int32 = 2

function main(): Unit =
    assert Derived(0).greet() == 102
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// F6: subclass WITHOUT implements still sees its override
#[test]
fn test_subclass_without_implements_sees_override() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function name(self): Int32
    function greet(self): Int32 = self.name() + 100

class Base(k: Int32) implements Greeter =
    public function name(self: Base): Int32 = 1

class Derived(k: Int32) extends Base(k) =
    public override function name(self: Derived): Int32 = 2

function main(): Unit =
    assert Derived(0).greet() == 102
"#,
    )
    .expect("class default materialization is virtual and per-declaration");
}

// round-24 regressions must still hold
#[test]
fn test_distinct_same_named_defaults_still_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Alpha =
    function tag(self): Int32 = 1

trait Beta =
    function tag(self): Int32 = 2

class C(k: Int32) implements Alpha and Beta =
    public function other(self: C): Int32 = 3

function main(): Unit =
    assert C(0).tag() == 1
"#,
    );
    assert!(
        !errors.is_empty(),
        "genuinely distinct same-named defaults must still be rejected"
    );
}
