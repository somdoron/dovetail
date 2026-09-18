mod common;

// --- Enum declaration tests (Phase 1: declaration only, no construction/match) ---

#[test]
fn test_simple_enum_declaration() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit = ()
"#,
    )
    .expect("simple enum declaration");
}

#[test]
fn test_enum_with_tuple_payloads() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Rectangle(Float64, Float64)
    Point

function main(): Unit = ()
"#,
    )
    .expect("enum with tuple payloads");
}

#[test]
fn test_enum_with_type_params() {
    common::check_no_errors(
        r#"
package a

enum Option<T> =
    Some(T)
    None
"#,
    );
}

#[test]
fn test_enum_with_multiple_type_params() {
    common::check_no_errors(
        r#"
package a

enum Result<T, E> =
    Ok(T)
    Error(E)
"#,
    );
}

#[test]
fn test_error_duplicate_variant_names() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Red

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate variant")),
        "expected duplicate variant error, got: {:?}",
        errors
    );
}

#[test]
fn test_enum_alongside_records_and_functions() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Color =
    Red
    Green
    Blue

function add(a: Int32, b: Int32): Int32 = a + b

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert add(p.x, p.y) == 3
"#,
    )
    .expect("enum alongside records and functions");
}

#[test]
fn test_public_enum_declaration() {
    common::compile_and_run(
        r#"
package a

public enum Direction =
    North
    South
    East
    West

function main(): Unit = ()
"#,
    )
    .expect("public enum declaration");
}

#[test]
fn test_private_enum_declaration() {
    common::compile_and_run(
        r#"
package a

private enum Status =
    Active
    Inactive

function main(): Unit = ()
"#,
    )
    .expect("private enum declaration");
}

#[test]
fn test_enum_with_record_payload_type() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Shape =
    Circle(Float64)
    AtPoint(Point)

function main(): Unit = ()
"#,
    )
    .expect("enum with record payload type");
}

#[test]
fn test_enum_forward_reference_in_record() {
    common::compile_and_run(
        r#"
package a

record Container =
    color: Color

enum Color =
    Red
    Green
    Blue

function main(): Unit = ()
"#,
    )
    .expect("enum forward reference in record");
}

#[test]
fn test_enum_single_variant() {
    common::compile_and_run(
        r#"
package a

enum Wrapper =
    Value(Int32)

function main(): Unit = ()
"#,
    )
    .expect("enum single variant");
}

#[test]
fn test_enum_payload_with_multiple_types() {
    common::compile_and_run(
        r#"
package a

enum Event =
    Click(Int32, Int32)
    KeyPress(Char)
    Resize(Int32, Int32, Int32, Int32)

function main(): Unit = ()
"#,
    )
    .expect("enum payload with multiple types");
}

#[test]
fn test_enum_with_string_payload() {
    common::compile_and_run(
        r#"
package a

enum Message =
    Text(String)
    Empty

function main(): Unit = ()
"#,
    )
    .expect("enum with string payload");
}

// --- Enum constructor tests (Phase 2: construction expressions) ---

#[test]
fn test_enum_no_payload_construction() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c: Color = Color.Red
    let g: Color = Color.Green
    let b: Color = Color.Blue
    ()
"#,
    )
    .expect("no-payload enum construction");
}

#[test]
fn test_enum_single_payload_construction() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Rectangle(Float64, Float64)
    Point

function main(): Unit =
    let s: Shape = Shape.Circle(5.0)
    ()
"#,
    )
    .expect("single-payload enum construction");
}

#[test]
fn test_enum_multi_payload_construction() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Rectangle(Float64, Float64)
    Point

function main(): Unit =
    let s: Shape = Shape.Rectangle(10.0, 20.0)
    ()
"#,
    )
    .expect("multi-payload enum construction");
}

#[test]
fn test_enum_mixed_variant_construction() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Rectangle(Float64, Float64)
    Point

function main(): Unit =
    let a: Shape = Shape.Circle(3.14)
    let b: Shape = Shape.Rectangle(10.0, 20.0)
    let c: Shape = Shape.Point
    ()
"#,
    )
    .expect("mixed variant construction");
}

#[test]
fn test_enum_as_function_parameter() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function f(c: Color): Unit = ()

function main(): Unit =
    f(Color.Red)
    f(Color.Green)
    f(Color.Blue)
"#,
    )
    .expect("enum as function parameter");
}

#[test]
fn test_enum_with_record_payload_construction() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Shape =
    Circle(Float64)
    AtPoint(Point)

function main(): Unit =
    let s: Shape = Shape.AtPoint(Point { x = 1; y = 2 })
    ()
