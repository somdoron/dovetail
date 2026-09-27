mod common;

#[test]
fn mutable_capture_boxing_stays_in_its_closure_body() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let read = (index: Int32) => () => index
    let counter = () =>
        let mutable index = 0
        () =>
            index = index + 1
            index
    let next = counter()
    let get = read(42)
    assert get() == 42
    assert next() == 1
    assert next() == 2
"#,
    )
    .expect("sibling closure locals must not box an immutable capture");
}

// ── Valid closures (check_no_errors — no codegen) ────────────────────

#[test]
fn bare_single_param() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x => x
    f
    ()
"#,
    );
}

#[test]
fn annotated_single_param() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f = (x: Int32) => x
    f
    ()
"#,
    );
}

#[test]
fn multi_param_annotated() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f = (x: Int32, y: Int32) => x
    f
    ()
"#,
    );
}

#[test]
fn multi_param_unannotated() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: (Int32, Int32) => Int32 = (x, y) => x
    f
    ()
"#,
    );
}

#[test]
fn single_parenthesized_unannotated() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x) => x
    f
    ()
"#,
    );
}

#[test]
fn mixed_annotations() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: (Int32, String) => Int32 = (x: Int32, y) => x
    f
    ()
"#,
    );
}

#[test]
fn multi_line_body() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x =>
        let y = x
        y
    f
    ()
"#,
    );
}

#[test]
fn annotated_return_type_inferred() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f = (x: Int32) => x + 1
    f
    ()
"#,
    );
}

// ── Error cases ──────────────────────────────────────────────────────

#[test]
fn error_no_context_no_annotations() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f = x => x
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot infer type")),
        "expected inference error, got: {:?}",
        errors
    );
}

#[test]
fn error_param_count_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x, y) => x
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("parameters")),
        "expected param count error, got: {:?}",
        errors
    );
}

#[test]
fn zero_param_closure_type_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Unit => Int32 = () => 42
    f
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("0 parameters") || e.contains("type mismatch")),
        "expected type mismatch or param count error, got: {:?}",
        errors
    );
}

// ── Param contravariance / return covariance checks ──────────────────

#[test]
fn param_annotation_matches_expected() {
    // Annotation == expected: no error
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x: Int32) => x
    f
    ()
"#,
    );
}

#[test]
fn error_param_annotation_incompatible_with_expected() {
    // Annotation is String but expected param is Int32 — Int32 not assignable to String
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x: String) => 0
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not assignable")),
        "expected assignability error, got: {:?}",
        errors
    );
}

#[test]
fn error_return_type_incompatible_with_expected() {
    // Body returns String but expected return is Int32
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x: Int32) => "hello"
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not assignable")),
        "expected return type error, got: {:?}",
        errors
    );
}

#[test]
fn param_contravariant_with_class_subtype() {
    // Expected param is Child, annotation is Parent — Parent is wider, so Child (expected)
    // is assignable to Parent (annotation). This is valid (contravariant).
    common::check_no_errors(
        r#"
package a

class Parent()
class Child() extends Parent()

function main(): Unit =
    let f: Child => Unit = (x: Parent) => ()
    f
    ()
"#,
    );
}

#[test]
fn error_param_covariant_with_class_subtype() {
    // Expected param is Parent, annotation is Child — Parent is NOT assignable to Child.
    // Contravariance means this should fail.
    let errors = common::compile_expecting_errors(
        r#"
package a

class Parent()
class Child() extends Parent()

function main(): Unit =
    let f: Parent => Unit = (x: Child) => ()
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not assignable")),
        "expected contravariance error, got: {:?}",
        errors
    );
}

#[test]
fn return_covariant_with_class_subtype() {
    // Body returns Child, expected return is Parent — Child assignable to Parent. Valid.
    common::check_no_errors(
        r#"
package a

class Parent()
class Child() extends Parent()

function make_child(): Child = Child()

function main(): Unit =
    let f: Unit => Parent = (_: Unit) => make_child()
    f
    ()
"#,
    );
}

