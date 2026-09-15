mod common;

#[test]
fn test_newtype_declare_and_construct() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x = Cents(100)
    ()
"#,
    )
    .expect("newtype declare and construct");
}

#[test]
fn test_newtype_as_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function double(c: Cents): Cents = Cents(200)

function main(): Unit =
    let x = double(Cents(100))
    ()
"#,
    )
    .expect("newtype as function param and return");
}

#[test]
fn test_newtype_rejects_bare_inner_type_assignment() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x: Cents = 5
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error for bare inner type assignment, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_rejects_newtype_to_inner_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x: Int32 = Cents(5)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error for newtype-to-inner assignment, got: {:?}",
        errors
    );
}

#[test]
fn test_distinct_newtypes_same_inner() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32
newtype Meters = Int32

function main(): Unit =
    let x: Cents = Meters(5)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for distinct newtypes, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_wrapping_string() {
    common::compile_and_run(
        r#"
package a

newtype Name = String

function main(): Unit =
    let n = Name("Alice")
    ()
"#,
    )
    .expect("newtype wrapping String");
}

#[test]
fn test_newtype_as_global_variable() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

let price: Cents = Cents(42)

function main(): Unit =
    let p = price
    ()
"#,
    )
    .expect("newtype as global variable");
}

#[test]
fn test_newtype_wrong_number_of_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x = Cents(1, 2)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 1 argument")),
        "expected wrong number of args error, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_zero_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x = Cents()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 1 argument")),
        "expected wrong number of args error for zero args, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_wrong_inner_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let x = Cents("hello")
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for wrong inner type, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_in_if_expression() {
    common::compile_and_run(
        r#"
package a

newtype Score = Int32

function main(): Unit =
    let s = if true then Score(10) else Score(20)
    ()
"#,
    )
    .expect("newtype in if expression");
}

#[test]
fn test_newtype_passed_to_function_and_back() {
    common::compile_and_run(
        r#"
package a

newtype Weight = Int32

function identity(w: Weight): Weight = w

function main(): Unit =
    let w = identity(Weight(100))
    ()
"#,
    )
    .expect("newtype passed to function and back");
}

#[test]
fn test_public_newtype() {
    common::check_no_errors(
        r#"
package a

public newtype Cents = Int32

function main(): Unit =
    let c = Cents(10)
    ()
"#,
    );
}

#[test]
fn test_newtype_type_annotation_on_let() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c: Cents = Cents(50)
    ()
"#,
    )
    .expect("newtype type annotation on let");
}

#[test]
fn test_newtype_in_block() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c =
        let base = 100
        Cents(base)
    ()
"#,
    )
    .expect("newtype in block");
}

#[test]
fn test_newtype_value_access() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    assert c.value == 42
"#,
    )
    .expect("newtype .value access");
}

#[test]
fn test_newtype_value_type_is_inner() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let n: Int32 = Cents(10).value
    assert n == 10
"#,
    )
    .expect("newtype .value type is inner type");
}

#[test]
fn test_newtype_value_on_string() {
    common::compile_and_run(
        r#"
package a

newtype Name = String

function main(): Unit =
    assert Name("Alice").value == "Alice"
"#,
    )
    .expect("newtype .value on String");
}

#[test]
fn test_newtype_value_in_expression() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    assert Cents(10).value + Cents(20).value == 30
"#,
    )
    .expect("newtype .value in expression context");
}

#[test]
fn test_newtype_value_as_function_arg() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit =
    let c = Cents(10)
    assert add(c.value, 20) == 30
"#,
    )
    .expect("newtype .value as function argument");
}

#[test]
fn test_newtype_value_chaining_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    let x = c.value.value
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for .value.value chaining on Int32, got no errors"
    );
}

#[test]
fn test_newtype_non_value_field_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    let x = c.foo
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for non-value field on newtype, got no errors"
    );
}

#[test]
fn test_newtype_equality() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Cents = Int32

function main(): Unit =
    assert Cents(5) == Cents(5)
"#,
    )
    .expect("newtype equality");
}

#[test]
fn test_newtype_inequality() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Cents = Int32

