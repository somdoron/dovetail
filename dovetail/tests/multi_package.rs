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
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
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

#[test]
fn test_multi_file_same_package() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("helper.dove"),
        r#"
package a

public function helper(): Int32 = 42
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit = assert helper() == 42
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    build_and_run(&project, dir.path());
}

#[test]
fn test_multi_package_cross_package_call() {
    let dir = tempdir().unwrap();

    // Package a.utils
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public function id(x: Int32): Int32 = x
"#,
    )
    .unwrap();

    // Package a (root)
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.id

function main(): Unit = assert id(5) == 5
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            // utils compiled first (dependency order)
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

    build_and_run(&project, dir.path());
}

#[test]
fn test_error_stops_orchestration() {
    let dir = tempdir().unwrap();

    // Package a.utils — has a type error
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public function bad(): Int32 = true
"#,
    )
    .unwrap();

    // Package a (root) — should not be processed
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

function main(): Unit = ()
"#,
    )
    .unwrap();

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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    assert!(result.wasm.is_none());
}

#[test]
fn test_cross_package_record_usage() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a record type
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32
"#,
    )
    .unwrap();

    // Package a (root): imports and uses the record
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Point

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.x == 3
"#,
    )
    .unwrap();

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

    build_and_run(&project, dir.path());
}

#[test]
fn test_cross_package_named_extension() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a record type
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32
"#,
    )
    .unwrap();

    // Package a.ext: defines named extension on Point
    let ext_src = dir.path().join("myapp").join("src").join("ext");
    fs::create_dir_all(&ext_src).unwrap();
    fs::write(
        ext_src.join("lib.dove"),
        r#"
package a.ext

import a.utils.Point

public extension PointHelpers for Point =
    public function sum(self): Int32 = self.x + self.y
"#,
    )
    .unwrap();

    // Package a (root): imports both the type and the named extension
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Point
import a.ext.PointHelpers

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.sum() == 7
"#,
    )
    .unwrap();

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
                path: PackagePath(vec!["a".to_string(), "ext".to_string()]),
                source_dir: ext_src,
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

    build_and_run(&project, dir.path());
}

#[test]
fn test_cross_package_named_extension_not_imported() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a record type
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32
"#,
    )
    .unwrap();

    // Package a.ext: defines named extension on Point
    let ext_src = dir.path().join("myapp").join("src").join("ext");
    fs::create_dir_all(&ext_src).unwrap();
    fs::write(
        ext_src.join("lib.dove"),
        r#"
package a.ext

import a.utils.Point

public extension PointHelpers for Point =
    public function sum(self): Int32 = self.x + self.y
"#,
    )
    .unwrap();

    // Package a (root): imports Point but NOT PointHelpers
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Point

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    p.sum()
"#,
    )
    .unwrap();

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
                path: PackagePath(vec!["a".to_string(), "ext".to_string()]),
                source_dir: ext_src,
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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method, got: {:?}",
        errors
    );
}

// ── Private extension methods: multi-file visibility ──────────────────

#[test]
fn test_private_extension_method_not_visible_in_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    // File A: defines a private extension method
    fs::write(
        src.join("ext.dove"),
        r#"
package a

extension Int32Ext for Int32 =
    private function secret(self): Int32 = self * 42
"#,
    )
    .unwrap();

    // File B: tries to use the private method
    fs::write(
        src.join("main.dove"),
        r#"
package a

import a.Int32Ext

function main(): Unit = assert 5.secret() == 210
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method for private ext method in other file, got: {:?}",
        errors
    );
}

#[test]
fn test_private_named_extension_method_not_visible_cross_package() {
    let dir = tempdir().unwrap();

    // Package a.ext: defines named extension with private method
    let ext_src = dir.path().join("myapp").join("src").join("ext");
    fs::create_dir_all(&ext_src).unwrap();
    fs::write(
        ext_src.join("lib.dove"),
        r#"
package a.ext

public extension IntSecrets for Int32 =
    private function hidden(self): Int32 = self * 99
"#,
    )
    .unwrap();

    // Package a (root): imports the named extension, tries to use the private method
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.ext.IntSecrets

function main(): Unit = assert 5.hidden() == 495
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "ext".to_string()]),
                source_dir: ext_src,
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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method for private ext method cross-package, got: {:?}",
        errors
    );
}

