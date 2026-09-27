//! End-to-end tests for prefixed string literals (`q"..."`).
//!
//! Constructs real workspaces on disk (Dovetail.toml + src), builds them, and
//! runs the resulting WASM. The compiler carries no knowledge of what any
//! prefix means, so these tests define their own toy builder and assert that
//! the lowering, the manifest registration, its inheritance, and the span
//! fidelity of bound failures all behave.

mod common;

use std::fs;

use tempfile::tempdir;

/// The toy builder every test lowers onto. Accumulates rendered text plus a
/// bracketed list of the values it was handed, so a test can assert on both
/// halves of what the literal produced.
const BUILDER_SRC: &str = r#"
package mylib

@stringLiteral
public type q = Builder

public record Frag =
    text: String
    values: String

public record Builder =
    text: String
    values: String

module Builder =
    public function empty(): Builder = Builder { text = ""; values = "" }

    public function literal(self, chunk: String): Builder =
        Builder { text = self.text ++ chunk; values = self.values }

    public function value<T>(self, item: T): Builder where T: Display =
        Builder { text = self.text ++ "?"; values = self.values ++ "[" ++ item.format() ++ "]" }

    public function spread<T>(self, items: Array<T>): Builder where T: Display =
        let mutable text = self.text
        let mutable values = self.values
        if items.length == 0 then text = text ++ "NULL"
        else
            let mutable i = 0
            while i < items.length do
                if i > 0 then text = text ++ ", " else ()
                text = text ++ "?"
                values = values ++ "[" ++ items.get(i).format() ++ "]"
                i = i + 1
        Builder { text = text; values = values }

    public function build(self): Frag = Frag { text = self.text; values = self.values }
"#;

fn build_workspace(workspace_root: &std::path::Path) -> dovetail::WorkspaceResult {
    let workspace =
        dovetail::manifest::load_manifest(workspace_root).expect("manifest loads without errors");
    dovetail::build_workspace(
        &workspace,
        None,
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    )
}

/// Build and run `run_project`, failing with the collected diagnostics.
fn run_workspace(workspace_root: &std::path::Path, run_project: &str) -> Result<(), String> {
    let result = build_workspace(workspace_root);

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| {
                format!(
                    "{}:{}:{}: {}",
                    d.span.file, d.span.line, d.span.column, d.message
                )
            })
            .collect();
        return Err(format!("build failed:\n{}", errors.join("\n")));
    }

    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(name, _)| name == run_project)
        .ok_or_else(|| format!("project '{run_project}' not in build results"))?;

    let wasm_bytes = project_result
        .wasm
        .as_ref()
        .ok_or_else(|| "missing WASM output".to_string())?;

    dovetail::runner::run_component(
        wasm_bytes,
        &dovetail::runner::FsPermissions::default(),
        &dovetail::runner::EnvPermissions::default(),
        &dovetail::runner::NetPermissions::default(),
    )
    .map_err(|e| format!("WASM execution error: {}", e.message))?;

    Ok(())
}

/// Every error diagnostic from a build, as `(file, line, column, message)`.
fn build_errors(workspace_root: &std::path::Path) -> Vec<(String, u32, u32, String)> {
    build_workspace(workspace_root)
        .diagnostics
        .iter()
        .filter(|d| d.severity == dovetail::common::diagnostics::Severity::Error)
        .map(|d| {
            (
                d.span.file.to_string(),
                d.span.line,
                d.span.column,
                d.message.clone(),
            )
        })
        .collect()
}

fn write(path: &std::path::Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// A workspace with `mylib` (the builder + a `q` registration) and `myapp`
/// (which declares no literal of its own).
fn lib_and_app(root: &std::path::Path, app_src: &str) {
    write(&root.join("mylib/src/lib.dove"), BUILDER_SRC);
    write(&root.join("myapp/src/main.dove"), app_src);
    write(
        &root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.1"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "myapp"
depends = ["mylib"]
packages = ["."]
"#,
    );
}

/// A prefix registered by a dependency is usable by a dependent that declares
/// nothing itself, and the lowering produces the expected text and values.
#[test]
fn literal_lowers_and_is_inherited_by_dependents() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    lib_and_app(
        root,
        r#"
package myapp

import mylib.Frag
import mylib.q

function main(): Unit =
    let name = "alice"
    let age = 30
    let f: Frag = q"WHERE name = $name AND age = ${age + 1}"
    assert f.text == "WHERE name = ? AND age = ?"
    assert f.values == "[alice][31]"
"#,
    );

    run_workspace(root, "myapp").expect("workspace runs to completion");
}

/// The headline property: a trait-bound failure on an interpolated value is
/// reported at the interpolation, not at the literal's start and not at a
/// synthetic span.
#[test]
fn bound_failure_is_reported_at_the_interpolation() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // `Opaque` implements no traits, so `value<T> where T: Display` cannot
    // accept it. In `    let f = q"value is $thing"` the literal starts at
    // column 13 and the `$` is at column 24.
    lib_and_app(
        root,
        r#"
package myapp

import mylib.q

record Opaque =
    n: Int32

function main(): Unit =
    let thing = Opaque { n = 1 }
    let f = q"value is $thing"
    ()
"#,
    );

    let errors = build_errors(root);
    let bound_error = errors
        .iter()
        .find(|(_, _, _, msg)| msg.contains("Display"))
        .unwrap_or_else(|| panic!("expected a Display bound failure, got: {errors:#?}"));

    let (_, line, column, _) = bound_error;
    assert_eq!(*line, 11, "bound failure should be on the literal's line");
    assert_eq!(
        *column, 24,
        "bound failure should point at the `$` of `$thing`, not at the literal's start (column 13)"
    );
}