#[test]
fn error_return_contravariant_with_class_subtype() {
    // Body returns Parent, expected return is Child — Parent NOT assignable to Child.
    let errors = common::compile_expecting_errors(
        r#"
package a

class Parent()
class Child() extends Parent()

function make_parent(): Parent = Parent()

function main(): Unit =
    let f: Unit => Child = (_: Unit) => make_parent()
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not assignable")),
        "expected covariance error, got: {:?}",
        errors
    );
}

// ── Closure calls ────────────────────────────────────────────────────

#[test]
fn call_single_param_closure() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x => x
    let result = f(42)
    assert result == 42
"#,
    );
}

#[test]
fn call_multi_param_closure() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: (Int32, Int32) => Int32 = (x, y) => x + y
    let result = f(1, 2)
    assert result == 3
"#,
    );
}

#[test]
fn call_closure_return_value_used() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x => x + 1
    let result = f(5)
    assert result == 6
"#,
    );
}

#[test]
fn call_closure_arg_inference() {
    // Expected type from function-typed variable pushes into closure argument
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let apply: (Int32 => Int32) => Int32 = f => f(10)
    let result = apply(x => x + 1)
    assert result == 11
"#,
    );
}

#[test]
fn call_higher_order() {
    // Function taking a closure param and calling it inside body
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let apply: (Int32, Int32 => Int32) => Int32 = (x, f) => f(x)
    let result = apply(5, x => x * 2)
    assert result == 10
"#,
    );
}

#[test]
fn error_call_wrong_arg_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x => x
    f(1, 2)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 1 argument")),
        "expected arg count error, got: {:?}",
        errors
    );
}

#[test]
fn error_call_wrong_arg_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = x => x
    f("hello")
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expected") && e.contains("Int32")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn error_call_non_function_variable() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x: Int32 = 5
    x(1)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not callable")),
        "expected not-callable error, got: {:?}",
        errors
    );
}

// ── Calling function-typed fields ─────────────────────────────────────

#[test]
fn record_with_function_field() {
    common::check_no_errors(
        r#"
package a

record Foo =
    f: (Int32) => Int32

function main(): Unit =
    let foo = Foo { f = (x: Int32) => x + 1 }
    let g: (Int32) => Int32 = foo.f
    let result = g(42)
    assert result == 43
"#,
    );
}

#[test]
fn call_record_function_field() {
    common::check_no_errors(
        r#"
package a

record Foo =
    f: (Int32) => Int32

function main(): Unit =
    let foo = Foo { f = (x: Int32) => x + 1 }
    let result = foo.f(42)
    assert result == 43
"#,
    );
}

#[test]
fn call_class_function_field() {
    common::check_no_errors(
        r#"
package a

class Bar(public f: (Int32) => Int32)

function main(): Unit =
    let bar = Bar((x: Int32) => x * 2)
    let result = bar.f(5)
    assert result == 10
"#,
    );
}

// ── Non-interference ─────────────────────────────────────────────────

#[test]
fn match_arms_still_work() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x = 5
    let result = match x with
        case 1 => 10
        case _ => 20
    assert result == 20
"#,
    );
}

#[test]
fn function_type_in_let_still_works() {
    common::check_no_errors(
        r#"
package a

function identity(x: Int32): Int32 = x

function main(): Unit =
    let x: Int32 = identity(42)
    assert x == 42
"#,
    );
}

// ── Capture analysis ──────────────────────────────────────────────────

#[test]
fn capture_immutable_variable() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x = 10
    let f: Int32 => Int32 = y => y + x
    f
    ()
"#,
    );
}

#[test]
fn capture_mutable_variable() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let mutable x = 10
    let f: Unit => Int32 = (_: Unit) => x
    f
    ()
"#,
    );
}

#[test]
fn capture_and_mutate() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let mutable x = 0
    let f: Unit => Unit = (_: Unit) =>
        x = x + 1
    f
    ()
"#,
    );
}

#[test]
fn capture_nested() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x = 5
    let f: Unit => Unit => Int32 = (_: Unit) =>
        let g: Unit => Int32 = (_: Unit) => x
        g
    f
    ()
"#,
    );
}

#[test]
fn capture_function_parameter() {
    common::check_no_errors(
        r#"
package a

function make_adder(n: Int32): Int32 => Int32 = x => x + n

function main(): Unit =
    let add5 = make_adder(5)
    add5
    ()
"#,
    );
}

