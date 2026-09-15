mod common;

use std::fs;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

/// Helper: build & run with the project's declared resources actually written to disk.
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
            .map(|d| {
                format!(
                    "{}:{}:{}: {}",
                    d.span.file, d.span.line, d.span.column, d.message
                )
            })
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
        dovetail::BuildMode::Check,
        &std::collections::HashMap::new(),
        false,
    );
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn test_resource_bytes_basic() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    // Write the resource file.
    let assets = dir.path().join("myapp").join("assets");
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("greeting.bin"), b"hello\x00\x01\x02").unwrap();

    // A program that loads the bytes and asserts their content.
    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let bytes = EmbeddedResource.bytes("assets/greeting.bin")
    assert bytes.length == 8
    assert bytes.get(0) == 104u8   // 'h'
    assert bytes.get(1) == 101u8   // 'e'
    assert bytes.get(2) == 108u8   // 'l'
    assert bytes.get(3) == 108u8   // 'l'
    assert bytes.get(4) == 111u8   // 'o'
    assert bytes.get(5) == 0u8
    assert bytes.get(6) == 1u8
    assert bytes.get(7) == 2u8
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
        resources: vec!["assets/greeting.bin".to_string()],
        macros: vec![],
        components: vec![],
    };

    build_and_run(&project, dir.path());
}

#[test]
fn test_resource_bytes_missing_resource_errors() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let _ = EmbeddedResource.bytes("missing.bin")
    ()
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
    let errors = build_expecting_errors(&project, dir.path());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("no resource named 'missing.bin'")),
        "expected missing-resource error, got: {:?}",
        errors
    );
}

#[test]
fn test_resource_bytes_requires_string_literal() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let name = "tzdata.bin"
    let _ = EmbeddedResource.bytes(name)
    ()
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
    let errors = build_expecting_errors(&project, dir.path());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires a string literal")),
        "expected literal-required error, got: {:?}",
        errors
    );
}
