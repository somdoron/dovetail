mod common;

#[test]
fn test_string_literal_binding() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s: String = "hello"
    ()
"#,
    )
    .expect("string literal binding");
}

#[test]
fn test_string_global_variable() {
    common::compile_and_run(
        r#"
package a

let greeting: String = "hello"

function main(): Unit =
    let s: String = greeting
    ()
"#,
    )
    .expect("string global variable");
}

#[test]
fn test_string_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

function identity(s: String): String = s

function main(): Unit =
    let s: String = identity("hello")
    ()
"#,
    )
    .expect("string function param and return");
}

#[test]
fn test_string_in_record() {
    common::compile_and_run(
        r#"
package a

record Person =
    name: String
    age: Int32

function main(): Unit =
    let p = Person { name = "Alice"; age = 30 }
    ()
"#,
    )
    .expect("string in record");
}

#[test]
fn test_multiple_string_literals() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = "world"
    ()
"#,
    )
    .expect("multiple string literals");
}

#[test]
fn test_duplicate_string_literals() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = "hello"
    ()
"#,
    )
    .expect("duplicate string literals");
}

#[test]
fn test_empty_string_literal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s: String = ""
    ()
"#,
    )
    .expect("empty string literal");
}

#[test]
fn test_mutable_global_string() {
    common::compile_and_run(
        r#"
package a

let mutable greeting: String = "hello"

function main(): Unit =
    greeting = "world"
    ()
"#,
    )
    .expect("mutable global string");
}

#[test]
fn test_string_equality_same() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "hello" == "hello"
"#,
    )
    .expect("string equality same");
}

#[test]
fn test_string_inequality_different_content() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "a" != "b"
"#,
    )
    .expect("string inequality different content");
}

#[test]
fn test_string_equality_empty() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "" == ""
"#,
    )
    .expect("string equality empty");
}

#[test]
fn test_string_inequality_different_lengths() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "ab" != "abc"
"#,
    )
    .expect("string inequality different lengths");
}

#[test]
fn test_string_equality_with_variables() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = "hello"
    assert a == b
"#,
    )
    .expect("string equality with variables");
}

#[test]
fn test_string_inequality_with_variables() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = "world"
    assert a != b
"#,
    )
    .expect("string inequality with variables");
}

#[test]
fn test_string_equality_global() {
    common::compile_and_run(
        r#"
package a

let greeting: String = "hello"

function main(): Unit = assert greeting == "hello"
"#,
    )
    .expect("string equality global");
}

#[test]
fn test_string_equality_in_if() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s: String = "yes"
    let result = if s == "yes" then 1 else 0
    assert result == 1
"#,
    )
    .expect("string equality in if");
}

#[test]
fn test_string_equality_function_return() {
    common::compile_and_run(
        r#"
package a

function are_equal(a: String, b: String): Bool = a == b

function main(): Unit =
    assert are_equal("hello", "hello")
    assert !are_equal("hello", "world")
"#,
    )
    .expect("string equality function return");
}

#[test]
fn test_string_concat_basic() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "hello" ++ " world" == "hello world"
"#,
    )
    .expect("string concat basic");
}

#[test]
fn test_string_concat_empty_left() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "" ++ "a" == "a"
"#,
    )
    .expect("string concat empty left");
}

#[test]
fn test_string_concat_empty_right() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "a" ++ "" == "a"
"#,
    )
    .expect("string concat empty right");
}

#[test]
fn test_string_concat_both_empty() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "" ++ "" == ""
"#,
    )
    .expect("string concat both empty");
}

#[test]
fn test_string_concat_chained() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "a" ++ "b" ++ "c" == "abc"
"#,
    )
    .expect("string concat chained");
}

#[test]
fn test_string_concat_with_variables() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = " world"
    assert a ++ b == "hello world"
"#,
    )
    .expect("string concat with variables");
}

#[test]
fn test_string_concat_in_function() {
    common::compile_and_run(
        r#"
package a

function concat(a: String, b: String): String = a ++ b

function main(): Unit = assert concat("foo", "bar") == "foobar"
"#,
    )
    .expect("string concat in function");
}