"#,
    )
    .expect("enum with record payload construction");
}

#[test]
fn test_error_no_payload_variant_with_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c: Color = Color.Red(42)
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("has no fields") || e.contains("without parentheses")),
        "expected no-fields error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_wrong_arg_types() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point

function main(): Unit =
    let s: Shape = Shape.Circle("hello")
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_unknown_variant() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c: Color = Color.Purple
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no variant")),
        "expected unknown variant error, got: {:?}",
        errors
    );
}

#[test]
fn test_error_payload_variant_via_field_access() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point

function main(): Unit =
    let s: Shape = Shape.Circle
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("requires 1 argument")),
        "expected missing args error, got: {:?}",
        errors
    );
}

// --- Recursive enum type tests ---

#[test]
fn test_recursive_enum_self_referential() {
    common::compile_and_run(
        r#"
package a

record Cons =
    head: Int32
    tail: IntList

enum IntList =
    Nil
    Node(Cons)

function main(): Unit =
    let empty: IntList = IntList.Nil
    let one: IntList = IntList.Node(Cons { head = 1; tail = IntList.Nil })
    let two: IntList = IntList.Node(Cons { head = 2; tail = one })
    ()
"#,
    )
    .expect("recursive enum (list via record)");
}

#[test]
fn test_recursive_enum_direct_self_reference() {
    common::compile_and_run(
        r#"
package a

enum Tree =
    Leaf(Int32)
    Branch(Tree, Tree)

function main(): Unit =
    let l1: Tree = Tree.Leaf(1)
    let l2: Tree = Tree.Leaf(2)
    let b: Tree = Tree.Branch(l1, l2)
    ()
"#,
    )
    .expect("recursive enum (direct self-reference)");
}

// --- Enum match tests (Phase 3: pattern matching on enums) ---

#[test]
fn test_match_no_payload_variants() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function color_value(c: Color): Int32 =
    match c with
        case Color.Red => 1
        case Color.Green => 2
        case Color.Blue => 3

function main(): Unit =
    assert color_value(Color.Red) == 1
    assert color_value(Color.Green) == 2
    assert color_value(Color.Blue) == 3
"#,
    )
    .expect("match no-payload variants");
}

#[test]
fn test_match_payload_variable_binding() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point

function radius(s: Shape): Float64 =
    match s with
        case Shape.Circle(r) => r
        case Shape.Point => 0.0

function main(): Unit =
    assert radius(Shape.Circle(5.0)) == 5.0
    assert radius(Shape.Point) == 0.0
"#,
    )
    .expect("match payload variable binding");
}

#[test]
fn test_match_multi_payload_extraction() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Rectangle(Float64, Float64)
    Point

function area(s: Shape): Float64 =
    match s with
        case Shape.Circle(r) => r * r * 3.14
        case Shape.Rectangle(w, h) => w * h
        case Shape.Point => 0.0

function main(): Unit =
    assert area(Shape.Rectangle(10.0, 20.0)) == 200.0
    assert area(Shape.Point) == 0.0
"#,
    )
    .expect("match multi-payload extraction");
}

#[test]
fn test_match_mixed_payload_no_payload() {
    common::compile_and_run(
        r#"
package a

enum Message =
    Text(String)
    Number(Int32)
    Empty

function describe(m: Message): Int32 =
    match m with
        case Message.Text(s) => 1
        case Message.Number(n) => n
        case Message.Empty => 0

function main(): Unit =
    assert describe(Message.Text("hello")) == 1
    assert describe(Message.Number(42)) == 42
    assert describe(Message.Empty) == 0
"#,
    )
    .expect("match mixed payload/no-payload");
}

#[test]
fn test_match_wildcard_catchall() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function is_red(c: Color): Bool =
    match c with
        case Color.Red => true
        case _ => false

function main(): Unit =
    assert is_red(Color.Red) == true
    assert is_red(Color.Green) == false
    assert is_red(Color.Blue) == false
"#,
    )
    .expect("match wildcard catch-all");
}

#[test]
fn test_match_recursive_enum() {
    common::compile_and_run(
        r#"
package a

enum Tree =
    Leaf(Int32)
    Branch(Tree, Tree)

function sum(t: Tree): Int32 =
    match t with
        case Tree.Leaf(n) => n
        case Tree.Branch(l, r) => sum(l) + sum(r)

function main(): Unit =
    let t = Tree.Branch(Tree.Leaf(1), Tree.Branch(Tree.Leaf(2), Tree.Leaf(3)))
    assert sum(t) == 6
"#,
    )
    .expect("match recursive enum");
}

