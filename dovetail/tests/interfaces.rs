//! Interface declarations: `interface` = trait + declaration-time object-safety
//! check + permission to appear in type position (interface object).
//!
//! Covers the declaration-time check, interface objects, and the gate: a
//! plain trait may not appear in type position (bounds and implement blocks
//! remain trait territory).

mod common;

// ── Positive: interfaces parse and work as bound and as object ──

#[test]
fn test_interface_declaration_parses() {
    common::check_no_errors(
        r#"
package a

interface Drawable =
    function draw(self): String

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_interface_usable_as_bound() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self): String

record Point =
    x: Int32

implement Describable for Point =
    function describe(self): String = "point"

function label<T>(value: T): String where T: Describable = value.describe()

function main(): Unit = assert label(Point { x = 1 }) == "point"
"#,
    )
    .expect("interface as bound with static dispatch");
}

#[test]
fn test_interface_usable_as_object() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self): String

record Point =
    x: Int32

record Circle =
    radius: Int32

implement Describable for Point =
    function describe(self): String = "point"

implement Describable for Circle =
    function describe(self): String = "circle"

function label(value: Describable): String = value.describe()

function main(): Unit =
    assert label(Point { x = 1 }) == "point"
    assert label(Circle { radius = 2 }) == "circle"
"#,
    )
    .expect("interface object coercion and dynamic dispatch");
}

#[test]
fn test_generic_interface_usable_as_object() {
    common::compile_and_run(
        r#"
package a

interface Producer<T> =
    function produce(self): T

record IntProducer =
    value: Int32

implement Producer<Int32> for IntProducer =
    function produce(self): Int32 = self.value

function main(): Unit =
    let p: Producer<Int32> = IntProducer { value = 7 }
    assert p.produce() == 7
"#,
    )
    .expect("generic interface object");
}

#[test]
fn test_interface_self_return_allowed() {
    common::check_no_errors(
        r#"
package a

interface Cloneable =
    function duplicate(self): Self

record Token =
    id: Int32

implement Cloneable for Token =
    function duplicate(self): Token = Token { id = self.id }

function reissue(t: Cloneable): Cloneable = t.duplicate()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_interface_property_allowed() {
    common::check_no_errors(
        r#"
package a

interface HasName =
    property name(self: Self): String

record Person =
    n: String

implement HasName for Person =
    property name(self: Person): String = self.n

function greet(x: HasName): String = x.name

function main(): Unit = ()
"#,
    );
}

// ── Negative: object-safety violations error at the declaration ──

#[test]
fn test_interface_static_method_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Makeable =
    function make(seed: Int32): Self

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must take self")),
        "expected 'must take self' error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_generic_method_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Mapper =
    function apply<U>(self, value: U): U

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot be generic")),
        "expected 'cannot be generic' error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_associated_type_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Container =
    type Item
    function first(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot declare associated type")),
        "expected associated-type error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_self_param_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Combinable =
    function combine(self, other: Self): Self

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_self_in_array_param_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Mergeable =
    function merge(self, others: Array<Self>): Self

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter error, got: {:?}",
        errors
    );
}

#[test]
fn test_interface_violations_accumulate() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Broken =
    function make(seed: Int32): Self
    function apply<U>(self, value: U): U
    function combine(self, other: Self): Self

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must take self")),
        "expected 'must take self' error, got: {:?}",
        errors
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot be generic")),
        "expected 'cannot be generic' error, got: {:?}",
        errors
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_not_checked_for_object_safety() {
    // A plain trait keeps full type-class power: static functions, generic
    // methods, Self in parameters — none of these are declaration errors.
    common::check_no_errors(
        r#"
package a

trait FullPower =
    function make(seed: Int32): Self
    function apply<U>(self, value: U): U
    function combine(self, other: Self): Self

function main(): Unit = ()
"#,
    );
}

// ── The gate: a plain trait may not appear in type position ──

#[test]
fn test_trait_in_param_position_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Describable =
    function describe(self): String

function label(value: Describable): String = value.describe()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type") && e.contains("interface")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_in_return_position_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Describable =
    function describe(self): String

record Point =
    x: Int32

implement Describable for Point =
    function describe(self): String = "point"

function make(): Describable = Point { x = 1 }

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_in_record_field_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Describable =
    function describe(self): String

record Holder =
    value: Describable

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_trait_in_type_position_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Producer<T> =
    function produce(self): T

function consume(p: Producer<Int32>): Int32 = p.produce()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_still_valid_as_bound_and_implement() {
    // The gate hits only type position — bounds and implement blocks are untouched.
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self): String

record Point =
    x: Int32

implement Describable for Point =
    function describe(self): String = "point"

function label<T>(value: T): String where T: Describable = value.describe()

function main(): Unit = assert label(Point { x = 1 }) == "point"
"#,
    )
    .expect("trait as bound after the gate");
}

