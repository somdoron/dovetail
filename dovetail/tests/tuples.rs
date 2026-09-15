mod common;

// --- Happy path tests ---

#[test]
fn test_tuple_construction_and_element_access() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, true)
    assert t._0 == 1
    assert t._1 == true
"#,
    )
    .expect("tuple construction and element access");
}

#[test]
fn test_triple_tuple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20, 30)
    assert t._0 == 10
    assert t._1 == 20
    assert t._2 == 30
"#,
    )
    .expect("triple tuple");
}

#[test]
fn test_tuple_as_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

function makePair(x: Int32, y: Bool): (Int32, Bool) = (x, y)

function getFirst(t: (Int32, Bool)): Int32 = t._0

function main(): Unit =
    let t = makePair(42, false)
    assert getFirst(t) == 42
    assert t._1 == false
"#,
    )
    .expect("tuple as function param and return");
}

#[test]
fn test_tuple_type_annotation() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t: (Int32, Bool) = (1, true)
    assert t._0 == 1
    assert t._1 == true
"#,
    )
    .expect("tuple type annotation");
}

#[test]
fn test_multiple_distinct_tuple_types() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = (1, 2)
    let b = (true, false, true)
    assert a._0 == 1
    assert a._1 == 2
    assert b._0 == true
    assert b._1 == false
    assert b._2 == true
"#,
    )
    .expect("multiple distinct tuple types");
}

#[test]
fn test_nested_tuples() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = ((1, 2), (3, 4))
    assert t._0._0 == 1
    assert t._0._1 == 2
    assert t._1._0 == 3
    assert t._1._1 == 4
"#,
    )
    .expect("nested tuples");
}

#[test]
fn test_tuple_with_string_elements() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = ("hello", 42)
    assert t._0 == "hello"
    assert t._1 == 42
"#,
    )
    .expect("tuple with string elements");
}

#[test]
fn test_tuple_in_let_and_reassign() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable t = (1, 2)
    assert t._0 == 1
    t = (3, 4)
    assert t._0 == 3
    assert t._1 == 4
"#,
    )
    .expect("tuple in let and reassign");
}

// --- Destructuring tests ---

#[test]
fn test_tuple_destructure_pair() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let (x, y) = (1, true)
    assert x == 1
    assert y == true
"#,
    )
    .expect("tuple destructure pair");
}

#[test]
fn test_tuple_destructure_wildcard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let (_, y) = t
    assert y == 20
"#,
    )
    .expect("tuple destructure wildcard");
}

#[test]
fn test_tuple_destructure_triple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let (a, _, c) = (10, 20, 30)
    assert a == 10
    assert c == 30
"#,
    )
    .expect("tuple destructure triple");
}

#[test]
fn test_tuple_destructure_with_annotation() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let (x, y): (Int32, Bool) = (42, false)
    assert x == 42
    assert y == false
"#,
    )
    .expect("tuple destructure with annotation");
}

#[test]
fn test_tuple_destructure_in_nested_scope() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = if true then (1, 2) else (3, 4)
    let (a, b) = result
    assert a == 1
    assert b == 2
"#,
    )
    .expect("tuple destructure in nested scope");
}

#[test]
fn test_nested_tuple_destructure() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = ((1, 2), (3, 4))
    let ((a, b), (c, d)) = t
    assert a == 1
    assert b == 2
    assert c == 3
    assert d == 4
"#,
    )
    .expect("nested tuple destructure");
}

#[test]
fn test_tuple_destructure_from_function() {
    common::compile_and_run(
        r#"
package a

function makePair(): (Int32, Bool) = (99, true)

function main(): Unit =
    let (x, y) = makePair()
    assert x == 99
    assert y == true
"#,
    )
    .expect("tuple destructure from function");
}

#[test]
fn test_tuple_destructure_arity_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let (a, b, c) = (10, 20)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("3 elements") && e.contains("2")),
        "expected arity mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_tuple_destructure_non_tuple() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let (a, b) = 42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-tuple")),
        "expected non-tuple error, got: {:?}",
        errors
    );
}

// --- Match pattern tests ---

#[test]
fn test_tuple_match_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (a, b) => a + b
    assert result == 30
"#,
    )
    .expect("tuple match basic");
}

