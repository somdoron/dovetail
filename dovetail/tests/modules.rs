mod common;

use std::fs;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

#[test]
fn module_static_function() {
    common::compile_and_run(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

function main(): Unit = assert Math.double(21) == 42
"#,
    )
    .expect("module static function");
}

#[test]
fn module_static_function_multiple() {
    common::compile_and_run(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2
    function triple(x: Int32): Int32 = x * 3

function main(): Unit =
    assert Math.double(5) == 10
    assert Math.triple(5) == 15
"#,
    )
    .expect("module multiple static functions");
}

#[test]
fn module_property() {
    common::compile_and_run(
        r#"
package a

module Constants =
    let property pi: Float64 = 3.14159

function main(): Unit = assert Constants.pi > 3.0
"#,
    )
    .expect("module property");
}

#[test]
fn module_let_global() {
    common::compile_and_run(
        r#"
package a

module Config =
    let maxSize: Int32 = 100

function main(): Unit = assert Config.maxSize == 100
"#,
    )
    .expect("module let global");
}

#[test]
fn module_overloaded_functions() {
    common::compile_and_run(
        r#"
package a

module Math =
    function add(x: Int32, y: Int32): Int32 = x + y
    function add(x: Float64, y: Float64): Float64 = x + y

function main(): Unit =
    assert Math.add(1, 2) == 3
    assert Math.add(1.0, 2.0) == 3.0
"#,
    )
    .expect("module overloaded functions");
}

#[test]
fn module_mixed_members() {
    common::compile_and_run(
        r#"
package a

module Utils =
    let maxRetries: Int32 = 3
    let property version: Int32 = 1
    function greet(): Int32 = 42

function main(): Unit =
    assert Utils.maxRetries == 3
    assert Utils.version == 1
    assert Utils.greet() == 42
"#,
    )
    .expect("module mixed members");
}

#[test]
fn module_self_param_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

module Math =
    function bad(self: Int32): Int32 = self

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("self") && e.contains("not allowed")),
        "expected self-param error, got: {:?}",
        errors
    );
}

#[test]
fn module_duplicate_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

module Math =
    function triple(x: Int32): Int32 = x * 3

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate module")),
        "expected duplicate module error, got: {:?}",
        errors
    );
}

#[test]
fn module_public_visibility() {
    common::compile_and_run(
        r#"
package a

module Math =
    public function double(x: Int32): Int32 = x * 2

function main(): Unit = assert Math.double(21) == 42
"#,
    )
    .expect("module public visibility");
}

#[test]
fn module_function_with_multiple_params() {
    common::compile_and_run(
        r#"
package a

module Math =
    function clamp(value: Int32, min: Int32, max: Int32): Int32 =
        if value < min then min
        else if value > max then max
        else value

function main(): Unit =
    assert Math.clamp(5, 0, 10) == 5
    assert Math.clamp(-1, 0, 10) == 0
    assert Math.clamp(15, 0, 10) == 10
"#,
    )
    .expect("module function with multiple params");
}

// ── Cross-package module helpers ─────────────────────────────────

fn mp_build_and_run(project: &ResolvedProject, workspace_root: &std::path::Path) {
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

fn mp_build_expecting_errors(
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

// ── Cross-package module tests ───────────────────────────────────

#[test]
fn cross_package_import_module_function() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Math =
    public function double(x: Int32): Int32 = x * 2
"#,
        r#"
package a

import a.utils.Math

function main(): Unit = assert Math.double(21) == 42
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_import_module_with_alias() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Math =
    public function double(x: Int32): Int32 = x * 2
"#,
        r#"
package a

import a.utils.Math as M

function main(): Unit = assert M.double(21) == 42
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_import_module_property() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Constants =
    public let property pi: Float64 = 3.14159
"#,
        r#"
package a

import a.utils.Constants

function main(): Unit = assert Constants.pi > 3.0
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_import_module_global() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Config =
    public let maxSize: Int32 = 100
"#,
        r#"
package a

import a.utils.Config

function main(): Unit = assert Config.maxSize == 100
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_non_public_module_members_not_accessible() {
    // Module with only internal members: import succeeds but members are not accessible.
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Secret =
    function hidden(): Int32 = 42
"#,
        r#"
package a

import a.utils.Secret

function main(): Unit =
    let x = Secret.hidden()
    ()
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        !errors.is_empty(),
        "expected error accessing internal module member cross-package, got no errors"
    );
}

// ── Module-for-type tests ────────────────────────────────────────

#[test]
fn module_for_record_instance_function() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 10; y = 20 }
    assert p.getX() == 10
