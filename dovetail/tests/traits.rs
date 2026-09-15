mod common;

#[test]
fn test_trait_single_method() {
    common::check_no_errors(
        r#"
package a

trait Greeter =
    function greet(name: String): String

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_multiple_methods() {
    common::check_no_errors(
        r#"
package a

trait Equatable =
    function equals(self: Int32, other: Int32): Bool
    function notEquals(self: Int32, other: Int32): Bool

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_with_self_param() {
    common::check_no_errors(
        r#"
package a

trait Printable =
    function toString(self: String): String

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_with_type_params() {
    common::check_no_errors(
        r#"
package a

trait Comparable<T> =
    function compareTo(self: T, other: T): Int32

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_method_with_type_params() {
    common::check_no_errors(
        r#"
package a

trait Container =
    function get<T>(index: Int32): T

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_public_visibility() {
    common::check_no_errors(
        r#"
package a

public trait Showable =
    function show(self: String): String

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_private_visibility() {
    common::check_no_errors(
        r#"
package a

private trait Internal =
    function process(x: Int32): Int32

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_method_no_return_type() {
    common::check_no_errors(
        r#"
package a

trait Sink =
    function consume(value: Int32)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_method_no_params() {
    common::check_no_errors(
        r#"
package a

trait Factory =
    function create(): Int32

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_single_line() {
    common::check_no_errors(
        r#"
package a

trait Simple = function identity(x: Int32): Int32

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_missing_equals_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Broken
    function identity(x: Int32): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expected '='")),
        "expected '=' error, got: {:?}",
        errors
    );
}

// --- Collect-phase tests ---

#[test]
fn test_trait_self_in_params() {
    common::check_no_errors(
        r#"
package a

trait Equatable =
    function equals(self: Self, other: Self): Bool

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_self_as_return_type() {
    common::check_no_errors(
        r#"
package a

trait Clonable =
    function clone(self: Self): Self

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_method_known_types() {
    common::check_no_errors(
        r#"
package a

trait Converter =
    function toInt(value: String): Int32
    function toBool(value: Int32): Bool
    function toString(value: Bool): String

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_unknown_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Bad =
    function process(x: UnknownType): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown type")),
        "expected 'unknown type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_duplicate_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Foo =
    function bar(): Int32

trait Foo =
    function baz(): Bool

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate trait")),
        "expected 'duplicate trait' error, got: {:?}",
        errors
    );
}

#[test]
fn test_duplicate_method_in_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Broken =
    function doStuff(x: Int32): Int32
    function doStuff(x: Bool): Bool

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("duplicate method")),
        "expected 'duplicate method' error, got: {:?}",
        errors
    );
}

// --- Implement block tests ---

#[test]
fn test_implement_simple_trait_for_record() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Display =
    function format(self: Self): String

implement Display for Point =
    function format(self: Point): String = "point"

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_implement_trait_for_primitive() {
    common::check_no_errors(
        r#"
package a

trait Describable =
    function describe(self: Self): String

implement Describable for Int32 =
    function describe(self: Int32): String = "an integer"

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_implement_trait_with_self_substitution() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Equatable =
    function equals(self: Self, other: Self): Bool

implement Equatable for Point =
    function equals(self: Point, other: Point): Bool = self.x == other.x

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_implement_call_trait_method_via_dot_syntax() {
    common::compile_and_run(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Display =
    function format(self: Self): String

implement Display for Point =
    function format(self: Point): String = "hello"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert p.format() == "hello"
"#,
    )
    .expect("should call trait method via dot syntax");
}

#[test]
fn test_implement_multiple_methods() {
    common::check_no_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Describable =
    function describe(self: Self): String
    function tag(self: Self): Int32

implement Describable for Point =
    function describe(self: Point): String = "point"
    function tag(self: Point): Int32 = 42

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_implement_unknown_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

implement NonExistent for Point =
    function foo(self: Point): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown trait")),
        "expected 'unknown trait' error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_missing_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait TwoMethods =
    function foo(self: Self): Int32
    function bar(self: Self): String

implement TwoMethods for Point =
    function foo(self: Point): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("missing implementation")),
        "expected 'missing implementation' error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_extra_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Simple =
    function foo(self: Self): Int32

implement Simple for Point =
    function foo(self: Point): Int32 = 1
    function bar(self: Point): String = "extra"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("not a member of trait")),
        "expected 'not a member of trait' error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_duplicate_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Display =
    function format(self: Self): String

implement Display for Point =
    function format(self: Point): String = "first"

implement Display for Point =
    function format(self: Point): String = "second"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("already implements")),
        "expected 'already implements' error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_wrong_param_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Equatable =
    function equals(self: Self, other: Self): Bool

implement Equatable for Point =
    function equals(self: Point, other: String): Bool = true

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("parameter type mismatch")),
        "expected 'parameter type mismatch' error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_wrong_return_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

trait Display =
    function format(self: Self): String

implement Display for Point =
    function format(self: Point): Int32 = 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("return type mismatch")),
        "expected 'return type mismatch' error, got: {:?}",
        errors
    );
}

// --- Orphan rule tests (multi-package) ---

use std::fs;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

/// Helper: build a multi-package project and return errors.
fn build_project_errors(
    project: &ResolvedProject,
    workspace_root: &std::path::Path,
) -> Vec<String> {
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
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

#[test]
fn test_orphan_rule_foreign_trait_foreign_type_error() {
    let dir = tempdir().unwrap();

    // Package a.traits: defines a trait
    let traits_src = dir.path().join("myapp").join("src").join("traits");
    fs::create_dir_all(&traits_src).unwrap();
    fs::write(
        traits_src.join("lib.dove"),
        r#"
package a.traits

public trait Display =
    function format(self: Self): String
"#,
    )
    .unwrap();

    // Package a.types: defines a record
    let types_src = dir.path().join("myapp").join("src").join("types");
    fs::create_dir_all(&types_src).unwrap();
    fs::write(
        types_src.join("lib.dove"),
        r#"
package a.types

public record Point =
    x: Int32
    y: Int32
"#,
    )
    .unwrap();

    // Package a (root): tries to implement foreign trait for foreign type
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.traits.Display
import a.types.Point

implement Display for Point =
    function format(self: Point): String = "point"

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
                path: PackagePath(vec!["a".to_string(), "traits".to_string()]),
                source_dir: traits_src,
            },
            ResolvedPackage {
                path: PackagePath(vec!["a".to_string(), "types".to_string()]),
                source_dir: types_src,
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

    let errors = build_project_errors(&project, dir.path());
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot implement foreign trait")),
        "expected orphan rule error, got: {:?}",
        errors
    );
}

#[test]
fn test_orphan_rule_own_trait_foreign_type_ok() {
    let dir = tempdir().unwrap();

    // Package a.types: defines a record
    let types_src = dir.path().join("myapp").join("src").join("types");
    fs::create_dir_all(&types_src).unwrap();
    fs::write(
        types_src.join("lib.dove"),
        r#"
package a.types

public record Point =
    x: Int32
    y: Int32
"#,
    )
    .unwrap();

    // Package a (root): defines trait locally, implements for foreign type
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.types.Point

trait Display =
    function format(self: Self): String

implement Display for Point =
    function format(self: Point): String = "point"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert p.format() == "point"
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
                path: PackagePath(vec!["a".to_string(), "types".to_string()]),
                source_dir: types_src,
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

    let errors = build_project_errors(&project, dir.path());
    assert!(
        errors.is_empty(),
        "expected no errors for own-trait + foreign-type, got: {:?}",
        errors
    );
}

#[test]
fn test_orphan_rule_foreign_trait_own_type_ok() {
    let dir = tempdir().unwrap();

    // Package a.traits: defines a trait
    let traits_src = dir.path().join("myapp").join("src").join("traits");
    fs::create_dir_all(&traits_src).unwrap();
    fs::write(
        traits_src.join("lib.dove"),
        r#"
package a.traits

public trait Display =
    function format(self: Self): String
"#,
    )
    .unwrap();

    // Package a (root): defines type locally, implements foreign trait for own type
    let root_src = dir.path().join("myapp").join("src");
    fs::write(
        root_src.join("main.dove"),
        r#"
package a

import a.traits.Display

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert p.format() == "point"
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
                path: PackagePath(vec!["a".to_string(), "traits".to_string()]),
                source_dir: traits_src,
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

    let errors = build_project_errors(&project, dir.path());
    assert!(
        errors.is_empty(),
        "expected no errors for foreign-trait + own-type, got: {:?}",
        errors
    );
}

// ── Type parameter rules ─────────────────────────────────────────────

#[test]
fn test_self_as_trait_type_param_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Foo<Self> =
    function bar(x: Int32): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("'Self' cannot be used as a type parameter name")),
        "expected Self type param error, got: {:?}",
        errors
    );
}

#[test]
fn test_self_as_trait_method_type_param_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Foo =
    function bar<Self>(x: Int32): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("'Self' cannot be used as a type parameter name")),
        "expected Self type param error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_method_type_param_shadows_trait_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Foo<T> =
    function bar<T>(x: T): T

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains(
            "type parameter 'T' on method 'bar' shadows type parameter 'T' on enclosing trait 'Foo'"
        )),
        "expected shadow error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_method_type_param_different_name_ok() {
    common::check_no_errors(
        r#"
package a

trait Foo<T> =
    function bar<U>(x: T): U

function main(): Unit = ()
"#,
    );
}

// --- Built-in trait method tests ---

#[test]
fn test_prelude_equatable_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 1i32.equals(1i32)
    assert 2i32.equals(2i32)
    let result = 1i32.equals(2i32)
    assert result == false
"#,
    )
    .expect("Int32.equals should work");
}

#[test]
fn test_prelude_equatable_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "hello".equals("hello")
    let result = "hello".equals("world")
    assert result == false
"#,
    )
    .expect("String.equals should work");
}

#[test]
fn test_prelude_equatable_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert true.equals(true)
    assert false.equals(false)
    let result = true.equals(false)
    assert result == false
"#,
    )
    .expect("Bool.equals should work");
}

#[test]
fn test_prelude_comparable_int32() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Ordering

function main(): Unit =
    assert 1i32.compare(2i32) == Ordering.Less
    assert 2i32.compare(2i32) == Ordering.Equal
    assert 3i32.compare(2i32) == Ordering.Greater
"#,
    )
    .expect("Int32.compare should work");
}

#[test]
fn test_prelude_display_string() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert "hello".format() == "hello"
"#,
    )
    .expect("String.format should work");
}