#[test]
fn test_bad_interface_boxed_reports_declaration_errors_only() {
    // A non-object-safe interface that is also boxed and called must produce
    // the declaration error(s) without panicking — the rules phase runs after
    // inference, and the deleted use-site checks must not be needed for safety.
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Combinable =
    function combine(self, other: Self): Self

record Pair =
    x: Int32

implement Combinable for Pair =
    function combine(self, other: Pair): Pair = other

function merge(c: Combinable): Combinable = c.combine(c)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter declaration error, got: {:?}",
        errors
    );
}

// ── Intersection interface objects: `A and B` ──

#[test]
fn test_intersection_coerce_and_call_both() {
    common::compile_and_run(
        r#"
package a

interface Drawable =
    function draw(self): String

interface Sizable =
    function size(self): Int32

record Shape =
    name: String
    area: Int32

implement Drawable for Shape =
    function draw(self): String = self.name

implement Sizable for Shape =
    function size(self): Int32 = self.area

function main(): Unit =
    let x: Drawable and Sizable = Shape { name = "circle"; area = 10 }
    assert x.draw() == "circle"
    assert x.size() == 10
"#,
    )
    .expect("coerce to intersection and dispatch through both components");
}

#[test]
fn test_intersection_upcast_to_each_component() {
    common::compile_and_run(
        r#"
package a

interface Drawable =
    function draw(self): String

interface Sizable =
    function size(self): Int32

record Shape =
    name: String
    area: Int32

implement Drawable for Shape =
    function draw(self): String = self.name

implement Sizable for Shape =
    function size(self): Int32 = self.area

function describe(d: Drawable): String = d.draw()

function measure(s: Sizable): Int32 = s.size()

function main(): Unit =
    let x: Drawable and Sizable = Shape { name = "square"; area = 4 }
    assert describe(x) == "square"
    assert measure(x) == 4
"#,
    )
    .expect("upcast intersection to each component and call through");
}

#[test]
fn test_three_way_intersection_subset_upcasts() {
    common::compile_and_run(
        r#"
package a

interface Named =
    function name(self): String

interface Aged =
    function age(self): Int32

interface Tagged =
    function tag(self): Int32

record Entity =
    n: String

implement Named for Entity =
    function name(self): String = self.n

implement Aged for Entity =
    function age(self): Int32 = 30

implement Tagged for Entity =
    function tag(self): Int32 = 7

function nameAndTag(x: Named and Tagged): Int32 = x.tag()

function justAged(x: Aged): Int32 = x.age()

function main(): Unit =
    let full: Named and Aged and Tagged = Entity { n = "e" }
    assert full.name() == "e"
    assert nameAndTag(full) == 7
    assert justAged(full) == 30
"#,
    )
    .expect("three-way intersection with multi-component and single upcasts");
}

#[test]
fn test_intersection_order_insensitive() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x + 1

function takeAB(v: Alpha and Beta): Int32 = v.alpha()

function main(): Unit =
    let ba: Beta and Alpha = Pair { x = 5 }
    assert takeAB(ba) == 5
    assert ba.beta() == 6
"#,
    )
    .expect("B and A is the same type as A and B");
}