function main(): Unit =
    assert Cents(5) != Cents(6)
"#,
    )
    .expect("newtype inequality");
}

#[test]
fn test_newtype_equality_false() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Cents = Int32

function main(): Unit =
    assert !(Cents(5) != Cents(5))
"#,
    )
    .expect("newtype inequality is false for equal values");
}

#[test]
fn test_newtype_cross_type_equality_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    assert Cents(5) == 5
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires operands of the same type")),
        "expected type error for cross-type comparison, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_string_equality() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Name = String

function main(): Unit =
    assert Name("Alice") == Name("Alice")
"#,
    )
    .expect("newtype string equality");
}

#[test]
fn test_newtype_equality_in_condition() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Cents = Int32

function main(): Unit =
    let result = if Cents(10) == Cents(10) then 1 else 0
    assert result == 1
"#,
    )
    .expect("newtype equality in condition");
}

#[test]
fn test_newtype_pattern_basic() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    let v = match c with
        case Cents(n) => n
    assert v == 42
"#,
    )
    .expect("newtype pattern basic");
}

#[test]
fn test_newtype_pattern_binding_in_expr() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(21)
    let v = match c with
        case Cents(n) => n * 2
    assert v == 42
"#,
    )
    .expect("newtype pattern binding in expr");
}

#[test]
fn test_newtype_pattern_wildcard() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    let v = match c with
        case Cents(_) => 1
    assert v == 1
"#,
    )
    .expect("newtype pattern wildcard");
}

#[test]
fn test_newtype_pattern_exhaustive() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(10)
    let v = match c with
        case Cents(n) => n + 5
    assert v == 15
"#,
    )
    .expect("newtype pattern exhaustive");
}

#[test]
fn test_newtype_pattern_with_wildcard_arm() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    let v = match c with
        case Cents(n) => n
        case _ => 0
    assert v == 42
"#,
    )
    .expect("newtype pattern with wildcard arm");
}

#[test]
fn test_newtype_pattern_with_guard() {
    common::compile_and_run(
        r#"
package a

newtype Score = Int32

function main(): Unit =
    let s = Score(95)
    let grade = match s with
        case Score(v) if v >= 90 => 1
        case Score(v) if v >= 80 => 2
        case Score(_) => 3
    assert grade == 1
"#,
    )
    .expect("newtype pattern with guard");
}

#[test]
fn test_newtype_pattern_string() {
    common::compile_and_run(
        r#"
package a

newtype Name = String

function main(): Unit =
    let n = Name("Alice")
    let s = match n with
        case Name(s) => s
    assert s == "Alice"
"#,
    )
    .expect("newtype pattern string");
}

#[test]
fn test_newtype_pattern_wrong_constructor() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32
newtype Dollars = Int32

function main(): Unit =
    let c = Cents(42)
    let v = match c with
        case Dollars(n) => n
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expected newtype constructor")),
        "expected wrong constructor error, got: {:?}",
        errors
    );
}

#[test]
fn test_private_newtype_construct_in_module() {
    common::compile_and_run(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)

function main(): Unit =
    let e = Email.create("test@example.com")
    ()
"#,
    )
    .expect("private newtype construct inside module");
}

#[test]
fn test_private_newtype_value_in_module() {
    common::compile_and_run(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)
    public function unwrap(e: Email): String = e.value

function main(): Unit =
    let s = Email.unwrap(Email.create("test@example.com"))
    assert s == "test@example.com"
"#,
    )
    .expect("private newtype .value inside module");
}

#[test]
fn test_private_newtype_pattern_in_module() {
    common::compile_and_run(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)

    public function unwrap(e: Email): String =
        match e with
            case Email(s) => s

function main(): Unit =
    let e = Email.create("test@example.com")
    assert Email.unwrap(e) == "test@example.com"
"#,
    )
    .expect("private newtype pattern inside module");
}

#[test]
fn test_private_newtype_construct_outside_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Email private = String

function main(): Unit =
    let e = Email("test@example.com")
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot construct private newtype")),
        "expected private newtype construct error, got: {:?}",
        errors
    );
}

#[test]
fn test_private_newtype_value_outside_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)

