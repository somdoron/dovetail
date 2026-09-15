mod common;

// ── Option.or ───────────────────────────────────────────────────────

#[test]
fn option_or_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(5)
    assert x.or(10) == 5
"#,
    )
    .expect("Option.or on Some");
}

#[test]
fn option_or_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    assert x.or(10) == 10
"#,
    )
    .expect("Option.or on None");
}

// ── Option.expect ───────────────────────────────────────────────────

#[test]
fn option_expect_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(42)
    assert x.expect("should exist") == 42
"#,
    )
    .expect("Option.expect on Some");
}

#[test]
fn option_expect_none_panics() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let _ = x.expect("should panic")
"#,
    );
}

// ── Option.require ──────────────────────────────────────────────────

#[test]
fn option_require_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(7)
    assert x.require == 7
"#,
    )
    .expect("Option.require on Some");
}

#[test]
fn option_require_none_panics() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let _ = x.require
"#,
    );
}

// ── Option.map ──────────────────────────────────────────────────────

#[test]
fn option_map_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(3)
    let y = x.map(v => v * 2)
    assert y.or(0) == 6
"#,
    )
    .expect("Option.map on Some");
}

#[test]
fn option_map_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let y = x.map(v => v * 2)
    assert y.isNone
"#,
    )
    .expect("Option.map on None");
}

// ── Option.andThen ──────────────────────────────────────────────────

#[test]
fn option_and_then_some_to_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(10)
    let y = x.andThen(v => if v % 2 == 0 then Some(v / 2) else None)
    assert y.or(0) == 5
"#,
    )
    .expect("Option.andThen Some to Some");
}

#[test]
fn option_and_then_some_to_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(3)
    let y = x.andThen(v => if v % 2 == 0 then Some(v / 2) else None)
    assert y.isNone
"#,
    )
    .expect("Option.andThen Some to None");
}

#[test]
fn option_and_then_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let y = x.andThen(v => if v % 2 == 0 then Some(v / 2) else None)
    assert y.isNone
"#,
    )
    .expect("Option.andThen on None");
}

// ── Option.filter ───────────────────────────────────────────────────

#[test]
fn option_filter_keeps() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(10)
    let y = x.filter(v => v > 5)
    assert y.or(0) == 10
"#,
    )
    .expect("Option.filter keeps");
}

#[test]
fn option_filter_removes() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(3)
    let y = x.filter(v => v > 5)
    assert y.isNone
"#,
    )
    .expect("Option.filter removes");
}

#[test]
fn option_filter_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let y = x.filter(v => v > 5)
    assert y.isNone
"#,
    )
    .expect("Option.filter on None");
}

// ── Option.isSome / isNone ──────────────────────────────────────────

#[test]
fn option_is_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(1)
    assert x.isSome
    assert x.isNone == false
"#,
    )
    .expect("Option.isSome");
}

#[test]
fn option_is_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    assert x.isNone
    assert x.isSome == false
"#,
    )
    .expect("Option.isNone");
}

// ── Option.toResult ─────────────────────────────────────────────────

#[test]
fn option_to_result_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(42)
    let r = x.toResult("missing")
    assert r.isOk
    assert r.or(0) == 42
"#,
    )
    .expect("Option.toResult on Some");
}

#[test]
fn option_to_result_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = None
    let r = x.toResult("missing")
    assert r.isError
"#,
    )
    .expect("Option.toResult on None");
}

// ── Result.or ───────────────────────────────────────────────────────

#[test]
fn result_or_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(5)
    assert r.or(10) == 5
"#,
    )
    .expect("Result.or on Ok");
}

#[test]
fn result_or_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    assert r.or(10) == 10
"#,
    )
    .expect("Result.or on Error");
}

// ── Result.expect ───────────────────────────────────────────────────

#[test]
fn result_expect_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(42)
    assert r.expect("should be ok") == 42
"#,
    )
    .expect("Result.expect on Ok");
}

#[test]
fn result_expect_error_panics() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let _ = r.expect("should panic")
"#,
    );
}

// ── Result.require ──────────────────────────────────────────────────

#[test]
fn result_require_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(7)
    assert r.require == 7
