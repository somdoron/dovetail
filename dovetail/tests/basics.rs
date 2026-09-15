mod common;

#[test]
fn test_minimal_program_runs() {
    common::compile_and_run(
        r#"
package a

function main(): Unit = ()
"#,
    )
    .expect("minimal program should run successfully");
}

#[test]
fn test_multi_line_body_runs() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    ()
"#,
    )
    .expect("multi-line body should run successfully");
}

#[test]
fn test_compile_error_unknown_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Foo = ()
"#,
    );
    assert!(!errors.is_empty());
    assert!(
        errors.iter().any(|e| e.contains("unknown type")),
        "expected 'unknown type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_compile_error_missing_package() {
    let errors = common::compile_expecting_errors(
        r#"
function main(): Unit = ()
"#,
    );
    assert!(!errors.is_empty());
}

#[test]
fn none_to_class_method_option_ref_type() {
    common::compile_and_run(
        r#"
package test

enum Color =
    Red
    Blue

class Foo(mutable dummy: Bool) =
    public function new(): Foo = Foo(false)

    public function process(self, opt: Option<Color>): Bool =
        match opt with
            case Some(_) => true
            case None => false

function main(): Unit =
    let foo = Foo.new()
    assert !foo.process(None)
    assert foo.process(Some(Color.Red))
"#,
    )
    .expect("None to class method with Option<RefType> should work");
}

#[test]
fn none_to_class_method_option_string() {
    common::compile_and_run(
        r#"
package test

class Processor(mutable dummy: Bool) =
    public function new(): Processor = Processor(false)

    public function process(self, opt: Option<String>): Bool =
        opt.isSome

function main(): Unit =
    let p = Processor.new()
    assert !p.process(None)
    assert p.process(Some("hello"))
"#,
    )
    .expect("None to class method with Option<String> should work");
}
