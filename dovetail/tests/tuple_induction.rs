mod common;

#[test]
fn extension_accessors_recover_unconstrained_operands() {
    common::compile_and_run(r#"
package a
function prefix<T, U>(left: T, right: U): T = (left ~ right).init
function projectPrefix<T, U>(value: T ~ U): T = value.init
function suffix<T, U>(left: T, right: U): U = (left ~ right).last
function delayedPrefix<T, U>(left: T, right: U): () => T = () => (left ~ right).init
function main(): Unit =
    assert prefix(1, true) == 1
    assert prefix((1, true), "x") == (1, true)
    assert projectPrefix<Int32, Bool>((1, true)) == 1
    assert projectPrefix<(Int32, Bool), String>((1, true, "x")) == (1, true)
    assert prefix(((1, true), "x"), 2) == ((1, true), "x")
    assert suffix(1, (true, "x")) == (true, "x")
    let delayed = delayedPrefix((1, true), "x")
    assert delayed() == (1, true)
"#).expect("extension projections preserve both operands without shape bounds");
}

#[test]
fn tuple_accessors_preserve_shape_and_evaluate_once() {
    common::compile_and_run(r#"
package a
function first<T>(value: T): Any where T: Tuple = value.init
function finalElement<T>(value: T): () => Any where T: Tuple = () => value.last
function main(): Unit =
    let pair = ((1, true), "x")
    assert pair.init._0 == 1
    assert pair.last == "x"
    let triple = (1, true, "x")
    assert triple.init._1 && triple.last == "x"
    assert (first((1, true)) as Int32) == 1
    assert (first(triple) as (Int32, Bool))._1
    let last = finalElement(triple)
    assert (last() as String) == "x"
    let mutable calls = 0
    let make = () =>
        calls = calls + 1
        (1, true, "x")
    let prefix = make().init
    assert calls == 1 && prefix._1
"#).expect("tuple accessors and generic projections");
}

#[test]
fn pair_and_recursive_implementations_are_disjoint() {
    common::compile_and_run(r#"
package a
trait Arity =
    function arity(self): Int32
implement <A, B> Arity for (A, B) =
    function arity(self: (A, B)): Int32 = 2
implement <T, U> Arity for T ~ U where T: Tuple, T: Arity =
    function arity(self: T ~ U): Int32 = self.init.arity() + 1
function count<T>(value: T): Int32 where T: Arity = value.arity()
function main(): Unit =
    assert count((1, true)) == 2
    assert count(((1, true), "x")) == 2
    assert count((1, true, "x")) == 3
    assert count((1, 2, 3, 4, 5, 6, 7, 8)) == 8
"#).expect("inductive tuple trait");
}

#[test]
fn tuple_constraint_rejects_scalars_and_nominal_wrappers() {
    for value in ["1", "Wrapped((1, true))", "()"] {
        let source = format!(r#"
package a
newtype Wrapped = (Int32, Bool)
function accept<T>(value: T): Unit where T: Tuple = ()
function main(): Unit = accept({value})
"#);
        assert!(dovetail::check(&source, "test.dove").diagnostics.has_errors());
    }
    let source = r#"
package a
implement Tuple for Int32
"#;
    assert!(dovetail::check(source, "test.dove").diagnostics.has_errors());
}

#[test]
fn recursive_tuple_heads_reject_overlap_in_both_orders() {
    let recursive = r#"
implement <T, U> Mark for T ~ U where T: Tuple =
    function mark(self: T ~ U): Int32 = 3
"#;
    let concrete = r#"
implement Mark for (Int32, Bool, String) =
    function mark(self: (Int32, Bool, String)): Int32 = 0
"#;
    for bodies in [format!("{recursive}{concrete}"), format!("{concrete}{recursive}")] {
        let source = format!(r#"
package a
trait Mark =
    function mark(self): Int32
{bodies}
"#);
        let result = dovetail::check(&source, "test.dove");
        assert!(result.diagnostics.iter().any(|d| d.message.contains("overlapping implementations")), "{:?}", result.diagnostics);
    }
}

#[test]
fn inductive_traits_resolve_associated_outputs_and_interface_dispatch() {
    common::compile_and_run(r#"
package a
trait FinalElement =
    type Output
    function finalElement(self): Output
implement <A, B> FinalElement for (A, B) =
    type Output = B
    function finalElement(self: (A, B)): B = self.last
implement <T, U> FinalElement for T ~ U where T: Tuple =
    type Output = U
    function finalElement(self: T ~ U): U = self.last
function pick<T, R>(value: T): R where T: FinalElement<Output = R> = value.finalElement()
interface Count =
    function count(self): Int32
implement <A, B> Count for (A, B) =
    function count(self: (A, B)): Int32 = 2
implement <T, U> Count for T ~ U where T: Tuple, T: Count =
    function count(self: T ~ U): Int32 = self.init.count() + 1
function erasedCount(value: Count): Int32 = value.count()
function main(): Unit =
    assert pick((1, true, "x")) == "x"
    assert pick(((1, true), "x")) == "x"
    assert erasedCount((1, 2, 3, 4, 5, 6, 7)) == 7
    assert (1, true, ("x", 2)).format() == "(1, true, (x, 2))"
"#).expect("associated outputs and erased inductive methods");
}

#[test]
fn symbolic_accessors_flow_through_generic_storage_and_extensions() {
    common::compile_and_run(r#"
package a
record Stored<T> = value: T
function prefix<T>(value: T): Any where T: Tuple =
    let stored = Stored {value = value.init}
    stored.value
function extendPrefix<T, U>(value: T, other: U): Any where T: Tuple = value.init ~ other
function forward<T>(value: T): Any where T: Tuple = prefix(value)
function main(): Unit =
    assert (forward((1, true, "x")) as (Int32, Bool))._1
    assert (extendPrefix((1, true), "x") as (Int32, String))._1 == "x"
    assert (extendPrefix((1, true, "x"), 7) as (Int32, Bool, Int32))._2 == 7
"#).expect("deferred projections normalize before storage and extension");
}

#[test]
fn tuple_shape_proofs_do_not_imply_element_or_prefix_bounds() {
    for source in [r#"
package a
function invalid<T>(value: T): Any = value.init
"#, r#"
package a
function needTuple<T>(value: T): Unit where T: Tuple = ()
function invalid<T>(value: T): Unit where T: Tuple = needTuple(value.init)
"#, r#"
package a
record Opaque = id: Int32
function main(): Unit = assert (1, true, Opaque {id = 1}) == (1, true, Opaque {id = 1})
"#, r#"
package a
implement Tuple for Int32
"#] {
        let result = dovetail::check(source, "test.dove");
        assert!(result.diagnostics.has_errors(), "accepted {source}");
    }
}

#[test]
fn tuple_aliases_prove_shape_but_user_named_tuple_does_not() {
    common::compile_and_run(r#"
package a
type Pair = (Int32, Bool)
function accept<T>(value: T): Unit where T: Tuple = ()
function main(): Unit =
    let pair: Pair = (1, true)
    accept(pair)
"#).expect("alias to tuple proves shape");
    let result = dovetail::check(r#"
package a
trait Tuple
function invalid<T>(value: T): Any where T: Tuple = value.init
"#, "test.dove");
    assert!(result.diagnostics.has_errors());
}

#[test]
fn decreasing_tuple_recursion_does_not_hit_the_cyclic_trait_depth_limit() {
    let elements = (1..=32).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
    let source = format!(r#"
package a
function main(): Unit =
    let value = ({elements})
    assert value == value
    assert value.hash() == value.hash()
"#);
    common::compile_and_run(&source).expect("32-element inductive tuple traits");
}

#[test]
fn constrained_extension_inference_can_decompose_only_larger_tuples() {
    common::compile_and_run(r#"
package a
function prefix<T, U>(value: T ~ U): T where T: Tuple = value.init
function forward<T, U>(value: T ~ U): T where T: Tuple = prefix(value)
function main(): Unit =
    assert prefix((1, true, "x")) == (1, true)
    assert forward((1, true, "x")) == (1, true)
"#).expect("constrained extension inference");
    let result = dovetail::check(r#"
package a
function prefix<T, U>(value: T ~ U): T where T: Tuple = value.init
function main(): Unit =
    let value = prefix(((1, true), "x"))
    ()
"#, "test.dove");
    assert!(result.diagnostics.has_errors());
}

#[test]
fn tuple_constraint_cannot_be_implemented_or_inherited() {
    for declaration in [
        "implement Tuple for Int32",
        "implement <T> Tuple for Array<T>",
        "class Fake() implements Tuple",
        "class Fake<T>(value: T) implements Tuple",
        "trait Fake extends Tuple",
    ] {
        let source = format!("package a\n{declaration}\n");
        let result = dovetail::check(&source, "test.dove");
        assert!(result.diagnostics.iter().any(|d| d.message.contains("Tuple is a built-in structural constraint")), "{declaration}: {:?}", result.diagnostics);
    }
}

#[test]
fn recursive_head_coherence_uses_trait_arguments_but_not_user_bounds() {
    let disjoint = r#"
package a
trait Mark<K> =
    function mark(self): Int32
implement <T, U> Mark<Int32> for T ~ U where T: Tuple =
    function mark(self: T ~ U): Int32 = 1
implement <T, U> Mark<Bool> for T ~ U where T: Tuple =
    function mark(self: T ~ U): Int32 = 2
"#;
    let checked = dovetail::check(disjoint, "test.dove");
    assert!(!checked.diagnostics.has_errors(), "{:?}", checked.diagnostics);
    let overlapping = r#"
package a
trait Mark =
    function mark(self): Int32
implement <T, U> Mark for T ~ U where T: Tuple, T: Equatable =
    function mark(self: T ~ U): Int32 = 1
implement <V, W> Mark for V ~ W where V: Tuple, V: Display =
    function mark(self: V ~ W): Int32 = 2
"#;
    let checked = dovetail::check(overlapping, "test.dove");
    assert!(checked.diagnostics.iter().any(|d| d.message.contains("overlapping implementations")), "{:?}", checked.diagnostics);
}

#[test]
fn ordinary_trait_depth_budget_remains_bounded() {
    let nested = format!("{}Int32{}", "Box<".repeat(32), ">".repeat(32));
    let source = format!(r#"
package a
trait Loop
record Box<T> = value: T
implement Loop for Int32
implement <T> Loop for Box<T> where T: Loop
function requireLoop<T>(value: T): Unit where T: Loop = ()
function check(value: {nested}): Unit = requireLoop(value)
"#);
    let checked = dovetail::check(&source, "test.dove");
    assert!(checked.diagnostics.iter().any(|d| d.message.contains("does not implement trait")), "{:?}", checked.diagnostics);
}

#[test]
fn recursive_interface_implementations_can_call_generic_helpers() {
    common::compile_and_run(r#"
package a
function renderValue<T>(value: T): String where T: Display = value.format()
interface Render =
    function render(self): String
implement <A, B> Render for (A, B) where A: Display, B: Display =
    function render(self: (A, B)): String = renderValue(self)
implement <T, U> Render for T ~ U where T: Tuple, T: Display, U: Display =
    function render(self: T ~ U): String = renderValue(self)
function useRender(value: Render): String = value.render()
function main(): Unit =
    assert useRender((1, 2, 3, 4, 5, 6, 7)) == "(1, 2, 3, 4, 5, 6, 7)"
"#).expect("generic helpers inside recursive interface implementations");
}

#[test]
fn late_generic_helpers_capture_and_box_tuple_projections() {
    common::compile_and_run(r#"
package a
function extractPrefix<T>(value: T): () => Any where T: Tuple = () => value.init
interface Extract =
    function extract(self): () => Any
implement <A, B> Extract for (A, B) =
    function extract(self: (A, B)): () => Any = extractPrefix(self)
implement <T, U> Extract for T ~ U where T: Tuple =
    function extract(self: T ~ U): () => Any = extractPrefix(self)
function useExtract(value: Extract): () => Any = value.extract()
function main(): Unit =
    let pair = useExtract((1, true))
    assert (pair() as Int32) == 1
    let larger = useExtract((1, 2, 3, 4, 5, 6, 7))
    assert (larger() as (Int32, Int32, Int32, Int32, Int32, Int32))._5 == 6
"#).expect("late generic helper capture and projection boxing");
}