#[test]
fn test_intersection_generic_component() {
    common::compile_and_run(
        r#"
package a

interface Producer<T> =
    function produce(self): T

interface Resettable =
    function reset(self): Int32

record Counter =
    value: Int32

implement Producer<Int32> for Counter =
    function produce(self): Int32 = self.value

implement Resettable for Counter =
    function reset(self): Int32 = 0

function main(): Unit =
    let c: Producer<Int32> and Resettable = Counter { value = 3 }
    assert c.produce() == 3
    assert c.reset() == 0
"#,
    )
    .expect("intersection with a generic interface component");
}

#[test]
fn test_intersection_template_coercion_in_generic_function() {
    common::compile_and_run(
        r#"
package a

interface Drawable =
    function draw(self): String

interface Sizable =
    function size(self): Int32

record Shape =
    name: String

implement Drawable for Shape =
    function draw(self): String = self.name

implement Sizable for Shape =
    function size(self): Int32 = 1

function box<T>(value: T): Drawable and Sizable where T: Drawable, T: Sizable =
    value

function main(): Unit =
    let b = box(Shape { name = "s" })
    assert b.draw() == "s"
    assert b.size() == 1
"#,
    )
    .expect("template coercion to an intersection inside a generic function");
}

#[test]
fn test_intersection_property_single_declarer() {
    common::compile_and_run(
        r#"
package a

interface HasName =
    property name(self: Self): String

interface Countable =
    function count(self): Int32

record Person =
    n: String

implement HasName for Person =
    property name(self: Person): String = self.n

implement Countable for Person =
    function count(self): Int32 = 1

function main(): Unit =
    let p: HasName and Countable = Person { n = "ada" }
    assert p.name == "ada"
"#,
    )
    .expect("property access through an intersection receiver");
}

#[test]
fn test_intersection_as_cast() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x * 2

function main(): Unit =
    let p = Pair { x = 4 }
    let v = p as Alpha and Beta
    assert v.alpha() == 4
    assert v.beta() == 8
"#,
    )
    .expect("`as` cast to an intersection");
}

// ── Intersection negatives ──

#[test]
fn test_intersection_non_interface_component_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

trait PlainTrait =
    function plain(self): Int32

function take(v: Alpha and PlainTrait): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_unknown_component_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

function take(v: Alpha and Nonexistent): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown type")),
        "expected unknown-type error, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_missing_one_impl_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record OnlyAlpha =
    x: Int32

implement Alpha for OnlyAlpha =
    function alpha(self): Int32 = self.x

function main(): Unit =
    let v: Alpha and Beta = OnlyAlpha { x = 1 }
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected a type mismatch for a type missing one component impl"
    );
}

#[test]
fn test_intersection_ambiguous_method_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Left =
    function pick(self): Int32

interface Right =
    function pick(self): Int32

record Both =
    x: Int32

implement Left for Both =
    function pick(self): Int32 = 1

implement Right for Both =
    function pick(self): Int32 = 2

function main(): Unit =
    let v: Left and Right = Both { x = 0 }
    let n = v.pick()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous method 'pick'")),
        "expected ambiguous-method error, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_duplicate_component_conflicting_args_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

function take(v: Producer<Int32> and Producer<String>): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("appears more than once")),
        "expected duplicate-component error, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_wrong_arity_component_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

interface Resettable =
    function reset(self): Int32

function take(v: Producer and Resettable): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type argument")),
        "expected arity error, got: {:?}",
        errors
    );
}

// ── Review-driven regression tests ──

#[test]
fn test_self_return_through_interface_object_runs() {
    // Bare Self-return is re-boxed to the interface at the vtable boundary.
    common::compile_and_run(
        r#"
package a

interface Fluent =
    function bump(self): Self
    function value(self): Int32

record Counter =
    n: Int32

implement Fluent for Counter =
    function bump(self): Counter = Counter { n = self.n + 1 }
    function value(self): Int32 = self.n

function main(): Unit =
    let f: Fluent = Counter { n = 1 }
    let g = f.bump()
    assert g.value() == 2
    assert g.bump().value() == 3
"#,
    )
    .expect("Self-return re-boxed as the interface type");
}