#[test]
fn test_match_construct_and_extract() {
    common::compile_and_run(
        r#"
package a

enum Wrapper =
    Value(Int32)

function unwrap(w: Wrapper): Int32 =
    match w with
        case Wrapper.Value(n) => n

function main(): Unit =
    let w = Wrapper.Value(42)
    assert unwrap(w) == 42
"#,
    )
    .expect("match construct and extract");
}

#[test]
fn test_error_non_exhaustive_enum_match() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function f(c: Color): Int32 =
    match c with
        case Color.Red => 1
        case Color.Green => 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("non-exhaustive") && e.contains("Blue")),
        "expected non-exhaustive error mentioning Blue, got: {:?}",
        errors
    );
}

#[test]
fn test_error_wrong_payload_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point

function f(s: Shape): Float64 =
    match s with
        case Shape.Circle(a, b) => a
        case Shape.Point => 0.0

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 1 payload")),
        "expected payload count error, got: {:?}",
        errors
    );
}

#[test]
fn test_match_enum_with_record_payload() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Shape =
    Circle(Float64)
    AtPoint(Point)

function get_x(s: Shape): Int32 =
    match s with
        case Shape.AtPoint(p) => p.x
        case Shape.Circle(r) => 0

function main(): Unit =
    let s = Shape.AtPoint(Point { x = 10; y = 20 })
    assert get_x(s) == 10
"#,
    )
    .expect("match enum with record payload");
}

#[test]
fn test_match_recursive_list() {
    common::compile_and_run(
        r#"
package a

record Cons =
    head: Int32
    tail: IntList

enum IntList =
    Nil
    Node(Cons)

function list_sum(l: IntList): Int32 =
    match l with
        case IntList.Nil => 0
        case IntList.Node(c) => c.head + list_sum(c.tail)

function main(): Unit =
    let l = IntList.Node(Cons { head = 1; tail = IntList.Node(Cons { head = 2; tail = IntList.Node(Cons { head = 3; tail = IntList.Nil }) }) })
    assert list_sum(l) == 6
"#,
    )
    .expect("match recursive list");
}

// --- Generic enum tests (Phase 4) ---

#[test]
fn test_generic_enum_construction_with_payload() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function main(): Unit =
    let x: Option<Int32> = Option.Some(42)
    ()
"#,
    )
    .expect("generic enum construction with payload");
}

#[test]
fn test_generic_enum_no_payload_inferred_from_annotation() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function main(): Unit =
    let x: Option<Int32> = Option.None
    ()
"#,
    )
    .expect("generic enum no-payload variant inferred from annotation");
}

#[test]
fn test_generic_enum_pattern_matching() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function unwrap_or(o: Option<Int32>, default: Int32): Int32 =
    match o with
        case Option.Some(v) => v
        case Option.None => default

function main(): Unit =
    let some = Option.Some(42)
    let none: Option<Int32> = Option.None
    assert unwrap_or(some, 0) == 42
    assert unwrap_or(none, 99) == 99
"#,
    )
    .expect("generic enum pattern matching");
}

#[test]
fn test_generic_enum_multi_type_params() {
    common::compile_and_run(
        r#"
package a

enum Result<T, E> =
    Ok(T)
    Error(E)

function get_value(r: Result<Int32, String>): Int32 =
    match r with
        case Result.Ok(v) => v
        case Result.Error(e) => -1

function main(): Unit =
    let ok: Result<Int32, String> = Result.Ok(42)
    let err: Result<Int32, String> = Result.Error("fail")
    assert get_value(ok) == 42
    assert get_value(err) == -1
"#,
    )
    .expect("generic enum multi type params");
}

#[test]
fn test_generic_enum_as_function_param() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function is_some(o: Option<Int32>): Bool =
    match o with
        case Option.Some(_) => true
        case Option.None => false

function main(): Unit =
    assert is_some(Option.Some(1)) == true
    let n: Option<Int32> = Option.None
    assert is_some(n) == false
"#,
    )
    .expect("generic enum as function param");
}

#[test]
fn test_generic_enum_with_different_instantiations() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function main(): Unit =
    let a: Option<Int32> = Option.Some(42)
    let b: Option<Bool> = Option.Some(true)
    let c: Option<String> = Option.Some("hello")
    ()
"#,
    )
    .expect("generic enum with different instantiations");
}

#[test]
fn test_generic_enum_exhaustiveness_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function f(o: Option<Int32>): Int32 =
    match o with
        case Option.Some(v) => v

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("non-exhaustive") && e.contains("None")),
        "expected non-exhaustive error mentioning None, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_enum_exhaustiveness_with_wildcard() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function f(o: Option<Int32>): Int32 =
    match o with
        case Option.Some(v) => v
        case _ => 0

