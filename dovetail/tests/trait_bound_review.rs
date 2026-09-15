mod common;

#[test]
fn explicit_application_does_not_check_unrelated_sibling_bounds() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Required =
    function required(self): Int32
trait Alpha<U> =
    function alpha(self): Int32
implement <T> Alpha<Int32> for Wrap<T> where T: Required =
    function alpha(self): Int32 = 1
implement <T> Alpha<String> for Wrap<T> =
    function alpha(self): Int32 = 2
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    assert Alpha<String>.alpha(w) == 2
    assert w.alpha() == 2
"#,
    )
    .expect("inapplicable sibling bounds do not reject explicit or implicit calls");
}

#[test]
fn concrete_impl_and_provider_require_the_exact_receiver() {
    for implemented_trait in ["Alpha", "Beta"] {
        let source = format!(
            r#"
package a
record Wrap<T> =
    item: T
trait Alpha =
    function alpha(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32 = 2
implement {implemented_trait} for Wrap<Int32> =
    function alpha(self): Int32 = 1
function bound<T>(v: T): Int32 where T: Alpha = 42
function main(): Unit =
    assert bound(Wrap<String> {{ item = "x" }}) == 42
"#
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("does not implement trait 'Alpha'")),
            "{errors:?}"
        );
    }
}

#[test]
fn generic_provider_applications_are_substituted_before_ambiguity_checks() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Wrap<T> =
    item: T
trait Alpha<U> =
    function alpha(self): Int32
trait Beta<U> extends Alpha<U> =
    function beta(self): Int32
trait Gamma<U> extends Alpha<U> =
    function gamma(self): Int32
implement <T> Beta<T> for Wrap<T> =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
implement <T> Gamma<T> for Wrap<T> =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4
function bound<T>(v: T): Int32 where T: Alpha<Int32> = 42
function main(): Unit =
    assert bound(Wrap<Int32> { item = 1 }) == 42
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous implementations of trait 'Alpha'")),
        "{errors:?}"
    );
}

#[test]
fn a_different_generic_direct_application_does_not_hide_provider_ambiguity() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Wrap<T> =
    item: T
trait Alpha<U> =
    function alpha(self): Int32
trait Beta extends Alpha<String> =
    function beta(self): Int32
trait Gamma extends Alpha<String> =
    function gamma(self): Int32
implement <T> Alpha<T> for Wrap<T> =
    function alpha(self): Int32 = 0
implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
implement <T> Gamma for Wrap<T> =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4
function bound<T>(v: T): Int32 where T: Alpha<String> = 42
function main(): Unit =
    assert bound(Wrap<Int32> { item = 1 }) == 42
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous implementations of trait 'Alpha'")),
        "{errors:?}"
    );
}

#[test]
fn a_provider_with_unsatisfied_bounds_does_not_compete() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Required =
    function required(self): Int32
trait Alpha =
    function alpha(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32
trait Gamma extends Alpha =
    function gamma(self): Int32
implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
implement <T> Gamma for Wrap<T> where T: Required =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4
function bound<T>(v: T): Int32 where T: Alpha = 42
function main(): Unit =
    assert bound(Wrap<Int32> { item = 1 }) == 42
"#,
    )
    .expect("an inapplicable provider cannot make a valid bound ambiguous");
}

#[test]
fn explicit_super_call_through_a_subtrait_bound_keeps_the_named_trait() {
    common::compile_and_run(
        r#"
package a
record Rec =
    item: Int32
trait Alpha =
    function alpha(self): Int32
    property tag(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32
implement Alpha for Rec =
    function alpha(self): Int32 = 10
    property tag(self): Int32 = 11
implement Beta for Rec =
    function alpha(self): Int32 = 20
    property tag(self): Int32 = 21
    function beta(self): Int32 = 2
function call<T>(v: T): Int32 where T: Beta = Alpha.alpha(v)
function read<T>(v: T): Int32 where T: Beta = Alpha.tag(v)
function main(): Unit =
    let r = Rec { item = 0 }
    assert call(r) == 10
    assert read(r) == 11
"#,
    )
    .expect("explicit Alpha calls should use Alpha's direct impl even with only a Beta bound");
}

#[test]
fn explicit_super_call_prefers_a_direct_intersection_component() {
    common::compile_and_run(
        r#"
package a
record Rec =
    item: Int32
interface Zup =
    function value(self): Int32
interface Alpha extends Zup =
    function alpha(self): Int32
implement Zup for Rec =
    function value(self): Int32 = 10
implement Alpha for Rec =
    function value(self): Int32 = 20
    function alpha(self): Int32 = 2
function main(): Unit =
    let v: Alpha and Zup = Rec { item = 0 }
    assert Zup.value(v) == 10
"#,
    )
    .expect("the named direct component wins over an earlier sorted subinterface");
}

#[test]
fn explicit_super_call_requires_a_unique_intersection_application() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Rec =
    item: Int32
interface Conv<U> =
    function tag(self): Int32
interface Alpha extends Conv<Int32> =
    function alpha(self): Int32
interface Beta extends Conv<String> =
    function beta(self): Int32
implement Alpha for Rec =
    function tag(self): Int32 = 10
    function alpha(self): Int32 = 1
implement Beta for Rec =
    function tag(self): Int32 = 20
    function beta(self): Int32 = 2
function main(): Unit =
    let v: Alpha and Beta = Rec { item = 0 }
    assert Conv.tag(v) == 10
"#,
    );
    assert!(errors.iter().any(|e| e.contains("ambiguous")), "{errors:?}");
}

