mod common;

use std::fs;
use std::path::Path;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

#[test]
fn test_basic_passing_test() {
    common::compile_and_run_tests(
        r#"
package a

test "simple" = assert 1 + 1 == 2
"#,
    )
    .expect("basic passing test should succeed");
}

#[test]
fn test_failing_test_traps() {
    common::compile_tests_and_expect_trap(
        r#"
package a

test "fail" = assert false
"#,
    );
}

#[test]
fn test_multiple_passing_tests() {
    common::compile_and_run_tests(
        r#"
package a

test "one" = assert 1 == 1
test "two" = assert 2 == 2
test "three" = assert 3 == 3
"#,
    )
    .expect("multiple passing tests should succeed");
}

#[test]
fn test_duplicate_test_name_error() {
    let errors = common::compile_tests_expecting_errors(
        r#"
package a

test "same name" = assert true
test "same name" = assert true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate test name")),
        "expected duplicate test name error, got: {:?}",
        errors
    );
}

#[test]
fn test_with_block_body() {
    common::compile_and_run_tests(
        r#"
package a

test "block body" =
    let x = 10
    let y = 20
    assert x + y == 30
"#,
    )
    .expect("test with block body should succeed");
}

#[test]
fn test_calling_package_function() {
    common::compile_and_run_tests(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

test "calls add" = assert add(3, 4) == 7
"#,
    )
    .expect("test calling a package function should succeed");
}

#[test]
fn test_build_ignores_tests() {
    // When compiling in build mode (dovetail::compile), test declarations are present
    // but ignored — the build produces a valid WASM component (no-op if no main).
    // Tests should not cause compilation errors in build mode.
    common::compile_and_run(
        r#"
package a

function main(): Unit = ()

test "some test" = assert true
"#,
    )
    .expect("build mode should succeed with both main and test declarations");
}

#[test]
fn test_name_is_not_reserved_keyword() {
    // "test" can still be used as a function name
    common::compile_and_run(
        r#"
package a

function test(): Int32 = 42

function main(): Unit = assert test() == 42
"#,
    )
    .expect("'test' as function name should work");
}

#[test]
fn test_mode_with_main_and_tests_runs_tests() {
    // In test mode, test functions take priority over main.
    // Main should NOT run; only tests should execute.
    common::compile_and_run_tests(
        r#"
package a

function main(): Unit = panic "main should not run in test mode"

test "runs instead of main" = assert 1 == 1
"#,
    )
    .expect("test mode should run tests, not main");
}

#[test]
fn test_visibility_modifier_error() {
    let errors = common::compile_tests_expecting_errors(
        r#"
package a

public test "visible" = assert true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("visibility")),
        "expected visibility modifier error, got: {:?}",
        errors
    );
}

// --- @skip attribute tests ---

#[test]
fn test_skip_attribute() {
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@skip
test "x" = panic "not run"
"#,
    )
    .expect("skipped test should compile and run");

    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            &run_result.results[0].status,
            dovetail::test_runner::TestStatus::Skip { .. }
        ),
        "expected Skip status"
    );
}

#[test]
fn test_skip_attribute_with_reason() {
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@skip("not ready")
test "x" = panic "not run"
"#,
    )
    .expect("skipped test with reason should compile and run");

    assert_eq!(run_result.results.len(), 1);
    match &run_result.results[0].status {
        dovetail::test_runner::TestStatus::Skip { reason } => {
            assert_eq!(reason.as_deref(), Some("not ready"));
        }
        other => panic!("expected Skip, got: {:?}", std::mem::discriminant(other)),
    }
}

// --- @panics attribute tests ---

#[test]
fn test_panics_expects_trap() {
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@panics
test "x" = panic "boom"
"#,
    )
    .expect("@panics test that panics should pass");

    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            &run_result.results[0].status,
            dovetail::test_runner::TestStatus::Pass
        ),
        "expected Pass status for @panics test that traps"
    );
}