function main(): Unit =
    assert f(Option.Some(10)) == 10
    let n: Option<Int32> = Option.None
    assert f(n) == 0
"#,
    )
    .expect("generic enum exhaustiveness with wildcard");
}

#[test]
fn test_generic_enum_with_record_payload() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Option<T> =
    Some(T)
    None

function get_x(o: Option<Point>): Int32 =
    match o with
        case Option.Some(p) => p.x
        case Option.None => 0

function main(): Unit =
    let p = Option.Some(Point { x = 10; y = 20 })
    assert get_x(p) == 10
    let n: Option<Point> = Option.None
    assert get_x(n) == 0
"#,
    )
    .expect("generic enum with record payload");
}

#[test]
fn test_generic_enum_type_annotation_in_expression() {
    common::compile_and_run(
        r#"
package a

enum Option<T> =
    Some(T)
    None

function make_none(): Option<Int32> =
    Option.None

function main(): Unit =
    let n = make_none()
    ()
"#,
    )
    .expect("generic enum type annotation in function return");
}

// --- Phase 5: Prelude Option/Result + Bare Variant Names ---

#[test]
fn test_prelude_option_qualified_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Option.Some(42)
    match x with
        case Option.Some(v) => assert v == 42
        case Option.None => assert false
"#,
    )
    .expect("prelude Option.Some qualified");
}

#[test]
fn test_prelude_option_qualified_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Option.None
    match x with
        case Option.Some(_) => assert false
        case Option.None => ()
"#,
    )
    .expect("prelude Option.None qualified");
}

#[test]
fn test_prelude_result_qualified() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let ok: Result<Int32, String> = Result.Ok(42)
    let err: Result<Int32, String> = Result.Error("fail")
    match ok with
        case Result.Ok(v) => assert v == 42
        case Result.Error(_) => assert false
    match err with
        case Result.Ok(_) => assert false
        case Result.Error(e) => assert e == "fail"
"#,
    )
    .expect("prelude Result qualified");
}

#[test]
fn test_bare_some_construction() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(42)
    match x with
        case Option.Some(v) => assert v == 42
        case Option.None => assert false
"#,
    )
    .expect("bare Some construction");
}

#[test]
fn test_bare_none_construction() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    match x with
        case Option.Some(_) => assert false
        case Option.None => ()
"#,
    )
    .expect("bare None construction");
}

#[test]
fn test_bare_ok_error_construction() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let ok: Result<Int32, String> = Ok(42)
    let err: Result<Int32, String> = Error("fail")
    match ok with
        case Result.Ok(v) => assert v == 42
        case Result.Error(_) => assert false
    match err with
        case Result.Ok(_) => assert false
        case Result.Error(e) => assert e == "fail"
"#,
    )
    .expect("bare Ok/Error construction");
}

#[test]
fn test_bare_variant_function_return() {
    common::compile_and_run(
        r#"
package a

function make_none(): Option<Int32> = None

function make_some(): Option<Int32> = Some(42)

function main(): Unit =
    let n = make_none()
    let s = make_some()
    match n with
        case Option.Some(_) => assert false
        case Option.None => ()
    match s with
        case Option.Some(v) => assert v == 42
        case Option.None => assert false
"#,
    )
    .expect("bare variant in function return");
}

#[test]
fn test_bare_variant_pattern_matching() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(42)
    match x with
        case Some(v) => assert v == 42
        case None => assert false
"#,
    )
    .expect("bare variant pattern matching");
}

#[test]
fn test_bare_result_pattern_matching() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let ok: Result<Int32, String> = Ok(10)
    let err: Result<Int32, String> = Error("bad")
    match ok with
        case Ok(v) => assert v == 10
        case Error(_) => assert false
    match err with
        case Ok(_) => assert false
        case Error(e) => assert e == "bad"
"#,
    )
    .expect("bare Result pattern matching");
}

#[test]
fn test_mixed_bare_and_qualified_patterns() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(5)
    match x with
        case Some(v) => assert v == 5
        case Option.None => assert false
"#,
    )
    .expect("mixed bare and qualified patterns");
}

#[test]
fn test_bare_none_inferred_from_param_type() {
    common::compile_and_run(
        r#"
package a

function check_none(o: Option<Int32>): Unit =
    match o with
        case Some(_) => assert false
        case None => ()

function main(): Unit = check_none(None)
"#,
    )
    .expect("bare None inferred from function parameter type");
}

