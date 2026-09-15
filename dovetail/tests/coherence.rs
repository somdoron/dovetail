mod common;

// Coherence (trait-design-appendix §6.2): two implement blocks for the same
// trait may not overlap — no type may match both. Conservative shape rule:
// where-bounds never disprove overlap. Also pins the fixes to the collect-time
// duplicate check (sibling instantiations are legal).

// ── Sibling instantiations are legal and dispatch correctly ─────────

#[test]
fn test_sibling_instantiations_accepted() {
    common::compile_and_run(
        r#"
package a

trait Show =
    function show(self): Int32

implement Show for List<Int32> =
    function show(self): Int32 = 1

implement Show for List<String> =
    function show(self): Int32 = 2

function main(): Unit =
    let xs: List<Int32> = [1, 2]
    let ys: List<String> = ["a"]
    assert xs.show() == 1
    assert ys.show() == 2
"#,
    )
    .expect("sibling instantiations register and dispatch to the right block");
}

// ── Exact duplicates still error early ──────────────────────────────

#[test]
fn test_exact_duplicate_still_early_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Rec =
    x: Int32

trait Show =
    function show(self): Int32

implement Show for Rec =
    function show(self): Int32 = 1

implement Show for Rec =
    function show(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("already implements")),
        "expected the collect-time duplicate error, got: {:?}",
        errors
    );
}

// ── Different trait args stay disjoint ──────────────────────────────

#[test]
fn test_trait_args_disjoint_regression() {
    common::check_no_errors(
        r#"
package a

record Rec =
    x: Int32

trait Convert<T> =
    function convert(self): T

implement Convert<Int32> for Rec =
    function convert(self): Int32 = self.x

implement Convert<String> for Rec =
    function convert(self): String = "s"

function main(): Unit = ()
"#,
    );
}

// ── Blanket + concrete overlap ──────────────────────────────────────

#[test]
fn test_blanket_plus_concrete_overlap() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

trait Show =
    function show(self): Int32

implement <T> Show for Box<T> =
    function show(self): Int32 = 1

implement Show for Box<Int32> =
    function show(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations of trait 'Show'") && e.contains("Box<Int32>")),
        "expected overlap error naming the witness, got: {:?}",
        errors
    );
}

// ── Two blankets overlap ────────────────────────────────────────────

#[test]
fn test_two_blankets_overlap() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

trait Show =
    function show(self): Int32

implement <T> Show for Box<T> =
    function show(self): Int32 = 1

implement <U> Show for Box<U> =
    function show(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations of trait 'Show'")),
        "expected overlap error for two blankets, got: {:?}",
        errors
    );
}

// ── Disjoint blanket shapes accepted ────────────────────────────────

#[test]
fn test_disjoint_blankets_accepted() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> =
    first: A
    second: B

trait Show =
    function show(self): Int32

implement <T> Show for Pair<T, Int32> =
    function show(self): Int32 = 1

implement <T> Show for Pair<T, String> =
    function show(self): Int32 = 2

function main(): Unit =
    let p = Pair { first = true; second = 5 }
    let q = Pair { first = true; second = "x" }
    assert p.show() == 1
    assert q.show() == 2
"#,
    )
    .expect("disjoint blanket shapes coexist and dispatch correctly");
}

// ── Where-bounds do not disprove overlap ────────────────────────────

#[test]
fn test_bounds_do_not_disprove_overlap() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

trait Red =
    function red(self): Int32

trait Blue =
    function blue(self): Int32

trait Show =
    function show(self): Int32

implement <T> Show for Box<T> where T: Red =
    function show(self): Int32 = 1

implement <T> Show for Box<T> where T: Blue =
    function show(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations of trait 'Show'")),
        "bounds must not disprove overlap (conservative rule), got: {:?}",
        errors
    );
}

// ── Round-5 review regressions ──────────────────────────────────────

#[test]
fn test_tuple_sibling_impls_distinct() {
    common::compile_and_run(
        r#"
package a

trait Tagged =
    function tag(self): Int32

implement Tagged for (Int32, Int32) =
    function tag(self): Int32 = 1

implement Tagged for (String, String) =
    function tag(self): Int32 = 2

function main(): Unit =
    let a = (1, 2)
    let b = ("x", "y")
    assert a.tag() == 1
    assert b.tag() == 2
"#,
    )
    .expect("tuple sibling impls mangle distinctly and dispatch correctly");
}

#[test]
fn test_function_type_sibling_impls_distinct() {
    common::compile_and_run(
        r#"
package a

trait Tagged2 =
    function tag(self): Int32

implement Tagged2 for Int32 => Int32 =
    function tag(self): Int32 = 1

implement Tagged2 for String => String =
    function tag(self): Int32 = 2

function main(): Unit =
    let f = (n: Int32) => n
    let g = (s: String) => s
    assert f.tag() == 1
    assert g.tag() == 2
"#,
    )
    .expect("function-type sibling impls mangle distinctly");
}

#[test]
fn test_tuple_interface_object_coercion() {
    common::compile_and_run(
        r#"
package a

interface Desc =
    function describe(self): Int32

implement Desc for (Int32, Int32) =
    function describe(self): Int32 =
        let (a, b) = self
        a + b

function label(v: Desc): Int32 = v.describe()

function main(): Unit =
    assert label((1, 2)) == 3
"#,
    )
    .expect("tuples box into interface objects");
}

#[test]
fn test_tuple_self_return_via_interface() {
    common::compile_and_run(
        r#"
package a

interface Fluent =
    function bump(self): Self
    function value(self): Int32

implement Fluent for (Int32, Int32) =
    function bump(self): (Int32, Int32) =
        let (a, b) = self
        (a + 1, b + 1)
    function value(self): Int32 =
        let (a, b) = self
        a + b

function main(): Unit =
    let f: Fluent = (1, 2)
    assert f.bump().value() == 5
"#,
    )
    .expect("Self-returning tuple impls re-box through the vtable");
}

#[test]
fn test_coherence_chained_same_side_binding_overlap_detected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    v: T

trait Tr3<A, B, C> =
    function f(self): Int32

implement <T> Tr3<T, Int32, T> for Box<T> =
    function f(self): Int32 = 1

implement <U, S> Tr3<List<S>, S, List<Int32>> for Box<U> =
    function f(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations")),
        "both blocks apply to Box<List<Int32>> at Tr3<List<Int32>, Int32, List<Int32>>, got: {:?}",
        errors
    );
}

#[test]
fn test_coherence_interface_object_for_type_overlap_detected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

trait Tr =
    function t(self): Int32

implement <T> Tr for Producer<T> =
    function t(self): Int32 = 1

implement Tr for Producer<Int32> =
    function t(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations")),
        "a generic and a concrete interface-object for_type overlap, got: {:?}",
        errors
    );
}

#[test]
fn test_coherence_witness_substitutes_tuple_and_newtype_shapes() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Tr =
    function t(self): Int32

implement <T> Tr for (T, Int32) =
    function t(self): Int32 = 1

implement Tr for (String, Int32) =
    function t(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations") && e.contains("(String, Int32)")),
        "the witness must render the SUBSTITUTED tuple, got: {:?}",
        errors
    );
}

#[test]
fn test_coherence_witness_substitutes_interface_object_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

trait Tr =
    function t(self): Int32

implement <T> Tr for Producer<T> =
    function t(self): Int32 = 1

implement Tr for Producer<Int32> =
    function t(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("overlapping implementations") && e.contains("Producer<Int32>")),
        "the witness must render the SUBSTITUTED interface application, got: {:?}",
        errors
    );
}