#[test]
fn test_string_concat_with_equality() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let result: String = "a" ++ "b"
    assert result == "ab"
    assert result != "a"
    assert result != "abc"
"#,
    )
    .expect("string concat with equality");
}

#[test]
fn test_string_null_escape() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "\0" != ""
"#,
    )
    .expect("string null escape");
}

#[test]
fn test_string_dollar_escape() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "\$" == "\$"
"#,
    )
    .expect("string dollar escape");
}

#[test]
fn test_string_unicode_escape_ascii() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "\u{41}" == "A"
"#,
    )
    .expect("string unicode escape ascii");
}

#[test]
fn test_string_unicode_escape_emoji() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "\u{2764}" == "❤"
"#,
    )
    .expect("string unicode escape emoji");
}

#[test]
fn test_string_unicode_escape_in_concat() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "hello\u{21}" == "hello!"
"#,
    )
    .expect("string unicode escape in concat");
}

#[test]
fn test_multiline_string_basic() {
    common::compile_and_run(
        r####"
package a

function main(): Unit =
    let s: String = """
hello
world
"""
    assert s == "hello\nworld\n"
"####,
    )
    .expect("multiline string basic");
}

#[test]
fn test_multiline_string_single_line_content() {
    common::compile_and_run(
        r####"
package a

function main(): Unit = assert """hello""" == "hello"
"####,
    )
    .expect("multiline string single line content");
}

#[test]
fn test_multiline_string_embedded_quotes() {
    common::compile_and_run(
        r####"
package a

function main(): Unit =
    let s: String = """
say "hello"
"""
    assert s == "say \"hello\"\n"
"####,
    )
    .expect("multiline string embedded quotes");
}

#[test]
fn test_multiline_string_with_escapes() {
    common::compile_and_run(
        r####"
package a

function main(): Unit =
    let s: String = """
hello\tworld
"""
    assert s == "hello\tworld\n"
"####,
    )
    .expect("multiline string with escapes");
}

#[test]
fn test_multiline_string_concat() {
    common::compile_and_run(
        r####"
package a

function main(): Unit =
    let s: String = """hello""" ++ """ world"""
    assert s == "hello world"
"####,
    )
    .expect("multiline string concat");
}

#[test]
fn test_string_interpolation_simple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let name: String = "world"
    assert "hello $name" == "hello world"
"#,
    )
    .expect("string interpolation simple");
}

#[test]
fn test_string_interpolation_expr() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: String = "world"
    assert "hello ${x}" == "hello world"
"#,
    )
    .expect("string interpolation expr");
}

#[test]
fn test_string_interpolation_function_call() {
    common::compile_and_run(
        r#"
package a

function get_name(): String = "world"

function main(): Unit = assert "hello ${get_name()}" == "hello world"
"#,
    )
    .expect("string interpolation function call");
}

#[test]
fn test_string_interpolation_multiple() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let first: String = "hello"
    let second: String = "world"
    assert "$first $second" == "hello world"
"#,
    )
    .expect("string interpolation multiple");
}

#[test]
fn test_string_interpolation_adjacent() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a: String = "hello"
    let b: String = "world"
    assert "$a$b" == "helloworld"
"#,
    )
    .expect("string interpolation adjacent");
}

#[test]
fn test_string_interpolation_escaped_dollar() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "price: \$5" == "price: \$5"
"#,
    )
    .expect("string interpolation escaped dollar");
}

#[test]
fn test_string_interpolation_concat_with_plus() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let b: String = "hello"
    assert "a $b c" ++ " d" == "a hello c d"
"#,
    )
    .expect("string interpolation concat with plus");
}

#[test]
fn test_string_interpolation_multiline() {
    common::compile_and_run(
        r####"
package a

function main(): Unit =
    let name: String = "world"
    let s: String = """
hello $name
"""
    assert s == "hello world\n"
"####,
    )
    .expect("string interpolation multiline");
}

#[test]
fn test_string_interpolation_in_assert_message() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: String = "expected"
    assert true, "value was $x"
"#,
    )
    .expect("string interpolation in assert message");
}