#[test]
fn test_bare_some_inferred_from_param_type() {
    common::compile_and_run(
        r#"
package a

function check_some(o: Option<Int32>): Int32 =
    match o with
        case Some(v) => v
        case None => 0

function main(): Unit = assert check_some(Some(42)) == 42
"#,
    )
    .expect("bare Some inferred from function parameter type");
}

#[test]
fn test_bare_variant_in_if_else() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let b = true
    let x: Option<Int32> = if b then Some(1) else None
    match x with
        case Some(v) => assert v == 1
        case None => assert false
"#,
    )
    .expect("bare variant in if-else");
}

#[test]
fn test_bare_variant_in_match_body() {
    common::compile_and_run(
        r#"
package a

function to_option(b: Bool): Option<Int32> =
    match b with
        case true => Some(1)
        case false => None

function main(): Unit =
    match to_option(true) with
        case Some(v) => assert v == 1
        case None => assert false
    match to_option(false) with
        case Some(_) => assert false
        case None => ()
"#,
    )
    .expect("bare variant in match arm body");
}

#[test]
fn test_bare_error_inferred_from_result_return() {
    common::compile_and_run(
        r#"
package a

function fail(): Result<Int32, String> = Error("fail")

function main(): Unit =
    match fail() with
        case Ok(_) => assert false
        case Error(e) => assert e == "fail"
"#,
    )
    .expect("bare Error inferred from Result return type");
}

#[test]
fn test_bare_variant_unwrap_helper() {
    common::compile_and_run(
        r#"
package a

function unwrap(o: Option<Int32>): Int32 =
    match o with
        case Some(v) => v
        case None => panic("unwrap on None")

function main(): Unit = assert unwrap(Some(42)) == 42
"#,
    )
    .expect("bare variant unwrap helper");
}

#[test]
fn test_bare_some_always_means_prelude_option() {
    // Bare `Some` always resolves to prelude Option.Some, not a local enum
    let errors = common::compile_expecting_errors(
        r#"
package a

enum MyOption<T> =
    Some(T)
    Nothing

function main(): Unit =
    let x: MyOption<Int32> = Some(42)
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error (bare Some is Option.Some, not MyOption.Some), got: {:?}",
        errors
    );
}

#[test]
fn test_bare_variant_passed_to_generic_function() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let x = identity(Some(42))
    match x with
        case Some(v) => assert v == 42
        case None => assert false
"#,
    )
    .expect("bare variant passed to generic function");
}

// --- Phase 7: Record-style variant payloads ---

#[test]
fn test_enum_record_variant_declaration() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point { x: Int32, y: Int32 }

function main(): Unit = ()
"#,
    )
    .expect("enum with record-style variant declaration");
}

#[test]
fn test_enum_record_variant_construction() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point { x = 1; y = 2 }
    assert true
"#,
    )
    .expect("record-style variant construction");
}

#[test]
fn test_enum_record_variant_pattern_match() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let s = Shape.Point { x = 10; y = 20 }
    match s with
        case Shape.Circle(r) => assert false
        case Shape.Point { x = px; y = py } => assert px + py == 30
"#,
    )
    .expect("record-style variant pattern match");
}

#[test]
fn test_enum_record_variant_field_reordering() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point { y = 20; x = 10 }
    match p with
        case Shape.Point { x = px; y = py } => assert px == 10
    match p with
        case Shape.Point { x = px; y = py } => assert py == 20
"#,
    )
    .expect("record-style variant field reordering");
}

#[test]
fn test_enum_mixed_tuple_and_record_variants() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point { x: Float64, y: Float64 }
    Line(Float64, Float64, Float64, Float64)

function main(): Unit =
    let c = Shape.Circle(3.14)
    let p = Shape.Point { x = 1.0; y = 2.0 }
    let l = Shape.Line(0.0, 0.0, 1.0, 1.0)
    match c with
        case Shape.Circle(r) => assert r == 3.14
        case Shape.Point { x = _; y = _ } => assert false
        case Shape.Line(_, _, _, _) => assert false
"#,
    )
    .expect("mixed tuple and record variants");
}

#[test]
fn test_enum_record_variant_exhaustiveness() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let s = Shape.Point { x = 5; y = 5 }
    match s with
        case Shape.Circle(_) => assert false
        case Shape.Point { x = px; y = py } => assert px == py
"#,
    )
    .expect("exhaustiveness with record-style variant");
}

#[test]
fn test_enum_record_variant_with_bare_field_shorthand() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let s = Shape.Point { x = 42; y = 99 }
    match s with
        case Shape.Point { x; y } => assert x == 42
    match s with
        case Shape.Point { x; y } => assert y == 99
"#,
    )
    .expect("record-style variant with bare field shorthand in pattern");
}