"#,
    )
    .expect("module-for-type instance function");
}

#[test]
fn module_for_record_instance_property() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    property sum(self): Int32 = self.x + self.y

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    assert p.sum == 10
"#,
    )
    .expect("module-for-type instance property");
}

#[test]
fn module_for_record_mixed_static_and_instance() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function new(x: Int32, y: Int32): Point = Point { x = x; y = y }
    function translate(self, dx: Int32, dy: Int32): Point = Point { x = self.x + dx; y = self.y + dy }

function main(): Unit =
    let p = Point.new(1, 2)
    let p2 = p.translate(10, 20)
    assert p2.x == 11
    assert p2.y == 22
"#,
    )
    .expect("module-for-type mixed static and instance");
}

#[test]
fn module_for_record_static_qualified_call_no_instance() {
    // Static qualified call works; instance methods should NOT be available via Type.method()
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let _x = Point.getX(p)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no function") || e.contains("no static method") || e.contains("undefined variable")),
        "expected error for calling instance method via Type.method(), got: {:?}",
        errors
    );
}

#[test]
#[ignore] // Requires enum variant construction (Color.Red) which is not yet implemented
fn module_for_enum_instance_function() {
    common::compile_and_run(
        r#"
package a

enum Color =
    Red
    Green
    Blue

module Color =
    function ordinal(self): Int32 = 42

function main(): Unit =
    let c = Color.Red
    assert c.ordinal() == 42
"#,
    )
    .expect("module-for-enum instance function");
}

#[test]
fn module_for_type_self_type_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function bad(self: Int32): Int32 = self

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected self type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn module_standalone_self_still_errors() {
    // Regression: standalone modules (no matching type) still reject `self`
    let errors = common::compile_expecting_errors(
        r#"
package a

module Math =
    function bad(self): Int32 = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("self") && e.contains("not allowed")),
        "expected self-param error for standalone module, got: {:?}",
        errors
    );
}

#[test]
fn cross_package_module_for_type_instance_method() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32

public module Point =
    public function getX(self): Int32 = self.x
    public function new(x: Int32, y: Int32): Point = Point { x = x; y = y }
"#,
        r#"
package a

import a.utils.Point

function main(): Unit =
    let p = Point.new(5, 10)
    assert p.getX() == 5
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_module_for_type_instance_property() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public record Point =
    x: Int32
    y: Int32

public module Point =
    public property sum(self): Int32 = self.x + self.y
"#,
        r#"
package a

import a.utils.Point

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    assert p.sum == 10
"#,
    );
    mp_build_and_run(&project, dir.path());
}

// ── Module visibility tests ──────────────────────────────────────

#[test]
fn cross_package_only_public_members_accessible() {
    // Module has public + internal members; only public should be accessible cross-package
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Math =
    public function double(x: Int32): Int32 = x * 2
    function triple(x: Int32): Int32 = x * 3
"#,
        r#"
package a

import a.utils.Math

function main(): Unit = assert Math.double(21) == 42
"#,
    );
    mp_build_and_run(&project, dir.path());
}

#[test]
fn cross_package_internal_member_not_accessible() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

module Math =
    public function double(x: Int32): Int32 = x * 2
    function triple(x: Int32): Int32 = x * 3
"#,
        r#"
package a

import a.utils.Math

function main(): Unit = assert Math.triple(7) == 21
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no function") || e.contains("no member") || e.contains("undefined variable")),
        "expected error for accessing internal member cross-package, got: {:?}",
        errors
    );
}