#[test]
fn test_self_return_on_intersection_returns_component() {
    common::compile_and_run(
        r#"
package a

interface Fluent =
    function bump(self): Self
    function value(self): Int32

interface Tagged =
    function tag(self): Int32

record Counter =
    n: Int32

implement Fluent for Counter =
    function bump(self): Counter = Counter { n = self.n + 1 }
    function value(self): Int32 = self.n

implement Tagged for Counter =
    function tag(self): Int32 = 9

function main(): Unit =
    let f: Fluent and Tagged = Counter { n = 5 }
    let g = f.bump()
    assert g.value() == 6
"#,
    )
    .expect("Self-return through an intersection dispatches via the component");
}

#[test]
fn test_nested_self_return_rejected_at_declaration() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Cloneish =
    function tryClone(self): Option<Self>

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("containing 'Self'")),
        "expected nested-Self-return declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_branch_unification_upcasts() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x + 1

function main(): Unit =
    let both: Alpha and Beta = Pair { x = 3 }
    let single: Alpha = Pair { x = 8 }
    let r: Alpha = if both.beta() == 4 then both else single
    assert r.alpha() == 3
"#,
    )
    .expect("if-branches mixing intersection and subset get upcasts");
}

#[test]
fn test_intersection_unknown_method_clean_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function main(): Unit =
    let v: Alpha and Beta = Pair { x = 1 }
    let n = v.gamma()
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected a clean unknown-method error (no panic)"
    );
}

#[test]
fn test_intersection_unknown_property_clean_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function main(): Unit =
    let v: Alpha and Beta = Pair { x = 1 }
    let n = v.missing
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected a clean unknown-property error (no panic)"
    );
}

#[test]
fn test_as_from_intersection_to_component() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x + 1

function main(): Unit =
    let v: Alpha and Beta = Pair { x = 4 }
    let a = v as Alpha
    assert a.alpha() == 4
"#,
    )
    .expect("`as` from an intersection to a component upcasts");
}

#[test]
fn test_intersection_satisfies_generic_bound() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function pick<T>(v: T): Int32 where T: Alpha = v.alpha()

function main(): Unit =
    let v: Alpha and Beta = Pair { x = 7 }
    assert pick(v) == 7
"#,
    )
    .expect("intersection value satisfies a component bound");
}

#[test]
fn test_function_type_variance_rejects_interface_subset() {
    // Function VALUES have no adapter: interface-object positions inside a
    // function type must match exactly.
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function handle(a: Alpha): Unit = ()

function main(): Unit =
    let f: (Alpha and Beta) => Unit = handle
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected function-position mismatch, got: {:?}",
        errors
    );
}

#[test]
fn test_bound_args_must_match_interface_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

record Counter =
    n: Int32

implement Producer<Int32> for Counter =
    function produce(self): Int32 = self.n

function bad<T>(x: T): Producer<String> where T: Producer<Int32> = x

function main(): Unit =
    let p = bad(Counter { n = 1 })
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected bound-arg mismatch, got: {:?}",
        errors
    );
}

#[test]
fn test_intersection_non_interface_known_type_message() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

function take(v: Alpha and Pair): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("is not an interface")),
        "expected not-an-interface message, got: {:?}",
        errors
    );
}

#[test]
fn test_gate_error_reported_once() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Describable =
    function describe(self): String

function label(value: Describable): String = value.describe()

function main(): Unit = ()
"#,
    );
    let gate_errors = errors
        .iter()
        .filter(|e| e.contains("cannot be used as a type"))
        .count();
    assert_eq!(
        gate_errors, 1,
        "expected exactly one gate error, got {}: {:?}",
        gate_errors, errors
    );
}

#[test]
fn test_implement_for_intersection_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

trait Marker =
    function mark(self): Int32

implement Marker for Alpha and Beta =
    function mark(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot implement a trait for an intersection type")),
        "expected implement-for-intersection error, got: {:?}",
        errors
    );
}

#[test]
fn test_extension_for_intersection_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

extension Extras for Alpha and Beta =
    function extra(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot declare an extension for an intersection type")),
        "expected extension-for-intersection error, got: {:?}",
        errors
    );
}

// ── Round-2 review regression tests ──

#[test]
fn test_static_call_on_intersection_clean_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

type Both = Alpha and Beta