#[test]
fn test_generic_enum_with_record_variant() {
    common::compile_and_run(
        r#"
package a

enum Wrapper<T> =
    Empty
    Value { data: T, label: Int32 }

function main(): Unit =
    let w: Wrapper<Int32> = Wrapper.Value { data = 42; label = 1 }
    match w with
        case Wrapper.Empty => assert false
        case Wrapper.Value { data = d; label = l } => assert d == 42
    match w with
        case Wrapper.Empty => assert false
        case Wrapper.Value { data = d; label = l } => assert l == 1
"#,
    )
    .expect("generic enum with record variant");
}

#[test]
fn test_enum_record_variant_literal_in_pattern() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let s = Shape.Point { x = 0; y = 42 }
    match s with
        case Shape.Point { x = 0; y = py } => assert py == 42
        case _ => assert false
"#,
    )
    .expect("literal in record-style variant pattern");
}

#[test]
fn test_enum_record_variant_tuple_syntax_error() {
    let result = common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point(1, 2)
    ()
"#,
    );
    assert!(
        result.is_err(),
        "tuple syntax on record variant should error"
    );
}

#[test]
fn test_enum_record_variant_on_tuple_variant_error() {
    let result = common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Float64)

function main(): Unit =
    let c = Shape.Circle { radius = 5.0 }
    ()
"#,
    );
    assert!(
        result.is_err(),
        "record syntax on tuple variant should error"
    );
}

#[test]
fn test_enum_record_variant_missing_field_error() {
    let result = common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point { x = 1 }
    ()
"#,
    );
    assert!(result.is_err(), "missing field should error");
}

#[test]
fn test_enum_record_variant_extra_field_error() {
    let result = common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point { x = 1; y = 2; z = 3 }
    ()
"#,
    );
    assert!(result.is_err(), "extra field should error");
}

#[test]
fn test_enum_record_variant_duplicate_field_error() {
    let result = common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let p = Shape.Point { x = 1; x = 2; y = 3 }
    ()
"#,
    );
    assert!(result.is_err(), "duplicate field should error");
}

#[test]
fn test_generic_enum_record_variant_inference() {
    common::compile_and_run(
        r#"
package a

enum Pair<T> =
    Pair { first: T, second: T }

function main(): Unit =
    let p: Pair<Int32> = Pair.Pair { first = 10; second = 20 }
    match p with
        case Pair.Pair { first = a; second = b } => assert a == 10
    match p with
        case Pair.Pair { first = a; second = b } => assert b == 20
"#,
    )
    .expect("generic enum with record variant inference");
}

// --- Negative tests: parenthesis validation ---

#[test]
fn test_error_no_payload_variant_construct_with_empty_parens() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c: Color = Color.Red()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("has no fields") || e.contains("without parentheses")),
        "expected error for empty parens on no-payload variant construction, got: {:?}",
        errors
    );
}

#[test]
fn test_error_no_payload_variant_match_with_empty_parens() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c = Color.Red
    match c with
        case Color.Red() => ()
        case Color.Green => ()
        case Color.Blue => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("has no fields") || e.contains("without parentheses")),
        "expected error for empty parens in no-payload variant pattern, got: {:?}",
        errors
    );
}

#[test]
fn test_error_no_args_pattern_on_tuple_variant() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Circle(Float64)
    Point

function main(): Unit =
    let s = Shape.Circle(3.14)
    match s with
        case Shape.Circle => ()
        case Shape.Point => ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 1 argument")),
        "expected error for no-args pattern on tuple variant, got: {:?}",
        errors
    );
}

#[test]
fn test_error_tuple_pattern_on_record_variant() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }

function main(): Unit =
    let s = Shape.Point { x = 1; y = 2 }
    match s with
        case Shape.Point(a, b) => ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("record-style pattern")),
        "expected error for tuple-style pattern on record variant, got: {:?}",
        errors
    );
}

#[test]
fn test_error_record_pattern_on_tuple_variant() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Shape =
    Circle(Float64)

function main(): Unit =
    let s = Shape.Circle(3.14)
    match s with
        case Shape.Circle { radius = r } => ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not have record-style payload")),
        "expected error for record-style pattern on tuple variant, got: {:?}",
        errors
    );
}

// --- Empty enum validation ---

#[test]
fn test_error_empty_enum() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Empty =

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expected variant name")),
        "expected error for empty enum, got: {:?}",
        errors
    );
}

// --- Guards with enum patterns ---

#[test]
fn test_match_enum_guard_tuple_variant() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Circle(Int32)
    Rectangle(Int32, Int32)