#[test]
fn same_package_internal_members_accessible() {
    // Internal (default) visibility should be accessible within the same package
    common::compile_and_run(
        r#"
package a

module Math =
    function double(x: Int32): Int32 = x * 2

function main(): Unit = assert Math.double(21) == 42
"#,
    )
    .expect("internal members accessible within same package");
}

fn make_single_package_two_files(
    module_source: &str,
    main_source: &str,
) -> (tempfile::TempDir, ResolvedProject) {
    let dir = tempdir().unwrap();

    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("module.dove"), module_source).unwrap();
    fs::write(src.join("main.dove"), main_source).unwrap();

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
fn private_module_function_not_accessible_cross_file() {
    let (dir, project) = make_single_package_two_files(
        r#"
package a

module Math =
    private function secret(): Int32 = 42
    function double(x: Int32): Int32 = x * 2
"#,
        r#"
package a

function main(): Unit =
    assert Math.double(21) == 42
    assert Math.secret() == 42
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no function") || e.contains("no member") || e.contains("undefined variable")),
        "expected error for accessing private function cross-file, got: {:?}",
        errors
    );
}

#[test]
fn private_module_global_not_accessible_cross_file() {
    let (dir, project) = make_single_package_two_files(
        r#"
package a

module Config =
    private let secretKey: Int32 = 42
    let maxSize: Int32 = 100
"#,
        r#"
package a

function main(): Unit =
    assert Config.maxSize == 100
    assert Config.secretKey == 42
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no member") || e.contains("undefined variable")),
        "expected error for accessing private global cross-file, got: {:?}",
        errors
    );
}

#[test]
fn private_module_property_not_accessible_cross_file() {
    let (dir, project) = make_single_package_two_files(
        r#"
package a

module Constants =
    private let property secret: Int32 = 42
    let property version: Int32 = 1
"#,
        r#"
package a

function main(): Unit =
    assert Constants.version == 1
    assert Constants.secret == 42
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no member") || e.contains("undefined variable")),
        "expected error for accessing private property cross-file, got: {:?}",
        errors
    );
}

#[test]
fn private_module_instance_method_not_accessible_cross_file() {
    let (dir, project) = make_single_package_two_files(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    private function secret(self): Int32 = self.x + self.y
    function getX(self): Int32 = self.x
"#,
        r#"
package a

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    assert p.getX() == 3
    assert p.secret() == 10
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no") && (e.contains("method") || e.contains("field") || e.contains("function"))),
        "expected error for accessing private instance method cross-file, got: {:?}",
        errors
    );
}

#[test]
fn private_module_instance_property_not_accessible_cross_file() {
    let (dir, project) = make_single_package_two_files(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    private property secret(self): Int32 = self.x + self.y
    property total(self): Int32 = self.x + self.y
"#,
        r#"
package a

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    assert p.total == 10
    assert p.secret == 10
"#,
    );
    let errors = mp_build_expecting_errors(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("no field")),
        "expected error for accessing private instance property cross-file, got: {:?}",
        errors
    );
}

// ── Intra-module unqualified access tests ────────────────────────

#[test]
fn intra_module_bare_function_call() {
    common::compile_and_run(
        r#"
package a

module Math =
    function helper(x: Int32): Int32 = x + 1
    function double(x: Int32): Int32 = helper(x) * 2

function main(): Unit = assert Math.double(5) == 12
"#,
    )
    .expect("bare function call within module");
}

#[test]
fn intra_module_bare_global_access() {
    common::compile_and_run(
        r#"
package a

module Config =
    let maxSize: Int32 = 100
    function getMax(): Int32 = maxSize

function main(): Unit = assert Config.getMax() == 100
"#,
    )
    .expect("bare global access within module");
}

#[test]
fn intra_module_bare_property_access() {
    common::compile_and_run(
        r#"
package a

module Constants =
    let property version: Int32 = 42
    function getVersion(): Int32 = version

function main(): Unit = assert Constants.getVersion() == 42
"#,
    )
    .expect("bare static property access within module");
}