function main(): Unit =
    let v = Both.make(1)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no static method 'make'")),
        "expected clean static-method error, got: {:?}",
        errors
    );
}

#[test]
fn test_covariant_container_rejects_intersection_subset() {
    // Per-element fat-pointer conversion cannot be reified: List<A and B>
    // is not assignable to List<A>.
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function takeList(l: List<Alpha>): Unit = ()

function main(): Unit =
    let ab: Alpha and Beta = Pair { x = 1 }
    let l: List<Alpha and Beta> = [ab]
    takeList(l)
"#,
    );
    assert!(!errors.is_empty(), "expected container-variance rejection");
}

#[test]
fn test_self_returning_bound_rejects_intersection() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Bump =
    function bump(self): Self
    function get(self): Int32

interface Named =
    function name(self): String

record R =
    v: Int32

implement Bump for R =
    function bump(self): R = R { v = self.v + 1 }
    function get(self): Int32 = self.v

implement Named for R =
    function name(self): String = "r"

function chain<T>(x: T): T where T: Bump = x.bump()

function main(): Unit =
    let bn: Bump and Named = R { v = 10 }
    let r = chain<Bump and Named>(bn)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("returns Self")),
        "expected Self-returning-bound rejection, got: {:?}",
        errors
    );
}

#[test]
fn test_dynamic_call_arg_upcast() {
    // An interface method taking an interface-object param, called with an
    // intersection argument: the upcast must be reified on the arg.
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32
    function plus(self, other: Alpha): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x
    function plus(self, other: Alpha): Int32 = self.x + other.alpha()

implement Beta for Pair =
    function beta(self): Int32 = self.x

function main(): Unit =
    let x: Alpha and Beta = Pair { x = 3 }
    let y: Alpha and Beta = Pair { x = 4 }
    assert x.plus(y) == 7
"#,
    )
    .expect("intersection arg upcast to interface-object param");
}

#[test]
fn test_closure_call_arg_upcast() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Beta for Pair =
    function beta(self): Int32 = self.x

function main(): Unit =
    let useA: (Alpha) => Int32 = (v: Alpha) => v.alpha()
    let ab: Alpha and Beta = Pair { x = 5 }
    assert useA(ab) == 5
"#,
    )
    .expect("intersection arg upcast at closure call");
}

#[test]
fn test_closure_call_concrete_arg_coerced() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function main(): Unit =
    let useA: (Alpha) => Int32 = (v: Alpha) => v.alpha()
    assert useA(Pair { x = 6 }) == 6
"#,
    )
    .expect("concrete arg coerced at closure call");
}

#[test]
fn test_where_and_gets_bound_hint() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

function f<T>(x: T): Int32 where T: Alpha and Beta = x.alpha()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("bounds combine with '+'")),
        "expected bound-syntax hint, got: {:?}",
        errors
    );
}

// ── Round-3 review regression tests ──

#[test]
fn test_signature_only_interface_types_compile() {
    // A function whose SIGNATURE mentions interface/intersection types, with no
    // coercion anywhere in the module (library pattern; `dovetail test` compiles
    // every project standalone).
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

function useBoth(x: Alpha and Beta): Int32 = x.alpha() + x.beta()

function useOne(x: Alpha): Int32 = x.alpha()

function main(): Unit = ()
"#,
    )
    .expect("signature-only interface types must compile without coercions");
}

#[test]
fn test_dynamic_call_arg_coercion_walk_order_independent() {
    // The calling function sorts BEFORE the function that coerces to the
    // interface — slot param types must come from the registry, not from
    // walk-order-dependent state.
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Sink =
    function put(self, item: Alpha): Int32

record Thing =
    v: Int32

record Basket =
    n: Int32

implement Alpha for Thing =
    function alpha(self): Int32 = self.v

implement Sink for Basket =
    function put(self, item: Alpha): Int32 = self.n + item.alpha()

function aaa(s: Sink, t: Thing): Int32 = s.put(t)

function main(): Unit =
    let s: Sink = Basket { n = 10 }
    assert aaa(s, Thing { v = 5 }) == 15
"#,
    )
    .expect("arg coercion independent of function walk order");
}

