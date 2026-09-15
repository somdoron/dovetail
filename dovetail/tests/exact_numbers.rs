//! Compiler diagnostics and cross-package behavior for exact numeric literals.
mod common;

fn errors(source: &str) -> String {
    let result = dovetail::compile(source, "exact.dove");
    assert!(result.diagnostics.has_errors(), "expected a diagnostic");
    format!("{:?}", result.diagnostics)
}

#[test]
fn malformed_literals_report_source_errors() {
    for (literal, message) in [
        ("1.2big", "invalid bigint digits"),
        ("1e3big", "invalid bigint digits"),
        ("0b2big", "invalid bigint digits"),
        ("0xbig", "invalid bigint digits"),
        ("1e+dec", "invalid decimal exponent"),
        ("1.2.3dec", "invalid decimal literal"),
        ("0b1dec", "invalid decimal"),
        ("1bigfoo", "invalid exact numeric suffix"),
        ("1decf32", "invalid exact numeric suffix"),
        ("1e-2147483648dec", "decimal scale out of range"),
        ("1e2147483647dec", "compiler resource limit"),
    ] {
        let source = format!("package a\nfunction main(): Unit =\n    let value = {literal}\n");
        let diagnostic = errors(&source);
        assert!(diagnostic.contains(message), "{literal}: {diagnostic}");
        assert!(diagnostic.contains("exact.dove"), "{diagnostic}");
    }
}

#[test]
fn exact_types_do_not_implicitly_coerce() {
    for expression in ["1big + 1", "1dec + 1", "1dec + 1big"] {
        let source = format!("package a\nfunction main(): Unit =\n    let value = {expression}\n");
        errors(&source);
    }
    errors("package a\nfunction main(): Unit =\n    let value: Decimal = 1.2\n");
    errors("package a\nfunction main(): Unit =\n    let value: BigInt = 1\n");
}

#[test]
fn literal_patterns_have_an_explicit_diagnostic() {
    for literal in ["1big", "-1big", "1dec", "-1dec"] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let value = match {literal} with\n        case {literal} => 1\n        case _ => 0\n"
        );
        assert!(errors(&source).contains("literal patterns are not supported"));
    }
}

#[test]
fn prelude_identity_is_independent_of_local_type_names() {
    common::compile_and_run(
        r#"
package a
record BigInt = value: Int32
record Decimal = value: Int32
let global = 12345678901234567890big
function main(): Unit =
    assert global.format() == "12345678901234567890"
    assert 1.20dec.format() == "1.2"
    assert 0xdec == 3564
    assert 1i64 + 2i64 == 3i64
    assert 1.25f32 == 1.25f32
"#,
    )
    .expect("canonical prelude literal types");
}

#[test]
fn invalid_runtime_scales_panic() {
    common::compile_and_expect_trap(
        r#"
package a
function main(): Unit =
    let value = Decimal.of(1big, -1)
"#,
    );
    common::compile_and_expect_trap(
        r#"
package a
function main(): Unit =
    let value = Decimal.of(1big, 2147483647) * 0.1dec
"#,
    );
}