#[test]
fn test_prelude_display_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert true.format() == "true"
    assert false.format() == "false"
"#,
    )
    .expect("Bool.format should work");
}

// --- Operator lowering tests ---

#[test]
fn test_operator_lowering_eq_for_record() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Equatable

record Point =
    x: Int32
    y: Int32

implement Equatable for Point =
    function equals(self: Point, other: Point): Bool =
        if self.x == other.x then self.y == other.y
        else false

function main(): Unit =
    let p1 = Point { x = 1; y = 2 }
    let p2 = Point { x = 1; y = 2 }
    let p3 = Point { x = 3; y = 4 }
    assert p1 == p2
    assert p1 != p3
"#,
    )
    .expect("== and != should work for records implementing Equatable");
}

#[test]
fn test_operator_lowering_comparison_for_record() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Comparable
import standard.prelude.Ordering

record Score =
    value: Int32

implement Comparable for Score =
    function compare(self: Score, other: Score): Ordering =
        self.value.compare(other.value)

function main(): Unit =
    let s1 = Score { value = 10 }
    let s2 = Score { value = 5 }
    let s3 = Score { value = 10 }
    assert s1 > s2
    assert s2 < s1
    assert s1 >= s3
    assert s1 <= s3
"#,
    )
    .expect("comparison operators should work for records implementing Comparable");
}

#[test]
fn test_operator_lowering_eq_missing_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p1 = Point { x = 1; y = 2 }
    let p2 = Point { x = 1; y = 2 }
    assert p1 == p2
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("consider implementing 'Equatable'")),
        "expected 'consider implementing Equatable' error, got: {:?}",
        errors
    );
}

#[test]
fn test_operator_lowering_lt_missing_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Score =
    value: Int32

function main(): Unit =
    let s1 = Score { value = 10 }
    let s2 = Score { value = 5 }
    assert s1 < s2
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("consider implementing 'Comparable'")),
        "expected 'consider implementing Comparable' error, got: {:?}",
        errors
    );
}

#[test]
fn test_operator_lowering_ne_for_record() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Equatable

record Color =
    r: Int32
    g: Int32
    b: Int32

implement Equatable for Color =
    function equals(self: Color, other: Color): Bool =
        if self.r == other.r then
            if self.g == other.g then self.b == other.b
            else false
        else false

function main(): Unit =
    let c1 = Color { r = 255; g = 0; b = 0 }
    let c2 = Color { r = 0; g = 255; b = 0 }
    assert c1 != c2
    let c3 = Color { r = 255; g = 0; b = 0 }
    let result = c1 != c3
    assert result == false
"#,
    )
    .expect("!= should work for records implementing Equatable");
}

// --- Display + string interpolation tests ---

#[test]
fn test_display_int32_format() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 42.format() == "42"
    assert 0.format() == "0"
    assert (-5).format() == "-5"
"#,
    )
    .expect("Int32.format should work");
}

#[test]
fn test_display_int64_format() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 100i64.format() == "100"
    assert 0i64.format() == "0"
    assert (-17i64).format() == "-17"
"#,
    )
    .expect("Int64.format should work");
}

#[test]
fn test_interpolation_with_int32() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 42
    assert "value: ${x}" == "value: 42"
"#,
    )
    .expect("string interpolation with Int32 should work");
}

#[test]
fn test_interpolation_with_different_numeric_types() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = 42i32
    let b = -17i64
    assert "a=${a}, b=${b}" == "a=42, b=-17"
"#,
    )
    .expect("string interpolation with different numeric types should work");
}

#[test]
fn test_interpolation_with_bool() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let flag = true
    assert "flag: ${flag}" == "flag: true"
    let f2 = false
    assert "f2: ${f2}" == "f2: false"
"#,
    )
    .expect("string interpolation with Bool should work");
}

#[test]
fn test_interpolation_with_string_identity() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let name = "world"
    assert "hello ${name}" == "hello world"
"#,
    )
    .expect("string interpolation with String should still work");
}

#[test]
fn test_interpolation_with_var_syntax() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x = 7
    assert "num: $x" == "num: 7"
"#,
    )
    .expect("$var syntax with Display should work");
}

#[test]
fn test_display_user_type_interpolation() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    public function format(self: Point): String = "point"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert "${p}" == "point"
"#,
    )
    .expect("string interpolation with user type implementing Display should work");
}

#[test]
fn test_display_missing_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record Foo =
    x: Int32

function main(): Unit =
    let f = Foo { x = 1 }
    let s = "value: ${f}"
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for interpolation of type without Display, got no errors"
    );
}

#[test]
fn test_display_char_format() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let c = 'A'
    assert c.format() == "A"
"#,
    )
    .expect("Char.format should work");
}

// --- New intrinsic tests ---

#[test]
fn test_array_extend() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3|]
    let extended = arr.extend(0, 5)
    assert extended.length == 5
    assert extended.get(0) == 1
    assert extended.get(1) == 2
    assert extended.get(2) == 3
    assert extended.get(3) == 0
    assert extended.get(4) == 0
"#,
    )
    .expect("Array.extend should work");
}

#[test]
fn test_array_extend_shrink() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let arr = [|1, 2, 3, 4, 5|]
    let shrunk = arr.extend(0, 2)
    assert shrunk.length == 2
    assert shrunk.get(0) == 1
    assert shrunk.get(1) == 2
"#,
    )
    .expect("Array.extend with smaller size should work");
}

#[test]
fn test_array_concat() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = [|1, 2, 3|]
    let b = [|4, 5|]
    let c = a.concat(b)
    assert c.length == 5
    assert c.get(0) == 1
    assert c.get(1) == 2
    assert c.get(2) == 3
    assert c.get(3) == 4
    assert c.get(4) == 5
"#,
    )
    .expect("Array.concat should work");
}

#[test]
fn test_string_from_bytes() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let buf = [|72u8, 101u8, 108u8, 108u8, 111u8|]
    let s = String.fromBytes(buf, 0, 5)
    assert s == "Hello"
"#,
    )
    .expect("String.fromBytes should work");
}

#[test]
fn test_string_from_bytes_slice() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let buf = [|72u8, 101u8, 108u8, 108u8, 111u8|]
    let s = String.fromBytes(buf, 1, 3)
    assert s == "ell"
"#,
    )
    .expect("String.fromBytes with offset should work");
}

#[test]
fn test_string_from_char() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert String.fromChar('A') == "A"
    assert String.fromChar('z') == "z"
"#,
    )
    .expect("String.fromChar should work");
}

// --- Trait bounds (where clause) tests ---

#[test]
fn test_where_clause_parse_basic() {
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display

function describe<T>(x: T): String where T: Display = x.format()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_where_clause_multiple_bounds() {
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display
import standard.prelude.Equatable

function describe<T>(x: T, y: T): String where T: Display + Equatable = x.format()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_where_clause_multiple_constraints() {
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display
import standard.prelude.Equatable

function pair<T, U>(x: T, y: U): String where T: Display, U: Equatable = x.format()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_where_clause_unknown_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function describe<T>(x: T): String where T: NonExistentTrait = "hello"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown trait")),
        "expected 'unknown trait' error, got: {:?}",
        errors
    );
}

#[test]
fn test_where_clause_constraint_on_nonexistent_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import standard.prelude.Display

function describe<T>(x: T): String where U: Display = "hello"

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not declared on this item")),
        "expected 'not declared' error, got: {:?}",
        errors
    );
}