/// `$..xs` lowers to `spread`, and an empty array takes the builder's empty
/// branch — the compiler has no opinion about either.
#[test]
fn spread_lowers_to_the_builders_spread_method() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    lib_and_app(
        root,
        r#"
package myapp

import mylib.Frag
import mylib.q

function main(): Unit =
    let ids: Array<Int32> = [|1, 2, 3|]
    let f: Frag = q"id IN ($..ids)"
    assert f.text == "id IN (?, ?, ?)"
    assert f.values == "[1][2][3]"

    let none: Array<Int32> = Array.empty()
    let g: Frag = q"id IN ($..none)"
    assert g.text == "id IN (NULL)"
    assert g.values == ""
"#,
    );

    run_workspace(root, "myapp").expect("workspace runs to completion");
}

/// A project may use the prefix it declares, in the very package that defines
/// the builder. Guards the collect-before-imports ordering.
#[test]
fn a_project_can_use_the_prefix_it_declares() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    write(
        &root.join("mylib/src/lib.dove"),
        &format!(
            "{BUILDER_SRC}\nfunction main(): Unit =\n    let f = q\"n = $one\"\n    assert f.text == \"n = ?\"\n    assert f.values == \"[1]\"\n\nlet one: Int32 = 1\n"
        ),
    );
    write(
        &root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.1"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]
"#,
    );

    run_workspace(root, "mylib").expect("workspace runs to completion");
}

/// A multi-line literal keeps its interior text and reports interpolations on
/// their real lines.
#[test]
fn multiline_literal_keeps_text_and_line_numbers() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    lib_and_app(
        root,
        "
package myapp

import mylib.Frag
import mylib.q

function main(): Unit =
    let a = 1
    let b = 2
    let f: Frag = q\"\"\"
SELECT $a
FROM t WHERE x = $b
\"\"\"
    assert f.text == \"SELECT ?\\nFROM t WHERE x = ?\\n\"
    assert f.values == \"[1][2]\"
",
    );

    run_workspace(root, "myapp").expect("workspace runs to completion");
}

/// A prefix that is not in scope points at the import that would fix it.
#[test]
fn a_prefix_that_is_not_imported_is_reported() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // `myapp` depends on mylib but never imports `q`.
    lib_and_app(
        root,
        r#"
package myapp

function main(): Unit =
    let f = q"hello"
    ()
"#,
    );

    let errors = build_errors(root);
    assert!(
        errors.iter().any(|(_, _, _, msg)| {
            msg.contains("unknown string-literal prefix `q`") && msg.contains("import")
        }),
        "expected an unknown-prefix error pointing at the import, got: {errors:#?}"
    );
}

/// A name that exists but was never marked `@stringLiteral` is not a prefix.
#[test]
fn an_unmarked_type_is_not_a_prefix() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    lib_and_app(
        root,
        r#"
package myapp

import mylib.Frag

function main(): Unit =
    let f = Frag"hello"
    ()
"#,
    );

    let errors = build_errors(root);
    assert!(
        errors.iter().any(
            |(_, _, _, msg)| msg.contains("is not a string-literal prefix")
                && msg.contains("@stringLiteral")
        ),
        "expected a not-a-prefix error naming the attribute, got: {errors:#?}"
    );
}

/// Two libraries may both export a prefix of the same name: the prefix is an
/// ordinary imported name, so a file simply imports the one it wants — and an
/// import alias renames the literal.
#[test]
fn same_named_prefixes_coexist_and_an_alias_renames_one() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    write(&root.join("mylib/src/lib.dove"), BUILDER_SRC);
    write(
        &root.join("otherlib/src/lib.dove"),
        &BUILDER_SRC.replace("package mylib", "package otherlib"),
    );
    // Both dependencies export `q`. Importing one under an alias makes the two
    // literals unambiguous at the use site — no manifest, no precedence rule.
    write(
        &root.join("myapp/src/main.dove"),
        r#"
package myapp

import mylib.Frag
import mylib.q
import otherlib.q as other

function main(): Unit =
    let a: Frag = q"first ${1}"
    let b = other"second ${2}"
    assert a.text == "first ?"
    assert b.text == "second ?"
    assert a.values == "[1]"
    assert b.values == "[2]"
"#,
    );
    write(
        &root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.1"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project]]
name = "otherlib"
root_package = "otherlib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "myapp"
depends = ["mylib", "otherlib"]
packages = ["."]
"#,
    );

    run_workspace(root, "myapp").expect("both prefixes coexist, the alias renames one");
}

/// A builder missing one of the five required methods says so, and says which.
#[test]
fn a_builder_missing_a_method_is_reported() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let without_spread = BUILDER_SRC
        .split("    public function spread<T>")
        .next()
        .unwrap()
        .to_string()
        + "    public function build(self): Frag = Frag { text = self.text; values = self.values }\n";

    write(&root.join("mylib/src/lib.dove"), &without_spread);
    write(
        &root.join("myapp/src/main.dove"),
        r#"
package myapp

import mylib.q

function main(): Unit =
    let f = q"n = 1"
    ()
"#,
    );
    write(
        &root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.1"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "myapp"
depends = ["mylib"]
packages = ["."]
"#,
    );

    let errors = build_errors(root);
    assert!(
        errors
            .iter()
            .any(|(_, _, _, msg)| msg.contains("has no method `spread`")),
        "expected a missing-method error naming `spread`, got: {errors:#?}"
    );
}
