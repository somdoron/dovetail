mod common;

// ── Covariant enum ──────────────────────────────────────────────────

#[test]
fn covariant_enum_never_assignable_to_concrete() {
    // Option<out T>: Option.None infers Option<Never>, assignable to Option<Int32>
    common::compile_and_run(
        r#"
package a

enum Option<out T> =
    Some(T)
    None

function take(o: Option<Int32>): Unit = ()

function main(): Unit =
    let n = Option.None
    take(n)
    assert true
"#,
    )
    .expect("covariant enum: Option<Never> assignable to Option<Int32>");
}

#[test]
fn covariant_enum_two_params() {
    // Result<out T, out E>: Result.Ok(42) infers Result<Int32, Never>, assignable to Result<Int32, String>
    common::check_no_errors(
        r#"
package a

enum Result<out T, out E> =
    Ok(T)
    Err(E)

function process(r: Result<Int32, String>): Unit = ()

function main(): Unit =
    let r = Result.Ok(42)
    process(r)
"#,
    );
}

#[test]
fn covariant_enum_partial_inference() {
    // MyResult<out T, out E>: MyResult.Err("oops") infers T=Never from covariant default, E=String from payload
    common::check_no_errors(
        r#"
package a

enum MyResult<out T, out E> =
    Ok(T)
    Err(E)

function process(r: MyResult<Int32, String>): Unit = ()

function main(): Unit =
    let r = MyResult.Err("oops")
    process(r)
"#,
    );
}

#[test]
fn covariant_record_never_assignable() {
    common::check_no_errors(
        r#"
package a

record Box<out T> =
    value: T

function take(b: Box<Int32>): Unit = ()

function make_never(): Box<Never> = panic "never"

function main(): Unit =
    let b: Box<Never> = make_never()
    take(b)
"#,
    );
}

// ── Invariant rejects subtyping ─────────────────────────────────────

#[test]
fn invariant_rejects_never_to_concrete() {
    // Cell<T> (invariant): Cell<Never> should NOT be assignable to Cell<Int32>
    let errors = common::compile_expecting_errors(
        r#"
package a

record Cell<T> =
    value: T

function take(c: Cell<Int32>): Unit = ()

function make_never(): Cell<Never> = panic "never"

function main(): Unit =
    let c: Cell<Never> = make_never()
    take(c)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected rejection for invariant Cell<Never> vs Cell<Int32>, got: {:?}",
        errors
    );
}

// ── Mixed variance ──────────────────────────────────────────────────

#[test]
fn mixed_variance_covariant_and_invariant() {
    // Pair<out A, B>: covariant in A, invariant in B
    // Pair<Never, Int32> should be assignable to Pair<Int32, Int32>
    common::check_no_errors(
        r#"
package a

enum Pair<out A, B> =
    MkPair(A, B)

function take(p: Pair<Int32, Int32>): Unit = ()

function make_never(): Pair<Never, Int32> = panic "never"

function main(): Unit =
    let p: Pair<Never, Int32> = make_never()
    take(p)
"#,
    );
}

#[test]
fn mixed_variance_invariant_rejects() {
    // Pair<out A, B>: covariant in A, invariant in B
    // Pair<Int32, Never> should NOT be assignable to Pair<Int32, Int32> because B is invariant
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Pair<out A, B> =
    MkPair(A, B)

function take(p: Pair<Int32, Int32>): Unit = ()

function make_never(): Pair<Int32, Never> = panic "never"

function main(): Unit =
    let p: Pair<Int32, Never> = make_never()
    take(p)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected rejection for invariant B: Pair<Int32, Never> vs Pair<Int32, Int32>, got: {:?}",
        errors
    );
}

// ── Existing generics still work (no regression) ────────────────────

#[test]
fn invariant_by_default_same_types_work() {
    // No variance annotation: same types still work
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

function take(b: Box<Int32>): Int32 = b.value

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert take(b) == 42
"#,
    )
    .expect("invariant Box<Int32> with matching types works");
}

// ── Covariant enum with match ───────────────────────────────────────

#[test]
fn covariant_enum_with_pattern_match() {
    common::compile_and_run(
        r#"
package a

enum Option<out T> =
    Some(T)
    None

function is_some(o: Option<Int32>): Bool =
    match o with
        case Option.Some(_) => true
        case Option.None => false

function main(): Unit =
    let n: Option<Never> = Option.None
    assert is_some(n) == false
"#,
    )
    .expect("covariant enum with pattern match after subtype assignment");
}

// ── Contravariant ───────────────────────────────────────────────────

