//! Integration tests for user-defined derive macros written in Rhai.
//!
//! These exercise the full pipeline: a Rhai script registered as a
//! `DeriveExpander` runs during the macro phase, emits Dovetail source,
//! and the generated `implement` block is then type-checked and
//! codegen'd alongside the user's code.

mod common;

/// Compile a Dovetail source with the given Rhai-defined derive macros
/// registered, then run it as a WASI CLI component.
fn compile_and_run_with_derives(
    source: &str,
    rhai_derives: &[(&str, &str)],
) -> Result<(), String> {
    let result = dovetail::compile_for_test_with_derives(source, "test.dove", rhai_derives);

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(format!("compilation failed: {}", errors.join("; ")));
    }

    let wasm_bytes = result.wasm.ok_or("compilation produced no WASM output")?;

    // Straight to the runner: it builds the p3 engine and loads the component
    // itself, and reports a load failure with `{e:?}`, i.e. with the cause chain
    // where wasmtime puts the actionable part. Loading it here first bought a
    // second engine, a second parse of the module, and a strictly worse message.
    let run_result = dovetail::test_runner::run_tests(&wasm_bytes, &result.test_exports)
        .map_err(|e| format!("test runner error: {e}"))?;

    for tr in &run_result.results {
        if let dovetail::test_runner::TestStatus::Fail { message } = &tr.status {
            return Err(format!("test '{}' failed: {}", tr.name, message));
        }
    }

    Ok(())
}

const TAG_DERIVE_SCRIPT: &str = r#"
// Derive macro: implement a `Tag` trait that returns the type's name.
fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}
let bound_list = [];
let arg_list = [];
for t in input.type_params {
    bound_list.push(`${t}: Tag`);
    arg_list.push(t);
}
let bounds = if input.type_params.is_empty() { "" } else { "<" + join(bound_list, ", ") + "> " };
let type_args = if input.type_params.is_empty() { "" } else { "<" + join(arg_list, ", ") + ">" };
`implement ${bounds}Tag for ${input.name}${type_args} =
    public function tag(self: ${input.name}${type_args}): String = "${input.name}"
`
"#;

#[test]
fn test_rhai_derive_simple_record() {
    compile_and_run_with_derives(
        r#"
package a

public trait Tag =
    function tag(self: Self): String

@derive(Tag)
record Cat =
    age: Int32

test "tag returns name" =
    let c = Cat { age = 3 }
    assert c.tag() == "Cat"
"#,
        &[("a.Tag", TAG_DERIVE_SCRIPT)],
    )
    .expect("rhai-defined Tag derive runs on a record");
}

#[test]
fn test_rhai_derive_enum() {
    compile_and_run_with_derives(
        r#"
package a

public trait Tag =
    function tag(self: Self): String

@derive(Tag)
enum Light =
    Red
    Green
    Blue

test "enum tag returns enum name" =
    assert Light.Red.tag() == "Light"
    assert Light.Green.tag() == "Light"
"#,
        &[("a.Tag", TAG_DERIVE_SCRIPT)],
    )
    .expect("rhai-defined Tag derive runs on an enum");
}

/// Demonstrates that a non-trivial Rhai-defined derive (a custom Equatable-
/// shaped trait) works end-to-end. We register the script under a different
/// FQN (`a.MyEq`) to avoid colliding with the built-in `Equatable`.
#[test]
fn test_rhai_equatable_derive_on_record() {
    let script = r#"
fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}
let parts = [];
for f in input.fields { parts.push(`self.${f.name} == other.${f.name}`); }
let body = if parts.is_empty() { "true" } else { join(parts, " && ") };
`implement MyEq for ${input.name} =
    public function eq(self: ${input.name}, other: ${input.name}): Bool =
        ${body}
`
"#;
    compile_and_run_with_derives(
        r#"
package a

public trait MyEq =
    function eq(self: Self, other: Self): Bool

@derive(MyEq)
record Point =
    x: Int32
    y: Int32

test "rhai MyEq derives a working equals" =
    let p1 = Point { x = 1; y = 2 }
    let p2 = Point { x = 1; y = 2 }
    let p3 = Point { x = 1; y = 3 }
    assert p1.eq(p2)
    assert p1.eq(p3) == false
"#,
        &[("a.MyEq", script)],
    )
    .expect("rhai MyEq derives a working equals");
}

#[test]
fn test_rhai_script_failure_surfaces_as_error() {
    let result = dovetail::compile_for_test_with_derives(
        r#"
package a

public trait T = function t(self): Unit

@derive(T)
record Foo = x: Int32
"#,
        "test.dove",
        &[("a.T", "this_is_not_valid_rhai @@@")],
    );
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("rhai script error")),
        "expected rhai script error, got: {:?}",
        errors
    );
}

#[test]
fn test_rhai_returns_unparseable_source() {
    // Script returns "implement Foo for" — a valid prefix that drives the
    // parser into the implement-decl path and then fails for missing pieces.
    let result = dovetail::compile_for_test_with_derives(
        r#"
package a

public trait T = function t(self): Unit

@derive(T)
record Foo = x: Int32
"#,
        "test.dove",
        &[("a.T", "\"implement\"")],
    );
    let errors: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("failed to parse") || e.contains("failed to lex")),
        "expected re-parse failure, got: {:?}",
        errors
    );
}
