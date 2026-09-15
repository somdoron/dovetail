mod common;

#[test]
fn symbolic_extension_widens_its_right_element() {
    common::compile_and_run(
        r#"
package a
function widen<T>(left: T): T ~ Any = left ~ 7
function widenFunction<T>(left: T): () => (T ~ Any) = () => left ~ 9
function widenTuple<T>(left: T): T ~ Any = left ~ (7, true)
function widenValue<T, U>(value: T ~ U, left: T, right: U): T ~ Any = value
function main(): Unit =
    let pair = widen(true)
    assert pair._0
    assert (pair._1 as Int32) == 7
    let triple = widen((1, true))
    assert triple._0 == 1 && triple._1
    assert (triple._2 as Int32) == 7
    let makePair = widenFunction(true)
    assert (makePair()._1 as Int32) == 9
    let makeTriple = widenFunction((1, true))
    assert (makeTriple()._2 as Int32) == 9
    let nested = widenTuple((1, true))
    assert (nested._2 as (Int32, Bool))._1
    let forwarded = widenValue((1, true, 11), (1, true), 11)
    assert (forwarded._2 as Int32) == 11
"#,
    )
    .expect("symbolic right covariance and concrete tuple boxing");
}

#[test]
fn symbolic_extension_cannot_widen_its_left_operand() {
    let result = dovetail::check(
        r#"
package a
function invalid<T>(value: T ~ Int32): Any ~ Int32 = value
"#,
        "test.dove",
    );
    assert!(result.diagnostics.has_errors());
}