#[test]
fn intra_module_bare_function_overloads() {
    common::compile_and_run(
        r#"
package a

module Math =
    function add(x: Int32, y: Int32): Int32 = x + y
    function add(x: Float64, y: Float64): Float64 = x + y
    function doubleAdd(x: Int32, y: Int32): Int32 = add(x, y) * 2

function main(): Unit = assert Math.doubleAdd(3, 4) == 14
"#,
    )
    .expect("bare function call with overloads within module");
}

#[test]
fn intra_module_instance_method_calls_static() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function helper(v: Int32): Int32 = v * 10
    function describe(self): Int32 = helper(self.x)

function main(): Unit =
    let p = Point { x = 3; y = 7 }
    assert p.describe() == 30
"#,
    )
    .expect("instance method calling sibling static function");
}

#[test]
fn intra_module_local_shadows_module_member() {
    common::compile_and_run(
        r#"
package a

module Math =
    let maxSize: Int32 = 100
    function test(): Int32 =
        let maxSize = 5
        maxSize

function main(): Unit = assert Math.test() == 5
"#,
    )
    .expect("local variable shadows module member");
}

#[test]
fn intra_module_import_shadows_module_member() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public function helper(): Int32 = 99
"#,
        r#"
package a

import a.utils.helper

module Math =
    function helper(): Int32 = 1
    function test(): Int32 = helper()

function main(): Unit = assert Math.test() == 99
"#,
    );
    mp_build_and_run(&project, dir.path());
}

// ── Generic module tests ─────────────────────────────────────────

#[test]
fn generic_module_instance_function_specialized() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function unwrap(self): T = self.value

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.unwrap() == 42
"#,
    )
    .expect("generic module instance function (specialized)");
}

#[test]
fn generic_module_instance_function_shared() {
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    inner: T

record Payload =
    x: Int32

module Wrapper<T> =
    function get(self): T = self.inner

function main(): Unit =
    let w = Wrapper<Payload> { inner = Payload { x = 99 } }
    assert w.get().x == 99
"#,
    )
    .expect("generic module instance function (shared)");
}

#[test]
fn generic_module_static_function_with_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }

function main(): Unit =
    let b = Box<Int32>.wrap(42)
    assert b.value == 42
"#,
    )
    .expect("generic module static function with explicit type args");
}

#[test]
fn generic_module_static_function_inferred_type_args() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }

function main(): Unit =
    let b: Box<Int32> = Box.wrap(42)
    assert b.value == 42
"#,
    )
    .expect("generic module static function with inferred type args");
}

#[test]
fn generic_module_instance_property() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    property inner(self): T = self.value

function main(): Unit =
    let b = Box<Int32> { value = 7 }
    assert b.inner == 7
"#,
    )
    .expect("generic module instance property");
}

#[test]
fn generic_module_concrete_global() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let defaultCapacity: Int32 = 16

function main(): Unit =
    assert Box<Int32>.defaultCapacity == 16
"#,
    )
    .expect("concrete global in generic module");
}

#[test]
fn generic_module_error_no_type_params_for_generic_type() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

module Box =
    function test(): Int32 = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires type parameters")),
        "expected error about missing type parameters, got: {:?}",
        errors
    );
}

#[test]
fn generic_module_error_type_param_count_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

module Box<T, U> =
    function test(self): T = self.value

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type parameters") && e.contains("has 2") && e.contains("has 1")),
        "expected type param count mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn generic_module_mixed_members() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let defaultCapacity: Int32 = 16
    function wrap(v: T): Box<T> = Box<T> { value = v }
    function unwrap(self): T = self.value
    property inner(self): T = self.value

function main(): Unit =
    assert Box<Int32>.defaultCapacity == 16
    let b = Box<Int32>.wrap(42)
    assert b.unwrap() == 42
    assert b.inner == 42
"#,
    )
    .expect("generic module mixed members");
}