#[test]
fn test_tuple_match_wildcard_element() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (_, b) => b
    assert result == 20
"#,
    )
    .expect("tuple match wildcard element");
}

#[test]
fn test_tuple_match_all_wildcards() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (_, _) => 42
    assert result == 42
"#,
    )
    .expect("tuple match all wildcards");
}

#[test]
fn test_tuple_match_triple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = match (1, 2, 3) with
        case (a, b, c) => a + b + c
    assert result == 6
"#,
    )
    .expect("tuple match triple");
}

#[test]
fn test_tuple_match_nested() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = match ((1, 2), (3, 4)) with
        case ((a, b), (c, d)) => a + b + c + d
    assert result == 10
"#,
    )
    .expect("tuple match nested");
}

#[test]
fn test_tuple_match_nested_wildcard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result = match ((1, 2), (3, 4)) with
        case ((_, b), (c, _)) => b + c
    assert result == 5
"#,
    )
    .expect("tuple match nested wildcard");
}

#[test]
fn test_tuple_match_with_guard() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (a, b) if a > 5 => a + b
        case _ => 0
    assert result == 30
"#,
    )
    .expect("tuple match with guard");
}

#[test]
fn test_tuple_match_guard_fallthrough() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, 20)
    let result = match t with
        case (a, b) if a > 5 => a + b
        case (a, b) => a * b
    assert result == 20
"#,
    )
    .expect("tuple match guard fallthrough");
}

#[test]
fn test_tuple_match_wildcard_catchall() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (a, b) if a > 100 => a + b
        case _ => 99
    assert result == 99
"#,
    )
    .expect("tuple match wildcard catchall");
}

#[test]
fn test_tuple_match_variable_catchall() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    let result = match t with
        case (a, b) if a > 100 => a + b
        case x => x._0 + x._1
    assert result == 30
"#,
    )
    .expect("tuple match variable catchall");
}

#[test]
fn test_tuple_match_in_function() {
    common::compile_and_run(
        r#"
package a

function swap(t: (Int32, Int32)): (Int32, Int32) =
    match t with
        case (a, b) => (b, a)

function main(): Unit =
    let result = swap((1, 2))
    assert result._0 == 2
    assert result._1 == 1
"#,
    )
    .expect("tuple match in function");
}

#[test]
fn test_tuple_match_mixed_types() {
    common::compile_and_run(
        r#"
package a

function check(t: (Int32, Bool, String)): Int32 =
    match t with
        case (n, b, s) => n

function main(): Unit =
    let t = (42, true, "hello")
    assert check(t) == 42
    assert t._1 == true
    assert t._2 == "hello"
"#,
    )
    .expect("tuple match mixed types");
}

#[test]
fn test_tuple_match_from_function_return() {
    common::compile_and_run(
        r#"
package a

function makePair(): (Int32, Int32) = (10, 20)

function main(): Unit =
    let result = match makePair() with
        case (a, b) => a + b
    assert result == 30
"#,
    )
    .expect("tuple match from function return");
}

#[test]
fn test_tuple_match_statement_context() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    match t with
        case (a, b) => assert a + b == 30
"#,
    )
    .expect("tuple match statement context");
}

#[test]
fn test_tuple_match_multiple_guarded_arms() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (3, 4)
    let result = match t with
        case (a, b) if a > 10 => 1
        case (a, b) if b > 10 => 2
        case (a, b) if a + b == 7 => 3
        case _ => 4
    assert result == 3
"#,
    )
    .expect("tuple match multiple guarded arms");
}

#[test]
fn test_tuple_match_arity_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    match t with
        case (a, b, c) => a + b + c
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("3 elements") && e.contains("2")),
        "expected arity mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_tuple_match_non_tuple_scrutinee() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    match 42 with
        case (a, b) => a + b
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-tuple")),
        "expected non-tuple error, got: {:?}",
        errors
    );
}

#[test]
fn test_tuple_match_non_exhaustive_guarded_only() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t = (10, 20)
    match t with
        case (a, b) if a > 5 => a + b
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "expected non-exhaustive error, got: {:?}",
        errors
    );
}

// --- Literal sub-pattern tests ---