#[test]
fn contravariant_record() {
    // Sink<in T>: if A <: B then Sink<B> <: Sink<A>
    // Never <: Int32, so Sink<Int32> <: Sink<Never>
    common::compile_and_run(
        r#"
package a

record Sink<in T> =
    dummy: Int32

function take(s: Sink<Never>): Unit = ()

function main(): Unit =
    let s = Sink<Int32> { dummy = 1 }
    take(s)
    assert true
"#,
    )
    .expect("contravariant: Sink<Int32> assignable to Sink<Never>");
}

#[test]
fn contravariant_rejects_wrong_direction() {
    // Sink<in T>: Sink<Never> should NOT be assignable to Sink<Int32>
    let errors = common::compile_expecting_errors(
        r#"
package a

record Sink<in T> =
    dummy: Int32

function take(s: Sink<Int32>): Unit = ()

function make_never(): Sink<Never> = panic "never"

function main(): Unit =
    let s: Sink<Never> = make_never()
    take(s)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected rejection for contravariant Sink<Never> vs Sink<Int32>, got: {:?}",
        errors
    );
}

// ── Prelude covariant types (qualified) ──────────────────────────────

#[test]
fn prelude_option_none_direct_pass() {
    // Option.None inferred as Option<Never>, covariant to Option<Int32>
    common::compile_and_run(
        r#"
package a

function take(o: Option<Int32>): Unit = ()

function main(): Unit =
    take(Option.None)
    assert true
"#,
    )
    .expect("prelude Option.None direct pass to Option<Int32>");
}

#[test]
fn prelude_option_none_let_inferred() {
    // let n = Option.None infers Option<Never>, covariant to Option<Int32>
    common::compile_and_run(
        r#"
package a

function take(o: Option<Int32>): Unit = ()

function main(): Unit =
    let n = Option.None
    take(n)
    assert true
"#,
    )
    .expect("prelude Option.None let inferred as Option<Never>");
}

#[test]
fn prelude_result_ok_direct_pass() {
    // Result.Ok(42) inferred as Result<Int32, Never>, covariant to Result<Int32, String>
    common::compile_and_run(
        r#"
package a

function process(r: Result<Int32, String>): Unit = ()

function main(): Unit =
    process(Result.Ok(42))
    assert true
"#,
    )
    .expect("prelude Result.Ok direct pass");
}

#[test]
fn prelude_result_ok_let_inferred() {
    // let r = Result.Ok(42) infers Result<Int32, Never>, covariant to Result<Int32, String>
    common::compile_and_run(
        r#"
package a

function process(r: Result<Int32, String>): Unit = ()

function main(): Unit =
    let r = Result.Ok(42)
    process(r)
    assert true
"#,
    )
    .expect("prelude Result.Ok let inferred as Result<Int32, Never>");
}

#[test]
fn prelude_option_covariant_with_match() {
    // Pattern match after covariant direct pass
    common::compile_and_run(
        r#"
package a

function is_some(o: Option<Int32>): Bool =
    match o with
        case Option.Some(_) => true
        case Option.None => false

function main(): Unit =
    assert is_some(Option.None) == false
"#,
    )
    .expect("prelude Option covariant with pattern match");
}

// ── Prelude covariant types (bare variant names) ─────────────────────

#[test]
fn prelude_bare_none_direct_pass() {
    // Bare None inferred as Option<Never>, covariant to Option<Int32>
    common::compile_and_run(
        r#"
package a

function take(o: Option<Int32>): Unit = ()

function main(): Unit =
    take(None)
    assert true
"#,
    )
    .expect("bare None direct pass to Option<Int32>");
}

#[test]
fn prelude_bare_none_let_inferred() {
    // let n = None infers Option<Never>, covariant to Option<Int32>
    common::compile_and_run(
        r#"
package a

function take(o: Option<Int32>): Unit = ()

function main(): Unit =
    let n = None
    take(n)
    assert true
"#,
    )
    .expect("bare None let inferred as Option<Never>");
}

#[test]
fn prelude_bare_some_direct_pass() {
    // Bare Some(42) inferred as Option<Int32>
    common::compile_and_run(
        r#"
package a

function take(o: Option<Int32>): Int32 =
    match o with
        case Option.Some(v) => v
        case Option.None => 0

function main(): Unit =
    assert take(Some(42)) == 42
"#,
    )
    .expect("bare Some direct pass to Option<Int32>");
}

#[test]
fn prelude_bare_ok_direct_pass() {
    // Bare Ok(42) inferred as Result<Int32, Never>, covariant to Result<Int32, String>
    common::compile_and_run(
        r#"
package a

function process(r: Result<Int32, String>): Unit = ()

function main(): Unit =
    process(Ok(42))
    assert true
"#,
    )
    .expect("bare Ok direct pass");
}

