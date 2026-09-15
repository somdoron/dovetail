mod common;

use std::fs;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

/// Helper: build a project from temp dir contents, run WASM via WASI component model.
fn build_and_run(project: &ResolvedProject, workspace_root: &std::path::Path) {
    let result = dovetail::build_project(
        project,
        workspace_root,
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build, &std::collections::HashMap::new(), false,
    );

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!("build_project failed: {}", errors.join("; "));
    }

    let wasm_bytes = result.wasm.expect("expected WASM output");

    dovetail::runner::run_component(
        &wasm_bytes,
        &dovetail::runner::FsPermissions::default(),
        &dovetail::runner::EnvPermissions::default(),
        &dovetail::runner::NetPermissions::default(),
    )
    .unwrap_or_else(|e| panic!("WASM execution error: {}", e.message));
}

/// Helper: build a project and return error messages.
fn build_expecting_errors(
    project: &ResolvedProject,
    workspace_root: &std::path::Path,
) -> Vec<String> {
    let result = dovetail::build_project(
        project,
        workspace_root,
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build, &std::collections::HashMap::new(), false,
    );
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Helper: create a two-package project (a.utils + a root) with given sources.
fn make_two_package_project(
    utils_source: &str,
    root_source: &str,
) -> (tempfile::TempDir, ResolvedProject) {
    let dir = tempdir().unwrap();

    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(utils_src.join("lib.dove"), utils_source).unwrap();

    let root_src = dir.path().join("myapp").join("src");
    fs::write(root_src.join("main.dove"), root_source).unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "utils".to_string()]),
                source_dir: utils_src,
            },
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string()]),
                source_dir: root_src,
            },
        ],
        project_dir: dir.path().join("myapp"),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };

    (dir, project)
}

// ── Symbol imports ────────────────────────────────────────────────

#[test]
fn test_import_symbol() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils.id

function main(): Unit = assert id(5) == 5
"#,
    );
    build_and_run(&project, dir.path());
}

#[test]
fn test_import_symbol_with_alias() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils.id as identity

function main(): Unit = assert identity(5) == 5
"#,
    );
    build_and_run(&project, dir.path());
}

// ── Package imports ───────────────────────────────────────────────

#[test]
fn test_import_package() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils

function main(): Unit = assert utils.id(5) == 5
"#,
    );
    build_and_run(&project, dir.path());
}

#[test]
fn test_import_package_with_alias() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils as u

function main(): Unit = assert u.id(5) == 5
"#,
    );
    build_and_run(&project, dir.path());
}

// ── FQN without import ────────────────────────────────────────────

#[test]
fn test_fqn_without_import() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

function main(): Unit = assert a.utils.id(5) == 5
"#,
    );
    build_and_run(&project, dir.path());
}

// ── Error cases ───────────────────────────────────────────────────

#[test]
fn test_import_not_found_error() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils.nonexistent

function main(): Unit = ()
"#,
    );
    let errors = build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("cannot resolve import")),
        "expected import resolution error, got: {:?}",
        errors
    );
}

#[test]
fn test_import_internal_function_error() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

function secret(): Int32 = 42
"#,
        r#"
package a

import a.utils.secret

function main(): Unit = ()
"#,
    );
    let errors = build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("cannot resolve import")),
        "expected import error for internal function, got: {:?}",
        errors
    );
}

#[test]
fn test_bare_name_without_import_error() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

function main(): Unit = assert id(5) == 5
"#,
    );
    let errors = build_expecting_errors(&project, dir.path());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("undefined function") && e.contains("import")),
        "expected undefined function with import suggestion, got: {:?}",
        errors
    );
}

#[test]
fn test_same_package_no_import_needed() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

function helper(): Int32 = 42

function main(): Unit = assert helper() == 42
"#,
    );
    build_and_run(&project, dir.path());
}

#[test]
fn test_import_duplicate_name_warning() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
        r#"
package a

import a.utils.id
import a.utils.id

function main(): Unit = assert id(5) == 5
"#,
    );
    // Duplicate import is a warning, not an error — build should succeed
    build_and_run(&project, dir.path());
}