#[test]
fn generic_module_cross_package() {
    let (dir, project) = make_two_package_project(
        r#"
package a.utils

public record Box<T> =
    value: T

public module Box<T> =
    public function wrap(v: T): Box<T> = Box<T> { value = v }
    public function unwrap(self): T = self.value
    public property inner(self): T = self.value
    public let defaultCapacity: Int32 = 16
"#,
        r#"
package a

import a.utils.Box

function main(): Unit =
    let b = Box<Int32>.wrap(42)
    assert b.unwrap() == 42
    assert b.inner == 42
    assert Box<Int32>.defaultCapacity == 16
"#,
    );
    mp_build_and_run(&project, dir.path());
}

// ── Method-level generic functions in modules ─────────────────────────

#[test]
fn module_static_generic_function() {
    common::compile_and_run(
        r#"
package a

module Utils =
    function identity<T>(x: T): T = x

function main(): Unit =
    assert Utils.identity(42) == 42
    assert Utils.identity(true) == true
"#,
    )
    .expect("module static generic function");
}

#[test]
fn module_static_generic_function_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

module Utils =
    function identity<T>(x: T): T = x

function main(): Unit =
    assert Utils.identity<Int32>(42) == 42
"#,
    )
    .expect("module static generic function with explicit type args");
}

#[test]
fn module_for_type_instance_generic_function() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    function withDefault<T>(self, value: T): T = value

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.withDefault(99) == 99
    assert p.withDefault(true) == true
"#,
    )
    .expect("module-for-type instance generic function");
}

#[test]
fn module_generic_function_two_type_params() {
    common::compile_and_run(
        r#"
package a

module Utils =
    function first<A, B>(a: A, b: B): A = a

function main(): Unit =
    assert Utils.first(42, true) == 42
    assert Utils.first(true, 10) == true
"#,
    )
    .expect("module generic function with two type params");
}

#[test]
fn generic_module_with_method_type_params() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }
    function replaceWith<U>(self, newValue: U): Box<U> = Box<U> { value = newValue }

function main(): Unit =
    let b = Box<Int32>.wrap(42)
    assert b.value == 42
    let b2 = b.replaceWith(true)
    assert b2.value == true
"#,
    )
    .expect("generic module with method type params");
}

#[test]
fn generic_module_static_with_method_type_params() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }
    function create<U>(primary: T, extra: U): Box<T> = Box<T> { value = primary }

function main(): Unit =
    let b = Box<Int32>.create(42, true)
    assert b.value == 42
"#,
    )
    .expect("generic module static with method type params");
}

#[test]
fn module_static_generic_property() {
    common::compile_and_run(
        r#"
package a

module Utils =
    property zero<T>(): Int32 = 0

function main(): Unit = assert Utils.zero<Bool> == 0
"#,
    )
    .expect("module static generic property");
}

#[test]
fn generic_module_static_generic_property() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }
    property zero<U>(): Int32 = 0

function main(): Unit =
    let x = Box<Int32>.zero<Bool>
    assert x == 0
"#,
    )
    .expect("generic module static generic property");
}

#[test]
fn generic_module_instance_generic_property() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }
    property tag<U>(self): T = self.value

function main(): Unit =
    let b = Box<Int32>.wrap(42)
    assert b.tag<Bool> == 42
"#,
    )
    .expect("generic module instance generic property");
}

#[test]
fn module_for_type_instance_generic_property() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

module Point =
    property tag<T>(self): Int32 = self.x

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    assert p.tag<Bool> == 3
"#,
    )
    .expect("module-for-type instance generic property");
}

#[test]
fn module_static_generic_property_with_type_param_return() {
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    tag: Int32

module Factory =
    property make<T>(): Wrapper<T> = Wrapper<T> { tag = 42 }

function main(): Unit =
    let w = Factory.make<Int32>
    assert w.tag == 42
"#,
    )
    .expect("module static generic property with type param in return type");
}

#[test]
fn module_static_generic_property_bidirectional() {
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    tag: Int32

module Factory =
    property make<T>(): Wrapper<T> = Wrapper<T> { tag = 42 }

function main(): Unit =
    let w: Wrapper<Int32> = Factory.make
    assert w.tag == 42
"#,
    )
    .expect("module static generic property with bidirectional inference");
}

