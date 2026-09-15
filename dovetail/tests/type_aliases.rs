mod common;

#[test]
fn test_type_alias_basic_int32() {
    common::compile_and_run(r#"
package a

type Cents = Int32

function main(): Unit =
    let x: Cents = 42
    assert x == 42
"#)
    .expect("basic type alias for Int32");
}

#[test]
fn test_type_alias_as_function_param_and_return() {
    common::compile_and_run(r#"
package a

type Cents = Int32

function double(c: Cents): Cents = c + c

function main(): Unit =
    let result = double(10)
    assert result == 20
"#)
    .expect("type alias as function param and return");
}

#[test]
fn test_type_alias_transparent_to_base_type() {
    common::compile_and_run(r#"
package a

type Cents = Int32

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit =
    let c: Cents = 10
    let result = add(c, 20)
    assert result == 30
"#)
    .expect("alias assignable to base type");
}

#[test]
fn test_type_alias_base_type_assignable_to_alias() {
    common::compile_and_run(r#"
package a

type Cents = Int32

function main(): Unit =
    let x: Int32 = 5
    let c: Cents = x
    assert c == 5
"#)
    .expect("base type assignable to alias");
}

#[test]
fn test_type_alias_to_string() {
    common::compile_and_run(r#"
package a

type Name = String

function greet(n: Name): String = n

function main(): Unit =
    let n: Name = "hello"
    assert greet(n) == "hello"
"#)
    .expect("type alias to String");
}

#[test]
fn test_type_alias_to_bool() {
    common::compile_and_run(r#"
package a

type Flag = Bool

function main(): Unit =
    let f: Flag = true
    assert f
"#)
    .expect("type alias to Bool");
}

#[test]
fn test_type_alias_to_generic_type() {
    common::compile_and_run(r#"
package a

type OptionString = Option<String>

function main(): Unit =
    let x: OptionString = Some("hello")
    match x with
        case Some(s) => assert s == "hello"
        case None => panic "expected Some"
"#)
    .expect("type alias to generic type");
}

#[test]
fn test_type_alias_to_record() {
    common::compile_and_run(r#"
package a

record Point =
    x: Int32
    y: Int32

type Coord = Point

function main(): Unit =
    let c: Coord = Point { x = 1; y = 2 }
    assert c.x == 1
    assert c.y == 2
"#)
    .expect("type alias to record type");
}

#[test]
fn test_type_alias_chain() {
    common::compile_and_run(r#"
package a

type Cents = Int32
type Money = Cents

function main(): Unit =
    let m: Money = 100
    let c: Cents = m
    let i: Int32 = c
    assert i == 100
"#)
    .expect("alias chain: Money -> Cents -> Int32");
}

#[test]
fn test_type_alias_public_visibility() {
    common::compile_and_run(r#"
package a

public type Cents = Int32

function main(): Unit =
    let c: Cents = 42
    assert c == 42
"#)
    .expect("public type alias");
}

#[test]
fn test_type_alias_private_visibility() {
    common::compile_and_run(r#"
package a

private type Cents = Int32

function main(): Unit =
    let c: Cents = 42
    assert c == 42
"#)
    .expect("private type alias within same file");
}

#[test]
fn test_type_alias_not_a_constructor() {
    let errors = common::compile_expecting_errors(r#"
package a

type Cents = Int32

function main(): Unit =
    let x = Cents(5)
    ()
"#);
    assert!(
        !errors.is_empty(),
        "expected errors when using type alias as constructor, got none"
    );
}

#[test]
fn test_type_alias_forward_reference_to_record() {
    common::compile_and_run(r#"
package a

type Coord = Point

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let c: Coord = Point { x = 10; y = 20 }
    assert c.x == 10
"#)
    .expect("type alias forward reference to record");
}

#[test]
fn test_type_alias_duplicate_last_wins() {
    // Duplicates silently overwrite (consistent with records/newtypes behavior).
    // The second definition wins.
    common::compile_and_run(r#"
package a

type Alias = Int32
type Alias = String

function main(): Unit =
    let x: Alias = "hello"
    assert x == "hello"
"#)
    .expect("duplicate type alias last wins");
}

#[test]
fn test_type_alias_in_let_binding() {
    common::compile_and_run(r#"
package a

type Count = Int32

function main(): Unit =
    let mutable c: Count = 0
    c = c + 1
    assert c == 1
"#)
    .expect("type alias in let binding");
}

#[test]
fn test_type_alias_to_tuple() {
    common::compile_and_run(r#"
package a

type Pair = (Int32, String)

function main(): Unit =
    let p: Pair = (42, "hello")
    let (n, s) = p
    assert n == 42
    assert s == "hello"
"#)
    .expect("type alias to tuple type");
}

#[test]
fn test_type_alias_to_array() {
    common::compile_and_run(r#"
package a

type Numbers = Array<Int32>

function main(): Unit =
    let nums: Numbers = [|1, 2, 3|]
    assert nums.length == 3
"#)
    .expect("type alias to array type");
}

// ─── Generic type alias tests ───

#[test]
fn test_generic_type_alias_option() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>

function main(): Unit =
    let x: Maybe<Int32> = Some(42)
    match x with
        case Some(v) => assert v == 42
        case None => panic "expected Some"
"#)
    .expect("generic type alias Maybe<T> = Option<T>");
}

#[test]
fn test_generic_type_alias_with_string() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>

function main(): Unit =
    let x: Maybe<String> = Some("hello")
    match x with
        case Some(s) => assert s == "hello"
        case None => panic "expected Some"
"#)
    .expect("generic type alias Maybe<String>");
}

#[test]
fn test_generic_type_alias_none() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>

function main(): Unit =
    let x: Maybe<Int32> = None
    match x with
        case Some(_) => panic "expected None"
        case None => assert true
"#)
    .expect("generic type alias Maybe<Int32> with None");
}

#[test]
fn test_generic_type_alias_tuple() {
    common::compile_and_run(r#"
package a

type Pair<A, B> = (A, B)

function main(): Unit =
    let p: Pair<Int32, String> = (42, "hello")
    let (n, s) = p
    assert n == 42
    assert s == "hello"
"#)
    .expect("generic type alias Pair<A, B> = (A, B)");
}

#[test]
fn test_generic_type_alias_to_generic_record() {
    common::compile_and_run(r#"
package a

record Box<T> =
    value: T

type WrappedBox<T> = Box<T>

function main(): Unit =
    let b: WrappedBox<Int32> = Box { value = 42 }
    assert b.value == 42
"#)
    .expect("generic alias to generic record");
}

#[test]
fn test_generic_type_alias_chain() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>
type StringOption = Maybe<String>

function main(): Unit =
    let x: StringOption = Some("world")
    match x with
        case Some(s) => assert s == "world"
        case None => panic "expected Some"
"#)
    .expect("alias chain: StringOption = Maybe<String> = Option<String>");
}

#[test]
fn test_generic_type_alias_in_function_signature() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>

function unwrap_or(opt: Maybe<Int32>, default: Int32): Int32 =
    match opt with
        case Some(v) => v
        case None => default

function main(): Unit =
    let x: Maybe<Int32> = Some(10)
    assert unwrap_or(x, 0) == 10
    let y: Maybe<Int32> = None
    assert unwrap_or(y, 99) == 99
"#)
    .expect("generic type alias in function signature");
}

#[test]
fn test_generic_type_alias_array() {
    common::compile_and_run(r#"
package a

type Listing<T> = Array<T>

function main(): Unit =
    let xs: Listing<Int32> = [|1, 2, 3|]
    assert xs.length == 3
"#)
    .expect("generic type alias Listing<T> = Array<T>");
}

#[test]
fn test_generic_type_alias_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(r#"
package a

type Maybe<T> = Option<T>

function main(): Unit =
    let x: Maybe<Int32, String> = Some(42)
    ()
"#);
    assert!(
        errors.iter().any(|e| e.contains("expected 1 type argument")),
        "expected type arg count error, got: {:?}",
        errors,
    );
}

#[test]
fn test_generic_type_alias_missing_type_args() {
    let errors = common::compile_expecting_errors(r#"
package a

type Maybe<T> = Option<T>

function main(): Unit =
    let x: Maybe = Some(42)
    ()
"#);
    assert!(
        !errors.is_empty(),
        "expected errors when using generic alias without type args"
    );
}

#[test]
fn test_generic_type_alias_used_as_generic_function_param() {
    common::compile_and_run(r#"
package a

type Maybe<T> = Option<T>

function is_some<T>(opt: Maybe<T>): Bool =
    match opt with
        case Some(_) => true
        case None => false

function main(): Unit =
    assert is_some(Some(42))
    let n: Maybe<Int32> = None
    assert !is_some(n)
"#)
    .expect("generic type alias with generic function");
}

// ─── Generic type alias trait bound tests ───

#[test]
fn test_generic_type_alias_with_trait_bound_satisfied() {
    common::compile_and_run(r#"
package a

trait Showable =
    function show(self: Self): String

record Name =
    value: String

implement Showable for Name =
    function show(self: Name): String = self.value

type ShowBox<T> where T: Showable = Option<T>

function main(): Unit =
    let x: ShowBox<Name> = Some(Name { value = "hello" })
    match x with
        case Some(n) => assert n.show() == "hello"
        case None => panic "expected Some"
"#)
    .expect("generic type alias with satisfied trait bound");
}

#[test]
fn test_generic_type_alias_with_trait_bound_violated() {
    let errors = common::compile_expecting_errors(r#"
package a

trait Showable =
    function show(self: Self): String

type ShowBox<T> where T: Showable = Option<T>

function main(): Unit =
    let x: ShowBox<Int32> = Some(42)
    ()
"#);
    assert!(
        errors.iter().any(|e| e.contains("does not implement trait")),
        "expected trait bound violation error, got: {:?}",
        errors,
    );
}

#[test]
fn test_generic_type_alias_multiple_type_params_with_bounds() {
    common::compile_and_run(r#"
package a

trait Showable =
    function show(self: Self): String

record Name =
    value: String

implement Showable for Name =
    function show(self: Name): String = self.value

type ShowPair<A, B> where A: Showable, B: Showable = (A, B)

function main(): Unit =
    let p: ShowPair<Name, Name> = (Name { value = "a" }, Name { value = "b" })
    let (x, y) = p
    assert x.show() == "a"
    assert y.show() == "b"
"#)
    .expect("generic type alias with multiple bounded type params");
}

// ─── Type alias in more contexts ───

#[test]
fn test_type_alias_as_record_field_type() {
    common::compile_and_run(r#"
package a

type Cents = Int32

record Wallet =
    amount: Cents

function main(): Unit =
    let w = Wallet { amount = 500 }
    assert w.amount == 500
"#)
    .expect("type alias as record field type");
}

#[test]
fn test_type_alias_as_enum_variant_payload() {
    common::compile_and_run(r#"
package a

type Cents = Int32

enum Payment =
    Cash(Cents)
    Card(String)

function main(): Unit =
    let p = Payment.Cash(100)
    match p with
        case Cash(c) => assert c == 100
        case Card(_) => panic "expected Cash"
"#)
    .expect("type alias as enum variant payload");
}