#[test]
fn test_where_clause_bound_satisfied_specialized() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

function describe<T>(x: T): String where T: Display = x.format()

function main(): Unit =
    assert describe<Int32>(42) == "42"
"#,
    )
    .expect("trait bound should be satisfied for Int32 + Display");
}

#[test]
fn test_where_clause_bound_satisfied_inferred() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

function describe<T>(x: T): String where T: Display = x.format()

function main(): Unit =
    assert describe(42) == "42"
"#,
    )
    .expect("trait bound should be satisfied for inferred Int32 + Display");
}

#[test]
fn test_where_clause_bound_violation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Serializable =
    function serialize(self: Self): String

function toJson<T>(x: T): String where T: Serializable = x.serialize()

function main(): Unit =
    let s = toJson(42)
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected 'does not implement trait' error, got: {:?}",
        errors
    );
}

#[test]
fn test_where_clause_with_user_trait() {
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

implement Describable for Int32 =
    function describe(self: Int32): String = "an integer"

function getDescription<T>(x: T): String where T: Describable = x.describe()

function main(): Unit =
    assert getDescription(42) == "an integer"
"#,
    )
    .expect("user-defined trait bound should work");
}

#[test]
fn test_where_clause_equatable_bound() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Equatable

function areEqual<T>(a: T, b: T): Bool where T: Equatable = a.equals(b)

function main(): Unit =
    assert areEqual(1, 1)
    let result = areEqual(1, 2)
    assert result == false
"#,
    )
    .expect("Equatable bound with method call should work");
}

#[test]
fn test_where_clause_multiple_type_params_with_bounds() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display
import standard.prelude.Equatable

function checkAndDescribe<T, U>(x: T, y: U): String where T: Equatable, U: Display =
    y.format()

function main(): Unit =
    assert checkAndDescribe(1, 42) == "42"
"#,
    )
    .expect("multiple type params with different bounds should work");
}

#[test]
fn test_where_clause_display_bool() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

function show<T>(x: T): String where T: Display = x.format()

function main(): Unit =
    assert show(true) == "true"
    assert show(false) == "false"
"#,
    )
    .expect("Display bound for Bool should work");
}

#[test]
fn test_where_clause_display_string_typecheck() {
    // String goes through the shared generic path; codegen for shared trait dispatch
    // is deferred to a later phase. For now, verify type-checking passes.
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display

function show<T>(x: T): String where T: Display = x.format()

function main(): Unit =
    let s = show("hello")
"#,
    );
}

// ── where clause on records ──────────────────────────────────────────

#[test]
fn test_record_where_clause_valid() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

record Wrapper<T> where T: Display =
    value: T

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert w.value == 42
"#,
    )
    .expect("generic record with satisfied where clause should compile and run");
}

#[test]
fn test_record_where_clause_invalid() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Serializable =
    function serialize(self: Int32): String

record Container<T> where T: Serializable =
    item: T

record Dummy

function main(): Unit =
    let c = Container<Dummy> { item = Dummy {} }
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected trait bound violation error, got: {:?}",
        errors
    );
}

#[test]
fn test_record_where_clause_multiple_bounds() {
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display

trait Hashable =
    function hash(self: Int32): Int32

implement Hashable for Int32 =
    function hash(self: Int32): Int32 = self

record KeyValue<K, V> where K: Hashable, V: Display =
    key: K
    value: V

function main(): Unit =
    let kv = KeyValue<Int32, Int32> { key = 1; value = 42 }
    assert kv.key == 1
"#,
    );
}

#[test]
fn test_record_where_clause_type_annotation() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

record Box<T> where T: Display =
    value: T

function main(): Unit =
    let b: Box<Int32> = Box<Int32> { value = 10 }
    assert b.value == 10
"#,
    )
    .expect("generic record type annotation with satisfied where clause should work");
}

// ── where clause on enums ────────────────────────────────────────────

#[test]
fn test_enum_where_clause_parses() {
    common::check_no_errors(
        r#"
package a

import standard.prelude.Display

enum Result<T> where T: Display =
    Ok(T)
    Err(String)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_record_where_clause_nonexistent_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import standard.prelude.Display

record Pair<A, B> where C: Display =
    first: A
    second: B

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not declared on this item")),
        "expected 'not declared on this item' error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_module_inherits_record_trait_bounds_valid() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

record Box<T> where T: Display =
    value: T

module Box<T> =
    function describe(self: Box<T>): String = self.value.format()

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.describe() == "42"
"#,
    )
    .expect("generic module inherits record trait bounds");
}

#[test]
fn test_generic_module_inherits_record_trait_bounds_invalid() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import standard.prelude.Display

record NoDisplay =
    x: Int32

record Box<T> where T: Display =
    value: T

module Box<T> =
    function wrap(x: T): Box<T> = Box<T> { value = x }

function main(): Unit =
    let b = Box.wrap(NoDisplay { x = 1 })
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("Display")),
        "expected trait bound error mentioning 'Display', got: {:?}",
        errors
    );
}

#[test]
fn test_generic_extension_where_clause_valid() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display
import a.BoxHelper

record Box<T> where T: Display =
    value: T

extension BoxHelper<T> for Box<T> where T: Display =
    function describe(self: Box<T>): String = self.value.format()

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.describe() == "42"
"#,
    )
    .expect("generic extension with matching where clause");
}

#[test]
fn test_generic_extension_missing_where_clause_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import standard.prelude.Display
import a.BoxHelper

record Box<T> where T: Display =
    value: T

extension BoxHelper<T> for Box<T> =
    function describe(self: Box<T>): String = self.value.format()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("must include trait bound") && e.contains("Display")),
        "expected error about missing trait bound, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_extension_bounds_violation_at_call_site() {
    let errors = common::compile_expecting_errors(
        r#"
package a

import standard.prelude.Display
import a.BoxHelper

record NoDisplay =
    x: Int32

record Box<T> where T: Display =
    value: T

extension BoxHelper<T> for Box<T> where T: Display =
    function describe(self: Box<T>): String = self.value.format()

function main(): Unit =
    let b = Box<NoDisplay> { value = NoDisplay { x = 1 } }
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("Display")),
        "expected trait bound violation error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_extension_extra_bounds_ok() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display
import standard.prelude.Equatable
import a.BoxHelper

record Box<T> where T: Display =
    value: T

extension BoxHelper<T> for Box<T> where T: Display + Equatable =
    function describe(self: Box<T>): String = self.value.format()

function main(): Unit =
    let b = Box<Int32> { value = 42 }
    assert b.describe() == "42"
"#,
    )
    .expect("generic extension with extra bounds is ok");
}

// ── Generic methods in non-generic traits (Phase 4) ──────────────────

#[test]
fn test_implement_generic_method_type_param_count_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

record Box =
    x: Int32

implement Mapper for Box =
    function map<A, B>(self: Box, value: A): A = value

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type parameter(s)")),
        "expected type param count mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_implement_generic_method_valid() {
    common::check_no_errors(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

record Box =
    x: Int32

implement Mapper for Box =
    function map<U>(self: Box, value: U): U = value

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_call_generic_trait_method_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

record Box =
    x: Int32

implement Mapper for Box =
    function map<U>(self: Box, value: U): U = value

function main(): Unit =
    let b = Box { x = 1 }
    assert b.map<Int32>(42) == 42
"#,
    )
    .expect("generic trait method with explicit type args");
}

#[test]
fn test_call_generic_trait_method_inferred() {
    common::compile_and_run(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

record Box =
    x: Int32

implement Mapper for Box =
    function map<U>(self: Box, value: U): U = value

function main(): Unit =
    let b = Box { x = 1 }
    let result: Int32 = b.map(42)
    assert result == 42
"#,
    )
    .expect("generic trait method with inferred type args");
}

#[test]
fn test_generic_trait_method_with_where_clause() {
    common::compile_and_run(
        r#"
package a

import standard.prelude.Display

trait Formatter =
    function fmt<U>(self: Self, value: U): String where U: Display

record Printer =
    prefix: String

implement Formatter for Printer =
    function fmt<U>(self: Printer, value: U): String where U: Display = value.format()

function main(): Unit =
    let p = Printer { prefix = ">" }
    assert p.fmt<Int32>(42) == "42"
"#,
    )
    .expect("generic trait method with where clause");
}

#[test]
fn test_generic_trait_method_different_type_param_names() {
    common::compile_and_run(
        r#"
package a

trait Transform =
    function apply<T>(self: Self, value: T): T

record Identity =
    id: Int32

implement Transform for Identity =
    function apply<V>(self: Identity, value: V): V = value

function main(): Unit =
    let i = Identity { id = 1 }
    assert i.apply<Int32>(99) == 99
"#,
    )
    .expect("generic trait method with different type param names in impl");
}

// ── Generic traits (Phase 5) ─────────────────────────────────────────