#[test]
fn prelude_bare_error_direct_pass() {
    // Bare Error("oops") inferred as Result<Never, String>, covariant to Result<Int32, String>
    common::compile_and_run(
        r#"
package a

function process(r: Result<Int32, String>): Unit = ()

function main(): Unit =
    process(Error("oops"))
    assert true
"#,
    )
    .expect("bare Error direct pass");
}

// ── Post-pass coercion: let binding ─────────────────────────────────────

#[test]
fn let_binding_variance_cast() {
    common::compile_and_run(
        r#"
package a

function take(o: Option<String>): Bool =
    match o with
        case Option.Some(_) => true
        case Option.None => false

function main(): Unit =
    let x: Option<String> = None
    assert take(x) == false
"#,
    )
    .expect("let binding with variance cast: Option<Never> to Option<String>");
}

// ── Post-pass coercion: return type ─────────────────────────────────────

#[test]
fn return_type_variance_cast() {
    common::compile_and_run(
        r#"
package a

function make_none(): Option<Int32> = None

function main(): Unit =
    let o = make_none()
    match o with
        case Option.Some(_) => assert false
        case Option.None => assert true
"#,
    )
    .expect("return type variance cast: None returns Option<Int32>");
}

// ── Post-pass coercion: variable assignment ─────────────────────────────

#[test]
fn variable_assignment_variance_cast() {
    common::compile_and_run(
        r#"
package a

function is_some(o: Option<Int32>): Bool =
    match o with
        case Option.Some(_) => true
        case Option.None => false

function main(): Unit =
    let mutable x: Option<Int32> = Some(1)
    assert is_some(x) == true
    x = None
    assert is_some(x) == false
"#,
    )
    .expect("variable assignment variance cast: None assigned to Option<Int32>");
}

// ── Post-pass coercion: enum variant arg ────────────────────────────────

#[test]
fn enum_variant_arg_variance_cast() {
    // Wrapping Option<Never> (None) in another enum constructor that expects Option<Int32>
    common::compile_and_run(
        r#"
package a

enum Wrapper<out T> =
    Wrap(T)
    Empty

function unwrap(w: Wrapper<Option<Int32>>): Option<Int32> =
    match w with
        case Wrapper.Wrap(v) => v
        case Wrapper.Empty => None

function main(): Unit =
    let w = Wrapper.Wrap(Some(42))
    let o = unwrap(w)
    match o with
        case Option.Some(v) => assert v == 42
        case Option.None => assert false
"#,
    )
    .expect("enum variant arg with nested covariant type");
}

// ── Variance position checking ──────────────────────────────────────

#[test]
fn contravariant_in_record_field_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Bad<in T> =
    value: T

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("contravariant") && e.contains("covariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn covariant_in_record_field_ok() {
    common::check_no_errors(
        r#"
package a

record Good<out T> =
    value: T

function main(): Unit = ()
"#,
    );
}

#[test]
fn contravariant_in_enum_payload_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Bad<in T> =
    Wrap(T)
    Empty

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("contravariant") && e.contains("covariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn covariant_in_enum_payload_ok() {
    common::check_no_errors(
        r#"
package a

enum Good<out T> =
    Wrap(T)
    Empty

function main(): Unit = ()
"#,
    );
}

#[test]
fn covariant_in_contravariant_nested_rejected() {
    // Sink<in B> flips variance: out A in Sink<A> means A is in contravariant position
    let errors = common::compile_expecting_errors(
        r#"
package a

record Sink<in B> =
    dummy: Int32

record Foo<out A> =
    sink: Sink<A>

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("contravariant position")),
        "expected variance position error for nested contravariant, got: {:?}",
        errors
    );
}

#[test]
fn contravariant_in_contravariant_nested_ok() {
    // Sink<in B> flips variance: in A in Sink<A> means A is in contra*contra = covariant... wait
    // Actually: field position = covariant, Sink's param is contravariant, so compose = contravariant
    // in A requires contravariant position — so this should be OK
    common::check_no_errors(
        r#"
package a

record Sink<in B> =
    dummy: Int32

record Foo<in A> =
    sink: Sink<A>

function main(): Unit = ()
"#,
    );
}

#[test]
fn invariant_param_anywhere_ok() {
    common::check_no_errors(
        r#"
package a

record Cell<T> =
    value: T

function main(): Unit = ()
"#,
    );
}

// ── Function type variance cast ────────────────────────────────────