function main(): Unit =
    let s = Shape.Circle(5)
    let result = match s with
        case Shape.Circle(r) if r > 10 => 0
        case Shape.Circle(r) => r
        case Shape.Rectangle(w, h) => w * h
    assert result == 5
"#,
    )
    .expect("guard on tuple variant");
}

#[test]
fn test_match_enum_guard_no_payload_variant() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

function main(): Unit =
    let c = Color.Red
    let x = 42
    let result = match c with
        case Color.Red if x > 100 => 0
        case Color.Red => 1
        case Color.Green => 2
        case Color.Blue => 3
    assert result == 1
"#,
    )
    .expect("guard on no-payload variant");
}

#[test]
fn test_match_enum_guard_bare_variant() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let opt: Option<Int32> = Some(3)
    let result = match opt with
        case Some(x) if x > 5 => 0
        case Some(x) => x
        case None => -1
    assert result == 3
"#,
    )
    .expect("guard on bare variant");
}

#[test]
fn test_match_enum_guard_record_variant() {
    common::compile_and_run(
        r#"
package a

enum Shape =
    Point { x: Int32, y: Int32 }
    Circle(Int32)

function main(): Unit =
    let s = Shape.Point { x = 10; y = 20 }
    let result = match s with
        case Shape.Point { x = px; y = py } if px > 15 => 0
        case Shape.Point { x = px; y = py } => px + py
        case Shape.Circle(r) => r
    assert result == 30
"#,
    )
    .expect("guard on record variant");
}

// --- Nested patterns ---

#[test]
fn test_nested_enum_pattern_some_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let opt: Option<Option<Int32>> = Some(None)
    let result = match opt with
        case Some(Some(x)) => x
        case Some(None) => -1
        case None => -2
    assert result == -1
"#,
    )
    .expect("nested Some(None) pattern");
}

#[test]
fn test_nested_enum_pattern_some_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let opt: Option<Option<Int32>> = Some(Some(42))
    let result = match opt with
        case Some(Some(x)) => x
        case Some(None) => -1
        case None => -2
    assert result == 42
"#,
    )
    .expect("nested Some(Some(x)) pattern");
}

#[test]
fn test_nested_enum_pattern_none_outer() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let opt: Option<Option<Int32>> = None
    let result = match opt with
        case Some(Some(x)) => x
        case Some(None) => -1
        case None => -2
    assert result == -2
"#,
    )
    .expect("nested None outer pattern");
}

#[test]
fn test_nested_enum_pattern_custom_enum() {
    common::compile_and_run(
        r#"
package a

enum Tree =
    Leaf(Int32)
    Node(Tree, Tree)

function sum(t: Tree): Int32 =
    match t with
        case Tree.Leaf(v) => v
        case Tree.Node(Tree.Leaf(l), Tree.Leaf(r)) => l + r
        case Tree.Node(left, right) => sum(left) + sum(right)

function main(): Unit =
    let t = Tree.Node(Tree.Leaf(10), Tree.Leaf(20))
    assert sum(t) == 30
"#,
    )
    .expect("nested custom enum pattern");
}

#[test]
fn test_nested_enum_in_record_variant() {
    common::compile_and_run(
        r#"
package a

enum Wrapper =
    Box { value: Option<Int32> }
    Empty

function main(): Unit =
    let w = Wrapper.Box { value = Some(99) }
    let result = match w with
        case Wrapper.Box { value = Some(x) } => x
        case Wrapper.Box { value = None } => -1
        case Wrapper.Empty => -2
    assert result == 99
"#,
    )
    .expect("nested enum in record variant field");
}

#[test]
fn test_nested_record_in_enum_tuple_variant() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

enum Shape =
    Circle(Int32)
    At(Point)

function main(): Unit =
    let s = Shape.At(Point { x = 3; y = 4 })
    let result = match s with
        case Shape.At(Point { x = px; y = py }) => px + py
        case Shape.Circle(r) => r
    assert result == 7
"#,
    )
    .expect("record pattern nested in enum tuple variant");
}

#[test]
fn test_prelude_list_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs = List.Cons(1, List.Cons(2, List.Cons(3, List.Nil)))
    match xs with
        case List.Cons(h, _) => assert h == 1
        case List.Nil => panic "should not be nil"
"#,
    )
    .expect("prelude List basic construction and match");
}

#[test]
fn test_prelude_list_recursive_match() {
    common::compile_and_run(
        r#"
package a

function length(xs: List<Int32>): Int32 =
    match xs with
        case List.Nil => 0
        case List.Cons(_, tail) => 1 + length(tail)

function main(): Unit =
    let xs = List.Cons(10, List.Cons(20, List.Cons(30, List.Nil)))
    assert length(xs) == 3
    let empty: List<Int32> = List.Nil
    assert length(empty) == 0
"#,
    )
    .expect("prelude List recursive matching");
}