// ── Generic module globals (Phase 6) ─────────────────────────────

#[test]
fn generic_module_global_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let tag: Int32 = 42

function main(): Unit = assert Box<Int32>.tag == 42
"#,
    )
    .expect("generic module global with explicit type args");
}

#[test]
fn generic_module_global_bidirectional() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let tag: Int32 = 42

function main(): Unit =
    let t: Int32 = Box<Int32>.tag
    assert t == 42
"#,
    )
    .expect("generic module global with bidirectional inference");
}

#[test]
fn generic_module_mutable_global_read() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let mutable count: Int32 = 0

function main(): Unit = assert Box<Int32>.count == 0
"#,
    )
    .expect("generic module mutable global read");
}

#[test]
fn generic_module_mutable_global_assign() {
    // Globals on generic modules have canonical storage — `Box<Int32>.count` and
    // `Box<Bool>.count` refer to the same global, since the rules pass forbids the
    // declared type from referencing the module's type parameters.
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    public let mutable count: Int32 = 0

function main(): Unit =
    Box<Int32>.count = 5
    assert Box<Int32>.count == 5
    assert Box<Bool>.count == 5
"#,
    )
    .expect("generic module mutable global shared storage");
}

#[test]
fn module_mutable_global_assign() {
    common::compile_and_run(
        r#"
package a

module Config =
    public let mutable maxSize: Int32 = 100

function main(): Unit =
    Config.maxSize = 200
    assert Config.maxSize == 200
"#,
    )
    .expect("non-generic module mutable global assignment");
}

#[test]
fn generic_module_immutable_global_assign_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let tag: Int32 = 42

function main(): Unit = Box<Int32>.tag = 99
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot assign to immutable global")),
        "expected immutable global error, got: {:?}",
        errors
    );
}

#[test]
fn module_immutable_global_assign_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

module Config =
    let maxSize: Int32 = 100

function main(): Unit = Config.maxSize = 200
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot assign to immutable global")),
        "expected immutable global error, got: {:?}",
        errors
    );
}

#[test]
fn generic_module_function_accesses_module_global() {
    // Globals on generic modules have canonical storage — all instantiations
    // increment the same counter.
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

module Box<T> =
    let mutable count: Int32 = 0
    function increment(): Unit =
        count = count + 1
    function getCount(): Int32 = count

function main(): Unit =
    Box<Int32>.increment()
    Box<Int32>.increment()
    assert Box<Int32>.getCount() == 2
    assert Box<Bool>.getCount() == 2
    Box<Bool>.increment()
    assert Box<Bool>.getCount() == 3
    assert Box<Int32>.getCount() == 3
"#,
    )
    .expect("generic module function accessing module global with bare names");
}

// ── Bidirectional inference for FieldAccess type params ──────────

#[test]
fn generic_module_global_bidirectional_object_type_params_rejected() {
    // The rules pass forbids globals on generic modules from referencing the module's
    // type parameters (here `default: Config<T>` references `T`), even when the value
    // is independent of the type parameters under erasure. Statics on a generic module
    // have one canonical storage; a `Config<T>`-typed slot is ambiguous across
    // instantiations.
    let errors = common::compile_expecting_errors(
        r#"
package a

record Config<T> =
    count: Int32

module Config<T> =
    let default: Config<T> = Config<T> { count = 0 }

function main(): Unit =
    let c: Config<Int32> = Config.default
    assert c.count == 0
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("references type parameter")),
        "expected type-parameter rejection, got: {:?}",
        errors
    );
}

#[test]
fn generic_module_static_property_bidirectional_field_type_params() {
    // field_type_params inferred from expected type: `let w: Wrapper<Int32> = Utils.make`
    // where `make` is `property make<T>(): Wrapper<T>`
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    tag: Int32

module Utils =
    property make<T>(): Wrapper<T> = Wrapper<T> { tag = 99 }

function main(): Unit =
    let w: Wrapper<Int32> = Utils.make
    assert w.tag == 99
"#,
    )
    .expect("static property bidirectional field_type_params");
}