#[test]
fn function_type_variance_cast_return() {
    // A closure returning Result<Int32, Never> should be castable to (Int32) => Result<Int32, String>
    common::compile_and_run(
        r#"
package a

function apply(f: (Int32) => Result<Int32, String>, x: Int32): Result<Int32, String> = f(x)

function main(): Unit =
    let f: (Int32) => Result<Int32, Never> = (x: Int32) => Ok(x * 2)
    let result = apply(f, 5)
    assert result.or(0) == 10
"#,
    )
    .expect("function type variance cast on return type");
}

#[test]
fn function_type_variance_cast_in_let() {
    // A closure with narrower return type assigned to variable with wider function type
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let f: (Int32) => Option<Int32> = (x: Int32) => if x > 0 then Some(x) else None
    assert f(5).or(0) == 5
    assert f(-1).isNone
"#,
    )
    .expect("function type in let binding");
}

// ── Class variance position checking ──────────────────────────────

#[test]
fn class_covariant_param_in_method_param_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo<out A>(value: A) =
    function take(self, a: A): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("contravariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn class_contravariant_param_in_return_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Bar<in A>(dummy: Int32) =
    function result(self): A = intrinsic

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("contravariant") && e.contains("covariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn class_covariant_field_ok() {
    common::check_no_errors(
        r#"
package a

class Foo<out T>(public value: T)

function main(): Unit = ()
"#,
    );
}

#[test]
fn class_contravariant_constructor_param_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo<in T>(value: T)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("contravariant") && e.contains("covariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn class_mutable_field_requires_invariant() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo<out T>(public mutable value: T)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("invariant position")),
        "expected variance position error for mutable field, got: {:?}",
        errors
    );
}

#[test]
fn class_immutable_covariant_field_ok() {
    common::check_no_errors(
        r#"
package a

class Box<out T>(public value: T)

function main(): Unit = ()
"#,
    );
}

#[test]
fn class_covariant_in_closure_return_ok() {
    // A => B has A in covariant position of the closure return
    // f: (B) => A — A appears in return (covariant) position: OK for out A
    common::check_no_errors(
        r#"
package a

class Foo<out A>(value: A) =
    function map<B>(self: Foo<A>, f: (A) => B): B = f(self.value)

function main(): Unit = ()
"#,
    );
}

#[test]
fn class_covariant_in_closure_param_rejected() {
    // f: (B) => A with A in closure param position — but wait,
    // f itself is a method param (contravariant), and A is in return of the function type (covariant of contra = contra)
    // So: method param position is contravariant, function return is covariant of that = contravariant
    // out A in contravariant position: rejected
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo<out A>(value: A) =
    function comap<B>(self: Foo<A>, f: (B) => A): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("contravariant position")),
        "expected variance position error for covariant in closure param, got: {:?}",
        errors
    );
}