#[test]
fn capture_multiple_variables() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let a = 1
    let b = 2
    let c = 3
    let f: Unit => Int32 = (_: Unit) => a + b + c
    f
    ()
"#,
    );
}

#[test]
fn no_false_captures() {
    // Closure that only uses its own params should not capture anything
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x = 10
    let f: Int32 => Int32 = y => y + 1
    f
    ()
"#,
    );
}

#[test]
fn shadowing_no_capture() {
    // Closure param shadows outer variable — not a capture
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let x = 10
    let f: Int32 => Int32 = x => x + 1
    f
    ()
"#,
    );
}

// ── First-class named function references ────────────────────────────

#[test]
fn function_ref_with_expected_type() {
    common::check_no_errors(
        r#"
package a

function identity(x: Int32): Int32 = x

function main(): Unit =
    let f: Int32 => Int32 = identity
    f
    ()
"#,
    );
}

#[test]
fn function_ref_passed_as_argument() {
    common::check_no_errors(
        r#"
package a

function identity(x: Int32): Int32 = x

function apply(f: Int32 => Int32): Int32 = f(42)

function main(): Unit =
    let result = apply(identity)
    assert result == 42
"#,
    );
}

#[test]
fn function_ref_multi_param() {
    common::check_no_errors(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit =
    let f: (Int32, Int32) => Int32 = add
    f
    ()
"#,
    );
}

#[test]
fn function_ref_inferred_from_single_overload() {
    common::check_no_errors(
        r#"
package a

function double(x: Int32): Int32 = x * 2

function main(): Unit =
    let f = double
    f
    ()
"#,
    );
}

#[test]
fn error_function_ref_ambiguous_overloads() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function convert(x: Int32): String = "int"
function convert(x: String): Int32 = 0

function main(): Unit =
    let f = convert
    f
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("ambiguous")),
        "expected ambiguous function reference error, got: {:?}",
        errors
    );
}

#[test]
fn error_generic_function_no_expected_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let f = identity
    f
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot infer type arguments")),
        "expected type inference error, got: {:?}",
        errors
    );
}

#[test]
fn function_ref_does_not_interfere_with_calls() {
    common::check_no_errors(
        r#"
package a

function identity(x: Int32): Int32 = x

function main(): Unit =
    let result = identity(42)
    assert result == 42
"#,
    );
}

#[test]
fn function_ref_disambiguated_by_expected_type() {
    common::check_no_errors(
        r#"
package a

function convert(x: Int32): String = "int"
function convert(x: String): Int32 = 0

function main(): Unit =
    let f: Int32 => String = convert
    f
    ()
"#,
    );
}

// ── Generic function references ──────────────────────────────────────

#[test]
fn generic_function_ref_with_expected_type() {
    common::check_no_errors(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let f: Int32 => Int32 = identity
    f
    ()
"#,
    );
}

#[test]
fn generic_function_ref_multi_param() {
    common::check_no_errors(
        r#"
package a

function first<T, U>(a: T, b: U): T = a

function main(): Unit =
    let f: (Int32, String) => Int32 = first
    f
    ()
"#,
    );
}

// ── Module function references ───────────────────────────────────────

#[test]
fn module_function_ref() {
    common::check_no_errors(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

function main(): Unit =
    let f: Int32 => Int32 = Math.double
    f
    ()
"#,
    );
}

#[test]
fn module_function_ref_inferred() {
    common::check_no_errors(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

function main(): Unit =
    let f = Math.double
    f
    ()
"#,
    );
}

// ── Static class function references ─────────────────────────────────

#[test]
fn static_class_function_ref_via_module() {
    common::check_no_errors(
        r#"
package a

class Foo(public value: Int32)

module Foo =
    function make(x: Int32): Foo = Foo(x)

function main(): Unit =
    let f: Int32 => Foo = Foo.make
    f
    ()
"#,
    );
}

// ── Bound method references (obj.method) ─────────────────────────────