#[test]
fn test_internal_extension_method_not_visible_cross_package() {
    let dir = tempdir().unwrap();

    // Package a.ext: defines named extension with internal method
    let ext_src = dir.path().join("myapp").join("src").join("ext");
    fs::create_dir_all(&ext_src).unwrap();
    fs::write(
        ext_src.join("lib.dove"),
        r#"
package a.ext

public extension IntOps for Int32 =
    function internalOnly(self): Int32 = self + 1
"#,
    )
    .unwrap();

    // Package a (root): imports the named extension, tries to use internal method
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.ext.IntOps

function main(): Unit = assert 5.internalOnly() == 6
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "ext".to_string()]),
                source_dir: ext_src,
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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("no method")),
        "expected error about no method for internal ext method cross-package, got: {:?}",
        errors
    );
}

#[test]
fn test_public_extension_method_visible_cross_package_internal_not() {
    let dir = tempdir().unwrap();

    // Package a.ext: defines named extension with both public and internal methods
    let ext_src = dir.path().join("myapp").join("src").join("ext");
    fs::create_dir_all(&ext_src).unwrap();
    fs::write(
        ext_src.join("lib.dove"),
        r#"
package a.ext

public extension IntOps for Int32 =
    public function triple(self): Int32 = self * 3
    function internalHelper(self): Int32 = self + 1
"#,
    )
    .unwrap();

    // Package a (root): imports the named extension, uses only the public method
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.ext.IntOps

function main(): Unit = assert 5.triple() == 15
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "ext".to_string()]),
                source_dir: ext_src,
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

    build_and_run(&project, dir.path());
}

#[test]
fn test_package_decl_mismatch() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    // File declares package 'b' but it's in a package with path 'a'
    fs::write(
        src.join("main.dove"),
        r#"
package b

function main(): Unit = ()
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result.diagnostics.iter().collect();
    assert!(
        errors
            .iter()
            .any(|e| e.message.contains("declares package 'b' but expected 'a'")),
        "expected package mismatch error, got: {:?}",
        errors
    );
}

// ── Private functions: multi-file visibility ──────────────────────────

#[test]
fn test_private_function_not_visible_in_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("helper.dove"),
        r#"
package a

private function secret(): Int32 = 42
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit = assert secret() == 42
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("undefined function")),
        "expected error about undefined function for private function in other file, got: {:?}",
        errors
    );
}

#[test]
fn test_private_function_not_importable_from_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("helper.dove"),
        r#"
package a

private function secret(): Int32 = 42
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

import a.secret

function main(): Unit = assert secret() == 42
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("cannot resolve import")),
        "expected error about cannot resolve import for private function, got: {:?}",
        errors
    );
}

// ── Private globals: multi-file visibility ────────────────────────────

#[test]
fn test_private_global_not_visible_in_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("constants.dove"),
        r#"
package a

private let SECRET: Int32 = 42
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit = assert SECRET == 42
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("undefined")),
        "expected error about undefined for private global in other file, got: {:?}",
        errors
    );
}

// ── Cross-package extension on primitive ───────────────────────────────

#[test]
fn test_cross_package_named_extension_on_primitive() {
    let dir = tempdir().unwrap();

    // Package a.mathlib: defines a named extension on Int32
    let mathlib_src = dir.path().join("myapp").join("src").join("mathlib");
    fs::create_dir_all(&mathlib_src).unwrap();
    fs::write(
        mathlib_src.join("lib.dove"),
        r#"
package a.mathlib

public extension IntMath for Int32 =
    public function cube(self): Int32 = self * self * self
"#,
    )
    .unwrap();

    // Package a (root): imports and uses the extension
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.mathlib.IntMath

function main(): Unit = assert 3.cube() == 27
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "mathlib".to_string()]),
                source_dir: mathlib_src,
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

    build_and_run(&project, dir.path());
}

// ── Private records: multi-file visibility ────────────────────────────

#[test]
fn test_private_record_not_visible_in_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("types.dove"),
        r#"
package a

private record Secret =
    x: Int32
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let s = Secret { x = 1 }
    assert s.x == 1
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("unknown")),
        "expected error about unknown type for private record in other file, got: {:?}",
        errors
    );
}

// ── Cross-package type aliases ────────────────────────────────────────

#[test]
fn test_cross_package_non_generic_type_alias() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a public type alias
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public type Cents = Int32
"#,
    )
    .unwrap();

    // Package a (root): imports and uses the alias
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Cents

function add_cents(a: Cents, b: Cents): Cents = a + b

function main(): Unit =
    let c: Cents = 100
    assert c == 100
    assert add_cents(30, 12) == 42
"#,
    )
    .unwrap();

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

    build_and_run(&project, dir.path());
}

#[test]
fn test_cross_package_generic_type_alias() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a public generic type alias
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public type Maybe<T> = Option<T>
"#,
    )
    .unwrap();

    // Package a (root): imports and uses the generic alias
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Maybe