#[test]
fn class_contravariant_in_closure_ok() {
    // in A: method param is contravariant, function param is contravariant of that = covariant... no:
    // method param position is contravariant.
    // f: (A) => B — A is in param position of function type = flipped = covariant (contra of contra)
    // in A in covariant position: not OK. Let me think again.
    // Actually: f is a method parameter → contravariant position.
    // Inside f's type (A) => B: A is in function param → flip → covariant (contra ∘ contra = covariant)
    // in A needs contravariant position, but this is covariant → rejected.
    //
    // For `in A` to work in a closure: f: (B) => A in method param position.
    // method param → contravariant. A in function return → same as outer → contravariant.
    // in A in contravariant position → OK.
    common::check_no_errors(
        r#"
package a

class Bar<in A>(dummy: Int32) =
    function comap<B>(self: Bar<A>, f: (B) => A): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn class_contravariant_in_closure_return_rejected() {
    // in A: method param is contravariant.
    // f: (A) => B — A in function param position = flip = covariant (contra of contra = covariant)
    // in A in covariant position → rejected.
    let errors = common::compile_expecting_errors(
        r#"
package a

class Bar<in A>(dummy: Int32) =
    function map<B>(self: Bar<A>, f: (A) => B): Unit = ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("contravariant") && e.contains("covariant position")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn class_covariant_method_return_ok() {
    common::check_no_errors(
        r#"
package a

class Box<out T>(value: T) =
    function get(self): T = self.value

function main(): Unit = ()
"#,
    );
}

#[test]
fn class_invariant_param_anywhere_ok() {
    common::check_no_errors(
        r#"
package a

class Cell<T>(public mutable value: T) =
    function get(self): T = self.value
    function set(self, v: T): Unit = self.value = v

function main(): Unit = ()
"#,
    );
}

/// A covariant type may hold a covariant type declared in ANOTHER package —
/// `Option` and `Result` from the prelude being the cases that matter. This
/// used to be rejected: the variance check re-derived each argument's variance
/// from the package's own registry, which cannot see the prelude, and defaulted
/// the miss to `Invariant`.
#[test]
fn covariant_class_may_hold_a_prelude_option() {
    common::compile_and_run(
        r#"
package a

public class Holder<out T>(public value: Option<T>, public produce: () => Option<T>)

function main(): Unit =
    let h = Holder<Int32>(Some(7i32), () => Some(7i32))
    let widened: Holder<Any> = h
    assert widened.value.isSome
"#,
    )
    .expect("a covariant class holding a prelude Option");
}

#[test]
fn covariant_class_may_hold_a_prelude_result() {
    common::compile_and_run(
        r#"
package a

public class Holder<out T, out E>(public produce: () => Result<T, E>)

function main(): Unit =
    let h = Holder<Int32, String>(() => Ok(7i32))
    let widened: Holder<Any, Any> = h
    match widened.produce() with
        case Ok(v) => assert (v as Int32) == 7i32
        case Error(_) => panic "expected Ok"
"#,
    )
    .expect("a covariant class holding a prelude Result behind a function");
}

/// The check still rejects a genuine violation — a covariant parameter in a
/// contravariant (function-argument) position — rather than passing everything.
#[test]
fn error_covariant_param_in_function_argument_position() {
    let errors = common::compile_expecting_errors(
        r#"
package a

public class Holder<out T>(public consume: (T) => Unit)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot appear in contravariant position")),
        "expected the contravariant misuse to be reported, got: {:?}",
        errors,
    );
}

// ── Array invariance ─────────────────────────────────────────────────
//
// `Array<T>` is mutable, so it must be invariant in `T`. Treating it
// covariantly is the classic array-store hole: the callee writes a supertype
// into the array and the caller reads it back through the narrower alias.

/// Passing `Array<Dog>` where `Array<Animal>` is wanted must be rejected —
/// otherwise the callee can store a plain `Animal` into it.
#[test]
fn array_is_not_covariant_in_its_element() {
    let err = common::compile_and_run(
        r#"
package a

class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function poison(xs: Array<Animal>): Unit =
    xs.set(0, Animal("not-a-dog"))

function main(): Unit =
    let dogs: Array<Dog> = [| Dog("rex") |]
    poison(dogs)
"#,
    )
    .expect_err("Array<Dog> must not be assignable to Array<Animal>");
    assert!(
        format!("{err:?}").contains("compilation failed"),
        "expected a type error, got: {err:?}"
    );
}

/// Nor contravariant.
#[test]
fn array_is_not_contravariant_in_its_element() {
    common::compile_and_run(
        r#"
package a

class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function takes(xs: Array<Dog>): Int32 = xs.length

function main(): Unit =
    let animals: Array<Animal> = [| Animal("generic") |]
    assert takes(animals) == 1
"#,
    )
    .expect_err("Array<Animal> must not be assignable to Array<Dog>");
}

/// `Array.empty()` with no context infers `Array<Never>`, which invariance must
/// keep out of an `Array<Int32>` — it previously slipped through and reached
/// codegen, which emitted invalid WASM.
#[test]
fn array_of_never_does_not_flow_into_a_concrete_array() {
    common::compile_and_run(
        r#"
package a

function takes(xs: Array<Int32>): Int32 = xs.length

function main(): Unit =
    let x = Array.empty()
    assert takes(x) == 0
"#,
    )
    .expect_err("Array<Never> must not be assignable to Array<Int32>");
}

/// The supported spellings must keep working: an annotation, or letting the
/// expected type flow straight into the call.
#[test]
fn concrete_arrays_still_assign() {
    common::compile_and_run(
        r#"
package a

function takes(xs: Array<Int32>): Int32 = xs.length

function main(): Unit =
    assert takes([| 1, 2 |]) == 2
    assert takes(Array.empty()) == 0
    let annotated: Array<Int32> = Array.empty()
    assert takes(annotated) == 0
"#,
    )
    .expect("concrete arrays still assign");
}

/// `List<out T>` stays covariant — invariance is specific to the mutable Array.
#[test]
fn list_remains_covariant() {
    common::compile_and_run(
        r#"
package a

class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function takes(xs: List<Animal>): Int32 = xs.length

function main(): Unit =
    assert takes([Dog("rex")]) == 1
    let dogs: List<Dog> = [Dog("a"), Dog("b")]
    assert takes(dogs) == 2
"#,
    )
    .expect("List stays covariant");
}