#[test]
fn test_generic_trait_concrete_impl() {
    common::compile_and_run(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From<Int32> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

function main(): Unit =
    let r = MyRecord.from(42)
    assert r.x == 42
"#,
    )
    .expect("generic trait concrete impl should work");
}

#[test]
fn test_generic_trait_instance_method() {
    common::compile_and_run(
        r#"
package a

trait Converter<T> =
    function convert(self: Self): T

record Wrapper = value: Int32

implement Converter<Int32> for Wrapper =
    function convert(self: Wrapper): Int32 = self.value

function main(): Unit =
    let w = Wrapper { value = 99 }
    assert w.convert() == 99
"#,
    )
    .expect("generic trait instance method should work");
}

#[test]
fn test_generic_trait_missing_type_args() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type parameter") && e.contains("no type arguments")),
        "expected missing type args error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_trait_wrong_type_arg_count() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From<Int32, Bool> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expects 1 type argument") && e.contains("2 were provided")),
        "expected type arg count mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_trait_extra_type_args_on_non_generic() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Display =
    function format(self: Self): String

record MyRecord = x: Int32

implement Display<Int32> for MyRecord =
    function format(self: MyRecord): String = "record"

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("no type parameters") && e.contains("type argument")),
        "expected extra type args error on non-generic trait, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_trait_multiple_impls_same_type() {
    common::compile_and_run(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From<Int32> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

implement From<Bool> for MyRecord =
    function from(value: Bool): MyRecord =
        if value then MyRecord { x = 1 }
        else MyRecord { x = 0 }

function main(): Unit =
    let r1 = MyRecord.from(42)
    assert r1.x == 42
    let r2 = MyRecord.from(true)
    assert r2.x == 1
    let r3 = MyRecord.from(false)
    assert r3.x == 0
"#,
    )
    .expect("multiple generic trait impls for same type should work");
}

#[test]
fn test_generic_trait_duplicate_impl_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From<Int32> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

implement From<Int32> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("already implements")),
        "expected duplicate impl error, got: {:?}",
        errors
    );
}

// ── Generic implement blocks (Phase 6) ───────────────────────────────

#[test]
fn test_generic_impl_block_non_generic_trait() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

record Wrapper<T> =
    value: T

implement <T> Display for Wrapper<T> =
    function format(self: Wrapper<T>): String = "wrapper"

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert w.format() == "wrapper"
"#,
    )
    .expect("generic impl block for non-generic trait");
}

#[test]
fn test_generic_impl_block_generic_trait() {
    common::compile_and_run(
        r#"
package a

trait Converter<T> =
    function convert(self: Self): T

record Box<T> =
    value: T

implement <T> Converter<T> for Box<T> =
    function convert(self: Box<T>): T = self.value

function main(): Unit =
    let b = Box<Int32> { value = 99 }
    assert b.convert() == 99
"#,
    )
    .expect("generic impl block for generic trait");
}

#[test]
fn test_generic_impl_block_call_site_multiple_types() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

record Wrapper<T> =
    value: T

implement <T> Display for Wrapper<T> =
    function format(self: Wrapper<T>): String = "wrapper"

function main(): Unit =
    let w1 = Wrapper<Int32> { value = 42 }
    let w2 = Wrapper<Bool> { value = true }
    assert w1.format() == "wrapper"
    assert w2.format() == "wrapper"
"#,
    )
    .expect("generic impl block called with multiple concrete types");
}

#[test]
fn test_generic_impl_block_where_clause() {
    common::compile_and_run(
        r#"
package a

trait Showable =
    function show(self: Self): String

implement Showable for Int32 =
    function show(self: Int32): String = "int"

record Wrapper<T> =
    value: T

implement <T> Showable for Wrapper<T> where T: Showable =
    function show(self: Wrapper<T>): String = self.value.show()

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert w.show() == "int"
"#,
    )
    .expect("generic impl block with where clause");
}

#[test]
fn test_generic_impl_block_bound_violation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Display =
    function format(self: Self): String

record Wrapper<T> =
    value: T

implement <T> Display for Wrapper<T> where T: Display =
    function format(self: Wrapper<T>): String = "wrapper"

record NoDisplay = x: Int32

function main(): Unit =
    let w = Wrapper<NoDisplay> { value = NoDisplay { x = 1 } }
    let result = w.format()
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for bound violation, got no errors"
    );
}

#[test]
fn test_generic_impl_block_with_generic_method() {
    common::compile_and_run(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

record Wrapper<T> =
    value: T

implement <T> Mapper for Wrapper<T> =
    function map<U>(self: Wrapper<T>, value: U): U = value

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert w.map<Int32>(99) == 99
"#,
    )
    .expect("generic impl block with method-level type params");
}

#[test]
fn test_generic_impl_block_parse_where_clause() {
    common::check_no_errors(
        r#"
package a

trait Display =
    function format(self: Self): String

trait Printable =
    function print(self: Self): String

record Pair<A, B> =
    first: A
    second: B

implement <A, B> Display for Pair<A, B> where A: Display, B: Display =
    function format(self: Pair<A, B>): String = "pair"

function main(): Unit = ()
"#,
    );
}

// ── Phase 7: Trait object types ─────────────────────────────────────

#[test]
fn test_trait_object_type_in_param() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

function foo(x: Display): Unit = panic "not yet"

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_type_in_let() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

function main(): Unit =
    let x: Display = panic "not yet"
"#,
    );
}

#[test]
fn test_trait_object_type_in_return() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

function foo(): Display = panic "not yet"

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_type_generic_trait() {
    common::check_no_errors(
        r#"
package a

interface Converter<T> =
    function convert(self: Self): T

function foo(x: Converter<Int32>): Unit = panic "not yet"

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_type_unknown_trait_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function foo(x: NonExistentTrait): Unit = panic "not yet"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unknown type")),
        "expected 'unknown type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_and_keyword_parses() {
    // Intersection components must be interfaces: plain traits in an
    // intersection hit the type-position gate.
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Display =
    function format(self: Self): String

trait Equatable =
    function equals(self: Self, other: Self): Bool

function foo(x: Display and Equatable): Unit = panic "not yet"

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_impl_where_clause_satisfied() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

record Inner =
    value: Int32

implement Display for Inner =
    function format(self: Inner): String = "inner"

record Wrapper<T> =
    inner: T

implement <T> Display for Wrapper<T> where T: Display =
    function format(self: Wrapper<T>): String = self.inner.format()

function main(): Unit =
    let w = Wrapper<Inner> { inner = Inner { value = 42 } }
    let s = w.format()
    assert s == "inner"
"#,
    )
    .expect("generic impl where clause satisfied");
}

#[test]
fn test_generic_impl_where_clause_not_satisfied() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Display =
    function format(self: Self): String

record NoDisplay =
    value: Int32

record Wrapper<T> =
    inner: T

implement <T> Display for Wrapper<T> where T: Display =
    function format(self: Wrapper<T>): String = "wrapper"

function main(): Unit =
    let w = Wrapper<NoDisplay> { inner = NoDisplay { value = 42 } }
    let s = w.format()
    ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("NoDisplay") && e.contains("Display")),
        "expected error about NoDisplay not implementing Display, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_impl_multiple_instantiations() {
    common::compile_and_run(
        r#"
package a

trait Display =
    function format(self: Self): String

record A =
    x: Int32

implement Display for A =
    function format(self: A): String = "A"

record B =
    y: Int32

implement Display for B =
    function format(self: B): String = "B"

record Wrapper<T> =
    inner: T

implement <T> Display for Wrapper<T> where T: Display =
    function format(self: Wrapper<T>): String = self.inner.format()

function main(): Unit =
    let wa = Wrapper<A> { inner = A { x = 1 } }
    let wb = Wrapper<B> { inner = B { y = 2 } }
    assert wa.format() == "A"
    assert wb.format() == "B"
"#,
    )
    .expect("generic impl multiple instantiations");
}

#[test]
fn test_generic_impl_eq_operator() {
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    value: T

implement <T> Equatable for Wrapper<T> where T: Equatable =
    public function equals(self: Wrapper<T>, other: Wrapper<T>): Bool =
        self.value == other.value

function main(): Unit =
    let w1 = Wrapper<Int32> { value = 42 }
    let w2 = Wrapper<Int32> { value = 42 }
    let w3 = Wrapper<Int32> { value = 99 }
    assert w1 == w2
    assert (w1 == w3) == false
"#,
    )
    .expect("generic impl eq operator");
}

#[test]
fn test_generic_impl_ne_operator() {
    common::compile_and_run(
        r#"
package a

record Wrapper<T> =
    value: T

implement <T> Equatable for Wrapper<T> where T: Equatable =
    public function equals(self: Wrapper<T>, other: Wrapper<T>): Bool =
        self.value == other.value

function main(): Unit =
    let w1 = Wrapper<Int32> { value = 42 }
    let w2 = Wrapper<Int32> { value = 99 }
    assert w1 != w2
"#,
    )
    .expect("generic impl ne operator");
}