#[test]
fn test_panics_fails_without_trap() {
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@panics
test "x" = assert true
"#,
    )
    .expect("@panics test that doesn't panic should fail");

    assert_eq!(run_result.results.len(), 1);
    match &run_result.results[0].status {
        dovetail::test_runner::TestStatus::Fail { message } => {
            assert!(
                message.contains("expected panic but test passed"),
                "unexpected fail message: {message}"
            );
        }
        other => panic!("expected Fail, got: {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_panics_with_message_match() {
    // Note: panic messages currently compile to just `unreachable` in WASM,
    // so @panics("msg") matches against the trap error string which contains "unreachable".
    // When runtime support for panic messages is added, this test can use the actual message.
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@panics("unreachable")
test "x" = panic "boom"
"#,
    )
    .expect("@panics with matching message should pass");

    assert_eq!(run_result.results.len(), 1);
    match &run_result.results[0].status {
        dovetail::test_runner::TestStatus::Pass => {}
        dovetail::test_runner::TestStatus::Fail { message } => {
            panic!("expected Pass but got Fail: {message}");
        }
        other => panic!("expected Pass, got: {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn test_panics_message_mismatch() {
    // Uses a message that won't appear in the trap error
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@panics("nonexistent message xyz")
test "x" = panic "bang"
"#,
    )
    .expect("@panics with mismatching message should fail");

    assert_eq!(run_result.results.len(), 1);
    match &run_result.results[0].status {
        dovetail::test_runner::TestStatus::Fail { message } => {
            assert!(
                message.contains("expected panic containing 'nonexistent message xyz'"),
                "unexpected fail message: {message}"
            );
        }
        other => panic!("expected Fail, got: {:?}", std::mem::discriminant(other)),
    }
}

// --- @timeout attribute tests ---

#[test]
fn test_timeout_within_limit() {
    let run_result = common::compile_and_run_tests_with_results(
        r#"
package a

@timeout(5000)
test "x" = assert true
"#,
    )
    .expect("@timeout test within limit should pass");

    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            &run_result.results[0].status,
            dovetail::test_runner::TestStatus::Pass
        ),
        "expected Pass status for @timeout test within limit"
    );
}

// --- Error cases ---

#[test]
fn test_unknown_attribute_error() {
    let errors = common::compile_tests_expecting_errors(
        r#"
package a

@foo
test "x" = assert true
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("unknown test attribute '@foo'")),
        "expected unknown attribute error, got: {:?}",
        errors
    );
}

#[test]
fn test_duplicate_attribute_error() {
    let errors = common::compile_tests_expecting_errors(
        r#"
package a

@skip
@skip
test "x" = assert true
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate @skip")),
        "expected duplicate attribute error, got: {:?}",
        errors
    );
}

#[test]
fn test_timeout_zero_error() {
    let errors = common::compile_tests_expecting_errors(
        r#"
package a

@timeout(0)
test "x" = assert true
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("@timeout value must be greater than 0")),
        "expected timeout zero error, got: {:?}",
        errors
    );
}

// --- Same-file private access tests ---

#[test]
fn test_private_function_accessible_from_test() {
    common::compile_and_run_tests(
        r#"
package a

private function secret(): Int32 = 42

test "calls private" = assert secret() == 42
"#,
    )
    .expect("test should access private function in same file");
}

#[test]
fn test_private_record_accessible_from_test() {
    common::compile_and_run_tests(
        r#"
package a

private record Secret(value: Int32)

test "uses private record" = assert Secret(42).value == 42
"#,
    )
    .expect("test should access private record in same file");
}

#[test]
fn test_private_global_accessible_from_test() {
    common::compile_and_run_tests(
        r#"
package a

private let secretNum: Int32 = 42

test "reads private global" = assert secretNum == 42
"#,
    )
    .expect("test should access private global in same file");
}

#[test]
fn test_private_enum_accessible_from_test() {
    common::compile_and_run_tests(
        r#"
package a

private enum Toggle = On | Off

test "uses private enum" =
    let t: Toggle = Toggle.On
    assert t is Toggle
"#,
    )
    .expect("test should access private enum in same file");
}

// --- Multi-file private access tests ---

fn make_two_file_test_project(
    file1_source: &str,
    file2_source: &str,
) -> (tempfile::TempDir, ResolvedProject) {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("impl.dove"), file1_source).unwrap();
    fs::write(src.join("tests.dove"), file2_source).unwrap();
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
    (dir, project)
}

