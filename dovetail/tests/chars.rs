mod common;

#[test]
fn test_char_literal_binding() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let c: Char = 'A'
    ()
"#,
    )
    .expect("char literal binding");
}

#[test]
fn test_char_equality() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 'A' == 'A'
"#,
    )
    .expect("char equality");
}

#[test]
fn test_char_inequality() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert 'A' != 'B'
"#,
    )
    .expect("char inequality");
}

#[test]
fn test_char_escape_newline() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert '\n' == '\n'
"#,
    )
    .expect("char escape newline");
}

#[test]
fn test_char_unicode_escape() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert '\u{41}' == 'A'
"#,
    )
    .expect("char unicode escape");
}

#[test]
fn test_char_comparison() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 'A' < 'B'
    assert 'B' > 'A'
    assert 'A' <= 'A'
    assert 'A' <= 'B'
    assert 'B' >= 'B'
    assert 'B' >= 'A'
"#,
    )
    .expect("char comparison");
}

#[test]
fn test_char_in_function() {
    common::compile_and_run(
        r#"
package a

function id(c: Char): Char = c

function main(): Unit = assert id('X') == 'X'
"#,
    )
    .expect("char in function");
}

#[test]
fn test_char_in_record() {
    common::compile_and_run(
        r#"
package a

record CharBox =
    value: Char

function main(): Unit =
    let box = CharBox { value = 'Z' }
    assert box.value == 'Z'
"#,
    )
    .expect("char in record");
}

#[test]
fn test_char_match() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let c: Char = 'A'
    let result = match c with
        case 'A' => 1
        case 'B' => 2
        case _ => 0
    assert result == 1
"#,
    )
    .expect("char match");
}