#[test]
fn test_tuple_match_literal_element() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, 20)
    let result = match t with
        case (0, b) => b
        case (1, b) => b + 100
        case _ => 0
    assert result == 120
"#,
    )
    .expect("tuple match literal element");
}

#[test]
fn test_tuple_match_all_literals() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, 2)
    let result = match t with
        case (1, 2) => 42
        case _ => 0
    assert result == 42
"#,
    )
    .expect("tuple match all literals");
}

#[test]
fn test_tuple_match_literal_fallthrough() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (5, 10)
    let result = match t with
        case (1, 2) => 100
        case (5, 10) => 200
        case _ => 0
    assert result == 200
"#,
    )
    .expect("tuple match literal fallthrough");
}

#[test]
fn test_tuple_match_literal_to_catchall() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (99, 99)
    let result = match t with
        case (1, 2) => 100
        case (3, 4) => 200
        case _ => 300
    assert result == 300
"#,
    )
    .expect("tuple match literal to catchall");
}

#[test]
fn test_tuple_match_mixed_literal_and_variable() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, 42)
    let result = match t with
        case (0, _) => 0
        case (1, x) => x
        case _ => 99
    assert result == 42
"#,
    )
    .expect("tuple match mixed literal and variable");
}

#[test]
fn test_tuple_match_bool_literal_elements() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (true, false)
    let result = match t with
        case (true, true) => 1
        case (true, false) => 2
        case (false, true) => 3
        case (false, false) => 4
    assert result == 2
"#,
    )
    .expect("tuple match bool literal elements");
}

#[test]
fn test_tuple_match_literal_non_exhaustive() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t = (1, 2)
    match t with
        case (1, 2) => 42
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "expected non-exhaustive error for literal-only tuple match, got: {:?}",
        errors
    );
}

// --- Error tests ---

#[test]
fn test_tuple_out_of_range_accessor() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t = (1, true)
    let x = t._2
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no field '_2'")),
        "expected out-of-range error, got: {:?}",
        errors
    );
}

#[test]
fn test_tuple_invalid_field_name() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t = (1, true)
    let x = t.x
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no field 'x'")),
        "expected invalid field error, got: {:?}",
        errors
    );
}

#[test]
fn test_tuple_type_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let t: (Int32, Bool) = (1, 2)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

// --- Tuple equality tests ---

#[test]
fn test_tuple_eq_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1, 2) == (1, 2)
"#,
    )
    .expect("tuple eq basic");
}

#[test]
fn test_tuple_ne_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1, 2) != (1, 3)
"#,
    )
    .expect("tuple ne basic");
}

#[test]
fn test_tuple_eq_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert ((1, 2) == (3, 4)) == false
"#,
    )
    .expect("tuple eq false");
}

#[test]
fn test_tuple_eq_mixed_types() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert (1, true, "hi") == (1, true, "hi")
"#,
    )
    .expect("tuple eq mixed types");
}

#[test]
fn test_tuple_eq_nested() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert ((1, 2), (3, 4)) == ((1, 2), (3, 4))
"#,
    )
    .expect("tuple eq nested");
}

#[test]
fn test_tuple_ne_operator() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = (1, 2)
    let b = (1, 2)
    let c = (1, 3)
    assert (a != b) == false
    assert a != c
"#,
    )
    .expect("tuple ne operator");
}

#[test]
fn test_tuple_eq_with_equatable_record() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Equatable

record Point =
    x: Int32
    y: Int32

implement Equatable for Point =
    public function equals(self: Point, other: Point): Bool =
        if self.x == other.x then self.y == other.y else false

function main(): Unit =
    let t1 = (Point { x = 1; y = 2 }, 10)
    let t2 = (Point { x = 1; y = 2 }, 10)
    assert t1 == t2
"#,
    )
    .expect("tuple eq with equatable record");
}

#[test]
fn test_tuple_eq_non_equatable_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record NoEq =
    x: Int32

function main(): Unit =
    let t1 = (NoEq { x = 1 }, 2)
    let t2 = (NoEq { x = 1 }, 2)
    let _ = t1 == t2
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for non-equatable tuple element, got none"
    );
}