#[test]
fn test_generic_impl_eq_bound_violation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record NoEq =
    x: Int32

record Wrapper<T> =
    value: T

implement <T> Equatable for Wrapper<T> where T: Equatable =
    public function equals(self: Wrapper<T>, other: Wrapper<T>): Bool = true

function main(): Unit =
    let w1 = Wrapper<NoEq> { value = NoEq { x = 1 } }
    let w2 = Wrapper<NoEq> { value = NoEq { x = 1 } }
    let _ = w1 == w2
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for bound violation, got none"
    );
}

#[test]
fn test_generic_impl_eq_bound_satisfied_multiple_params() {
    common::compile_and_run(
        r#"
package a

record Pair<A, B> =
    first: A
    second: B

implement <A, B> Equatable for Pair<A, B> where A: Equatable, B: Equatable =
    public function equals(self: Pair<A, B>, other: Pair<A, B>): Bool =
        if self.first == other.first then self.second == other.second else false

function main(): Unit =
    let p1 = Pair<Int32, String> { first = 1; second = "hi" }
    let p2 = Pair<Int32, String> { first = 1; second = "hi" }
    let p3 = Pair<Int32, String> { first = 2; second = "hi" }
    assert p1 == p2
    assert p1 != p3
"#,
    )
    .expect("generic impl eq with multiple type params");
}

// ── Phase 8: Trait Object Coercion and Method Resolution ─────────────

#[test]
fn test_trait_object_coercion_let_binding() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let d: Display = p
"#,
    );
}

#[test]
fn test_trait_object_coercion_function_arg() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function show(d: Display): Unit = ()

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    show(p)
"#,
    );
}

#[test]
fn test_trait_object_coercion_primitive() {
    common::check_no_errors(
        r#"
package a

interface Fmt =
    function fmt(self): String

implement Fmt for Int32 =
    function fmt(self): String = "${self}"

function show(d: Fmt): Unit = ()

function main(): Unit =
    show(42)
"#,
    );
}

#[test]
fn test_trait_object_coercion_generic_trait() {
    common::check_no_errors(
        r#"
package a

interface Producer<T> =
    function produce(self): T

record MyRecord = x: Int32

implement Producer<Int32> for MyRecord =
    function produce(self): Int32 = self.x

function accept(f: Producer<Int32>): Unit = ()

function main(): Unit =
    let r = MyRecord { x = 42 }
    accept(r)
"#,
    );
}

#[test]
fn test_trait_object_coercion_return_type() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function makeDisplay(): Display =
    let p = Point { x = 1; y = 2 }
    p

function main(): Unit =
    let d = makeDisplay()
"#,
    );
}

#[test]
fn test_trait_object_coercion_missing_impl_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let d: Display = p
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_coercion_wrong_trait_type_args_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record MyRecord = x: Int32

implement From<Int32> for MyRecord =
    function from(value: Int32): MyRecord = MyRecord { x = value }

function accept(f: From<String>): Unit = ()

function main(): Unit =
    let r = MyRecord { x = 42 }
    accept(r)
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for wrong trait type args, got no errors"
    );
}

#[test]
fn test_trait_object_method_call_simple() {
    common::check_no_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function show(d: Display): String = d.format()

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let d: Display = p
    let s = d.format()
"#,
    );
}

#[test]
fn test_trait_object_method_call_generic_trait() {
    common::check_no_errors(
        r#"
package a

interface Converter<T> =
    function convert(self: Self): T

record Wrapper = value: Int32

implement Converter<Int32> for Wrapper =
    function convert(self: Wrapper): Int32 = self.value

function extract(c: Converter<Int32>): Int32 = c.convert()

function main(): Unit =
    let w = Wrapper { value = 99 }
    let c: Converter<Int32> = w
    let v = c.convert()
"#,
    );
}

#[test]
fn test_trait_object_method_call_multi_arg() {
    // Methods with Self-typed non-self parameters are not object-safe
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Equatable =
    function equals(self: Self, other: Self): Bool

record Point =
    x: Int32
    y: Int32

implement Equatable for Point =
    function equals(self: Point, other: Point): Bool = self.x == other.x

function check(e: Equatable): Bool =
    e.equals(e)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_method_not_found_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

function show(d: Display): String = d.nonExistent()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method 'nonExistent'")),
        "expected no method error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_method_wrong_arg_count_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Display =
    function format(self: Self): String

function show(d: Display): String = d.format(42)

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("expects 0 argument")),
        "expected wrong arg count error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_property_access() {
    common::check_no_errors(
        r#"
package a

interface HasName =
    property name(self: Self): String

record Person =
    n: String

implement HasName for Person =
    property name(self: Person): String = self.n

function getName(h: HasName): String = h.name

function main(): Unit = ()
"#,
    );
}

// ── Generic trait bound type args ──────────────────────────────────────

#[test]
fn test_generic_trait_bound_with_type_args_satisfied() {
    common::compile_and_run(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record Wrapper =
    value: Int32

implement From<Int32> for Wrapper =
    function from(value: Int32): Wrapper = Wrapper { value = value }

function convert<T>(value: Int32): T where T: From<Int32> = T.from(value)

function main(): Unit =
    let w = convert<Wrapper>(42)
    assert w.value == 42
"#,
    )
    .expect("generic trait bound with type args satisfied");
}

#[test]
fn test_generic_trait_bound_with_type_args_rejected() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record Wrapper =
    value: Int32

implement From<String> for Wrapper =
    function from(value: String): Wrapper = Wrapper { value = 0 }

function convert<T>(value: Int32): T where T: From<Int32> = T.from(value)

function main(): Unit =
    let w = convert<Wrapper>(42)
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for trait bound type arg mismatch"
    );
    assert!(
        errors.iter().any(|e| e.contains("From<Int32>")),
        "error should mention From<Int32>, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_trait_bound_bare_name_backward_compat() {
    common::compile_and_run(
        r#"
package a

trait From<T> =
    function from(value: T): Self

record Wrapper =
    value: Int32

implement From<Int32> for Wrapper =
    function from(value: Int32): Wrapper = Wrapper { value = value }

function convert<T>(value: Int32): T where T: From = T.from(value)

function main(): Unit =
    let w = convert<Wrapper>(42)
    assert w.value == 42
"#,
    )
    .expect("bare trait name (no type args) still works");
}

#[test]
fn test_generic_impl_bound_with_type_args_recursive_check() {
    common::compile_and_run(
        r#"
package a

trait Showable =
    function show(self: Self): String

trait From<T> =
    function from(value: T): Self

record Wrapper =
    value: Int32

implement Showable for Wrapper =
    function show(self: Wrapper): String = "wrapper"

implement From<Int32> for Wrapper =
    function from(value: Int32): Wrapper = Wrapper { value = value }

record Box<T> =
    inner: T

implement <T> Showable for Box<T> where T: Showable =
    function show(self: Box<T>): String = self.inner.show()

function wrapAndShow<T>(v: T): String where T: Showable + From<Int32> =
    v.show()

function main(): Unit =
    let w = Wrapper { value = 42 }
    assert wrapAndShow<Wrapper>(w) == "wrapper"
"#,
    )
    .expect("multiple trait bounds with type args in where clause");
}

#[test]
fn test_generic_trait_bound_cross_reference_type_params() {
    common::check_no_errors(
        r#"
package a

trait From<T> =
    function from(value: T): Self

function convert<T, U>(value: T): U where U: From<T> = U.from(value)

function main(): Unit = ()
"#,
    );
}

// ── Phase 9: Trait Object Codegen ────────────────────────────────────

#[test]
fn test_trait_object_codegen_basic_method_call() {
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function show(d: Display): String = d.format()

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    assert show(p) == "point"
"#,
    )
    .expect("basic trait object coercion and method call");
}

#[test]
fn test_trait_object_codegen_primitive_int32() {
    common::compile_and_run(
        r#"
package a

interface Fmt =
    function fmt(self): String

implement Fmt for Int32 =
    function fmt(self): String = "${self}"

function show(d: Fmt): String = d.fmt()

function main(): Unit =
    assert show(42) == "42"
"#,
    )
    .expect("interface object with primitive Int32");
}

#[test]
fn test_trait_object_codegen_primitive_bool() {
    common::compile_and_run(
        r#"
package a

interface Fmt =
    function fmt(self): String

implement Fmt for Bool =
    function fmt(self): String = "${self}"

function show(d: Fmt): String = d.fmt()

function main(): Unit =
    assert show(true) == "true"
    assert show(false) == "false"
"#,
    )
    .expect("interface object with primitive Bool");
}

#[test]
fn test_trait_object_codegen_string() {
    common::compile_and_run(
        r#"
package a

interface Fmt =
    function fmt(self): String

implement Fmt for String =
    function fmt(self): String = self

function show(d: Fmt): String = d.fmt()

function main(): Unit =
    assert show("hello") == "hello"
"#,
    )
    .expect("interface object with String");
}

