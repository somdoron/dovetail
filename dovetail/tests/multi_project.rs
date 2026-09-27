mod common;

use std::fs;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject, ResolvedWorkspace};
use tempfile::tempdir;

#[test]
fn class_identity_across_dependency_layouts_and_generic_bounds() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib
public class Base(public mutable value: Int32)
public class GenericBase<T>(public mutable payload: T)
public function identityHash<T>(value: T): Int64 where T: class = ClassIdentity.hash(value)
public function same<T>(a: T, b: T): Bool where T: class = ClassIdentity.equals(a, b)
"#,
        r#"
package app
import lib.Base
import lib.GenericBase
import lib.identityHash
import lib.same
class Child(public pair: (Int32, Int32)) extends Base(3)
class GenericChild(public pair: (Int32, Int32)) extends GenericBase<Int32>(7)
function main(): Unit =
    let child = Child((5, 6))
    let base: Base = child
    let hash = identityHash(child)
    assert identityHash(base) == hash
    assert same(base, base)
    base.value = 4
    assert base.value == 4
    assert child.value == 4
    assert child.pair._1 == 6
    assert identityHash(child) == hash
    let genericChild = GenericChild((8, 9))
    let genericBase: GenericBase<Int32> = genericChild
    assert identityHash(genericChild) == identityHash(genericBase)
    genericBase.payload = 11
    assert genericChild.payload == 11
    assert genericChild.pair._1 == 9
"#,
    );
    build_workspace_and_run(&workspace, Some("myapp"), "myapp");
    build_workspace_and_run(&workspace, Some("myapp"), "myapp");
}

/// Helper: build workspace, find target project's WASM, run via WASI component model.
fn build_workspace_and_run(
    workspace: &ResolvedWorkspace,
    project_filter: Option<&str>,
    run_project: &str,
) {
    let result = dovetail::build_workspace(
        workspace,
        project_filter,
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );

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
        panic!("build_workspace failed:\n{}", errors.join("\n"));
    }

    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(name, _)| name == run_project)
        .unwrap_or_else(|| panic!("project '{run_project}' not found in results"));

    let wasm_bytes = project_result.wasm.as_ref().expect("expected WASM output");

    dovetail::runner::run_component(
        wasm_bytes,
        &dovetail::runner::FsPermissions::default(),
        &dovetail::runner::EnvPermissions::default(),
        &dovetail::runner::NetPermissions::default(),
    )
    .unwrap_or_else(|e| panic!("WASM execution error: {}", e.message));
}

/// Helper: create a two-project workspace (mylib + myapp) with given sources.
fn make_two_project_workspace(
    lib_source: &str,
    app_source: &str,
) -> (tempfile::TempDir, ResolvedWorkspace) {
    let dir = tempdir().unwrap();

    let lib_src = dir.path().join("mylib").join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(lib_src.join("lib.dove"), lib_source).unwrap();

    let app_src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&app_src).unwrap();
    fs::write(app_src.join("main.dove"), app_source).unwrap();

    let workspace = ResolvedWorkspace {
        dependency_aliases: Default::default(),
        projects: vec![
            ResolvedProject {
                resolved_identity: None,
                name: ProjectName("mylib".to_string()),
                root_package: PackagePath(vec!["lib".to_string()]),
                depends: vec![],
                packages: vec![ResolvedPackage {
                    path: PackagePath(vec!["lib".to_string()]),
                    source_dir: lib_src,
                }],
                project_dir: dir.path().join("mylib"),
                main_function: None,
                resources: vec![],
                macros: vec![],
                components: vec![],
            },
            ResolvedProject {
                resolved_identity: None,
                name: ProjectName("myapp".to_string()),
                root_package: PackagePath(vec!["app".to_string()]),
                depends: vec![ProjectName("mylib".to_string())],
                packages: vec![ResolvedPackage {
                    path: PackagePath(vec!["app".to_string()]),
                    source_dir: app_src,
                }],
                project_dir: dir.path().join("myapp"),
                main_function: None,
                resources: vec![],
                macros: vec![],
                components: vec![],
            },
        ],
        workspace_root: dir.path().to_path_buf(),
    };

    (dir, workspace)
}

// ── Cross-project calls ──────────────────────────────────────────

#[test]
fn test_two_projects_cross_project_call() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public function double(x: Int32): Int32 = x + x
"#,
        r#"
package app

import lib.double

function main(): Unit = assert double(21) == 42
"#,
    );
    build_workspace_and_run(&workspace, None, "myapp");
}