#[test]
fn test_string_bytes_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "hello".bytes().length == 5
"#,
    )
    .expect("string bytes length");
}

#[test]
fn test_string_bytes_get() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "A".bytes().get(0) == 65u8
"#,
    )
    .expect("string bytes get");
}

#[test]
fn test_string_bytes_empty() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "".bytes().length == 0
"#,
    )
    .expect("string bytes empty");
}

#[test]
fn test_string_bytes_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = assert "\u{00E9}".bytes().length == 2
"#,
    )
    .expect("string bytes multibyte");
}

#[test]
fn test_string_bytes_returns_copy() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "hello"
    let b = s.bytes()
    b.set(0, 0u8)
    assert s.bytes().get(0) == 104u8
"#,
    )
    .expect("string bytes returns copy");
}

#[test]
fn test_string_unsafe_bytes_private() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit = "hello".unsafe_bytes()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error about unsafe_bytes being private, got no errors"
    );
}

// ---- String comparison (lexicographic) tests ----

#[test]
fn test_string_lt() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "abc" < "abd"
    assert "a" < "b"
    assert "apple" < "banana"
"#,
    )
    .expect("string less-than");
}

#[test]
fn test_string_gt() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "b" > "a"
    assert "abd" > "abc"
    assert "banana" > "apple"
"#,
    )
    .expect("string greater-than");
}

#[test]
fn test_string_le_ge() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "abc" <= "abd"
    assert "abc" <= "abc"
    assert "abd" >= "abc"
    assert "abc" >= "abc"
"#,
    )
    .expect("string le/ge");
}

#[test]
fn test_string_cmp_prefix() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "ab" < "abc"
    assert "abc" > "ab"
    assert "ab" <= "abc"
    assert "abc" >= "ab"
"#,
    )
    .expect("string prefix comparison");
}

#[test]
fn test_string_cmp_equal() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "abc" <= "abc"
    assert "abc" >= "abc"
    assert !("abc" < "abc")
    assert !("abc" > "abc")
"#,
    )
    .expect("string equal comparison");
}

#[test]
fn test_string_cmp_empty() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "" < "a"
    assert "" <= "a"
    assert "a" > ""
    assert "a" >= ""
    assert "" <= ""
    assert "" >= ""
    assert !("" < "")
    assert !("" > "")
"#,
    )
    .expect("string empty comparison");
}

// ---- String length/byteLength/getChar tests ----

#[test]
fn test_string_length_ascii() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "hello".length == 5
    assert "".length == 0
    assert "a".length == 1
"#,
    )
    .expect("string length ascii");
}

#[test]
fn test_string_length_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "\u{00E9}".length == 1
    assert "\u{2764}".length == 1
    assert "\u{1F600}".length == 1
    assert "h\u{00E9}llo".length == 5
"#,
    )
    .expect("string length multibyte");
}

#[test]
fn test_string_byte_length_ascii() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "hello".byteLength == 5
    assert "".byteLength == 0
    assert "a".byteLength == 1
"#,
    )
    .expect("string byteLength ascii");
}

#[test]
fn test_string_byte_length_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "\u{00E9}".byteLength == 2
    assert "\u{2764}".byteLength == 3
    assert "\u{1F600}".byteLength == 4
    assert "h\u{00E9}llo".byteLength == 6
"#,
    )
    .expect("string byteLength multibyte");
}

#[test]
fn test_string_length_vs_byte_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "hello"
    assert s.length == s.byteLength
    let s2 = "\u{00E9}"
    assert s2.length == 1
    assert s2.byteLength == 2
"#,
    )
    .expect("string length vs byteLength");
}

#[test]
fn test_string_get_char_ascii() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "hello"
    assert s.getChar(0) == 'h'
    assert s.getChar(1) == 'e'
    assert s.getChar(4) == 'o'
"#,
    )
    .expect("string getChar ascii");
}

#[test]
fn test_string_get_char_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "h\u{00E9}llo"
    assert s.getChar(0) == 'h'
    assert s.getChar(1) == '\u{00E9}'
    assert s.getChar(2) == 'l'
    assert s.getChar(3) == 'l'
    assert s.getChar(4) == 'o'
