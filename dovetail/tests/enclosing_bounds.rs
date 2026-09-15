mod common;

#[test]
fn where_clauses_without_visible_parameters_reject_unknown_names() {
    for declaration in [
        r#"
function invalid(): Unit where T: Display = ()
"#,
        r#"
module Plain =
    function invalid(): Unit where T: Display = ()
"#,
        r#"
class Plain(public value: Int32) =
    public function invalid(self): Unit where T: Display = ()
"#,
    ] {
        let source = format!(
            r#"
package a
{declaration}
"#
        );
        let checked = dovetail::check(&source, "test.dove");
        assert!(
            checked
                .diagnostics
                .iter()
                .any(|d| d.message.contains("not declared on this item")),
            "{:?}",
            checked.diagnostics
        );
    }
}

#[test]
fn extension_method_bounds_include_enclosing_parameters() {
    common::compile_and_run(r#"
package a
import a.BoxExt
record Box<T> = value: T
extension BoxExt<T> for Box<T> =
    function formatted<U>(self, other: U): String where T: Display, U: Display = self.value.format() ++ other.format()
function forward<T>(value: Box<T>): String where T: Display = value.formatted(true)
function main(): Unit = assert forward(Box { value = 12 }) == "12true"
"#).expect("extension method enclosing and local bounds");
}

#[test]
fn implementation_method_cannot_strengthen_trait_contract() {
    let source = r#"
package a
trait Inspect =
    function inspect(self): Unit
record Box<T> = value: T
implement <T> Inspect for Box<T> =
    function inspect(self: Box<T>): Unit where T: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}
#[test]
fn module_bounds_are_local_and_available_in_closures() {
    common::compile_and_run(
        r#"
package a
record Box<T> = value: T
module Box<T> =
    function formatValue(self: Box<T>): () => String where T: Display = () => self.value.format()
    function unchanged(self: Box<T>): T = self.value
function forward<T>(value: Box<T>): String where T: Display =
    let format = value.formatValue()
    format()
function main(): Unit =
    assert forward(Box { value = 12 }) == "12"
    assert Box { value = 12 }.unchanged() == 12
"#,
    )
    .expect("module methods may constrain enclosing parameters");
}

#[test]
fn class_bounds_apply_without_method_parameters() {
    common::compile_and_run(
        r#"
package a
class Box<T>(public value: T) =
    public function formatValue(self): String where T: Display = self.value.format()
function forward<T>(value: Box<T>): String where T: Display = value.formatValue()
function main(): Unit = assert forward(Box(12)) == "12"
"#,
    )
    .expect("class method enclosing bound");
}

#[test]
fn enclosing_bounds_do_not_leak_to_sibling_methods() {
    let source = r#"
package a
record Box<T> = value: T
module Box<T> =
    function valid(self: Box<T>): String where T: Display = self.value.format()
    function invalid(self: Box<T>): String = self.value.format()
"#;
    assert!(
        dovetail::check(source, "test.dove")
            .diagnostics
            .has_errors()
    );
}

#[test]
fn class_method_call_requires_enclosing_bound() {
    let source = r#"
package a
record Hidden = value: Int32
class Box<T>(public value: T) =
    public function allowed(self): Unit where T: Display = ()
function main(): Unit = Box(Hidden { value = 1 }).allowed()
"#;
    assert!(
        dovetail::check(source, "test.dove")
            .diagnostics
            .has_errors()
    );
}

#[test]
fn generic_trait_method_contract_is_inherited_and_cannot_be_strengthened() {
    common::compile_and_run(
        r#"
package a
trait Render =
    function render<T>(self, value: T): String where T: Display
record Printer = value: Int32
implement Render for Printer =
    function render<U>(self: Printer, value: U): String = value.format()
function main(): Unit = assert Printer { value = 0 }.render(12) == "12"
"#,
    )
    .expect("trait method bounds survive parameter renaming");
    let source = r#"
package a
trait Consume =
    function consume<T>(self, value: T): Unit
record Printer = value: Int32
implement Consume for Printer =
    function consume<U>(self: Printer, value: U): Unit where U: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn override_bounds_respect_substituted_parent_contracts() {
    common::compile_and_run(
        r#"
package a
class Base<T>(public value: T) =
    public function render(self): String where T: Display = self.value.format()
class Child<U>(value: U) extends Base<U>(value) =
    public override function render(self): String where U: Display = self.value.format()
function main(): Unit = assert Child(12).render() == "12"
"#,
    )
    .expect("override repeats a bound under renamed enclosing parameters");
    let source = r#"
package a
class Base<T>(public value: T) =
    public function inspect(self): Unit = ()
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect(self): Unit where U: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn bounded_members_enforce_requirements_for_calls_and_references() {
    let declarations = r#"
package a
import a.BoxExt
record Hidden = value: Int32
record Box<T> = value: T
module Box<T> =
    function inspect(self: Box<T>): Unit where T: Display = ()
    function inspectStatic(value: T): Unit where T: Display = ()
extension BoxExt<T> for Box<T> =
    function inspectExtension(self): Unit where T: Display = ()
class Holder<T>(public value: T) =
    public function inspect(self): Unit where T: Display = ()
    public function inspectStatic(value: T): Unit where T: Display = ()
"#;
    let declarations_checked = dovetail::check(declarations, "test.dove");
    assert!(
        !declarations_checked.diagnostics.has_errors(),
        "{:?}",
        declarations_checked.diagnostics
    );
    for body in [
        "Box { value = Hidden { value = 1 } }.inspect()",
        "Box<Hidden>.inspectStatic(Hidden { value = 1 })",
        "Box { value = Hidden { value = 1 } }.inspectExtension()",
        "Holder(Hidden { value = 1 }).inspect()",
        "Holder<Hidden>.inspectStatic(Hidden { value = 1 })",
        "let inspect: () => Unit = Box { value = Hidden { value = 1 } }.inspect",
        "let inspect: () => Unit = Box { value = Hidden { value = 1 } }.inspectExtension",
        "let inspect: () => Unit = Holder(Hidden { value = 1 }).inspect",
        "let inspect: Hidden => Unit = Box<Hidden>.inspectStatic",
    ] {
        let source = format!(
            r#"
{declarations}
function main(): Unit =
    {body}
    ()
"#
        );
        let checked = dovetail::check(&source, "test.dove");
        assert!(
            checked.diagnostics.has_errors(),
            "missing bound check: {body}"
        );
    }
}

#[test]
fn generic_callers_must_prove_enclosing_bounds() {
    for member in ["value.inspect()", "let inspect: () => Unit = value.inspect"] {
        let source = format!(
            r#"
package a
class Holder<T>(public value: T) =
    public function inspect(self): Unit where T: Display = ()
function invalid<T>(value: Holder<T>): Unit =
    {member}
    ()
"#
        );
        assert!(
            dovetail::check(&source, "test.dove")
                .diagnostics
                .has_errors(),
            "{member}"
        );
    }
}

#[test]
fn method_bounds_do_not_restrict_construction_or_other_members() {
    common::compile_and_run(
        r#"
package a
record Hidden = value: Int32
record Box<T> = value: T
module Box<T> =
    function render(self: Box<T>): String where T: Display = self.value.format()
    function unchanged(self: Box<T>): T = self.value
class Holder<T>(public value: T) =
    public function render(self): String where T: Display = self.value.format()
    public function unchanged(self): T = self.value
class Child<T>(value: T) extends Holder<T>(value)
class Override<T>(value: T) extends Holder<T>(value) =
    public override function render(self): String where T: Display = self.value.format()
function main(): Unit =
    assert Box { value = Hidden { value = 1 } }.unchanged().value == 1
    assert Holder(Hidden { value = 2 }).unchanged().value == 2
    assert Child(Hidden { value = 3 }).unchanged().value == 3
    assert Override(Hidden { value = 4 }).unchanged().value == 4
"#,
    )
    .expect("method-local requirements leave other members usable");
}

#[test]
fn override_contract_selection_uses_parameter_types() {
    let source = r#"
package a
class Base<T>(public value: T) =
    public function inspect(self, tag: Int32): Unit where T: Display = ()
    public function inspect(self, tag: Bool): Unit = ()
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect(self, tag: Bool): Unit where U: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn override_method_parameters_can_reuse_ancestor_parameter_names() {
    let source = r#"
package a
class Base<T>(public value: T) =
    public function inspect<V>(self, other: V): Unit where T: Display, V: Equatable = ()
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect<T>(self, other: T): Unit where U: Display, T: Equatable = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn bounds_can_relate_enclosing_and_method_parameters() {
    common::compile_and_run(r#"
package a
trait Contains<T> =
    function contains(self, value: T): Bool
record Number = value: Int32
implement Contains<Int32> for Number =
    function contains(self: Number, value: Int32): Bool = self.value == value
record Box<T> = value: T
module Box<T> =
    function contains<U>(self: Box<T>, other: U): Bool where T: Contains<U> = self.value.contains(other)
class Holder<T>(public value: T) =
    public function contains<U>(self, other: U): Bool where T: Contains<U> = self.value.contains(other)
function forward<T, U>(box: Box<T>, value: U): Bool where T: Contains<U> = box.contains(value)
function main(): Unit =
    assert forward(Box { value = Number { value = 12 } }, 12)
    assert Holder(Number { value = 12 }).contains(12)
"#).expect("bounds refer to parameters from both scopes");
}

#[test]
fn implementation_block_guarantees_allow_repeated_method_bounds() {
    common::compile_and_run(
        r#"
package a
interface Render =
    function render(self): String
record Box<T> = value: T
implement <T> Render for Box<T> where T: Display =
    function render(self: Box<T>): String where T: Display = self.value.format()
function main(): Unit =
    let rendered: Render = Box { value = 12 }
    assert rendered.render() == "12"
"#,
    )
    .expect("block guarantees justify method bounds under interface dispatch");
}

#[test]
fn inline_class_trait_methods_cannot_strengthen_contracts() {
    let source = r#"
package a
trait Inspect =
    function inspect(self): Unit
class Holder<T>(public value: T) implements Inspect =
    public function inspect(self): Unit where T: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|d| d.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn trait_method_bounds_substitute_self_before_contract_checking() {
    common::compile_and_run(
        r#"
package a
trait Accept<T> =
    function accept(self, value: T): Unit
trait Send =
    function send<T>(self, target: T): Unit where T: Accept<Self>
record Message = value: Int32
record Sink = value: Int32
implement Accept<Message> for Sink =
    function accept(self: Sink, value: Message): Unit = assert self.value == value.value
implement Send for Message =
    function send<U>(self: Message, target: U): Unit where U: Accept<Message> = target.accept(self)
function main(): Unit = Message { value = 12 }.send(Sink { value = 12 })
"#,
    )
    .expect("trait Self in method bounds refers to the implementation receiver");
}

#[test]
fn tuple_bounds_in_signatures_and_doubly_generic_methods() {
    common::compile_and_run(r#"
package a
record Box<T> = value: T
module Box<T> =
    function append<U>(self: Box<T>, other: U): T ~ U where T: Tuple = self.value ~ other
class Holder<T>(public value: T) =
    public function append<U>(self, other: U): T ~ U where T: Tuple = self.value ~ other
    public function first(self): Any where T: Tuple = self.value.init
    public function formatted<U>(self, other: U): () => String where T: Display, U: Display = () => self.value.format() ++ other.format()
function forward<T>(value: Holder<T>): T ~ String where T: Tuple = value.append("x")
function main(): Unit =
    assert Box { value = (1, true) }.append("x") == (1, true, "x")
    assert forward(Holder((1, true))) == (1, true, "x")
    assert (Holder((1, true)).first() as Int32) == 1
    let formatted = Holder(12).formatted(true)
    assert formatted() == "12true"
"#).expect("enclosing bounds in generic signatures and closures");
}

#[test]
fn abstract_method_bounds_apply_to_signatures() {
    let source = r#"
package a
record Wrapped<T> where T: Display = value: T
abstract class Holder<T>(public value: T) =
    public abstract function wrapped(self, input: Wrapped<T>): Wrapped<T> where T: Display
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn abstract_method_bounds_include_own_and_enclosing_parameters() {
    let source = r#"
package a
trait Contains<T> =
    function contains(self, value: T): Bool
record Wrapped<T> where T: Display = value: T
abstract class Holder<T>(public value: T) =
    public abstract function inspect<U>(self): Unit where T: Display, U: Display
    public abstract function wrapped<U>(self, input: Wrapped<U>): Wrapped<U> where U: Display
    public abstract function contains<U>(self, other: U): Bool where T: Contains<U>
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn abstract_generic_method_bounds_do_not_leak_to_siblings() {
    let source = r#"
package a
record Wrapped<T> where T: Display = value: T
abstract class Holder<T>(public value: T) =
    public abstract function valid<U>(self, input: Wrapped<U>): Wrapped<T> where T: Display, U: Display
    public abstract function invalid<U>(self, input: Wrapped<U>): Wrapped<T>
"#;
    let checked = dovetail::check(source, "test.dove");
    let errors: Vec<_> = checked
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .message
                .contains("does not implement trait 'Display'")
        })
        .collect();
    assert_eq!(errors.len(), 2, "{:?}", checked.diagnostics);
    assert!(errors.iter().all(|diagnostic| diagnostic.span.line == 6));
}

#[test]
fn abstract_method_bounds_do_not_leak_to_sibling_signatures() {
    let source = r#"
package a
record Wrapped<T> where T: Display = value: T
abstract class Holder<T>(public value: T) =
    public abstract function valid(self): Wrapped<T> where T: Display
    public abstract function invalid(self): Wrapped<T>
"#;
    let checked = dovetail::check(source, "test.dove");
    let errors: Vec<_> = checked
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .message
                .contains("does not implement trait 'Display'")
        })
        .collect();
    assert_eq!(errors.len(), 1, "{:?}", checked.diagnostics);
    assert_eq!(errors[0].span.line, 6);
}

#[test]
fn abstract_calls_and_references_require_enclosing_bounds() {
    for operation in ["value.inspect()", "let inspect: () => Unit = value.inspect"] {
        let source = format!(
            r#"
package a
abstract class Holder<T>(public value: T) =
    public abstract function inspect(self): Unit where T: Display
function invalid<T>(value: Holder<T>): Unit =
    {operation}
    ()
"#
        );
        let checked = dovetail::check(&source, "test.dove");
        assert!(checked.diagnostics.has_errors(), "{operation}");
    }
}

#[test]
fn override_bound_arguments_preserve_enclosing_parameter_names() {
    let source = r#"
package a
trait Accept<T> =
    function accept(self, value: T): Unit
class Base<T>(public value: T) =
    public function inspect<U>(self, other: U): Unit where U: Accept<T> = ()
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect<V>(self, other: V): Unit where V: Accept<U> = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn implementation_bound_arguments_preserve_enclosing_parameter_names() {
    let source = r#"
package a
trait Accept<T> =
    function accept(self, value: T): Unit
trait Inspect<T> =
    function inspect<U>(self, first: T, other: U): T where U: Accept<T>
record Box<T> = value: T
implement <U> Inspect<U> for Box<U> =
    function inspect<V>(self: Box<U>, first: U, other: V): U where V: Accept<U> = self.value
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn override_signature_substitution_cannot_hide_strengthened_bounds() {
    let source = r#"
package a
class Base<T>(public value: T) =
    public function inspect<U>(self, first: T, second: U): Unit = ()
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect<V>(self, first: U, second: V): Unit where V: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn override_contract_search_continues_past_unrelated_overloads() {
    let source = r#"
package a
class Base<T>(public value: T) =
    public function inspect(self, tag: Bool): Unit = ()
class Middle<T>(value: T) extends Base<T>(value) =
    public function inspect(self, tag: Int32): Unit = ()
class Child<T>(value: T) extends Middle<T>(value) =
    public override function inspect(self, tag: Bool): Unit where T: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("strengthen")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn inherited_constrained_methods_use_ancestor_parameter_bindings() {
    common::compile_and_run(
        r#"
package a
record Hidden = value: Int32
class Holder<T>(public value: T) =
    public function render(self): String where T: Display = self.value.format()
    public function unchanged(self): T = self.value
class Child<U>(value: U) extends Holder<U>(value)
class Grandchild<V>(value: V) extends Child<V>(value)
function main(): Unit =
    assert Child(Hidden { value = 3 }).unchanged().value == 3
    assert Grandchild(Hidden { value = 4 }).unchanged().value == 4
    assert Child(12).render() == "12"
"#,
    )
    .expect("inherited methods specialize with their declaring class parameters");
}

#[test]
fn conditional_virtual_method_bounds_cannot_change_under_variance() {
    let checked = dovetail::check(
        r#"
package a
class Consumer<in T>() =
    public function inspect(self): Unit where T: Display = ()
function main(): Unit =
    let original = Consumer<Any>()
    let narrowed: Consumer<Int32> = original
    narrowed.inspect()
"#,
        "test.dove",
    );
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("invariant position")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn conditional_virtual_bound_arguments_must_be_invariant() {
    let checked = dovetail::check(
        r#"
package a
trait Accept<T> =
    function accept(self, value: T): Unit
class Consumer<T, in U>(public value: T) =
    public function inspect(self): Unit where T: Accept<U> = ()
"#,
        "test.dove",
    );
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("'U'")
                && diagnostic.message.contains("invariant position")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn class_guaranteed_virtual_bounds_preserve_variance() {
    common::compile_and_run(
        r#"
package a
class Holder<out T>(public value: T) where T: Display =
    public function render(self): String where T: Display = self.value.format()
function main(): Unit = assert Holder(12).render() == "12"
"#,
    )
    .expect("class guarantees remain true for every legal instantiation");
}

#[test]
fn inherited_conditional_bounds_cannot_be_bypassed_with_child_variance() {
    let checked = dovetail::check(
        r#"
package a
class Consumer<T>() =
    public function inspect(self): Unit where T: Display = ()
class Child<in U>() extends Consumer<U>()
"#,
        "test.dove",
    );
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("'U'")
                && diagnostic.message.contains("invariant position")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn class_supertrait_guarantees_preserve_variance() {
    let checked = dovetail::check(
        r#"
package a
trait Marker extends Display =
    function marker(self): Unit
class Holder<out T>(public value: T) where T: Marker =
    public function render(self): String where T: Display = self.value.format()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn generic_override_contract_search_skips_ordinary_overloads() {
    let source = r#"
package a
class Base() =
    public function inspect<U>(self, value: U): Unit where U: Display = ()
class Middle() extends Base() =
    public override function inspect(self, value: Bool): Unit = ()
class Child() extends Middle() =
    public override function inspect<V>(self, value: V): Unit where V: Display = ()
"#;
    let checked = dovetail::check(source, "test.dove");
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn unconstrained_virtual_overload_cannot_dispatch_to_constrained_stub() {
    common::compile_and_run(
        r#"
package a
record Hidden = value: Int32
class Holder<T>(public value: T) =
    public function inspect(self, tag: Int32): Unit = ()
    public function inspect(self, tag: Bool): Unit where T: Display = ()
function main(): Unit = Holder(Hidden { value = 1 }).inspect(1)
"#,
    )
    .expect("unconstrained overload remains callable");
}

#[test]
fn overloaded_virtual_methods_keep_distinct_slots_through_inheritance() {
    common::compile_and_run(
        r#"
package a
class Base<T>(public value: T) =
    public function inspect(self, tag: Int32): Int32 = 1
    public function inspect(self, tag: Bool): Int32 where T: Display = 2
class Child<U>(value: U) extends Base<U>(value) =
    public override function inspect(self, tag: Bool): Int32 where U: Display = 3
class Grandchild<V>(value: V) extends Child<V>(value)
function main(): Unit =
    let base: Base<Int32> = Grandchild(12)
    assert base.inspect(1) == 1
    assert base.inspect(true) == 3
    assert Grandchild(12).inspect(true) == 3
"#,
    )
    .expect("overloaded virtual dispatch preserves ancestor slot indices");
}

#[test]
fn overrides_match_parent_parameters_after_renaming() {
    common::compile_and_run(
        r#"
package a
class Base<T>() =
    public function inspect(self, value: T): Int32 = 1
    public function inspect(self, value: Bool): Int32 = 2
class Child<U>() extends Base<U>() =
    public override function inspect(self, value: U): Int32 = 3
function main(): Unit =
    let base: Base<Int32> = Child<Int32>()
    assert base.inspect(12) == 3
    assert base.inspect(true) == 2
"#,
    )
    .expect("overrides compare bound ancestor parameter types");
}

#[test]
fn unrelated_class_overloads_do_not_inherit_trait_contracts() {
    let checked = dovetail::check(
        r#"
package a
trait Inspect =
    function inspect(self, tag: Int32): Unit
class Holder<T>(public value: T) implements Inspect =
    public function inspect(self, tag: Int32): Unit = ()
    public function inspect(self, tag: Bool): Unit where T: Display = ()
    public function inspect<U>(self, tag: U): Unit where U: Display = ()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn trait_contract_matching_substitutes_enclosing_arguments() {
    let checked = dovetail::check(
        r#"
package a
trait Inspect<V> =
    function inspect(self, tag: V): Unit
class Holder<T>(public value: T) implements Inspect<T> =
    public function inspect(self, tag: T): Unit where T: Display = ()
    public function inspect(self, tag: Bool): Unit = ()
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("cannot strengthen its trait contract")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn implementation_contract_substitutes_associated_types_in_repeated_bounds() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item
    function consume<T>(self, value: T): Unit where T: Related<Item>
record Printer = value: Int32
implement Consumer for Printer =
    type Item = Int32
    function consume<U>(self: Printer, value: U): Unit where U: Related<Int32> = value.related(12)
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn generic_implementation_inherits_bounds_using_its_associated_type() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item
    function consume<T>(self, value: T): Unit where T: Related<Item>
record Printer<V> = value: V
implement <V> Consumer for Printer<V> =
    type Item = V
    function consume<U>(self: Printer<V>, value: U): Unit = value.related(self.value)
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn implementation_associated_types_do_not_allow_stronger_method_bounds() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item
    function consume<T>(self, value: T): Unit where T: Related<Item>
record Printer = value: Int32
implement Consumer for Printer =
    type Item = Int32
    function consume<U>(self: Printer, value: U): Unit where U: Related<Bool> = ()
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("cannot strengthen its trait contract")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn class_trait_default_cannot_dispatch_to_unavailable_overload() {
    common::compile_and_run(
        r#"
package a
record Hidden = value: Int32
trait Read =
    function inspect(self, tag: Int32): Int32
    function answer(self): Int32 = self.inspect(1)
class Base<T>(public value: T) =
    public function inspect(self, tag: Bool): Int32 where T: Display = 2
    public function inspect(self, tag: Int32): Int32 = 1
class Child(value: Hidden) extends Base<Hidden>(value) implements Read =
    public override function inspect(self, tag: Int32): Int32 = 1
function main(): Unit = assert Child(Hidden { value = 1 }).answer() == 1
"#,
    )
    .expect("trait defaults select the available overload signature");
}

#[test]
fn class_trait_defaults_select_declared_parameter_types() {
    common::compile_and_run(
        r#"
package a
trait Read =
    function inspect(self, value: Any): Int32
    function answer(self): Int32 = self.inspect(1)
class Reader() implements Read =
    public function inspect(self, value: Int32): Int32 = 1
    public function inspect(self, value: Any): Int32 = 2
function main(): Unit = assert Reader().answer() == 2
"#,
    )
    .expect("default calls preserve the trait's selected signature");
}

#[test]
fn repeated_implementation_bounds_expand_generic_associated_types() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item<X>
    function consume<T>(self, value: T): Unit where T: Related<Item<Int32>>
record Printer = value: Int32
implement Consumer for Printer =
    type Item<X> = X
    function consume<U>(self: Printer, value: U): Unit where U: Related<Int32> = ()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn generic_implementation_bounds_expand_associated_types_with_block_parameters() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item<X>
    function consume<T>(self, value: T): Unit where T: Related<Item<Int32>>
record Printer<T> = value: T
implement <T> Consumer for Printer<T> =
    type Item<X> = (X, T)
    function consume<U>(self: Printer<T>, value: U): Unit where U: Related<(Int32, T)> = ()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn generic_associated_definitions_do_not_allow_stronger_method_bounds() {
    let checked = dovetail::check(
        r#"
package a
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item<X>
    function consume<T>(self, value: T): Unit where T: Related<Item<Int32>>
record Printer = value: Int32
implement Consumer for Printer =
    type Item<X> = X
    function consume<U>(self: Printer, value: U): Unit where U: Related<Bool> = ()
"#,
        "test.dove",
    );
    assert!(
        checked.diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("cannot strengthen its trait contract")),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn repeated_method_bounds_expand_gats_inside_classes_and_interfaces() {
    let checked = dovetail::check(
        r#"
package a
class Box<X>(public value: X)
interface View<X> =
    function read(self): X
trait Related<V> =
    function related(self, value: V): Unit
trait Consumer =
    type Item<X>
    function consume<T>(self, value: T): Unit where T: Related<(Box<Item<Int32>>, View<Item<Bool>>)>
record Printer = value: Int32
implement Consumer for Printer =
    type Item<X> = X
    function consume<U>(self: Printer, value: U): Unit where U: Related<(Box<Int32>, View<Bool>)> = ()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn repeated_method_equalities_expand_gats_inside_classes() {
    let checked = dovetail::check(
        r#"
package a
class Box<X>(public value: X)
trait Related =
    type Output
    function related(self): Output
trait Consumer =
    type Item<X>
    function consume<T>(self, value: T): Unit where T: Related<Output = Box<Item<Int32>>>
record Printer = value: Int32
implement Consumer for Printer =
    type Item<X> = X
    function consume<U>(self: Printer, value: U): Unit where U: Related<Output = Box<Int32>> = ()
"#,
        "test.dove",
    );
    assert!(
        !checked.diagnostics.has_errors(),
        "{:?}",
        checked.diagnostics
    );
}

#[test]
fn primitive_bounds_on_module_methods_preserve_readonly_covariance() {
    common::compile_and_run(
        r#"
package a
record Box<T> = value: ReadonlySlice<T>
class Sink<E>(public marker: E) =
    public function accept(self, bytes: ReadonlySlice<Uint8>): Int32 = bytes.length
module Box<T> =
    function bytes(self): ReadonlySlice<Uint8> where T: Uint8 = self.value
    function send<E>(self, sink: Sink<E>): Int32 where T: Uint8 = sink.accept(self.value)
function identityByte<T>(value: T): Uint8 where T: Uint8 = value
function forward<T>(box: Box<T>): ReadonlySlice<Uint8> where T: Uint8 = box.bytes()
function main(): Unit =
    assert identityByte(7u8) == 7u8
    let box = Box { value = [|1u8, 2u8|].readonly }
    assert forward(box)[1] == 2u8
    assert box.send(Sink(true)) == 2
    let empty = Box { value = Array<Never>.empty().readonly }
    assert empty.bytes().length == 0
"#,
    )
    .expect("primitive subtype bounds preserve readonly covariance");
}

#[test]
fn primitive_bounds_reject_other_storage_types() {
    let errors = common::compile_expecting_errors(
        r#"
package a
function byte<T>(value: T): Uint8 where T: Uint8 = value
function main(): Unit =
    byte(7)
    ()
"#,
    );
    assert!(
        errors.iter().any(|error| error.contains("Uint8")),
        "{errors:?}"
    );
    let errors = common::compile_expecting_errors(
        r#"
package a
class Sink<E>(public marker: E) =
    public function accept(self, bytes: Array<Uint8>): Unit = ()
function invalid<T>(sink: Sink<Bool>, bytes: Array<T>): Unit where T: Uint8 = sink.accept(bytes)
"#,
    );
    assert!(!errors.is_empty(), "mutable arrays must remain invariant");
}

#[test]
fn primitive_bounds_do_not_imply_class_identity() {
    let errors = common::compile_expecting_errors(
        r#"
package a
function identity<T>(value: T): Int64 where T: Uint8 = ClassIdentity.hash(value)
"#,
    );
    assert!(!errors.is_empty());
}

#[test]
fn primitive_bounds_select_conditional_implementations() {
    common::compile_and_run(
        r#"
package a
record ByteBox<T> = value: T
interface ByteValue =
    function byte(self): Uint8
implement <T> ByteValue for ByteBox<T> where T: Uint8 =
    public function byte(self): Uint8 = self.value
function read<T>(value: T): Uint8 where T: ByteValue = value.byte()
function main(): Unit =
    let box = ByteBox { value = 3u8 }
    assert read(box) == 3u8
    let value: ByteValue = box
    assert value.byte() == 3u8
"#,
    )
    .expect("primitive bounds constrain conditional implementations");
    let errors = common::compile_expecting_errors(
        r#"
package a
record ByteBox<T> = value: T
interface ByteValue =
    function byte(self): Uint8
implement <T> ByteValue for ByteBox<T> where T: Uint8 =
    public function byte(self): Uint8 = self.value
function invalid(): ByteValue = ByteBox { value = 3 }
"#,
    );
    assert!(!errors.is_empty());
}