function main(): Unit =
    let e = Email.create("test@example.com")
    let s = e.value
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot access .value on private newtype")),
        "expected private newtype .value error, got: {:?}",
        errors
    );
}

#[test]
fn test_private_newtype_pattern_outside_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)

function main(): Unit =
    let e = Email.create("test@example.com")
    let s = match e with
        case Email(v) => v
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot pattern match on private newtype")),
        "expected private newtype pattern error, got: {:?}",
        errors
    );
}

#[test]
fn test_non_private_newtype_still_works() {
    common::compile_and_run(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents(42)
    assert c.value == 42
    let v = match c with
        case Cents(n) => n
    assert v == 42
"#,
    )
    .expect("non-private newtype still works");
}

#[test]
fn test_private_newtype_equality_works_outside() {
    common::compile_and_run(
        r#"
package a

newtype Email private = String

module Email =
    public function create(s: String): Email = Email(s)
    public function equalValues(a: Email, b: Email): Bool = a.value == b.value

implement Equatable for Email =
    public function equals(self: Email, other: Email): Bool = Email.equalValues(self, other)

function main(): Unit =
    let a = Email.create("test@example.com")
    let b = Email.create("test@example.com")
    assert a == b
    assert !(a != b)
"#,
    )
    .expect("private newtype equality works outside module");
}

#[test]
fn test_private_newtype_construct_in_wrong_module_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Email private = String

module Other =
    public function make(): Email = Email("test@example.com")

function main(): Unit =
    let e = Other.make()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot construct private newtype")),
        "expected private newtype construct error in wrong module, got: {:?}",
        errors
    );
}

#[test]
fn test_newtype_self_in_record_constructor() {
    common::compile_and_run(
        r#"
package a

newtype Handle = Int32

record Wrapper =
    handle: Handle
    value: Int32

module Handle =
    function wrap(self, v: Int32): Wrapper =
        Wrapper { handle = self; value = v }

function main(): Unit =
    let h = Handle(42)
    let w = h.wrap(10)
    assert w.value == 10
"#,
    )
    .expect("newtype self in record constructor");
}

// ==================== Generic Newtype Tests ====================

#[test]
fn test_generic_newtype_basic() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w = Wrapper<Int32>(42)
    assert w.value == 42
"#,
    )
    .expect("basic generic newtype");
}

#[test]
fn test_generic_newtype_infer_type_arg() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w = Wrapper(42)
    assert w.value == 42
"#,
    )
    .expect("generic newtype infer type arg from argument");
}

#[test]
fn test_generic_newtype_string() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w = Wrapper("hello")
    assert w.value == "hello"
"#,
    )
    .expect("generic newtype with string");
}

#[test]
fn test_generic_newtype_phantom_type() {
    common::compile_and_run(
        r#"
package a

newtype Tag<T> = String

function main(): Unit =
    let t = Tag<Int32>("hello")
    assert t.value == "hello"
"#,
    )
    .expect("phantom type generic newtype");
}

#[test]
fn test_generic_newtype_phantom_type_distinct() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Tag<T> = String

function main(): Unit =
    let a: Tag<Int32> = Tag<Int32>("hello")
    let b: Tag<Bool> = a
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch for distinct phantom types, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_phantom_requires_explicit_type_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Tag<T> = String

function main(): Unit =
    let t = Tag("hello")
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot infer type argument")),
        "expected cannot infer type argument error for phantom type, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_as_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function unwrap(w: Wrapper<Int32>): Int32 = w.value

function main(): Unit =
    let w = Wrapper(42)
    assert unwrap(w) == 42
"#,
    )
    .expect("generic newtype as function param/return");
}

#[test]
fn test_generic_newtype_pattern_matching() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w = Wrapper(42)
    let v = match w with
        case Wrapper(n) => n
    assert v == 42
"#,
    )
    .expect("generic newtype pattern matching");
}

#[test]
fn test_generic_newtype_equality() {
    common::compile_and_run(
        r#"
package a

@derive(Equatable)
newtype Wrapper<T> = T

function main(): Unit =
    assert Wrapper(5) == Wrapper(5)
    assert Wrapper(5) != Wrapper(6)
"#,
    )
    .expect("generic newtype equality");
}