#[test]
fn test_tuple_satisfies_equatable_bound() {
    common::compile_and_run(
        r#"
package a

function eq<T>(a: T, b: T): Bool where T: Equatable = a == b

function main(): Unit =
    assert eq((1, 2), (1, 2))
    assert eq((1, 2), (1, 3)) == false
"#,
    )
    .expect("tuple satisfies equatable bound");
}

#[test]
fn test_tuple_inside_result_enum() {
    common::compile_and_run(
        r#"
package a

record Foo =
    x: Int32

function parse(s: String): (Option<Foo>, String) =
    if s.length >= 2 then
        (Some(Foo { x = 1 }), s)
    else (None, s)

function main(): Unit =
    let (optFoo, rest) = parse("hello")
    assert rest == "hello"
    match optFoo with
        case Some(f) => assert f.x == 1
        case None => panic "expected Some"
"#,
    )
    .expect("tuple with Option element in if-else branches");
}

#[test]
fn test_tuple_equatable_bound_with_non_equatable_element() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record NoEq =
    x: Int32

function eq<T>(a: T, b: T): Bool where T: Equatable = a == b

function main(): Unit =
    let _ = eq((NoEq { x = 1 }, 2), (NoEq { x = 1 }, 2))
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for non-equatable element in tuple bound, got none"
    );
}

#[test]
fn test_tuple_element_widened_to_any_keeps_consistent_wasm_type() {
    // Regression: a tuple literal whose element is itself a reference subtype
    // (here a nested tuple `(Any, Any)`) assigned into a context expecting
    // `Any` for that slot must materialize at the expected element type. Tuple
    // types are nominal in WASM codegen, so without widening element 0 to `Any`
    // the literal gets the struct type `((Any, Any), Int32)` while the return
    // context expects `(Any, Int32)` — two distinct WASM GC types that the
    // validator rejects. See infer_tuple_literal.
    common::compile_and_run(
        r#"
package a

function nest(flag: Bool): (Any, Int32) =
    if flag then
        let inner: (Any, Any) = (1, 2)
        (inner, 10)
    else
        let inner: (Any, Any) = (3, 4)
        (inner, 20)

function main(): Unit =
    let a = nest(true)
    let b = nest(false)
    assert a._1 == 10
    assert b._1 == 20
    let innerA = a._0 as (Any, Any)
    assert innerA._0 as Int32 == 1
"#,
    )
    .expect("tuple element widened to Any across branches");
}

// --- Phase 2: multi-value tuple returns ---

#[test]
fn test_tuple_return_nested_multivalue() {
    common::compile_and_run(
        r#"
package a

function nested(): ((Int32, Int32), Bool) = ((3, 4), true)

function main(): Unit =
    let r = nested()
    assert r._0._0 == 3
    assert r._0._1 == 4
    assert r._1 == true
"#,
    )
    .expect("nested tuple multi-value return");
}

#[test]
fn test_tuple_return_virtual_method() {
    common::compile_and_run(
        r#"
package a

class Animal(public legs: Int32) =
    public function describe(self: Animal): (Int32, Bool) = (self.legs, false)

class Dog(public breed: Int32) extends Animal(4) =
    public override function describe(self: Dog): (Int32, Bool) = (self.legs + 100, true)

function getDesc(a: Animal): (Int32, Bool) = a.describe()

function main(): Unit =
    let a = Animal(2)
    let d = Dog(42)
    let ra = getDesc(a)
    let rd = getDesc(d)
    assert ra._0 == 2
    assert ra._1 == false
    assert rd._0 == 104
    assert rd._1 == true
"#,
    )
    .expect("tuple return through virtual dispatch");
}

#[test]
fn test_tuple_return_trait_object() {
    common::compile_and_run(
        r#"
package a

interface Stats =
    function stats(self: Self): (Int32, Bool)

record Player =
    score: Int32

implement Stats for Player =
    function stats(self: Player): (Int32, Bool) = (self.score, true)

function getStats(s: Stats): (Int32, Bool) = s.stats()

function main(): Unit =
    let p = Player { score = 99 }
    let r = getStats(p)
    assert r._0 == 99
    assert r._1 == true
"#,
    )
    .expect("tuple return through trait object");
}

