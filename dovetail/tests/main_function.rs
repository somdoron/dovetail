mod common;

#[test]
fn test_main_must_return_unit() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Bool = true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must return Unit")),
        "expected 'must return Unit' error, got: {:?}",
        errors
    );
}
