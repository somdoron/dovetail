mod common;

#[test]
fn debug_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = debug("hello")
"#,
    )
    .expect("debug string");
}

#[test]
fn debug_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = debug(42)
"#,
    )
    .expect("debug int32");
}

#[test]
fn debug_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = debug(true)
"#,
    )
    .expect("debug bool");
}

#[test]
fn debug_multiple_calls() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    debug("start")
    debug(123)
    debug(false)
    debug("end")
"#,
    )
    .expect("debug multiple calls");
}

#[test]
fn debug_in_block_with_assert() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 10
    debug(x)
    assert x == 10
"#,
    )
    .expect("debug in block with assert");
}