#[test]
fn test_generic_newtype_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w = Wrapper<Int32, Bool>(42)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type argument")),
        "expected wrong type arg count error, got: {:?}",
        errors
    );
}

#[test]
fn test_type_args_on_non_generic_newtype_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Cents = Int32

function main(): Unit =
    let c = Cents<Int32>(42)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not generic")),
        "expected not generic error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_variance() {
    common::check_no_errors(
        r#"
package a

newtype Wrapper<out T> = T

function main(): Unit =
    let w = Wrapper(42)
    ()
"#,
    );
}

#[test]
fn test_generic_newtype_private_inner() {
    common::compile_and_run(
        r#"
package a

newtype Secret<T> private = T

module Secret =
    public function create(v: Int32): Secret<Int32> = Secret<Int32>(v)
    public function reveal(s: Secret<Int32>): Int32 = s.value

function main(): Unit =
    let s = Secret.create(42)
    assert Secret.reveal(s) == 42
"#,
    )
    .expect("generic newtype private inner");
}

#[test]
fn test_generic_newtype_private_construct_outside_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Secret<T> private = T

function main(): Unit =
    let s = Secret<Int32>(42)
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot construct private newtype")),
        "expected private newtype construct error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_in_type_annotation() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

function main(): Unit =
    let w: Wrapper<Int32> = Wrapper(42)
    assert w.value == 42
"#,
    )
    .expect("generic newtype in type annotation");
}

#[test]
fn test_generic_newtype_with_associated_module() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

module Wrapper<T> =
    function unwrap(self): T = self.value

function main(): Unit =
    let w = Wrapper(42)
    assert w.unwrap() == 42
"#,
    )
    .expect("generic newtype with associated module");
}

#[test]
fn test_generic_newtype_module_static_function() {
    common::compile_and_run(
        r#"
package a

newtype Wrapper<T> = T

module Wrapper<T> =
    public function create(v: T): Wrapper<T> = Wrapper<T>(v)
    function unwrap(self): T = self.value

function main(): Unit =
    let w = Wrapper.create(99)
    assert w.unwrap() == 99
"#,
    )
    .expect("generic newtype module static function");
}

#[test]
fn test_generic_newtype_module_private_inner() {
    common::compile_and_run(
        r#"
package a

newtype Secret<T> private = T

module Secret<T> =
    public function wrap(v: T): Secret<T> = Secret<T>(v)
    public function peek(self): T = self.value

function main(): Unit =
    let s = Secret.wrap(42)
    assert s.peek() == 42
"#,
    )
    .expect("generic newtype module with private inner");
}

#[test]
fn test_generic_newtype_module_private_inner_outside_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Secret<T> private = T

module Secret<T> =
    public function wrap(v: T): Secret<T> = Secret<T>(v)

function main(): Unit =
    let s = Secret.wrap(42)
    let v = s.value
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot access .value on private newtype")),
        "expected private newtype .value error outside module, got: {:?}",
        errors
    );
}

// ==================== Generic Newtype Hardening Tests ====================

#[test]
fn test_generic_newtype_trait_bounds_on_type_params() {
    common::check_no_errors(
        r#"
package a

trait Showable =
    function show(self: Self): String

newtype Wrapper<T> where T: Showable = T

record MyRec =
    value: Int32

implement Showable for MyRec =
    function show(self: MyRec): String = "MyRec"

function main(): Unit =
    let w = Wrapper<MyRec>(MyRec { value = 1 })
    ()
"#,
    );
}

#[test]
fn test_generic_newtype_trait_bounds_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Showable =
    function show(self: Self): String

newtype Wrapper<T> where T: Showable = T

function main(): Unit =
    let w = Wrapper<Int32>(42)
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected trait bound violation error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_variance_violation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

newtype Bad<out T> = (T) => Bool

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("contravariant")),
        "expected variance position error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_newtype_implement_block() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

newtype Wrapper<T> = T

implement <T> Display for Wrapper<T> =
    function format(self: Wrapper<T>): String = "wrapper"