"#,
    )
    .expect("string getChar multibyte");
}

#[test]
fn test_string_get_char_out_of_bounds() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let s = "hello"
    s.getChar(5)
    ()
"#,
    );
}

#[test]
fn test_string_get_char_negative_index() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let s = "hello"
    s.getChar(-1)
    ()
"#,
    );
}

#[test]
fn test_string_concat_preserves_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = "hello"
    let b = " world"
    let c = a ++ b
    assert c.length == a.length + b.length
    assert c.byteLength == a.byteLength + b.byteLength
"#,
    )
    .expect("string concat preserves length");
}

#[test]
fn test_string_concat_multibyte_preserves_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = "h\u{00E9}llo"
    let b = " w\u{00F6}rld"
    let c = a ++ b
    assert c.length == a.length + b.length
"#,
    )
    .expect("string concat multibyte preserves length");
}

#[test]
fn test_string_from_char_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = String.fromChar('A')
    assert s.length == 1
    assert s.byteLength == 1
    let s2 = String.fromChar('\u{00E9}')
    assert s2.length == 1
    assert s2.byteLength == 2
"#,
    )
    .expect("string fromChar length");
}

#[test]
fn test_string_from_bytes_length() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let buf = "hello".bytes()
    let s = String.fromBytes(buf, 0, 5)
    assert s.length == 5
    assert s.byteLength == 5
    assert s == "hello"
"#,
    )
    .expect("string fromBytes length");
}

#[test]
fn test_string_unsafe_bytes_zero_copy() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "hello"
    let b = s.bytes()
    assert b.length == 5
    assert b.get(0) == 104u8
"#,
    )
    .expect("string unsafe_bytes zero copy");
}

#[test]
fn test_string_is_ascii_true() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "hello".isAscii
    assert "".isAscii
    assert "abc123!@#".isAscii
"#,
    )
    .expect("isAscii true for ASCII strings");
}

#[test]
fn test_string_is_ascii_false() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "héllo".isAscii == false
    assert "日本語".isAscii == false
"#,
    )
    .expect("isAscii false for non-ASCII strings");
}

#[test]
fn test_string_is_ascii_concat() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let ab = "hello" ++ "world"
    let r = ab.isAscii
    assert r
"#,
    )
    .expect("isAscii preserved through concat");
}

#[test]
fn test_string_is_ascii_concat_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = "hello"
    let c = "héllo"
    let ac = a ++ c
    assert ac.isAscii == false
"#,
    )
    .expect("isAscii false when concat includes non-ASCII");
}

#[test]
fn test_string_is_ascii_from_char() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert String.fromChar('a').isAscii
    assert String.fromChar('é').isAscii == false
"#,
    )
    .expect("isAscii from fromChar");
}

#[test]
fn test_string_index_access_ascii() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "hello"
    assert s[0] == 'h'
    assert s[1] == 'e'
    assert s[4] == 'o'
"#,
    )
    .expect("string index access ASCII");
}

#[test]
fn test_string_index_access_multibyte() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let s = "aé"
    assert s[0] == 'a'
    assert s[1] == 'é'
"#,
    )
    .expect("string index access multibyte");
}

#[test]
fn test_string_repeat() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "ab".repeat(3) == "ababab"
    assert "ab".repeat(1) == "ab"
    assert "ab".repeat(0) == ""
    assert "".repeat(5) == ""
"#,
    )
    .expect("string repeat");
}

/// `unitLength * n` is Int32 arithmetic: 4 * (2^30 + 1) wraps to 4, so the
/// unguarded version answered a 4 GiB request with a four-byte string and no
/// error at all. Trapping is the only honest answer — the allocation cannot be
/// served, and a silently short string is a wrong answer rather than a stopped
/// program.
#[test]
fn test_string_repeat_length_overflow_traps() {
    common::compile_and_expect_trap(
        r#"
package a

function main(): Unit =
    let s = "abcd".repeat(1073741825)
    assert s.length == 4
"#,
    );
}
