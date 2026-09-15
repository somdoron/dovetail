//! End-to-end tests for `[[project.macro]]` manifest entries.
//!
//! Constructs a real workspace on disk (Dovetail.toml + src + macro scripts),
//! loads it via `dovetail::manifest::load_manifest`, builds it via
//! `dovetail::build_workspace`, and runs the resulting WASM. Verifies that a
//! macro declared by one project is available to dependent projects.

mod common;

use std::fs;

use tempfile::tempdir;

fn run_workspace(workspace_root: &std::path::Path, run_project: &str) -> Result<(), String> {
    let workspace = dovetail::manifest::load_manifest(workspace_root)
        .map_err(|errs| format!("manifest load errors: {errs:?}"))?;

    let result = dovetail::build_workspace(
        &workspace,
        None,
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.span.file, d.message))
            .collect();
        return Err(format!("build_workspace failed:\n{}", errors.join("\n")));
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

const TAG_RHAI: &str = r#"
fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}
let tp_decl = "";
let tp_args = "";
if !input.type_params.is_empty() {
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
}
`implement ${tp_decl}Tag for ${input.name}${tp_args} =
    public function tag(self: ${input.name}${tp_args}): String = "${input.name}"
`
"#;

/// A library project declares a macro via `[[project.macro]]`; the
/// dependent app project uses it.
#[test]
fn test_manifest_macro_consumed_across_projects() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // mylib/src/lib.dove: defines the Tag trait.
    let lib_src = root.join("mylib").join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(
        lib_src.join("lib.dove"),
        r#"
package mylib

public trait Tag =
    function tag(self: Self): String
"#,
    )
    .unwrap();

    // mylib/macros/Tag.rhai: the derive script.
    let macros_dir = root.join("mylib").join("macros");
    fs::create_dir_all(&macros_dir).unwrap();
    fs::write(macros_dir.join("Tag.rhai"), TAG_RHAI).unwrap();

    // myapp/src/main.dove: uses @derive(mylib.Tag) on a record.
    let app_src = root.join("myapp").join("src");
    fs::create_dir_all(&app_src).unwrap();
    fs::write(
        app_src.join("main.dove"),
        r#"
package myapp

import mylib.Tag

@derive(Tag)
record Widget =
    n: Int32

function main(): Unit =
    let w = Widget { n = 1 }
    assert w.tag() == "Widget"
"#,
    )
    .unwrap();

    // Dovetail.toml: mylib declares the macro, myapp depends on mylib.
    fs::write(
        root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.0"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project.macro]]
name = "Tag"
package = "mylib"
kind = "derive"
trait = "mylib.Tag"
script = "macros/Tag.rhai"

[[project]]
name = "myapp"
root_package = "myapp"
depends = ["mylib"]
packages = ["."]
"#,
    )
    .unwrap();

    run_workspace(root, "myapp").expect("workspace runs to completion");
}

#[test]
fn test_manifest_macro_load_failure_surfaces_during_resolve() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let lib_src = root.join("mylib").join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(lib_src.join("lib.dove"), "package mylib\n").unwrap();

    // Note: macros/Tag.rhai is intentionally NOT created.
    fs::write(
        root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.0"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project.macro]]
name = "Tag"
package = "mylib"
kind = "derive"
script = "macros/Tag.rhai"
"#,
    )
    .unwrap();

    let result = dovetail::manifest::load_manifest(root);
    let errors = result.expect_err("expected resolve failure");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, dovetail::manifest::ManifestError::MacroScriptNotFound { .. })),
        "expected MacroScriptNotFound, got: {:?}",
        errors
    );
}

#[test]
fn test_manifest_macro_unsupported_kind_surfaces_during_resolve() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let lib_src = root.join("mylib").join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(lib_src.join("lib.dove"), "package mylib\n").unwrap();

    let macros_dir = root.join("mylib").join("macros");
    fs::create_dir_all(&macros_dir).unwrap();
    fs::write(macros_dir.join("Tag.rhai"), "\"\"").unwrap();

    fs::write(
        root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.0"
[[project]]
name = "mylib"
root_package = "mylib"
packages = ["."]

[[project.macro]]
name = "Tag"
package = "mylib"
kind = "function"
script = "macros/Tag.rhai"
"#,
    )
    .unwrap();

    let errors = dovetail::manifest::load_manifest(root).expect_err("expected unsupported-kind");
    assert!(
        errors
            .iter()
            .any(|e| matches!(e, dovetail::manifest::ManifestError::UnsupportedMacroKind { .. })),
        "expected UnsupportedMacroKind, got: {:?}",
        errors
    );
}