#[test]
fn test_dynamic_call_tuple_literal_arg_with_interface_element() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Sink =
    function put(self, pair: (Alpha, Int32)): Int32

record Thing =
    v: Int32

record Basket =
    n: Int32

implement Alpha for Thing =
    function alpha(self): Int32 = self.v

implement Sink for Basket =
    function put(self, pair: (Alpha, Int32)): Int32 = self.n + pair._0.alpha() + pair._1

function main(): Unit =
    let s: Sink = Basket { n = 1 }
    assert s.put((Thing { v = 2 }, 3)) == 6
"#,
    )
    .expect("tuple-literal arg with interface element coerced element-wise");
}

#[test]
fn test_generic_interface_slot_instantiated_at_intersection() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

interface Sink<T> =
    function put(self, item: T): Int32

record Thing =
    v: Int32

record Basket =
    n: Int32

implement Alpha for Thing =
    function alpha(self): Int32 = self.v

implement Beta for Thing =
    function beta(self): Int32 = self.v * 2

implement Sink<Alpha and Beta> for Basket =
    function put(self, item: Alpha and Beta): Int32 = self.n + item.alpha() + item.beta()

function main(): Unit =
    let s: Sink<Alpha and Beta> = Basket { n = 100 }
    assert s.put(Thing { v = 3 }) == 109
"#,
    )
    .expect("generic interface slot instantiated at an intersection");
}

#[test]
fn test_interface_component_args_invariant() {
    // Sink<Alpha> is not a Sink<Alpha and Beta> (and vice versa): generic
    // interface args feed erased slots where values flow both directions.
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Beta =
    function beta(self): Int32

interface Sink<T> =
    function put(self, item: T): Int32

record Thing =
    v: Int32

record Basket =
    n: Int32

implement Alpha for Thing =
    function alpha(self): Int32 = self.v

implement Beta for Thing =
    function beta(self): Int32 = self.v

implement Sink<Alpha and Beta> for Basket =
    function put(self, item: Alpha and Beta): Int32 = self.n

function takeAlphaSink(s: Sink<Alpha>): Unit = ()

function main(): Unit =
    let s: Sink<Alpha and Beta> = Basket { n = 1 }
    takeAlphaSink(s)
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected invariant component-arg rejection"
    );
}

// ── Round-4 review regression tests ──

#[test]
fn test_nested_tuple_literal_arg_with_interface_element() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Taker =
    function take(self, t: ((Alpha, Int32), Int32)): Int32

record Pair =
    x: Int32

record Machine =
    n: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Taker for Machine =
    function take(self, t: ((Alpha, Int32), Int32)): Int32 =
        self.n + t._0._0.alpha() + t._0._1 + t._1

function main(): Unit =
    let tk: Taker = Machine { n = 100 }
    assert tk.take(((Pair { x = 1 }, 2), 3)) == 106
"#,
    )
    .expect("nested tuple literal with interface element coerced");
}

#[test]
fn test_non_literal_tuple_arg_with_interface_element() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Taker =
    function take(self, t: (Alpha, Int32)): Int32

record Pair =
    x: Int32

record Machine =
    n: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

implement Taker for Machine =
    function take(self, t: (Alpha, Int32)): Int32 = self.n + t._0.alpha() + t._1

function main(): Unit =
    let tk: Taker = Machine { n = 10 }
    let tup = (Pair { x = 1 }, 2)
    assert tk.take(tup) == 13
"#,
    )
    .expect("non-literal tuple arg spilled and element-coerced");
}

#[test]
fn test_option_interface_payload_requires_precoerced_value() {
    // `Option<Pair>` cannot be laundered into `Option<Alpha>` through branch
    // unification (interface positions inside generic args are exact) — the
    // raw form is a clean compile error, and the pre-coerced form runs.
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function pick(n: Int32): Option<Alpha> =
    if n > 0 then Some(Pair { x = n }) else None

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected clean rejection (was: runtime cast trap), got: {:?}",
        errors
    );

    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function pick(n: Int32): Option<Alpha> =
    if n > 0 then
        let a: Alpha = Pair { x = n }
        Some(a)
    else
        None