#[test]
fn generic_module_static_property_bidirectional_both_type_params() {
    // Both object_type_params and field_type_params inferred from expected type:
    // `let w: Wrapper<Bool> = Box<Int32>.make` where Box is generic module, make is generic property
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

record Wrapper<U> =
    tag: Int32

module Box<T> =
    property make<U>(): Wrapper<U> = Wrapper<U> { tag = 77 }

function main(): Unit =
    let w: Wrapper<Bool> = Box<Int32>.make
    assert w.tag == 77
"#,
    )
    .expect("static property bidirectional both type params");
}

#[test]
fn generic_module_instance_property_bidirectional_field_type_params() {
    // Instance property with field_type_params inferred from expected type:
    // `let w: Wrapper<Bool> = b.convert` where `convert<U>` returns `Wrapper<U>`
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

record Wrapper<U> =
    tag: Int32

module Box<T> =
    function wrap(v: T): Box<T> = Box<T> { value = v }
    property convert<U>(self): Wrapper<U> = Wrapper<U> { tag = 55 }

function main(): Unit =
    let b = Box<Int32>.wrap(42)
    let w: Wrapper<Bool> = b.convert
    assert w.tag == 55
"#,
    )
    .expect("instance property bidirectional field_type_params");
}

#[test]
fn test_module_for_trait_typechecks() {
    common::compile_and_run(r#"
package a

trait Marker

module Marker =
    function staticHelper(): Int32 = 42

function main(): Unit =
    assert Marker.staticHelper() == 42
"#)
    .expect("module for trait typechecks");
}

// --- `where` clauses on members of a generic module --------------------------
//
// A member of a generic module may constrain the MODULE's type parameter
// (`function run(self): Unit where E: Display` in `module Async<T, E>`), and
// that constraint was collected and then dropped: not stored on the member, not
// checked at the call site, not carried into instantiation. The call therefore
// type-checked and the missing impl surfaced in codegen as
// `ImplFunctionCall not resolved by monomorphize`, which is a compiler crash
// rather than a diagnosis. These pin both halves: the bound is enforced, and it
// is enforced only where it is unsatisfied.

#[test]
fn test_generic_module_member_where_clause_is_checked() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Tag =
    function tag(self: Self): String

record Box<T> =
    value: T

record Untagged =
    x: Int32

module Box<T> =
    public function describe(self): String where T: Tag = self.value.tag()

function main(): Unit =
    let b = Box<Untagged> { value = Untagged { x = 1 } }
    let _ = b.describe()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait 'Tag'")),
        "expected the member's where clause to be enforced, got: {errors:?}"
    );
}

#[test]
fn test_generic_module_static_member_where_clause_is_checked() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Tag =
    function tag(self: Self): String

record Box<T> =
    value: T

record Untagged =
    x: Int32

module Box<T> =
    public function describeDefault(value: T): String where T: Tag = value.tag()

function main(): Unit =
    let _ = Box<Untagged>.describeDefault(Untagged { x = 1 })
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait 'Tag'")),
        "expected the static member's where clause to be enforced, got: {errors:?}"
    );
}

#[test]
fn test_generic_module_member_where_clause_satisfied_runs() {
    common::compile_and_run(
        r#"
package a

trait Tag =
    function tag(self: Self): String

record Box<T> =
    value: T

record Tagged =
    x: Int32

implement Tag for Tagged =
    public function tag(self: Tagged): String = "tagged"

module Box<T> =
    public function describe(self): String where T: Tag = self.value.tag()
    public function describeDefault(value: T): String where T: Tag = value.tag()

function main(): Unit =
    let b = Box<Tagged> { value = Tagged { x = 1 } }
    assert b.describe() == "tagged"
    assert Box<Tagged>.describeDefault(Tagged { x = 2 }) == "tagged"
"#,
    )
    .expect("a satisfied where clause on a generic module member still resolves");
}