#[test]
fn test_tuple_return_function_value() {
    common::compile_and_run(
        r#"
package a

function pair(n: Int32): (Int32, Bool) = (n, true)

function applyPair(f: Int32 => (Int32, Bool)): (Int32, Bool) = f(7)

function main(): Unit =
    let r = applyPair(pair)
    assert r._0 == 7
    assert r._1 == true
"#,
    )
    .expect("tuple return through function value (ref trampoline)");
}

#[test]
fn test_tuple_return_into_array() {
    common::compile_and_run(
        r#"
package a

function pair(): (Int32, Bool) = (5, true)

function main(): Unit =
    let arr: Array<(Int32, Bool)> = [|pair(), pair()|]
    let e = arr.get(0)
    assert e._0 == 5
    assert e._1 == true
"#,
    )
    .expect("tuple return stored into array (boxed boundary)");
}

// --- Phase 3: flattened (unboxed) tuple representation ---

#[test]
fn test_flat_local_nested_and_fieldaccess() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let t = (1, (2, 3), 4)
    assert t._0 == 1
    assert t._1._0 == 2
    assert t._1._1 == 3
    assert t._2 == 4
    let inner = t._1
    assert inner._0 + inner._1 == 5
"#,
    )
    .expect("flattened nested tuple locals and field access");
}

#[test]
fn test_flat_tuple_param_direct_call() {
    common::compile_and_run(
        r#"
package a

function sum(p: (Int32, Int32), q: (Int32, Int32)): Int32 = p._0 + p._1 + q._0 + q._1

function main(): Unit =
    assert sum((1, 2), (3, 4)) == 10
"#,
    )
    .expect("tuple params to a direct call");
}

#[test]
fn test_flat_if_and_match_return_tuple() {
    common::compile_and_run(
        r#"
package a

function pick(b: Bool): (Int32, Int32) =
    if b then (1, 2) else (3, 4)

function classify(n: Int32): (Int32, Bool) =
    match n with
        case 0 => (0, true)
        case _ => (n, false)

function main(): Unit =
    let a = pick(true)
    assert a._0 == 1
    let b = pick(false)
    assert b._1 == 4
    let c = classify(0)
    assert c._1 == true
    let d = classify(7)
    assert d._0 == 7
    assert d._1 == false
"#,
    )
    .expect("if/match returning tuples");
}

#[test]
fn test_flat_record_with_tuple_field() {
    common::compile_and_run(
        r#"
package a

record Pair =
    label: Int32
    coords: (Int32, Int32)

function main(): Unit =
    let p = Pair { label = 9; coords = (4, 5) }
    assert p.label == 9
    assert p.coords._0 == 4
    assert p.coords._1 == 5
    let c = p.coords
    assert c._0 + c._1 == 9
"#,
    )
    .expect("record with a concrete tuple field");
}

#[test]
fn test_flat_array_of_mixed_width_tuple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr: Array<(Int32, Float64)> = [|(1, 2.0), (3, 4.0)|]
    let a = arr.get(0)
    assert a._0 == 1
    assert a._1 == 2.0
    let b = arr.get(1)
    assert b._0 == 3
    assert b._1 == 4.0
"#,
    )
    .expect("Array of mixed-width tuple (boxed element boundary)");
}

#[test]
fn test_flat_option_of_tuple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let o: Option<(Int32, Int32)> = Some((6, 7))
    match o with
        case Some(t) =>
            assert t._0 == 6
            assert t._1 == 7
        case None => panic "expected Some"
"#,
    )
    .expect("Option of tuple (erased/boxed boundary)");
}

#[test]
fn test_flat_closure_captures_and_returns_tuple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let base = (10, 20)
    let f: Int32 => (Int32, Int32) = (n) => (base._0 + n, base._1 + n)
    let r = f(5)
    assert r._0 == 15
    assert r._1 == 25
"#,
    )
    .expect("closure capturing and returning a tuple (boxed indirect boundary)");
}

#[test]
fn test_flat_record_tuple_field_pattern_match() {
    common::compile_and_run(
        r#"
package a

record Seg =
    id: Int32
    span: (Int32, Int32)
    tag: Bool

function main(): Unit =
    let s = Seg { id = 1; span = (4, 9); tag = true }
    match s with
        case Seg { id = i, span = sp, tag = t } =>
            assert i == 1
            assert sp._0 == 4
            assert sp._1 == 9
            assert t == true
"#,
    )
    .expect("record pattern with a spliced tuple field");
}