fn build_test_expecting_errors(project: &ResolvedProject, workspace_root: &Path) -> Vec<String> {
    let result = dovetail::build_project(
        project,
        workspace_root,
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Test,
        &std::collections::HashMap::new(),
        false,
    );
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

fn build_and_run_tests(
    project: &ResolvedProject,
    workspace_root: &Path,
) -> dovetail::test_runner::TestRunResult {
    build_and_run_filtered_tests(project, workspace_root, &[], None)
}

fn build_and_run_filtered_tests(
    project: &ResolvedProject,
    workspace_root: &Path,
    filters: &[&str],
    file_filter: Option<&str>,
) -> dovetail::test_runner::TestRunResult {
    let result = dovetail::build_project(
        project,
        workspace_root,
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Test,
        &std::collections::HashMap::new(),
        false,
    );
    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!("build failed: {}", errors.join("; "));
    }
    let wasm_bytes = result.wasm.expect("expected WASM output");
    let filtered_exports: Vec<dovetail::TestExportInfo> = result
        .test_exports
        .iter()
        .filter(|t| {
            if let Some(fp) = file_filter
                && !t.source_file.contains(fp)
            {
                return false;
            }
            if !filters.is_empty() {
                return filters.iter().any(|f| t.fqtn.contains(*f));
            }
            true
        })
        .cloned()
        .collect();
    dovetail::test_runner::run_tests(&wasm_bytes, &filtered_exports)
        .expect("test runner should not fail")
}

#[test]
fn test_cross_file_private_function_not_accessible_from_test() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

private function secret(): Int32 = 42
"#,
        r#"
package a

test "x" = assert secret() == 42
"#,
    );
    let errors = build_test_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("secret")),
        "expected error about 'secret' not being accessible, got: {:?}",
        errors
    );
}

#[test]
fn test_cross_file_internal_function_accessible_from_test() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

function helper(): Int32 = 42
"#,
        r#"
package a

test "x" = assert helper() == 42
"#,
    );
    let run_result = build_and_run_tests(&project, dir.path());
    assert!(
        run_result
            .results
            .iter()
            .all(|r| matches!(r.status, dovetail::test_runner::TestStatus::Pass)),
        "internal function should be accessible from test in another file"
    );
}

#[test]
fn test_same_file_private_function_accessible_in_multi_file_package() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

private function secret(): Int32 = 42

test "calls secret" = assert secret() == 42
"#,
        r#"
package a

test "other" = assert true
"#,
    );
    let run_result = build_and_run_tests(&project, dir.path());
    assert_eq!(run_result.results.len(), 2);
    for r in &run_result.results {
        assert!(
            matches!(r.status, dovetail::test_runner::TestStatus::Pass),
            "test '{}' should pass, got: {:?}",
            r.name,
            std::mem::discriminant(&r.status)
        );
    }
}

// --- Filter tests ---

#[test]
fn test_filter_by_name_substring() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

test "addition" = assert 1 + 1 == 2
test "subtraction" = assert 3 - 1 == 2
"#,
        r#"
package a
"#,
    );
    let run_result = build_and_run_filtered_tests(&project, dir.path(), &["addition"], None);
    assert_eq!(run_result.results.len(), 1);
    assert!(run_result.results[0].name.contains("addition"));
    assert!(matches!(
        run_result.results[0].status,
        dovetail::test_runner::TestStatus::Pass
    ));
}

#[test]
fn test_filter_no_match() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

test "addition" = assert 1 + 1 == 2
"#,
        r#"
package a
"#,
    );
    let run_result = build_and_run_filtered_tests(&project, dir.path(), &["nonexistent"], None);
    assert_eq!(run_result.results.len(), 0);
}

#[test]
fn test_filter_multiple_or() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

test "addition" = assert 1 + 1 == 2
test "subtraction" = assert 3 - 1 == 2
test "multiplication" = assert 2 * 3 == 6
"#,
        r#"
package a
"#,
    );
    let run_result =
        build_and_run_filtered_tests(&project, dir.path(), &["addition", "subtraction"], None);
    assert_eq!(run_result.results.len(), 2);
    assert!(
        run_result
            .results
            .iter()
            .all(|r| matches!(r.status, dovetail::test_runner::TestStatus::Pass))
    );
}

#[test]
fn test_file_filter() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

test "in impl" = assert 1 == 1
"#,
        r#"
package a