function main(): Unit =
    match pick(3) with
    case Some(a) => assert a.alpha() == 3
    case None => panic "unexpected"
"#,
    )
    .expect("pre-coerced interface payload in prelude Option runs");
}

#[test]
fn test_class_constructor_interface_param() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

class Holder(public item: Alpha) =
    public function get(self): Int32 = self.item.alpha()

function main(): Unit =
    let h = Holder(Pair { x = 3 })
    assert h.get() == 3
"#,
    )
    .expect("class constructor arg coerced to interface param");
}

#[test]
fn test_class_method_interface_param() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

class Feeder(public n: Int32) =
    public function feed(self, other: Alpha): Int32 = self.n + other.alpha()

function main(): Unit =
    let f = Feeder(10)
    assert f.feed(Pair { x = 4 }) == 14
"#,
    )
    .expect("class method arg coerced to interface param");
}

// ── Round-5 review regression tests ──

#[test]
fn test_tuple_with_interface_element_at_all_sinks() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

record Wrap =
    t: (Alpha, Int32)

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function makeConcrete(): (Pair, Int32) = (Pair { x = 1 }, 2)

function viaReturn(): (Alpha, Int32) =
    let concrete = makeConcrete()
    concrete

function main(): Unit =
    let concrete = (Pair { x = 5 }, 7)
    let t: (Alpha, Int32) = concrete
    assert t._0.alpha() + t._1 == 12
    let w = Wrap { t = concrete }
    assert w.t._0.alpha() == 5
    let w2 = w with t = makeConcrete()
    assert w2.t._0.alpha() == 1
    let r = viaReturn()
    assert r._0.alpha() + r._1 == 3
"#,
    )
    .expect("non-literal interface tuples reified at let/field/with/return sinks");
}

#[test]
fn test_closure_returning_interface_tuple() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function main(): Unit =
    let f: (Int32) => (Alpha, Int32) = (n: Int32) =>
        let concrete = (Pair { x = n }, n + 1)
        concrete
    let r = f(4)
    assert r._0.alpha() + r._1 == 9
"#,
    )
    .expect("closure declared-return interface tuple reified");
}

#[test]
fn test_nested_interface_in_generic_arg_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function main(): Unit =
    let concrete = (Pair { x = 1 }, 2)
    let o: Option<(Alpha, Int32)> = Some(concrete)
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected rejection of tuple-nested interface in generic arg"
    );
}

#[test]
fn test_else_none_with_interface_payload_unifies() {
    // Never flows through the exact-match guards: `else None` unifies with an
    // interface-payload Option.
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function pick(n: Int32): Option<Alpha> =
    if n > 0 then
        let a: Alpha = Pair { x = n }
        Some(a)
    else
        None

function main(): Unit =
    let x = if true then pick(3) else None
    match x with
    case Some(a) => assert a.alpha() == 3
    case None => panic "unexpected"
"#,
    )
    .expect("Never-typed branches unify with interface-payload enums");
}

#[test]
fn test_record_variant_interface_payload_from_bound() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

enum Holder<T> =
    Wrap { item: T }

function put<T>(v: T): Holder<Alpha> where T: Alpha = Holder.Wrap { item = v }

function main(): Unit =
    match put(Pair { x = 6 }) with
    case Holder.Wrap { item } => assert item.alpha() == 6
"#,
    )
    .expect("record-variant enum payload coerced from bound-typed value");
}

#[test]
fn test_record_pattern_on_interface_scrutinee_clean_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

function main(): Unit =
    let a: Alpha = Pair { x = 1 }
    match a with
    case Pair { x } => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot match a record pattern against interface type")),
        "expected clean pattern rejection, got: {:?}",
        errors
    );
}

// ── Round-6 review regression tests ──

#[test]
fn test_function_type_with_interface_tuple_param_strict() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Dog =
    d: Int32

implement Alpha for Dog =
    function alpha(self): Int32 = self.d

function main(): Unit =
    let g: ((Alpha, Int32)) => Int32 = (p: (Alpha, Int32)) => p._0.alpha() + p._1
    let f: ((Dog, Int32)) => Int32 = g
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected function-position rejection of interface-in-tuple param variance"
    );
}