#[test]
fn test_trait_object_codegen_let_binding() {
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let d: Display = p
    assert d.format() == "point"
"#,
    )
    .expect("trait object via let binding");
}

#[test]
fn test_trait_object_codegen_return_type() {
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32
    y: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function makeDisplay(): Display =
    let p = Point { x = 1; y = 2 }
    p

function main(): Unit =
    let d = makeDisplay()
    assert d.format() == "point"
"#,
    )
    .expect("trait object as return type");
}

#[test]
fn test_trait_object_codegen_multiple_types_same_trait() {
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point = x: Int32

implement Display for Point =
    function format(self: Point): String = "point"

record Circle = r: Int32

implement Display for Circle =
    function format(self: Circle): String = "circle"

function show(d: Display): String = d.format()

function main(): Unit =
    assert show(Point { x = 1 }) == "point"
    assert show(Circle { r = 5 }) == "circle"
"#,
    )
    .expect("trait object with multiple concrete types");
}

#[test]
fn test_trait_object_codegen_property() {
    common::compile_and_run(
        r#"
package a

interface HasName =
    property name(self: Self): String

record Person =
    n: String

implement HasName for Person =
    property name(self: Person): String = self.n

function getName(h: HasName): String = h.name

function main(): Unit =
    let p = Person { n = "Alice" }
    assert getName(p) == "Alice"
"#,
    )
    .expect("trait object property access");
}

#[test]
fn test_trait_object_codegen_generic_trait() {
    common::compile_and_run(
        r#"
package a

interface Converter<T> =
    function convert(self: Self): T

record Wrapper = value: Int32

implement Converter<Int32> for Wrapper =
    function convert(self: Wrapper): Int32 = self.value

function extract(c: Converter<Int32>): Int32 = c.convert()

function main(): Unit =
    let w = Wrapper { value = 99 }
    assert extract(w) == 99
"#,
    )
    .expect("trait object with generic trait");
}

#[test]
fn test_trait_object_codegen_generic_impl_block() {
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Wrapper<T> =
    value: T

implement <T> Display for Wrapper<T> =
    function format(self: Wrapper<T>): String = "wrapper"

function show(d: Display): String = d.format()

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert show(w) == "wrapper"
"#,
    )
    .expect("trait object with generic impl block");
}

#[test]
fn test_trait_object_codegen_multiple_methods() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String
    function tag(self: Self): Int32

record Widget =
    name: String
    id: Int32

implement Describable for Widget =
    function describe(self: Widget): String = self.name
    function tag(self: Widget): Int32 = self.id

function test(d: Describable): String = d.describe()
function testTag(d: Describable): Int32 = d.tag()

function main(): Unit =
    let w = Widget { name = "button"; id = 7 }
    assert test(w) == "button"
    assert testTag(w) == 7
"#,
    )
    .expect("trait object with multiple methods");
}

#[test]
fn test_trait_object_not_object_safe_self_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Combinable =
    function combine(self: Self, other: Self): Self

record Pair =
    x: Int32

implement Combinable for Pair =
    function combine(self: Pair, other: Pair): Pair = other

function use_combinable(c: Combinable): Combinable = c.combine(c)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_not_object_safe_self_in_array_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Mergeable =
    function merge(self: Self, others: Array<Self>): Self

record Item =
    name: String

implement Mergeable for Item =
    function merge(self: Item, others: Array<Item>): Item = self

function use_mergeable(m: Mergeable, items: Array<Item>): Mergeable = m.merge(items)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("parameter of type 'Self'")),
        "expected Self-parameter declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_object_self_return_type_allowed() {
    // Self as return type should be allowed — the trait object method can
    // return the trait object type. We just need the vtable to include it.
    common::compile_and_run(
        r#"
package a

interface Display =
    function format(self: Self): String

record Point =
    x: Int32

implement Display for Point =
    function format(self: Point): String = "point"

function show(d: Display): String = d.format()

function main(): Unit =
    let p = Point { x = 1 }
    assert show(p) == "point"
"#,
    )
    .expect("Self return type should be allowed on trait objects");
}

// =============================================================================
// Associated Types
// =============================================================================

#[test]
fn test_associated_type_basic() {
    common::check_no_errors(
        r#"
package a

record MyRec =
    x: Int32

trait HasOutput =
    type Output
    function produce(self: Self): Output

implement HasOutput for MyRec =
    type Output = Int32
    function produce(self: MyRec): Int32 = self.x

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_associated_type_method_uses_associated_type() {
    common::compile_and_run(
        r#"
package a

record MyRec =
    x: Int32

trait HasOutput =
    type Output
    function produce(self: Self): Output

implement HasOutput for MyRec =
    type Output = Int32
    function produce(self: MyRec): Int32 = self.x

function main(): Unit =
    let r = MyRec { x = 42 }
    assert r.produce() == 42
"#,
    )
    .expect("associated type in method signature");
}

#[test]
fn test_associated_type_missing_in_impl() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record MyRec =
    x: Int32

trait HasOutput =
    type Output
    function produce(self: Self): Output

implement HasOutput for MyRec =
    function produce(self: MyRec): Int32 = self.x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("missing associated type definition 'Output'")),
        "expected missing associated type error, got: {:?}",
        errors
    );
}

#[test]
fn test_associated_type_extra_in_impl() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record MyRec =
    x: Int32

trait NoAssoc =
    function foo(self: Self): Int32

implement NoAssoc for MyRec =
    type Output = Int32
    function foo(self: MyRec): Int32 = self.x

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("is not declared in trait")),
        "expected extra associated type error, got: {:?}",
        errors
    );
}

#[test]
fn test_associated_type_duplicate_in_trait() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait BadTrait =
    type Foo
    type Foo
    function bar(self: Self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("duplicate associated type")),
        "expected duplicate associated type error, got: {:?}",
        errors
    );
}

#[test]
fn test_associated_type_with_generic_trait() {
    common::compile_and_run(
        r#"
package a

record MyRec =
    x: Int32

trait Transformer<T> =
    type Output
    function transform(self: Self, input: T): Output

implement Transformer<Int32> for MyRec =
    type Output = String
    function transform(self: MyRec, input: Int32): String = "hello"

function main(): Unit =
    let r = MyRec { x = 1 }
    assert r.transform(42) == "hello"
"#,
    )
    .expect("associated type with generic trait");
}

#[test]
fn test_associated_type_in_generic_impl_block() {
    common::check_no_errors(
        r#"
package a

record Wrapper<T> =
    value: T

trait HasInner<T> =
    type Inner
    function getInner(self: Self): Inner

implement <T> HasInner<T> for Wrapper<T> =
    type Inner = T
    function getInner(self: Wrapper<T>): T = self.value

function main(): Unit =
    let w = Wrapper<Int32> { value = 42 }
    assert w.getInner() == 42
"#,
    );
}

#[test]
fn test_associated_type_trait_bound_violation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

record MyRec =
    x: Int32

trait HasOutput =
    type Output
    function produce(self: Self): Output

function getOutput<T>(value: T): Int32 where T: HasOutput = value.produce()

function main(): Unit =
    let r = MyRec { x = 1 }
    let _ = getOutput(r)
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement trait")),
        "expected trait bound violation error, got: {:?}",
        errors
    );
}

#[test]
fn test_associated_type_multiple_impls_with_trait_bound() {
    common::compile_and_run(
        r#"
package a

record Dog =
    name: String

record Cat =
    name: String

trait HasSound =
    type Info
    function sound(self: Self): String

implement HasSound for Dog =
    type Info = String
    function sound(self: Dog): String = "woof"

implement HasSound for Cat =
    type Info = Int32
    function sound(self: Cat): String = "meow"

function makeSound<T>(animal: T): String where T: HasSound = animal.sound()

function main(): Unit =
    assert makeSound(Dog { name = "Rex" }) == "woof"
    assert makeSound(Cat { name = "Whiskers" }) == "meow"
"#,
    )
    .expect("trait bound should work with multiple impls having different associated types");
}

// ── Phase 11: Generic Methods on Trait-Bound Type Parameters ────────