test "in tests" = assert 2 == 2
"#,
    );
    let run_result = build_and_run_filtered_tests(&project, dir.path(), &[], Some("impl.dove"));
    assert_eq!(run_result.results.len(), 1);
    assert!(run_result.results[0].name.contains("in impl"));
}

#[test]
fn test_combined_filter_and_file() {
    let (dir, project) = make_two_file_test_project(
        r#"
package a

test "addition" = assert 1 + 1 == 2
test "subtraction" = assert 3 - 1 == 2
"#,
        r#"
package a

test "addition other" = assert 10 + 10 == 20
"#,
    );
    // Filter by name "addition" AND file "impl.dove" — should only match "addition" from impl.dove
    let run_result =
        build_and_run_filtered_tests(&project, dir.path(), &["addition"], Some("impl.dove"));
    assert_eq!(run_result.results.len(), 1);
    assert!(run_result.results[0].name.contains("addition"));
}

// --- Integration test directory (test/) tests ---

fn make_project_with_test_dir(
    src_source: &str,
    test_source: &str,
) -> (tempfile::TempDir, ResolvedProject) {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("main.dove"), src_source).unwrap();

    let test_dir = dir.path().join("myapp").join("test");
    fs::create_dir_all(&test_dir).unwrap();
    fs::write(test_dir.join("main_test.dove"), test_source).unwrap();

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
    (dir, project)
}

#[test]
fn test_integration_test_basic() {
    let (dir, project) = make_project_with_test_dir(
        r#"
package a

public function add(x: Int32, y: Int32): Int32 = x + y
"#,
        r#"
package test

import a.add

test "addition" = assert add(1, 2) == 3
"#,
    );
    let run_result = build_and_run_tests(&project, dir.path());
    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            run_result.results[0].status,
            dovetail::test_runner::TestStatus::Pass
        ),
        "integration test should pass"
    );
}

#[test]
fn test_integration_test_internal_access() {
    let (dir, project) = make_project_with_test_dir(
        r#"
package a

function internal_helper(): Int32 = 42
"#,
        r#"
package test

import a.internal_helper

test "internal access" = assert internal_helper() == 42
"#,
    );
    let run_result = build_and_run_tests(&project, dir.path());
    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            run_result.results[0].status,
            dovetail::test_runner::TestStatus::Pass
        ),
        "integration test should access internal functions"
    );
}

#[test]
fn test_integration_test_private_denied() {
    let (dir, project) = make_project_with_test_dir(
        r#"
package a

private function secret(): Int32 = 42
"#,
        r#"
package test

import a.secret

test "private access" = assert secret() == 42
"#,
    );
    let errors = build_test_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("secret")),
        "expected error about 'secret' not being accessible, got: {:?}",
        errors
    );
}

#[test]
fn test_reserved_test_prefix_in_src() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        "package test\n\nfunction main(): Unit = ()",
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".to_string()),
        root_package: PackagePath(vec!["test".to_string()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["test".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };
    let errors = build_test_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("reserved 'test' prefix")),
        "expected reserved test prefix error, got: {:?}",
        errors
    );
}

#[test]
fn test_no_test_directory() {
    // Project with no test/ directory — should produce 0 integration tests, no error
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"
package a

test "unit test" = assert 1 == 1
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
    let run_result = build_and_run_tests(&project, dir.path());
    // Only the unit test from src, no integration tests
    assert_eq!(run_result.results.len(), 1);
    assert!(
        matches!(
            run_result.results[0].status,
            dovetail::test_runner::TestStatus::Pass
        ),
        "unit test should still pass"
    );
}

#[test]
fn test_mixed_unit_and_integration() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"
package a

public function add(x: Int32, y: Int32): Int32 = x + y

test "unit add" = assert add(2, 3) == 5
"#,
    )
    .unwrap();

    let test_dir = dir.path().join("myapp").join("test");
    fs::create_dir_all(&test_dir).unwrap();
    fs::write(
        test_dir.join("integration.dove"),
        r#"
package test

import a.add

test "integration add" = assert add(10, 20) == 30
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
    let run_result = build_and_run_tests(&project, dir.path());
    assert_eq!(
        run_result.results.len(),
        2,
        "should have both unit and integration tests"
    );
    assert!(
        run_result
            .results
            .iter()
            .all(|r| matches!(r.status, dovetail::test_runner::TestStatus::Pass)),
        "all tests should pass"
    );
}