#[test]
fn test_prelude_list_nil_inference() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let xs: List<String> = List.Nil
    let ys = List.Cons("hello", List.Nil)
    match xs with
        case List.Nil => ()
        case List.Cons(_, _) => panic "should be nil"
    match ys with
        case List.Cons(s, _) => assert s == "hello"
        case List.Nil => panic "should not be nil"
"#,
    )
    .expect("prelude List Nil type inference");
}

// ── Nested-pattern exhaustiveness ─────────────────────────────────────

#[test]
fn nested_option_patterns_are_exhaustive() {
    common::compile_and_run(
        r#"
package a

function f(x: Option<Option<Int32>>): Int32 =
    match x with
        case Some(Some(v)) => v
        case Some(None) => -1
        case None => -2

function main(): Unit =
    assert f(Some(Some(5))) == 5
    assert f(None) == -2
"#,
    )
    .expect("nested option exhaustive");
}

#[test]
fn nested_option_missing_an_inner_variant_is_reported() {
    let err = common::compile_and_run(
        r#"
package a

function f(x: Option<Option<Int32>>): Int32 =
    match x with
        case Some(Some(v)) => v
        case None => -2

function main(): Unit = assert f(None) == -2
"#,
    )
    .expect_err("non-exhaustive");
    let text = format!("{err:?}");
    assert!(text.contains("non-exhaustive"), "{text}");
    assert!(text.contains("Some(None)"), "{text}");
}

/// A wildcard payload still covers its whole variant.
#[test]
fn wildcard_payload_covers_the_variant() {
    common::compile_and_run(
        r#"
package a

function f(x: Option<Option<Int32>>): Int32 =
    match x with
        case Some(_) => 1
        case None => 0

function main(): Unit = assert f(Some(None)) == 1
"#,
    )
    .expect("wildcard payload");
}

/// Three levels deep, the shape `standard-io` uses for fiber results.
#[test]
fn three_level_nesting_is_exhaustive() {
    common::compile_and_run(
        r#"
package a

enum Cause =
    Failed
    Panicked
    Interrupted

enum Outcome =
    Succeeded(Int32)
    Died(Cause)

function f(x: Option<Outcome>): Int32 =
    match x with
        case Some(Outcome.Succeeded(v)) => v
        case Some(Outcome.Died(Cause.Failed)) => -1
        case Some(Outcome.Died(Cause.Panicked)) => -2
        case Some(Outcome.Died(Cause.Interrupted)) => -3
        case None => -4

function main(): Unit =
    assert f(Some(Outcome.Succeeded(7))) == 7
    assert f(Some(Outcome.Died(Cause.Panicked))) == -2
    assert f(None) == -4
"#,
    )
    .expect("three level nesting");
}

#[test]
fn nested_enum_missing_a_deep_variant_is_reported() {
    let err = common::compile_and_run(
        r#"
package a

enum Cause =
    Failed
    Panicked
    Interrupted

enum Outcome =
    Succeeded(Int32)
    Died(Cause)

function f(x: Outcome): Int32 =
    match x with
        case Succeeded(v) => v
        case Died(Cause.Failed) => -1
        case Died(Cause.Panicked) => -2

function main(): Unit = assert f(Succeeded(1)) == 1
"#,
    )
    .expect_err("non-exhaustive");
    let text = format!("{err:?}");
    assert!(text.contains("non-exhaustive"), "{text}");
    assert!(text.contains("Interrupted"), "{text}");
}

/// An earlier argument can pin a type parameter that a later argument needs.
///
/// `Cons(T, List<T>)` fixes `T` from argument 1, so argument 2 can be inferred
/// at `List<Int32>`. Without that, a member which takes its type from context —
/// a generic module property like `List.empty` — failed to resolve here even
/// though the information was available.
#[test]
fn earlier_argument_supplies_a_later_argument_expected_type() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    // No annotation and no expected type: `T` comes from the first argument.
    assert List.Cons(1, List.empty).length == 1
    assert List.Cons(1, List.Cons(2, List.Cons(3, List.empty))).length == 3
"#,
    )
    .expect("later argument inferred from an earlier one");
}

/// The pre-existing spellings must keep working — the variant has its own
/// covariant fallback, and the literal has `List<Never>`.
#[test]
fn nil_and_literal_still_infer_without_context() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert List.Cons(1, List.Nil).length == 1
    assert List.Cons(1, []).length == 1
    assert Some(5).require == 5
"#,
    )
    .expect("variant and literal spellings unaffected");
}