#[test]
fn test_trait_bound_generic_method_inferred() {
    common::check_no_errors(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

function applyMap<T>(value: T, x: Int32): Int32 where T: Mapper =
    value.map(x)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_bound_generic_method_explicit_type_params() {
    common::check_no_errors(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

function applyMap<T>(value: T, x: Int32): Int32 where T: Mapper =
    value.map<Int32>(x)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_bound_generic_method_bidirectional() {
    common::check_no_errors(
        r#"
package a

trait Mapper =
    function map<U>(self: Self, value: U): U

function applyMap<T>(value: T, x: Int32): Int32 where T: Mapper =
    let result: Int32 = value.map(x)
    result

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_generic_trait_bound_generic_method_inferred() {
    common::check_no_errors(
        r#"
package a

trait Container<T> =
    function transform<U>(self: Self, value: U): U

function apply<T, C>(c: C, x: String): String where C: Container<T> =
    c.transform(x)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_generic_trait_bound_generic_method_explicit() {
    common::check_no_errors(
        r#"
package a

trait Container<T> =
    function transform<U>(self: Self, value: U): U

function apply<T, C>(c: C, x: String): String where C: Container<T> =
    c.transform<String>(x)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_associated_type_return_not_object_safe() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface HasOutput =
    type Output
    function produce(self: Self): Output

record Foo =
    x: Int32

implement HasOutput for Foo =
    type Output = Int32
    function produce(self: Foo): Int32 = self.x

function callProduce(obj: HasOutput): Unit =
    let x = obj.produce()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot declare associated type")),
        "expected associated-type declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_bound_associated_type_result_is_preserved() {
    common::check_no_errors(
        r#"
package a

trait HasOutput =
    type Output
    function produce(self: Self): Output

function callProduce<T>(value: T): Unit where T: HasOutput =
    let x = value.produce()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_associated_type_param_not_object_safe() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Processor =
    type Input
    function process(self: Self, input: Input): Int32

record Foo =
    x: Int32

implement Processor for Foo =
    type Input = Int32
    function process(self: Foo, input: Int32): Int32 = input

function callProcess(obj: Processor): Unit =
    let x = obj.process(42)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot declare associated type")),
        "expected associated-type declaration error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_with_associated_type_not_boxable() {
    // A trait with an associated type can never be an interface, so it is
    // bound-only: using it in type position errors at the use site.
    let errors = common::compile_expecting_errors(
        r#"
package a

trait HasOutput =
    type Output
    function describe(self: Self): String

record Foo =
    x: Int32

implement HasOutput for Foo =
    type Output = Int32
    function describe(self: Foo): String = "foo"

function callDescribe(obj: HasOutput): String =
    obj.describe()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_trait_bound_method_without_associated_type_still_works() {
    common::check_no_errors(
        r#"
package a

trait HasOutput =
    type Output
    function describe(self: Self): String

function callDescribe<T>(value: T): String where T: HasOutput =
    value.describe()

function main(): Unit = ()
"#,
    );
}

// --- Generic Associated Types (GATs) ---

#[test]
fn test_gat_declaration_and_impl_basic() {
    common::check_no_errors(
        r#"
package a

trait Transformer =
    type Output<U>
    function identity(self: Self): Int32

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<U> = Array<U>
    function identity(self: IntBox): Int32 = self.value

function main(): Unit =
    let b = IntBox { value = 42 }
    assert b.identity() == 42
"#,
    );
}

#[test]
fn test_gat_non_generic_associated_type_still_works() {
    common::check_no_errors(
        r#"
package a

trait HasOutput =
    type Output
    function get(self: Self): Output

record Wrapper = value: Int32

implement HasOutput for Wrapper =
    type Output = Int32
    function get(self: Wrapper): Int32 = self.value

function main(): Unit =
    let w = Wrapper { value = 10 }
    assert w.get() == 10
"#,
    );
}

#[test]
fn test_gat_type_param_count_mismatch_too_few() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Transformer =
    type Output<U>
    function identity(self: Self): Int32

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output = Int32
    function identity(self: IntBox): Int32 = self.value

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expects 1 type parameter(s), but 0 were provided")),
        "expected type param count mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_gat_type_param_count_mismatch_too_many() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Transformer =
    type Output
    function identity(self: Self): Int32

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<U> = Array<U>
    function identity(self: IntBox): Int32 = self.value

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("expects 0 type parameter(s), but 1 were provided")),
        "expected type param count mismatch error, got: {:?}",
        errors
    );
}

#[test]
fn test_gat_on_trait_object_blocked() {
    // A trait with a GAT + generic method can never be an interface, so it is
    // bound-only: using it in type position errors at the use site.
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Transformer =
    type Output<U>
    function transform<U>(self: Self, value: U): Output<U>

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<U> = Array<U>
    function transform<U>(self: IntBox, value: U): Array<U> = [|value|]

function callTransform(obj: Transformer): Unit =
    let x = obj.transform(42)
    ()

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot be used as a type")),
        "expected trait-in-type-position error, got: {:?}",
        errors
    );
}

#[test]
fn test_gat_on_trait_bound_preserves_abstract_result() {
    common::check_no_errors(
        r#"
package a

trait Transformer =
    type Output<U>
    function transform<U>(self: Self, value: U): Output<U>

function callTransform<T>(value: T): Unit where T: Transformer =
    let x = value.transform(42)
    ()

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_gat_methods_not_using_gat_still_work() {
    common::compile_and_run(
        r#"
package a

trait Transformer =
    type Output<U>
    function identity(self: Self): Int32

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<U> = Array<U>
    function identity(self: IntBox): Int32 = self.value

function main(): Unit =
    let b = IntBox { value = 7 }
    assert b.identity() == 7
"#,
    )
    .expect("methods not using GAT should work");
}

#[test]
fn test_gat_with_multiple_type_params() {
    common::check_no_errors(
        r#"
package a

trait BiMapper =
    type Mapped<U, V>
    function id(self: Self): Int32

record MyRec = value: Int32

implement BiMapper for MyRec =
    type Mapped<U, V> = (U, V)
    function id(self: MyRec): Int32 = self.value

function main(): Unit =
    let r = MyRec { value = 1 }
    assert r.id() == 1
"#,
    );
}

#[test]
fn test_gat_generic_impl_block() {
    common::check_no_errors(
        r#"
package a

trait Container =
    type Rebind<U>
    function size(self: Self): Int32

record Box<T> = value: T

implement <T> Container for Box<T> =
    type Rebind<U> = Box<U>
    function size(self: Box<T>): Int32 = 1

function main(): Unit =
    let b = Box { value = 42 }
    assert b.size() == 1
"#,
    );
}

#[test]
fn test_gat_impl_body_type_error_without_instantiation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

enum Option<T> =
    Some(T)
    None

trait Transformer =
    type Output<U>
    function transform<U>(self: Self, value: U): Output<U>

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<U> = Option<U>
    function transform<U>(self: IntBox, value: U): Option<U> =
        let x: Int32 = "hello world"
        None

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")
            || e.contains("expected 'Int32'")
            || e.contains("cannot assign")),
        "expected type error in impl body, got: {:?}",
        errors
    );
}

#[test]
fn test_gat_type_param_name_mismatch() {
    let errors = common::compile_expecting_errors(
        r#"
package a

trait Transformer =
    type Output<U>
    function identity(self: Self): Int32

record IntBox = value: Int32

implement Transformer for IntBox =
    type Output<V> = Array<V>
    function identity(self: IntBox): Int32 = self.value

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("type parameter name mismatch")
                && e.contains("expected 'U'")
                && e.contains("found 'V'")),
        "expected type param name mismatch error, got: {:?}",
        errors
    );
}

// ── EarlyReturn trait ───────────────────────────────────────────────

#[test]
fn test_early_return_result_unwrap_ok() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Ok(42)
    let result = r.unwrap()
    match result with
    case Ok(x) => assert x == 42
    case Error(_) => panic "expected Ok"
"#,
    )
    .expect("Result unwrap Ok");
}

#[test]
fn test_early_return_result_unwrap_error() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let r: Result<Int32, String> = Error("oops")
    let result = r.unwrap()
    match result with
    case Ok(_) => panic "expected Error"
    case Error(inner) =>
        match inner with
        case Error(e) => assert e == "oops"
        case Ok(_) => panic "expected inner Error"
"#,
    )
    .expect("Result unwrap Error");
}

#[test]
fn test_early_return_option_unwrap_some() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let o: Option<Int32> = Some(42)
    let result = o.unwrap()
    match result with
    case Ok(x) => assert x == 42
    case Error(_) => panic "expected Ok"
"#,
    )
    .expect("Option unwrap Some");
}

#[test]
fn test_early_return_option_unwrap_none() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let o: Option<Int32> = None
    let result = o.unwrap()
    match result with
    case Ok(_) => panic "expected Error"
    case Error(inner) =>
        match inner with
        case None => assert true
        case Some(_) => panic "expected None"
"#,
    )
    .expect("Option unwrap None");
}

// ── try/orReturn type checking ───────────────────────────────────────

#[test]
fn test_try_result_typechecks() {
    common::check_no_errors(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

function process(): Result<Bool, String> =
    let x: Int32 = try getResult()
    Ok(x == 42)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_or_return_result_typechecks() {
    common::check_no_errors(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

function process(): Result<Bool, String> =
    let x: Int32 = getResult().orReturn
    Ok(x == 42)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_try_option_typechecks() {
    common::check_no_errors(
        r#"
package a

function getOption(): Option<Int32> = Some(42)

function process(): Option<Bool> =
    let x: Int32 = try getOption()
    Some(x == 42)

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_try_wrong_return_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

function process(): Option<Int32> =
    let x: Int32 = try getResult()
    Some(x)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("not assignable to function return type")),
        "expected return type error, got: {:?}",
        errors
    );
}

#[test]
fn test_try_non_early_return_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function process(): Int32 =
    try 42

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("does not implement EarlyReturn")),
        "expected EarlyReturn error, got: {:?}",
        errors
    );
}