function main(): Unit =
    let w = Wrapper(42)
    assert w.format() == "wrapper"
"#,
    )
    .expect("generic implement block for newtype");
}

#[test]
fn test_generic_newtype_implement_block_with_where_clause() {
    common::compile_and_run(
        r#"
package a

trait Showable =
    function show(self: Self): String

newtype Wrapper<T> = T

implement Showable for Int32 =
    function show(self: Int32): String = "int"

implement <T> Showable for Wrapper<T> where T: Showable =
    function show(self: Wrapper<T>): String = "wrapped"

function main(): Unit =
    let w = Wrapper(42)
    assert w.show() == "wrapped"
"#,
    )
    .expect("generic implement block with where clause for newtype");
}

#[test]
fn test_concrete_implement_block_for_generic_newtype() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

newtype Wrapper<T> = T

implement Display for Wrapper<Int32> =
    function format(self: Wrapper<Int32>): String = "int-wrapper"

function main(): Unit =
    let w = Wrapper(42)
    assert w.format() == "int-wrapper"
"#,
    )
    .expect("concrete implement block for generic newtype");
}

#[test]
fn test_generic_newtype_phantom_type_with_trait_bound() {
    common::check_no_errors(
        r#"
package a

trait Serializable =
    function serialize(self: Self): String

newtype Tag<T> where T: Serializable = String

implement Serializable for Int32 =
    function serialize(self: Int32): String = "int"

function main(): Unit =
    let t = Tag<Int32>("hello")
    ()
"#,
    );
}

#[test]
fn test_generic_newtype_phantom_type_trait_bound_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Serializable =
    function serialize(self: Self): String

newtype Tag<T> where T: Serializable = String

record Foo =
    x: Int32

function main(): Unit =
    let t = Tag<Foo>("hello")
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected trait bound violation for phantom type, got: {:?}",
        errors
    );
}

// ── ByName<T> implicit wrapping ─────────────────────────────────────

#[test]
fn byname_type_resolves() {
    common::check_no_errors(
        r#"
package a

function foo(x: ByName<Int32>): Unit = ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn byname_basic_implicit_wrapping() {
    common::compile_and_run(
        r#"
package a

function foo(x: ByName<Int32>): Int32 = x.get

function main(): Unit =
    let result = foo(42)
    assert result == 42
"#,
    )
    .expect("ByName basic wrapping");
}

#[test]
fn byname_deferred_evaluation() {
    common::compile_and_run(
        r#"
package a

function getOrDefault(value: Option<Int32>, default: ByName<Int32>): Int32 =
    match value with
        case Some(v) => v
        case None => default.get

function main(): Unit =
    let result1 = getOrDefault(Option.Some(10), 99)
    assert result1 == 10
    let result2 = getOrDefault(Option.None, 99)
    assert result2 == 99
"#,
    )
    .expect("ByName deferred evaluation");
}

#[test]
fn byname_with_expression() {
    common::compile_and_run(
        r#"
package a

function compute(x: ByName<Int32>): Int32 = x.get + x.get

function main(): Unit =
    let result = compute(10 + 5)
    assert result == 30
"#,
    )
    .expect("ByName with expression");
}

#[test]
fn byname_captures_variable() {
    common::compile_and_run(
        r#"
package a

function evaluate(thunk: ByName<Int32>): Int32 = thunk.get

function main(): Unit =
    let x = 42
    let result = evaluate(x + 1)
    assert result == 43
"#,
    )
    .expect("ByName captures variable");
}

#[test]
fn byname_with_string() {
    common::compile_and_run(
        r#"
package a

function greet(name: ByName<String>): String = name.get

function main(): Unit =
    let result = greet("world")
    assert result == "world"
"#,
    )
    .expect("ByName with string");
}

#[test]
fn byname_explicit_closure_still_works() {
    common::compile_and_run(
        r#"
package a

function evaluate(thunk: () => Int32): Int32 = thunk()

function main(): Unit =
    let result = evaluate(() => 42)
    assert result == 42
"#,
    )
    .expect("explicit zero-param closure still works");
}
