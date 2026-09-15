mod common;

use std::fs;
use std::path::PathBuf;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject, ResolvedWorkspace};
use tempfile::tempdir;

/// Get the workspace root (repo root).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// Build a workspace that includes wasi + a test app, then run it.
fn build_and_run_random_app(app_source: &str) {
    let root = repo_root();
    let dir = tempdir().unwrap();

    // Write test app source
    let app_src = dir.path().join("testapp").join("src");
    fs::create_dir_all(&app_src).unwrap();
    fs::write(app_src.join("main.dove"), app_source).unwrap();

    let workspace = ResolvedWorkspace {
        dependency_aliases: Default::default(),
        projects: vec![
            ResolvedProject {
                resolved_identity: None,
                name: ProjectName("wasi".to_string()),
                root_package: PackagePath(vec![
                    "standard".to_string(),
                    "wasi".to_string(),
                ]),
                depends: vec![],
                packages: vec![
                    ResolvedPackage {
                        path: PackagePath(vec![
                            "standard".to_string(),
                            "wasi".to_string(),
                        ]),
                        source_dir: root.join("wasi").join("src"),
                    },
                    ResolvedPackage {
                        path: PackagePath(vec![
                            "standard".to_string(),
                            "wasi".to_string(),
                            "random".to_string(),
                        ]),
                        source_dir: root.join("wasi").join("src").join("random"),
                    },
                ],
                project_dir: root.join("wasi"),
                main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
            },
            ResolvedProject {
                resolved_identity: None,
                name: ProjectName("testapp".to_string()),
                root_package: PackagePath(vec!["testapp".to_string()]),
                depends: vec![
                    ProjectName("wasi".to_string()),
                ],
                packages: vec![ResolvedPackage {
                    path: PackagePath(vec!["testapp".to_string()]),
                    source_dir: app_src,
                }],
                project_dir: dir.path().join("testapp"),
                main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
            },
        ],
        workspace_root: dir.path().to_path_buf(),
    };

    let result = dovetail::build_workspace(&workspace, Some("testapp"), dovetail::BuildMode::Build, &std::collections::HashMap::new(), false, None);

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
        panic!("build failed:\n{}", errors.join("\n"));
    }

    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(name, _)| name == "testapp")
        .expect("testapp not found in results");

    let wasm_bytes = project_result.wasm.as_ref().expect("expected WASM output");

    dovetail::runner::run_component(
        wasm_bytes,
        &dovetail::runner::FsPermissions::default(),
        &dovetail::runner::EnvPermissions::default(),
        &dovetail::runner::NetPermissions::default(),
    )
    .unwrap_or_else(|e| panic!("WASM execution error: {}", e.message));
}

#[test]
fn test_random_int64() {
    build_and_run_random_app(r#"
package testapp

import standard.wasi.random.Random

function main(): Unit =
    let a = Random.int64()
    let b = Random.int64()
    assert a + 0i64 == a
    assert b + 0i64 == b
"#);
}

#[test]
fn test_random_bytes_length() {
    build_and_run_random_app(r#"
package testapp

import standard.wasi.random.Random

function main(): Unit =
    let bytes = Random.bytes(10i64)
    assert bytes.length == 10
"#);
}

#[test]
fn test_random_bytes_empty() {
    build_and_run_random_app(r#"
package testapp

import standard.wasi.random.Random

function main(): Unit =
    let bytes = Random.bytes(0i64)
    assert bytes.length == 0
"#);
}