#[test]
fn explicit_inherited_call_preserves_the_selected_intersection_component() {
    common::compile_and_run(
        r#"
package a
record Rec =
    item: Int32
interface Conv<U> =
    function tag(self): Int32
    property code(self): Int32
interface Alpha extends Conv<Int32> =
    function alpha(self): Int32
interface Beta extends Conv<String> =
    function beta(self): Int32
implement Alpha for Rec =
    function tag(self): Int32 = 10
    property code(self): Int32 = 11
    function alpha(self): Int32 = 1
implement Beta for Rec =
    function tag(self): Int32 = 20
    property code(self): Int32 = 21
    function beta(self): Int32 = 2
function main(): Unit =
    let v: Alpha and Beta = Rec { item = 0 }
    assert Conv<Int32>.tag(v) == 10
    assert Conv<String>.tag(v) == 20
    assert Beta.tag(v) == 20
    assert Conv<String>.code(v) == 21
    assert Beta.code(v) == 21
"#,
    )
    .expect("inherited dispatch must navigate through the explicitly selected component");
}

#[test]
fn explicit_super_call_filters_generic_providers_by_the_receiver_application() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Alpha<U> =
    function alpha(self): Int32
trait Beta<U> extends Alpha<U> =
    function beta(self): Int32
trait Gamma extends Alpha<String> =
    function gamma(self): Int32
implement <T> Beta<T> for Wrap<T> =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
implement <T> Gamma for Wrap<T> =
    function alpha(self): Int32 = 3
    function gamma(self): Int32 = 4
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    assert Alpha<String>.alpha(w) == 3
"#,
    )
    .expect("the Beta<Int32> provider does not compete for Alpha<String>");
}

#[test]
fn distinct_applications_of_one_provider_remain_ambiguous() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Rec =
    item: Int32
trait Alpha =
    function alpha(self): Int32
trait Beta<U> extends Alpha =
    function beta(self): Int32
implement Beta<Int32> for Rec =
    function alpha(self): Int32 = 1
    function beta(self): Int32 = 2
implement Beta<String> for Rec =
    function alpha(self): Int32 = 3
    function beta(self): Int32 = 4
function bound<T>(v: T): Int32 where T: Alpha = 42
function main(): Unit =
    assert bound(Rec { item = 1 }) == 42
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ambiguous implementations of trait 'Alpha'")),
        "{errors:?}"
    );
}

#[test]
fn an_inapplicable_direct_impl_does_not_hide_a_provider() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Required =
    function required(self): Int32
trait Alpha =
    function alpha(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32
implement <T> Alpha for Wrap<T> where T: Required =
    function alpha(self): Int32 = 1
implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    assert Alpha.alpha(w) == 2
"#,
    )
    .expect("an unsatisfied direct impl does not hide a valid provider");
}

#[test]
fn associated_type_constraints_filter_direct_implementations_before_provider_selection() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait HasOutput =
    type Output
implement HasOutput for Int32 =
    type Output = Int32
implement HasOutput for String =
    type Output = String
trait Alpha =
    function alpha(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32
implement <T> Alpha for Wrap<T> where T: HasOutput<Output = String> =
    function alpha(self): Int32 = 1
implement <T> Beta for Wrap<T> where T: HasOutput<Output = Int32> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3
function bound<T>(value: T): Int32 where T: Alpha = value.alpha()
function main(): Unit =
    let provided = Wrap<Int32> { item = 7 }
    assert Alpha.alpha(provided) == 2
    assert provided.alpha() == 2
    assert bound(provided) == 2
    let direct = Wrap<String> { item = "ok" }
    assert Alpha.alpha(direct) == 1
    assert direct.alpha() == 1
    assert bound(direct) == 1
"#,
    )
    .expect("associated-type bounds select the applicable direct or inherited implementation");
}

#[test]
fn inherited_associated_outputs_are_inferred_at_generic_call_sites() {
    common::compile_and_run(
        r#"
package a
record Value =
    item: Int32
trait Produce =
    type Output
    function produce(self): Output
trait Extended extends Produce =
    function extra(self): Int32
implement Extended for Value =
    type Output = Int32
    function produce(self): Int32 = self.item
    function extra(self): Int32 = 1
function get<T, O>(value: T): O where T: Produce<Output = O> = value.produce()
function forward<T, O>(value: T): O where T: Extended<Output = O> = get(value)
function main(): Unit =
    assert get(Value { item = 7 }) == 7
    assert forward(Value { item = 8 }) == 8
"#,
    )
    .expect("associated output inference follows an inherited trait provider");
}

#[test]
fn a_direct_associated_output_cannot_be_replaced_by_an_inherited_output() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Value =
    item: Int32
trait Produce =
    type Output
    function produce(self): Output
trait Extended extends Produce =
    function extra(self): Int32
implement Produce for Value =
    type Output = String
    function produce(self): String = "direct"
implement Extended for Value =
    type Output = Int32
    function produce(self): Int32 = self.item
    function extra(self): Int32 = 1
function get<T>(value: T): Int32 where T: Produce<Output = Int32> = value.produce()
function main(): Unit =
    assert get(Value { item = 7 }) == 7
"#,
    );
    assert!(
        errors.iter().any(|error| error.contains("does not implement trait")),
        "{errors:?}"
    );
}