#[test]
fn test_flat_record_with_override_tuple_field() {
    common::compile_and_run(
        r#"
package a

record Box =
    a: Int32
    pair: (Int32, Int32)

function main(): Unit =
    let b = Box { a = 1; pair = (2, 3) }
    let c = b with pair = (8, 9)
    assert c.a == 1
    assert c.pair._0 == 8
    assert c.pair._1 == 9
"#,
    )
    .expect("record `with` overriding a spliced tuple field");
}

#[test]
fn test_flat_class_with_tuple_field() {
    common::compile_and_run(
        r#"
package a

class Region(public name: Int32, public bounds: (Int32, Int32)) =
    public function area(self: Region): Int32 = self.bounds._1 - self.bounds._0

function main(): Unit =
    let r = Region(7, (3, 10))
    assert r.name == 7
    assert r.bounds._0 == 3
    assert r.bounds._1 == 10
    assert r.area() == 7
    let b = r.bounds
    assert b._0 + b._1 == 13
"#,
    )
    .expect("class with a spliced immutable tuple field");
}

#[test]
fn test_flat_class_mutable_tuple_field() {
    common::compile_and_run(
        r#"
package a

class Counter(public mutable pos: (Int32, Int32)) =
    public function bump(self: Counter): Unit = self.pos = (self.pos._0 + 1, self.pos._1 + 2)

function main(): Unit =
    let c = Counter((0, 0))
    c.bump()
    c.bump()
    assert c.pos._0 == 2
    assert c.pos._1 == 4
"#,
    )
    .expect("class with a mutable (boxed) tuple field");
}

#[test]
fn test_flat_nested_tuple_boxed_in_array_and_option() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr: Array<(Int32, (Bool, Int32))> = [|(1, (true, 2)), (3, (false, 4))|]
    let a = arr.get(0)
    assert a._0 == 1
    assert a._1._0 == true
    assert a._1._1 == 2
    let b = arr.get(1)
    assert b._1._1 == 4
    let o: Option<(Int32, (Bool, Int32))> = Some((9, (true, 8)))
    match o with
        case Some((x, (flag, y))) =>
            assert x == 9
            assert flag == true
            assert y == 8
        case None => panic "expected Some"
"#,
    )
    .expect("nested tuple boxed in array + Option, fully flattened struct");
}

#[test]
fn test_flat_trait_method_tuple_param() {
    common::compile_and_run(
        r#"
package a

interface Adder =
    function addPair(self: Self, p: (Int32, Int32)): Int32

record Calc =
    base: Int32

implement Adder for Calc =
    function addPair(self: Calc, p: (Int32, Int32)): Int32 = self.base + p._0 + p._1

function run(a: Adder): Int32 = a.addPair((10, 20))

function main(): Unit =
    let c = Calc { base = 5 }
    assert run(c) == 35
"#,
    )
    .expect("trait method with a tuple param via trait object");
}

#[test]
fn test_flat_virtual_method_tuple_param() {
    common::compile_and_run(
        r#"
package a

class Base(public k: Int32) =
    public function combine(self: Base, p: (Int32, Int32)): Int32 = self.k + p._0 + p._1

class Sub(public extra: Int32) extends Base(100) =
    public override function combine(self: Sub, p: (Int32, Int32)): Int32 = self.k + p._0 * p._1

function dispatch(b: Base, p: (Int32, Int32)): Int32 = b.combine(p)

function main(): Unit =
    let base = Base(1)
    let sub = Sub(2)
    assert dispatch(base, (3, 4)) == 8
    assert dispatch(sub, (3, 4)) == 112
"#,
    )
    .expect("virtual method with a tuple param (boxed vtable slot)");
}