"#,
    )
    .expect("Result.require on Ok");
}

#[test]
fn result_require_error_panics() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let _ = r.require
"#,
    );
}

// ── Result.map ──────────────────────────────────────────────────────

#[test]
fn result_map_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(3)
    let s = r.map(v => v * 2)
    assert s.or(0) == 6
"#,
    )
    .expect("Result.map on Ok");
}

#[test]
fn result_map_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let s = r.map(v => v * 2)
    assert s.isError
"#,
    )
    .expect("Result.map on Error");
}

// ── Result.mapError ─────────────────────────────────────────────────

#[test]
fn result_map_error_on_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let s = r.mapError(e => "wrapped")
    assert s.isError
"#,
    )
    .expect("Result.mapError on Error");
}

#[test]
fn result_map_error_on_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(42)
    let s = r.mapError(e => "wrapped")
    assert s.or(0) == 42
"#,
    )
    .expect("Result.mapError on Ok");
}

// ── Result.andThen ──────────────────────────────────────────────────

#[test]
fn result_and_then_ok_to_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(5)
    let s = r.andThen(x => Ok(x * 2))
    assert s.or(0) == 10
"#,
    )
    .expect("Result.andThen Ok to Ok");
}

#[test]
fn result_and_then_ok_to_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(3)
    let s = r.andThen(x => if x % 2 == 0 then Ok(x) else Result.error("odd"))
    assert s.isError
"#,
    )
    .expect("Result.andThen Ok to Error");
}

#[test]
fn result_and_then_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let s = r.andThen(x => Ok(x * 2))
    assert s.isError
"#,
    )
    .expect("Result.andThen on Error");
}

// ── Result.isOk / isError ───────────────────────────────────────────

#[test]
fn result_is_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(1)
    assert r.isOk
    assert r.isError == false
"#,
    )
    .expect("Result.isOk");
}

#[test]
fn result_is_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    assert r.isError
    assert r.isOk == false
"#,
    )
    .expect("Result.isError");
}

// ── Result.toOption ─────────────────────────────────────────────────

#[test]
fn result_to_option_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(42)
    let o = r.toOption()
    assert o.isSome
    assert o.or(0) == 42
"#,
    )
    .expect("Result.toOption on Ok");
}

#[test]
fn result_to_option_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Result.error("fail")
    let o = r.toOption()
    assert o.isNone
"#,
    )
    .expect("Result.toOption on Error");
}

// ── Chaining ────────────────────────────────────────────────────────

#[test]
fn option_method_chaining() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Option<Int32> = Some(10)
    let result = x.map(v => v + 5).filter(v => v > 10).or(0)
    assert result == 15
"#,
    )
    .expect("Option method chaining");
}

#[test]
fn result_method_chaining() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(5)
    let result = r.map(v => v * 3).or(0)
    assert result == 15
"#,
    )
    .expect("Result method chaining");
}

// ── orReturn chaining ───────────────────────────────────────────────

#[test]
fn or_return_chain_tuple_field() {
    common::compile_and_run(
        r#"
package a

function getPair(): Result<(Int32, String), String> = Ok((42, "hello"))

function process(): Result<Int32, String> =
    let x = getPair().orReturn._0
    Ok(x)

function main(): Unit =
    assert process().or(0) == 42
"#,
    )
    .expect("orReturn chaining with tuple field");
}

#[test]
fn or_return_chain_method() {
    common::compile_and_run(
        r#"
package a

function getResult(): Result<Option<Int32>, String> = Ok(Some(99))

function process(): Result<Bool, String> =
    let x = getResult().orReturn.isSome
    Ok(x)

function main(): Unit =
    assert process().or(false)
"#,
    )
    .expect("orReturn chaining with method");
}

#[test]
fn or_return_chain_tuple_field_second() {
    common::compile_and_run(
        r#"
package a

function getPair(): Result<(Int32, String), String> = Ok((42, "hello"))

function process(): Result<String, String> =
    let x = getPair().orReturn._1
    Ok(x)

function main(): Unit =
    assert process().or("") == "hello"
"#,
    )
    .expect("orReturn chaining with second tuple field");
}