#[test]
fn test_cross_project_fqn_without_import() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public function double(x: Int32): Int32 = x + x
"#,
        r#"
package app

function main(): Unit = assert lib.double(21) == 42
"#,
    );
    build_workspace_and_run(&workspace, None, "myapp");
}

#[test]
fn test_cross_project_package_import() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public function double(x: Int32): Int32 = x + x
"#,
        r#"
package app

import lib as l

function main(): Unit = assert l.double(21) == 42
"#,
    );
    build_workspace_and_run(&workspace, None, "myapp");
}

// ── Project filtering ────────────────────────────────────────────

#[test]
fn test_project_filter_target_and_deps() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public function double(x: Int32): Int32 = x + x
"#,
        r#"
package app

import lib.double

function main(): Unit = assert double(21) == 42
"#,
    );

    // Filter to "myapp" → builds both (mylib + myapp)
    let result = dovetail::build_workspace(
        &workspace,
        Some("myapp"),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );
    assert!(!result.diagnostics.has_errors());
    assert_eq!(result.project_results.len(), 2);
    assert_eq!(result.project_results[0].0, "mylib");
    assert_eq!(result.project_results[1].0, "myapp");

    // Filter to "mylib" → builds only mylib
    let result = dovetail::build_workspace(
        &workspace,
        Some("mylib"),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );
    assert!(!result.diagnostics.has_errors());
    assert_eq!(result.project_results.len(), 1);
    assert_eq!(result.project_results[0].0, "mylib");
}

// ── Error propagation ────────────────────────────────────────────

#[test]
fn test_error_in_dependency_stops_build() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public function bad(): Int32 = true
"#,
        r#"
package app

function main(): Unit = ()
"#,
    );

    let result = dovetail::build_workspace(
        &workspace,
        None,
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );
    assert!(result.diagnostics.has_errors());
    // Only mylib should have been attempted
    assert_eq!(result.project_results.len(), 1);
    assert_eq!(result.project_results[0].0, "mylib");
    assert!(result.project_results[0].1.wasm.is_none());
}

// ── Cross-project extension methods ──────────────────────────────

#[test]
fn test_cross_project_named_extension_on_primitive() {
    let (_dir, workspace) = make_two_project_workspace(
        r#"
package lib

public extension IntMath for Int32 =
    public function cube(self): Int32 = self * self * self
"#,
        r#"
package app

import lib.IntMath

function main(): Unit = assert 3.cube() == 27
"#,
    );
    build_workspace_and_run(&workspace, None, "myapp");
}

// ── End-to-end with manifest ─────────────────────────────────────

#[test]
fn test_build_workspace_via_manifest() {
    let dir = tempdir().unwrap();

    // Write Dovetail.toml
    fs::write(
        dir.path().join("Dovetail.toml"),
        r#"compiler-version = "0.1.3"
[[project]]
name = "mylib"
root_package = "lib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "app"
depends = ["mylib"]
packages = ["."]
"#,
    )
    .unwrap();

    // mylib source
    let lib_src = dir.path().join("mylib").join("src");
    fs::create_dir_all(&lib_src).unwrap();
    fs::write(
        lib_src.join("lib.dove"),
        r#"
package lib

public function triple(x: Int32): Int32 = x + x + x
"#,
    )
    .unwrap();

    // myapp source
    let app_src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&app_src).unwrap();
    fs::write(
        app_src.join("main.dove"),
        r#"
package app

import lib.triple

function main(): Unit = assert triple(10) == 30
"#,
    )
    .unwrap();

    // Load manifest and build
    let workspace = dovetail::manifest::load_manifest(dir.path()).unwrap();
    build_workspace_and_run(&workspace, None, "myapp");
}

#[test]
fn dependency_test_classes_do_not_leak_into_consumers() {
    let (dir, workspace) = make_two_project_workspace(
        r#"
package lib
public function answer(): Int32 = 42
"#,
        r#"
package app
import lib.answer
function main(): Unit = assert answer() == 42
"#,
    );
    let tests = dir.path().join("mylib/test");
    fs::create_dir_all(&tests).unwrap();
    fs::write(
        tests.join("box.dove"),
        r#"
package test
class TestBox<T>(public value: T)
test "generic test class" = assert TestBox(42).value == 42
"#,
    )
    .unwrap();
    let result = dovetail::build_workspace(
        &workspace,
        None,
        dovetail::BuildMode::Test,
        &std::collections::HashMap::new(),
        false,
        None,
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(!result.diagnostics.has_errors(), "{errors:?}");
    assert_eq!(result.project_results.len(), 2);
}