#[test]
fn test_flat_partially_erased_tuple_param_through_trait() {
    // The trait method's param `(Int32, C)` is *partially* erased: element 0 is concrete (i32),
    // element 1 is the trait's type parameter `C`. When `C` is itself a tuple `(Bool, Int32)`,
    // the concrete arg `(Int32, (Bool, Int32))` (a 3-leaf run) must be coerced to the slot's shape
    // (`$Tuple_2` with the sub-tuple boxed as one anyref) at the trait-object boundary — a
    // width-changing per-element coercion (`coerce_run`).
    common::compile_and_run(
        r#"
package a

interface Boxer<C> =
    function wrap(self: Self, p: (Int32, C)): C

record IntBoxer =
    tag: Int32

implement Boxer<(Bool, Int32)> for IntBoxer =
    function wrap(self: IntBoxer, p: (Int32, (Bool, Int32))): (Bool, Int32) = p._1

function useIt(b: Boxer<(Bool, Int32)>): (Bool, Int32) = b.wrap((1, (true, 2)))

function main(): Unit =
    let b = IntBoxer { tag = 9 }
    let r = useIt(b)
    assert r._0 == true
    assert r._1 == 2
"#,
    )
    .expect("partially-erased tuple param coerced through a trait object");
}

#[test]
fn test_flat_partially_erased_primitive_element_through_trait() {
    // `(Int32, T)` with `T = Int32`: a primitive concrete element next to a type-parameter element.
    common::compile_and_run(
        r#"
package a

interface Pairer<T> =
    function pair(self: Self, p: (Int32, T)): Int32

record Summer =
    base: Int32

implement Pairer<Int32> for Summer =
    function pair(self: Summer, p: (Int32, Int32)): Int32 = self.base + p._0 + p._1

function run(x: Pairer<Int32>): Int32 = x.pair((10, 20))

function main(): Unit =
    let s = Summer { base = 5 }
    assert run(s) == 35
"#,
    )
    .expect("partially-erased primitive tuple element via trait object");
}

#[test]
fn test_flat_mutable_tuple_captured_by_closure() {
    // A mutable tuple captured (and reassigned) by a closure is stored in a shared mutable box.
    // The mutation must be visible to the outer scope after the closure runs.
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable pair = (1, 2)
    let bump = () => pair = (pair._0 + 10, pair._1 + 20)
    bump()
    assert pair._0 == 11
    assert pair._1 == 22
    bump()
    assert pair._0 == 21
    assert pair._1 == 42
"#,
    )
    .expect("mutable tuple captured and reassigned by a closure");
}

#[test]
fn test_flat_mutable_tuple_class_field() {
    // A *mutable* tuple field splices into N mutable WASM sub-fields (like an immutable one) and is
    // reassigned leaf-by-leaf in place — no boxed `(ref $Tuple)` slot.
    common::compile_and_run(
        r#"
package a

class Box(public mutable pair: (Int32, Int32)) =
    public function bump(self: Box): Unit =
        self.pair = (self.pair._0 + 1, self.pair._1 + 10)

function main(): Unit =
    let b = Box((1, 2))
    assert b.pair._0 == 1
    assert b.pair._1 == 2
    b.bump()
    assert b.pair._0 == 2
    assert b.pair._1 == 12
    b.pair = (100, 200)
    assert b.pair._0 == 100
    assert b.pair._1 == 200
"#,
    )
    .expect("mutable tuple class field splices into mutable sub-fields");
}

#[test]
fn test_flat_mutable_tuple_local() {
    // A mutable tuple local is a run of mutable locals, reassigned in place.
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable t = (1, 2)
    assert t._0 == 1
    assert t._1 == 2
    t = (3, 4)
    assert t._0 == 3
    assert t._1 == 4
"#,
    )
    .expect("mutable tuple local reassigned in place");
}

#[test]
fn test_flat_let_bound_tuple_from_result_require() {
    common::compile_and_run(
        r#"
package a

function makePair(): Result<(Array<Int32>, Bool), Int32> = Ok((Array<Int32>.empty(), true))

function main(): Unit =
    let result = makePair().require
    assert result._0.length == 0
"#,
    )
    .expect("require tuple repro");
}

#[test]
fn test_flat_closure_env_tuple_capture_field_offsets() {
    // A tuple capture sits *before* other captures, so its spliced fields must shift the
    // subsequent captures' field indices in the env struct.
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let pair = (10i32, true)
    let n = 7i32
    let m = 100i32
    let f = () =>
        let base = if pair._1 then pair._0 else 0i32
        base + n + m
    assert f() == 117i32
"#,
    )
    .expect("closure env tuple capture field offsets");
}