#[test]
fn test_array_set_with_interface_element() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Cat =
    c: Int32

record Dog =
    d: Int32

implement Alpha for Cat =
    function alpha(self): Int32 = self.c

implement Alpha for Dog =
    function alpha(self): Int32 = self.d

function main(): Unit =
    let first: Alpha = Dog { d = 1 }
    let arr = [|first, first|]
    arr.set(1, Cat { c = 4 })
    assert arr.get(1).alpha() == 4
    assert arr.get(0).alpha() == 1
"#,
    )
    .expect("array.set with concrete value into interface-element array");
}

#[test]
fn test_newtype_with_interface_inner() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Dog =
    d: Int32

implement Alpha for Dog =
    function alpha(self): Int32 = self.d

newtype Wrapped = Alpha

function main(): Unit =
    let w = Wrapped(Dog { d = 7 })
    assert w.value.alpha() == 7
"#,
    )
    .expect("newtype construction coerces to interface inner");
}

#[test]
fn test_literal_pattern_on_interface_scrutinee_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

implement Alpha for Int32 =
    function alpha(self): Int32 = self

function main(): Unit =
    let a: Alpha = 5
    match a with
    case 5 => ()
    case _ => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot match a literal pattern against interface type")),
        "expected literal-pattern rejection, got: {:?}",
        errors
    );
}

#[test]
fn test_partially_erased_tuple_payload_from_bound() {
    // `Holder<T> = Wrap((T, Int32))` instantiated with an interface via a bound:
    // the template's flattened (anyref, i32) fields must cast back per-leaf.
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Pair =
    x: Int32

implement Alpha for Pair =
    function alpha(self): Int32 = self.x

enum Holder<T> =
    Wrap((T, Int32))

function put<T>(v: T): Holder<Alpha> where T: Alpha = Holder.Wrap((v, 9))

function main(): Unit =
    match put(Pair { x = 6 }) with
    case Holder.Wrap((item, n)) => assert item.alpha() + n == 15
"#,
    )
    .expect("partially-erased tuple payload casts back per-leaf");
}

#[test]
fn test_partially_erased_tuple_payload_concrete() {
    // The interface-independent shape of the same bug: generic enum payload
    // `(T, Int32)` instantiated at a reference type.
    common::compile_and_run(
        r#"
package a

record Dog =
    d: Int32

enum Holder<T> =
    Wrap((T, Int32))

function main(): Unit =
    let h: Holder<Dog> = Holder.Wrap((Dog { d = 3 }, 4))
    match h with
    case Holder.Wrap((item, n)) => assert item.d + n == 7
"#,
    )
    .expect("partially-erased tuple payload with concrete reference type");
}

#[test]
fn test_array_literal_interface_elements_annotated() {
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Cat =
    c: Int32

record Dog =
    d: Int32

implement Alpha for Cat =
    function alpha(self): Int32 = self.c

implement Alpha for Dog =
    function alpha(self): Int32 = self.d

function main(): Unit =
    let first: Alpha = Dog { d = 1 }
    let arr = [|first, Cat { c = 2 }|]
    assert arr.get(0).alpha() + arr.get(1).alpha() == 3
"#,
    )
    .expect("interface-element array literal coerces later concrete elements");
}

// ── Round-7 review regression tests ──

#[test]
fn test_uncalled_closure_with_interface_param() {
    // The interface type appears ONLY inside a function type — no signature,
    // no coercion. TypeDef registration must still happen.
    common::compile_and_run(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

record Dog =
    d: Int32

implement Alpha for Dog =
    function alpha(self): Int32 = self.d

function main(): Unit =
    let f = (x: Alpha) => 1
    ()
"#,
    )
    .expect("uncalled closure with interface param compiles");
}

#[test]
fn test_interface_extends_collects() {
    common::check_no_errors(
        r#"
package a

interface Alpha =
    function alpha(self): Int32

interface Gamma extends Alpha =
    function gamma(self): Int32

record Rec =
    x: Int32

implement Gamma for Rec =
    function alpha(self): Int32 = 1
    function gamma(self): Int32 = 2

function main(): Unit = ()
"#,
    );
}