#[test]
fn bound_method_ref_module_instance_self_only() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let f = p.getX
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_module_instance_with_params() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function add(self, dx: Int32, dy: Int32): Point =
        Point { x = self.x + dx; y = self.y + dy }

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let f: (Int32, Int32) => Point = p.add
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_class_instance() {
    common::check_no_errors(
        r#"
package a

class Counter(public value: Int32) =
    public function getValue(self: Counter): Int32 = self.value

function main(): Unit =
    let c = Counter(42)
    let f = c.getValue
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_trait_impl() {
    common::check_no_errors(
        r#"
package a

record Wrapper =
    value: Int32

trait Describable =
    function describe(self: Self): Int32

implement Describable for Wrapper =
    function describe(self: Wrapper): Int32 = self.value

function main(): Unit =
    let w = Wrapper { value = 10 }
    let f = w.describe
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_named_extension() {
    common::check_no_errors(
        r#"
package a

import a.BoxExt

record MyBox =
    value: Int32

extension BoxExt for MyBox =
    function unwrap(self): Int32 = self.value

function main(): Unit =
    let b = MyBox { value = 5 }
    let f = b.unwrap
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_inferred_single_overload() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let f = p.getX
    f
    ()
"#,
    );
}

#[test]
fn bound_method_ref_passed_as_argument() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function addX(self, dx: Int32): Int32 = self.x + dx

function apply(f: Int32 => Int32, arg: Int32): Int32 = f(arg)

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    let result = apply(p.addX, 10)
    assert result == 52
"#,
    );
}

#[test]
fn bound_method_ref_does_not_interfere_with_calls() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    let result = p.getX()
    assert result == 42
"#,
    );
}

#[test]
fn bound_method_ref_does_not_interfere_with_fields() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    assert p.x == 42
"#,
    );
}

#[test]
fn bound_method_ref_does_not_interfere_with_properties() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    property sum(self): Int32 = self.x + self.y

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert p.sum == 3
"#,
    );
}

// ── Unbound method references (Type.method) ──────────────────────────

#[test]
fn unbound_method_ref_module_instance() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let f: Point => Int32 = Point.getX
    f
    ()
"#,
    );
}

#[test]
fn unbound_method_ref_trait_impl() {
    common::check_no_errors(
        r#"
package a

record Wrapper =
    value: Int32

trait Describable =
    function describe(self: Self): Int32

implement Describable for Wrapper =
    function describe(self: Wrapper): Int32 = self.value

function main(): Unit =
    let f: Wrapper => Int32 = Wrapper.describe
    f
    ()
"#,
    );
}

#[test]
fn unbound_method_ref_named_extension() {
    common::check_no_errors(
        r#"
package a

import a.BoxExt

record MyBox =
    value: Int32

extension BoxExt for MyBox =
    function unwrap(self): Int32 = self.value

function main(): Unit =
    let f: MyBox => Int32 = MyBox.unwrap
    f
    ()
"#,
    );
}

#[test]
fn unbound_method_ref_passed_as_argument() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function apply(f: Point => Int32, p: Point): Int32 = f(p)

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    let result = apply(Point.getX, p)
    assert result == 42
"#,
    );
}

#[test]
fn unbound_method_ref_inferred_single_overload() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let f = Point.getX
    f
    ()
"#,
    );
}

// ── Closure codegen (compile_and_run) ─────────────────────────────────

#[test]
fn run_simple_closure_no_captures() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: Int32 => Int32 = (x: Int32) => x + 1
    assert f(5) == 6
"#,
    )
    .expect("simple closure no captures");
}

#[test]
fn run_multi_param_closure() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: (Int32, Int32) => Int32 = (a: Int32, b: Int32) => a + b
    assert f(3, 4) == 7
"#,
    )
    .expect("multi-param closure");
}

#[test]
fn run_immutable_capture() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 10
    let f: Int32 => Int32 = (y: Int32) => y + x
    assert f(5) == 15
"#,
    )
    .expect("immutable capture");
}

#[test]
fn run_mutable_capture_read() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable x = 10
    let f: Unit => Int32 = (_: Unit) => x
    assert f(()) == 10
"#,
    )
    .expect("mutable capture read");
}

#[test]
fn run_mutable_capture_write_shared_state() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable x = 0
    let inc: Unit => Unit = (_: Unit) =>
        x = x + 1
    inc(())
    assert x == 1