// ── try/orReturn runtime tests ──────────────────────────────────────

#[test]
fn test_try_result_runs() {
    common::compile_and_run(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

function process(): Result<Int32, String> =
    let x: Int32 = try getResult()
    Ok(x)

function unwrapOk(r: Result<Int32, String>): Int32 =
    match r with
        case Ok(v) => v
        case Error(_) => panic "expected Ok"

function main(): Unit =
    let r = process()
    assert unwrapOk(r) == 42
"#,
    )
    .expect("try Result should run");
}

#[test]
fn test_try_result_error_path() {
    common::compile_and_run(
        r#"
package a

function getResult(): Result<Int32, String> = Error("fail")

function process(): Result<Int32, String> =
    let x: Int32 = try getResult()
    Ok(x + 1)

function isError(r: Result<Int32, String>): Bool =
    match r with
        case Ok(_) => false
        case Error(_) => true

function main(): Unit =
    let r = process()
    assert isError(r)
"#,
    )
    .expect("try Result error path");
}

#[test]
fn test_or_return_result_runs() {
    common::compile_and_run(
        r#"
package a

function getResult(): Result<Int32, String> = Ok(42)

function process(): Result<Int32, String> =
    let x: Int32 = getResult().orReturn
    Ok(x)

function unwrapOk(r: Result<Int32, String>): Int32 =
    match r with
        case Ok(v) => v
        case Error(_) => panic "expected Ok"

function main(): Unit =
    let r = process()
    assert unwrapOk(r) == 42
"#,
    )
    .expect("orReturn Result should run");
}

#[test]
fn test_or_return_result_error_path() {
    common::compile_and_run(
        r#"
package a

function getResult(): Result<Int32, String> = Error("fail")

function process(): Result<Int32, String> =
    let x: Int32 = getResult().orReturn
    Ok(x + 1)

function isError(r: Result<Int32, String>): Bool =
    match r with
        case Ok(_) => false
        case Error(_) => true

function main(): Unit =
    let r = process()
    assert isError(r)
"#,
    )
    .expect("orReturn Result error path");
}

#[test]
fn test_or_return_option_runs() {
    common::compile_and_run(
        r#"
package a

function getOption(): Option<Int32> = Some(42)

function process(): Option<Int32> =
    let x: Int32 = getOption().orReturn
    Some(x + 1)

function unwrapSome(o: Option<Int32>): Int32 =
    match o with
        case Some(v) => v
        case None => panic "expected Some"

function main(): Unit =
    let r = process()
    assert unwrapSome(r) == 43
"#,
    )
    .expect("orReturn Option should run");
}

#[test]
fn test_or_return_option_none_path() {
    common::compile_and_run(
        r#"
package a

function getOption(): Option<Int32> = None

function process(): Option<Int32> =
    let x: Int32 = getOption().orReturn
    Some(x + 1)

function isNone(o: Option<Int32>): Bool =
    match o with
        case Some(_) => false
        case None => true

function main(): Unit =
    let r = process()
    assert isNone(r)
"#,
    )
    .expect("orReturn Option None path");
}

#[test]
fn test_try_multiple_in_sequence() {
    common::compile_and_run(
        r#"
package a

function a(): Result<Int32, String> = Ok(10)
function b(): Result<Int32, String> = Ok(20)

function process(): Result<Int32, String> =
    let x = try a()
    let y = try b()
    Ok(x + y)

function unwrapOk(r: Result<Int32, String>): Int32 =
    match r with
        case Ok(v) => v
        case Error(_) => panic "expected Ok"

function main(): Unit =
    let r = process()
    assert unwrapOk(r) == 30
"#,
    )
    .expect("multiple try in sequence");
}

#[test]
fn test_empty_marker_trait() {
    common::check_no_errors(
        r#"
package a

trait Marker

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_empty_implement_block() {
    common::check_no_errors(
        r#"
package a

trait Marker

record Foo

implement Marker for Foo

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_empty_implement_with_where_clause() {
    common::check_no_errors(
        r#"
package a

trait Marker

enum Box<out T> =
    Val(T)

implement <T> Marker for Box<T> where T : Marker

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_trait_object_codegen_generic_trait_two_instantiations() {
    common::compile_and_run(
        r#"
package a

interface Converter<T> =
    function convert(self: Self): T

record IntWrapper = value: Int32

implement Converter<Int32> for IntWrapper =
    function convert(self: IntWrapper): Int32 = self.value

record StringWrapper = label: String

implement Converter<String> for StringWrapper =
    function convert(self: StringWrapper): String = self.label

function extractInt(c: Converter<Int32>): Int32 = c.convert()
function extractString(c: Converter<String>): String = c.convert()

function main(): Unit =
    let iw = IntWrapper { value = 42 }
    let sw = StringWrapper { label = "hello" }
    assert extractInt(iw) == 42
    assert extractString(sw) == "hello"
"#,
    )
    .expect("two different instantiations of generic trait as trait objects");
}

#[test]
fn test_trait_object_generic_class_two_instantiations() {
    // Regression: a generic class implementing a generic trait, coerced to a trait object at two
    // different type args. Trait objects share one per-trait WASM type, but each generic-impl
    // *instantiation* needs its own vtable instance (keyed on the concrete type args, not the
    // erased class name) — otherwise the two iterators collide on one vtable and dispatch to the
    // wrong monomorphized `next`. Exercises `Array.iterator()` → `ArrayIterator<T>` (prelude).
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let ints = [|10, 20, 30|]
    let strs = [|"a", "b", "c"|]
    let intIter: Iterator<Int32> = ints.iterator()
    let strIter: Iterator<String> = strs.iterator()
    match intIter.next() with
        case Some(x) => assert x == 10
        case None => panic "ints empty"
    match strIter.next() with
        case Some(s) => assert s == "a"
        case None => panic "strs empty"
"#,
    )
    .expect("two generic-class iterator instantiations as trait objects");
}

#[test]
fn test_generic_method_on_trait_impl() {
    common::compile_and_run(
        r#"
package a

trait Mapper =
  function map<B>(self: Self, f: (Self) => B): B

record MyBox =
  value: Int32

implement Mapper for MyBox =
  function map<B>(self: MyBox, f: (MyBox) => B): B = f(self)

function main(): Unit =
  let b = MyBox { value = 42 }
  let result = b.map((x: MyBox) => x.value)
  assert result == 42
"#,
    )
    .expect("generic method on trait impl");
}

#[test]
fn test_generic_method_on_trait_bound() {
    common::compile_and_run(
        r#"
package a

trait Mapper =
  function map<B>(self: Self, f: (Self) => B): B

record MyBox =
  value: Int32

implement Mapper for MyBox =
  function map<B>(self: MyBox, f: (MyBox) => B): B = f(self)

function bar<A, B>(a: A, f: (A) => B): B where A: Mapper = a.map(f)

function main(): Unit =
  let b = MyBox { value = 42 }
  let result = bar<MyBox, Int32>(b, (x: MyBox) => x.value)
  assert result == 42
  let s = bar<MyBox, String>(b, (x: MyBox) => "hello")
  assert s == "hello"
"#,
    )
    .expect("generic method on trait bound");
}

#[test]
fn test_closure_body_coerced_to_trait_object_return() {
    // Regression: a closure whose body yields a class with a trait-object
    // return type must coerce the body to the trait object. The closure's
    // inferred return type was the bare class, so codegen built a closure
    // returning the class struct where the call boundary expected the
    // trait-object struct — a WASM `(ref $type)` mismatch. Fixed by widening
    // the closure return type to the expected one (infer_closure) and applying
    // return coercion to the closure body (coerce pass).
    common::compile_and_run(
        r#"
package a

interface Sink =
    function emit(self: Self): Int32

class FileThing(n: Int32) implements Sink =
    public function emit(self): Int32 = self.n

function applyMap<U>(f: (Int32) => U): U = f(7)

function applyFn(f: (Int32) => Sink): Sink = f(7)

function main(): Unit =
    // Non-generic: implicit coercion of the class body to the trait return.
    let implicit: Sink = applyFn((n: Int32) => FileThing(n))
    assert implicit.emit() == 7
    // Generic: explicit `as Sink` in the closure body.
    let explicit: Sink = applyMap<Sink>((n: Int32) => FileThing(n) as Sink)
    assert explicit.emit() == 7
"#,
    )
    .expect("closure body coerced to trait-object return type");
}

#[test]
fn test_closure_body_boxed_to_any_return() {
    // Same widening, but for a primitive body flowing into an `Any` return
    // slot — the body must be boxed.
    common::compile_and_run(
        r#"
package a

function applyFn(f: (Int32) => Any): Any = f(7)

function main(): Unit =
    let a: Any = applyFn((n: Int32) => n + 1)
    assert a as Int32 == 8
"#,
    )
    .expect("closure body boxed into Any return slot");
}