function main(): Unit =
    let x: Maybe<Int32> = Some(42)
    match x with
        case Some(v) => assert v == 42
        case None => panic "expected Some"
    let y: Maybe<String> = Some("hello")
    match y with
        case Some(s) => assert s == "hello"
        case None => panic "expected Some"
"#,
    )
    .unwrap();

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

    build_and_run(&project, dir.path());
}

#[test]
fn test_private_type_alias_not_visible_cross_package() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a private type alias
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

type Secret = Int32
"#,
    )
    .unwrap();

    // Package a (root): tries to import the private alias
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Secret

function main(): Unit =
    let s: Secret = 42
    ()
"#,
    )
    .unwrap();

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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot resolve import") || e.contains("unknown")),
        "expected error for private type alias cross-package, got: {:?}",
        errors
    );
}

#[test]
fn test_cross_package_generic_type_alias_with_trait_bound() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines trait + bounded generic alias
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public trait Showable =
    function show(self: Self): String

public type ShowBox<T> where T: Showable = Option<T>
"#,
    )
    .unwrap();

    // Package a (root): uses the bounded alias with a valid type
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Showable
import a.utils.ShowBox

record Name =
    value: String

implement Showable for Name =
    function show(self: Name): String = self.value

function main(): Unit =
    let x: ShowBox<Name> = Some(Name { value = "hi" })
    match x with
        case Some(n) => assert n.show() == "hi"
        case None => panic "expected Some"
"#,
    )
    .unwrap();

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

    build_and_run(&project, dir.path());
}

#[test]
fn test_cross_package_generic_type_alias_trait_bound_violated() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines trait + bounded generic alias
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public trait Showable =
    function show(self: Self): String

public type ShowBox<T> where T: Showable = Option<T>
"#,
    )
    .unwrap();

    // Package a (root): uses the bounded alias with Int32 (doesn't impl Showable)
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.ShowBox

function main(): Unit =
    let x: ShowBox<Int32> = Some(42)
    ()
"#,
    )
    .unwrap();

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

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected trait bound violation error, got: {:?}",
        errors
    );
}

#[test]
fn test_cross_package_type_alias_to_record() {
    let dir = tempdir().unwrap();

    // Package a.utils: defines a record and a public alias to it
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32

public type Coord = Point
"#,
    )
    .unwrap();

    // Package a (root): imports and uses the alias
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Point
import a.utils.Coord

function main(): Unit =
    let c: Coord = Point { x = 10; y = 20 }
    assert c.x == 10
    assert c.y == 20
"#,
    )
    .unwrap();

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

    build_and_run(&project, dir.path());
}

#[test]
fn test_private_type_alias_not_visible_in_other_file() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    // File A: defines a private type alias
    fs::write(
        src.join("types.dove"),
        r#"
package a

private type Secret = Int32
"#,
    )
    .unwrap();

    // File B: tries to use the private alias
    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let s: Secret = 42
    assert s == 42
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };

    let result = dovetail::build_project(
        &project,
        dir.path(),
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
    );
    assert!(result.diagnostics.has_errors());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    assert!(
        errors.iter().any(|e| e.contains("unknown")),
        "expected error about unknown type for private type alias in other file, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_associated_default_across_packages() {
    let dir = tempdir().unwrap();

    // Package a.utils
    let utils_src = dir.path().join("myapp").join("src").join("utils");
    fs::create_dir_all(&utils_src).unwrap();
    fs::write(
        utils_src.join("lib.dove"),
        r#"
package a.utils

public trait Wrapper =
    type Wrapped<T>
    function wrap<T>(self, value: T): Wrapped<T>
    function forward<T>(self, value: T): Wrapped<T> = self.wrap(value)

public function wrapValue<W, T>(wrapper: W, value: T): W.Wrapped<T> where W: Wrapper =
    wrapper.forward(value)
"#,
    )
    .unwrap();

    // Package a (root)
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.utils.Wrapper
import a.utils.wrapValue

record ArrayWrapper = id: Int32
implement Wrapper for ArrayWrapper =
    type Wrapped<T> = Array<T>
    function wrap<T>(self, value: T): Array<T> = [|value|]
function main(): Unit =
    assert wrapValue(ArrayWrapper { id = 0 }, 42) == [|42|]
    assert wrapValue(ArrayWrapper { id = 0 }, "text") == [|"text"|]
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["a".to_string()]),
        depends: vec![],
        packages: vec![
            // utils compiled first (dependency order)
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

    build_and_run(&project, dir.path());
}