"#,
    )
    .expect("mutable capture write + shared state");
}

#[test]
fn run_higher_order_function() {
    common::compile_and_run(
        r#"
package a

function apply(f: Int32 => Int32, x: Int32): Int32 = f(x)

function main(): Unit =
    let result = apply((x: Int32) => x * 3, 5)
    assert result == 15
"#,
    )
    .expect("higher-order function");
}

#[test]
fn run_closure_returned_from_function() {
    common::compile_and_run(
        r#"
package a

function make_adder(n: Int32): Int32 => Int32 = (x: Int32) => x + n

function main(): Unit =
    let add5 = make_adder(5)
    assert add5(10) == 15
"#,
    )
    .expect("closure returned from function");
}

#[test]
fn run_nested_closures() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 100
    let outer: Unit => Int32 => Int32 = (_: Unit) =>
        (z: Int32) => x + z
    let inner = outer(())
    assert inner(1) == 101
"#,
    )
    .expect("nested closures");
}

#[test]
fn run_string_capture() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let greeting = "hello"
    let f: Unit => String = (_: Unit) => greeting
    assert f(()) == "hello"
"#,
    )
    .expect("reference type capture (String)");
}

#[test]
fn run_record_with_function_field() {
    common::compile_and_run(
        r#"
package a

record Holder =
    f: Int32 => Int32

function main(): Unit =
    let h = Holder { f = (x: Int32) => x + 10 }
    let result = h.f(5)
    assert result == 15
"#,
    )
    .expect("record with function field");
}

// ── FunctionRef codegen (compile_and_run) ─────────────────────────────

#[test]
fn run_function_ref_simple() {
    common::compile_and_run(
        r#"
package a

function identity(x: Int32): Int32 = x

function main(): Unit =
    let f: Int32 => Int32 = identity
    assert f(42) == 42
"#,
    )
    .expect("function ref simple");
}

#[test]
fn run_function_ref_multi_param() {
    common::compile_and_run(
        r#"
package a

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit =
    let f: (Int32, Int32) => Int32 = add
    assert f(3, 4) == 7
"#,
    )
    .expect("function ref multi param");
}

#[test]
fn run_function_ref_passed_as_argument() {
    common::compile_and_run(
        r#"
package a

function identity(x: Int32): Int32 = x

function apply(f: Int32 => Int32, v: Int32): Int32 = f(v)

function main(): Unit =
    assert apply(identity, 42) == 42
"#,
    )
    .expect("function ref passed as argument");
}

#[test]
fn run_generic_function_ref() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let f: Int32 => Int32 = identity
    assert f(42) == 42
"#,
    )
    .expect("generic function ref");
}

#[test]
fn run_module_function_ref() {
    common::compile_and_run(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

function main(): Unit =
    let f: Int32 => Int32 = Math.double
    assert f(5) == 10
"#,
    )
    .expect("module function ref");
}

#[test]
fn run_method_ref_self_only() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    let f = p.getX
    assert f() == 42
"#,
    )
    .expect("method ref self only");
}

#[test]
fn run_method_ref_with_params() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function addX(self, dx: Int32): Int32 = self.x + dx

function main(): Unit =
    let p = Point { x = 10; y = 0 }
    let f: Int32 => Int32 = p.addX
    assert f(5) == 15
"#,
    )
    .expect("method ref with params");
}

#[test]
fn run_method_ref_passed_as_argument() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function addX(self, dx: Int32): Int32 = self.x + dx

function apply(f: Int32 => Int32, arg: Int32): Int32 = f(arg)

function main(): Unit =
    let p = Point { x = 42; y = 0 }
    assert apply(p.addX, 10) == 52
"#,
    )
    .expect("method ref passed as argument");
}

#[test]
fn run_method_ref_class() {
    common::compile_and_run(
        r#"
package a

class Counter(public value: Int32) =
    public function getValue(self: Counter): Int32 = self.value

function main(): Unit =
    let c = Counter(99)
    let f = c.getValue
    assert f() == 99
"#,
    )
    .expect("method ref class");
}

#[test]
fn run_function_ref_closure_field() {
    common::compile_and_run(
        r#"
package a

function double(x: Int32): Int32 = x * 2

record Holder =
    f: Int32 => Int32

function main(): Unit =
    let h = Holder { f = double }
    assert h.f(5) == 10
"#,
    )
    .expect("function ref closure field");
}

// ── Tuple destructuring in closure parameters ────────────────────────

#[test]
fn tuple_destructure_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: ((Int32, Int32)) => Int32 = ((a, b)) => a + b
    assert f((1, 2)) == 3
"#,
    )
    .expect("tuple destructure closure");
}

#[test]
fn tuple_destructure_nested() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: (((Int32, Int32), Int32)) => Int32 = (((a, b), c)) => a + b + c
    assert f(((1, 2), 3)) == 6
"#,
    )
    .expect("nested tuple destructure closure");
}

#[test]
fn tuple_destructure_wildcard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: ((Int32, Int32)) => Int32 = ((_, b)) => b
    assert f((42, 7)) == 7
"#,
    )
    .expect("tuple destructure with wildcard");
}

#[test]
fn tuple_destructure_mixed_params() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: ((Int32, Int32), Int32) => Int32 = ((a, b), c) => a + b + c
    assert f((10, 20), 30) == 60
"#,
    )
    .expect("tuple destructure mixed params");
}

#[test]
fn tuple_destructure_deeply_nested() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: (((Int32, Int32), (Int32, Int32))) => Int32 = (((a, b), (c, d))) => a + b + c + d
    assert f(((1, 2), (3, 4))) == 10
"#,
    )
    .expect("deeply nested tuple destructure");
}

#[test]
fn tuple_destructure_with_capture() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let offset = 100
    let f: ((Int32, Int32)) => Int32 = ((a, b)) => a + b + offset
    assert f((1, 2)) == 103
"#,
    )
    .expect("tuple destructure closure with capture");
}

#[test]
fn tuple_destructure_passed_as_argument() {
    common::compile_and_run(
        r#"
package a

function apply(f: ((Int32, Int32)) => Int32, t: (Int32, Int32)): Int32 = f(t)

function main(): Unit =
    let result = apply(((a, b)) => a * b, (3, 4))
    assert result == 12
"#,
    )
    .expect("tuple destructure closure passed as argument");
}

#[test]
fn tuple_destructure_check_only() {
    common::check_no_errors(
        r#"
package a

function main(): Unit =
    let f: ((Int32, Bool)) => Int32 = ((x, _)) => x
    f
    ()
"#,
    );
}

#[test]
fn error_tuple_destructure_non_tuple_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Int32 = ((a, b)) => a + b
    f
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot destructure non-tuple")),
        "expected non-tuple error, got: {:?}",
        errors
    );
}

// ── Closure variance (always-erased closures) ─────────────────────────
//
// Under always-erased closures, every `Type::Function` of arity N lowers to the same
// canonical `(ref Closure_N)` WASM type. Variance falls out for free: a `(B) => String`
// slot accepts an `(A) => String` value when B extends A (contravariant param), because
// both lower to the same WASM struct and the closure body's prologue casts anyref → A
// at runtime — which succeeds for any B value (since B IS-A A).

#[test]
fn run_contravariant_param_class_subtype() {
    common::compile_and_run(
        r#"
package a

class A(public tag: Int32)
class B(t: Int32) extends A(t)

function foo(f: B => String): String = f(B(42))

function main(): Unit =
    let f: A => String = (a: A) => "ok"
    assert foo(f) == "ok"
"#,
    )
    .expect("contravariant class-subtype closure");
}

#[test]
fn never_closure_parameters_are_unreachable_for_all_value_representations() {
    common::compile_and_run(
        r#"
package a

function integer(value: Int32): Int32 = value
function wideInteger(value: Int64): Int64 = value
function reference(value: String): String = value

function main(): Unit =
    let narrow = (value: Never) => integer(value)
    let wide = (value: Never) => wideInteger(value)
    let text = (value: Never) => reference(value)
    let captured = (value: Never) => () => reference(value)
    ()
"#,
    )
    .expect("uninhabited closure parameters must produce valid unreachable Wasm");
}
