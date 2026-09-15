mod common;

use std::fs;

use std::path::Path;

use dovetail::common::types::PackagePath;
use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;
use tempfile::tempdir;

/// Helper: typecheck a project, return error messages (empty = success).
fn typecheck_project(project: &ResolvedProject, workspace_root: &Path) -> Vec<String> {
    let result = dovetail::build_project(
        project,
        workspace_root,
        &Registry::new(),
        TypedModule::empty(),
        &dovetail::macros::MacroRegistry::new(),
        dovetail::BuildMode::Check, &std::collections::HashMap::new(), false,
    );
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == dovetail::common::diagnostics::Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

// --- Happy path tests (compile_and_run with assert) ---

#[test]
fn test_basic_class_declaration() {
    common::compile_and_run(
        r#"
package a

class Point(public x: Int32, public y: Int32)

function main(): Unit = ()
"#,
    )
    .expect("basic class declaration");
}

#[test]
fn test_class_construction() {
    common::compile_and_run(
        r#"
package a

class Point(public x: Int32, public y: Int32)

function main(): Unit =
    let p = Point(1, 2)
    ()
"#,
    )
    .expect("class construction");
}

#[test]
fn test_class_public_field_access() {
    common::compile_and_run(
        r#"
package a

class Point(public x: Int32, public y: Int32)

function main(): Unit =
    let p = Point(1, 2)
    assert p.x == 1
    assert p.y == 2
"#,
    )
    .expect("class field access");
}

#[test]
fn test_class_private_field_access_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Point(x: Int32, y: Int32)

function main(): Unit =
    let p = Point(1, 2)
    let a = p.x
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("private")),
        "expected private access error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_with_body_let_bindings() {
    common::compile_and_run(
        r#"
package a

class Rectangle(public width: Int32, public height: Int32) =
    public let area: Int32 = width * height

function main(): Unit =
    let r = Rectangle(3, 4)
    assert r.area == 12
"#,
    )
    .expect("class let bindings");
}

#[test]
fn test_class_instance_method() {
    common::compile_and_run(
        r#"
package a

class Counter(public value: Int32) =
    public function doubled(self: Counter): Int32 = self.value * 2

function main(): Unit =
    let c = Counter(5)
    assert c.doubled() == 10
"#,
    )
    .expect("class instance method");
}

#[test]
fn test_class_instance_method_accesses_constructor_params() {
    common::compile_and_run(
        r#"
package a

class Point(x: Int32, y: Int32) =
    public function sum(self: Point): Int32 = self.x + self.y

function main(): Unit =
    let p = Point(3, 4)
    assert p.sum() == 7
"#,
    )
    .expect("method accesses constructor params");
}

#[test]
fn test_class_static_method() {
    common::compile_and_run(
        r#"
package a

class Counter(public value: Int32) =
    public function zero(): Counter = Counter(0)

function main(): Unit =
    let c: Counter = Counter.zero()
    assert c.value == 0
"#,
    )
    .expect("class static method");
}

#[test]
fn test_class_static_factory_method() {
    common::compile_and_run(
        r#"
package a

class Point(public x: Int32, public y: Int32) =
    public function origin(): Point = Point(0, 0)

function main(): Unit =
    let p: Point = Point.origin()
    assert p.x == 0
    assert p.y == 0
"#,
    )
    .expect("class static factory method");
}

#[test]
fn test_class_as_function_param_and_return() {
    common::compile_and_run(
        r#"
package a

class Point(public x: Int32, public y: Int32)

function getX(p: Point): Int32 = p.x

function makePoint(): Point = Point(10, 20)

function main(): Unit =
    let p = makePoint()
    assert getX(p) == 10
"#,
    )
    .expect("class as function param and return");
}

#[test]
fn test_class_let_binding_type_inference() {
    common::compile_and_run(
        r#"
package a

class Wrapper(public value: Int32) =
    public let doubled = value * 2

function main(): Unit =
    let w = Wrapper(5)
    assert w.doubled == 10
"#,
    )
    .expect("class let binding type inference");
}

#[test]
fn test_class_multiple_methods() {
    common::compile_and_run(
        r#"
package a

class Calculator(public value: Int32) =
    public function add(self: Calculator, n: Int32): Int32 = self.value + n
    public function sub(self: Calculator, n: Int32): Int32 = self.value - n

function main(): Unit =
    let c = Calculator(10)
    assert c.add(5) == 15
    assert c.sub(3) == 7
"#,
    )
    .expect("class multiple methods");
}

#[test]
fn test_class_top_level_expression_in_body() {
    common::compile_and_run(
        r#"
package a

class Logger(public tag: String) =
    let prefix: String = tag

function main(): Unit =
    let l = Logger("test")
    ()
"#,
    )
    .expect("class top-level expression in body");
}

#[test]
fn test_class_constructor_visibility() {
    common::compile_and_run(
        r#"
package a

class Point public (public x: Int32, public y: Int32)

function main(): Unit =
    let p = Point(1, 2)
    assert p.x == 1
"#,
    )
    .expect("class constructor visibility");
}

#[test]
fn test_class_internal_field_access() {
    common::compile_and_run(
        r#"
package a

class Point(internal x: Int32, internal y: Int32)

function main(): Unit =
    let p = Point(1, 2)
    let a: Int32 = p.x
    assert a == 1
"#,
    )
    .expect("class internal field access");
}

#[test]
fn test_class_mutable_constructor_param() {
    common::compile_and_run(
        r#"
package a

class Counter(public mutable value: Int32)

function main(): Unit =
    let c = Counter(0)
    assert c.value == 0
"#,
    )
    .expect("class mutable constructor param");
}

#[test]
fn test_class_instance_method_accesses_let_bindings() {
    common::compile_and_run(
        r#"
package a

class Square(public side: Int32) =
    public let area: Int32 = side * side
    public function getArea(self: Square): Int32 = self.area

function main(): Unit =
    let s = Square(5)
    assert s.getArea() == 25
"#,
    )
    .expect("class instance method accesses let bindings");
}

// --- Non-public let binding inference tests ---

#[test]
fn test_class_private_let_binding_inferred_at_inference() {
    common::compile_and_run(
        r#"
package a

class Wrapper(public value: Int32) =
    let doubled = value * 2
    public function getDoubled(self: Wrapper): Int32 = self.doubled

function main(): Unit =
    let w = Wrapper(5)
    assert w.getDoubled() == 10
"#,
    )
    .expect("private let binding inferred at inference");
}

#[test]
fn test_class_private_let_binding_not_accessible_outside() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Wrapper(public value: Int32) =
    let doubled = value * 2

function main(): Unit =
    let w = Wrapper(5)
    let d = w.doubled
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("private")),
        "expected private access error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_internal_let_binding_accessible_same_package() {
    common::compile_and_run(
        r#"
package a

class Wrapper(public value: Int32) =
    internal let doubled: Int32 = value * 2

function main(): Unit =
    let w = Wrapper(5)
    assert w.doubled == 10
"#,
    )
    .expect("internal let binding accessible same package");
}

#[test]
fn test_class_mixed_public_private_let_bindings() {
    common::compile_and_run(
        r#"
package a

class Stats(public value: Int32) =
    let internal_cache = value * 2
    public let result: Int32 = value + 1

function main(): Unit =
    let s = Stats(10)
    assert s.result == 11
"#,
    )
    .expect("mixed public/private let bindings");
}

#[test]
fn test_class_private_let_binding_accessible_inside_class_method() {
    common::compile_and_run(
        r#"
package a

class Calculator(public value: Int32) =
    let cache = value * 2
    public function getCached(self: Calculator): Int32 = self.cache

function main(): Unit =
    let c = Calculator(5)
    assert c.getCached() == 10
"#,
    )
    .expect("private let binding accessible inside class method");
}

// --- Cross-file class access tests ---

#[test]
fn test_class_cross_file_public_field_access() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("point.dove"),
        r#"
package a

public class Point(public x: Int32, public y: Int32) =
    public let sum: Int32 = x + y
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let p = Point(3, 4)
    let s: Int32 = p.sum
    let a: Int32 = p.x
    ()
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".into()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".into()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };

    let errors = typecheck_project(&project, dir.path());
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
}

#[test]
fn test_class_cross_file_private_field_not_accessible() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("point.dove"),
        r#"
package a

public class Point(x: Int32, y: Int32)
"#,
    )
    .unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

function main(): Unit =
    let p = Point(1, 2)
    let a = p.x
    ()
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".into()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".into()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };

    let errors = typecheck_project(&project, dir.path());
    assert!(
        !errors.is_empty(),
        "expected error accessing private field, got no errors"
    );
}

// --- Parsing tests for inheritance syntax (Sub-step 2B) ---

#[test]
fn test_parse_final_class() {
    common::compile_and_run(
        r#"
package a

final class Immutable(public x: Int32)

function main(): Unit =
    let i = Immutable(42)
    assert i.x == 42
"#,
    )
    .expect("final class");
}

#[test]
fn test_parse_class_extends() {
    // Just test parsing — inheritance semantics in Sub-step 2C
    common::check_no_errors(
        r#"
package a

class Animal(public name: String)

class Dog(public breed: String) extends Animal("Rex")

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_parse_override_method() {
    common::check_no_errors(
        r#"
package a

class Base(public x: Int32) =
    function greet(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(0) =
    override function greet(self: Child): Int32 = self.y

function main(): Unit = ()
"#,
    );
}

#[test]
fn test_parse_final_method() {
    common::check_no_errors(
        r#"
package a

class Base(public x: Int32) =
    final function value(self: Base): Int32 = self.x

function main(): Unit = ()
"#,
    );
}

// --- Inheritance compile_and_run tests (Sub-step 2C) ---

#[test]
fn test_inheritance_child_access_parent_fields() {
    common::compile_and_run(
        r#"
package a

class Animal(public name: String, public legs: Int32)

class Dog(public breed: String) extends Animal("Rex", 4)

function main(): Unit =
    let d = Dog("Labrador")
    assert d.legs == 4
    assert d.breed == "Labrador"
"#,
    )
    .expect("inheritance field access");
}

#[test]
fn test_inheritance_subtype_assignment() {
    common::compile_and_run(
        r#"
package a

class Animal(public legs: Int32)

class Dog(public breed: String) extends Animal(4)

function countLegs(a: Animal): Int32 = a.legs

function main(): Unit =
    let d = Dog("Labrador")
    assert countLegs(d) == 4
"#,
    )
    .expect("subtype assignment");
}

#[test]
fn test_inheritance_inherited_method() {
    common::compile_and_run(
        r#"
package a

class Animal(public legs: Int32) =
    public function describe(self: Animal): Int32 = self.legs * 2

class Dog(public breed: String) extends Animal(4)

function main(): Unit =
    let d = Dog("Lab")
    assert d.describe() == 8
"#,
    )
    .expect("inherited method");
}

#[test]
fn test_inheritance_child_with_let_binding() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32)

class Child(public y: Int32) extends Base(10) =
    public let doubled: Int32 = y * 2

function main(): Unit =
    let c = Child(20)
    assert c.x == 10
    assert c.y == 20
    assert c.doubled == 40
"#,
    )
    .expect("child with let binding");
}

#[test]
fn test_cannot_extend_final_class() {
    let errors = common::compile_expecting_errors(
        r#"
package a

final class Sealed(public x: Int32)

class Derived(public y: Int32) extends Sealed(1)

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot extend final class")),
        "expected 'cannot extend final class' error, got: {:?}",
        errors
    );
}

#[test]
fn test_override_nonexistent_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32)

class Child(public y: Int32) extends Base(0) =
    override function nonexistent(self: Child): Int32 = 0

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("override") && e.contains("no matching")),
        "expected 'no matching method to override' error, got: {:?}",
        errors
    );
}

#[test]
fn test_cannot_override_final_method() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32) =
    final function value(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(0) =
    override function value(self: Child): Int32 = self.y

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot override final method")),
        "expected 'cannot override final method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_inheritance_with_super_args() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32, public y: Int32)

class Child(public z: Int32) extends Base(z * 2, z * 3)

function main(): Unit =
    let c = Child(5)
    assert c.x == 10
    assert c.y == 15
    assert c.z == 5
"#,
    )
    .expect("super args from child constructor params");
}

#[test]
fn test_inheritance_multi_level() {
    common::compile_and_run(
        r#"
package a

class A(public x: Int32)

class B(public y: Int32) extends A(1)

class C(public z: Int32) extends B(2)

function main(): Unit =
    let c = C(3)
    assert c.x == 1
    assert c.y == 2
    assert c.z == 3
"#,
    )
    .expect("multi-level inheritance");
}

#[test]
fn test_inheritance_subtype_multi_level() {
    common::compile_and_run(
        r#"
package a

class A(public x: Int32)

class B(public y: Int32) extends A(1)

class C(public z: Int32) extends B(2)

function getX(a: A): Int32 = a.x

function main(): Unit =
    let c = C(3)
    assert getX(c) == 1
"#,
    )
    .expect("multi-level subtype");
}

#[test]
fn test_virtual_dispatch_override() {
    common::compile_and_run(
        r#"
package a

class Animal(public legs: Int32) =
    public function legCount(self: Animal): Int32 = self.legs

class Dog(public breed: Int32) extends Animal(4) =
    public override function legCount(self: Dog): Int32 = self.legs + 100

function getLegCount(a: Animal): Int32 = a.legCount()

function main(): Unit =
    let a = Animal(2)
    let d = Dog(42)
    assert getLegCount(a) == 2
    assert getLegCount(d) == 104
"#,
    )
    .expect("virtual dispatch with override");
}

#[test]
fn test_virtual_dispatch_no_override() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function getX(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(10)

function callGetX(b: Base): Int32 = b.getX()

function main(): Unit =
    let c = Child(20)
    assert callGetX(c) == 10
"#,
    )
    .expect("virtual dispatch without override");
}

#[test]
fn test_final_class_direct_dispatch() {
    common::compile_and_run(
        r#"
package a

final class Counter(public count: Int32) =
    public function next(self: Counter): Int32 = self.count + 1

function main(): Unit =
    let c = Counter(5)
    assert c.next() == 6
"#,
    )
    .expect("final class direct dispatch");
}

#[test]
fn test_virtual_dispatch_multi_level() {
    common::compile_and_run(
        r#"
package a

class A(public x: Int32) =
    public function value(self: A): Int32 = self.x

class B(public y: Int32) extends A(1) =
    public override function value(self: B): Int32 = self.y * 10

class C(public z: Int32) extends B(2) =
    public override function value(self: C): Int32 = self.z * 100

function getValue(a: A): Int32 = a.value()

function main(): Unit =
    let a = A(1)
    let b = B(2)
    let c = C(3)
    assert getValue(a) == 1
    assert getValue(b) == 20
    assert getValue(c) == 300
"#,
    )
    .expect("multi-level virtual dispatch");
}

#[test]
fn test_final_method_direct_dispatch() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public final function getX(self: Base): Int32 = self.x
    public function doubled(self: Base): Int32 = self.x * 2

class Child(public y: Int32) extends Base(10)

function main(): Unit =
    let c = Child(20)
    assert c.getX() == 10
    assert c.doubled() == 20
"#,
    )
    .expect("final method uses direct dispatch");
}

// --- Method visibility tests ---

#[test]
fn test_protected_method_accessible_inside_class() {
    common::compile_and_run(
        r#"
package a

class Foo(public x: Int32) =
    protected function secret(self: Foo): Int32 = self.x * 2
    public function reveal(self: Foo): Int32 = self.secret()

function main(): Unit =
    let f = Foo(5)
    assert f.reveal() == 10
"#,
    )
    .expect("protected method accessible inside class");
}

#[test]
fn test_protected_method_accessible_from_subclass() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    protected function secret(self: Base): Int32 = self.x * 2

class Child(public y: Int32) extends Base(10) =
    public function revealSecret(self: Child): Int32 = self.secret()

function main(): Unit =
    let c = Child(20)
    assert c.revealSecret() == 20
"#,
    )
    .expect("protected method accessible from subclass");
}

#[test]
fn test_protected_method_not_accessible_outside() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo(public x: Int32) =
    protected function secret(self: Foo): Int32 = self.x * 2

function main(): Unit =
    let f = Foo(5)
    let v = f.secret()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("protected")),
        "expected protected access error, got: {:?}",
        errors
    );
}

#[test]
fn test_protected_static_method_not_accessible_outside() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo(public x: Int32) =
    protected function create(): Foo = Foo(0)

function main(): Unit =
    let f = Foo.create()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("protected")),
        "expected protected access error, got: {:?}",
        errors
    );
}

#[test]
fn test_private_method_not_accessible_outside() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo(public x: Int32) =
    function secret(self: Foo): Int32 = self.x * 2

function main(): Unit =
    let f = Foo(5)
    let v = f.secret()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("private")),
        "expected private access error, got: {:?}",
        errors
    );
}

#[test]
fn test_private_method_accessible_inside_class() {
    common::compile_and_run(
        r#"
package a

class Foo(public x: Int32) =
    function secret(self: Foo): Int32 = self.x * 2
    public function reveal(self: Foo): Int32 = self.secret()

function main(): Unit =
    let f = Foo(5)
    assert f.reveal() == 10
"#,
    )
    .expect("private method accessible inside class");
}

#[test]
fn test_inheritance_private_field_shadowing() {
    // Private fields can be shadowed — each class sees its own
    common::compile_and_run(
        r#"
package a

class Base(private x: Int32) =
    public let bx: Int32 = x + 1

class Child(private x: Int32) extends Base(x * 10) =
    public let cx: Int32 = x + 100

function main(): Unit =
    let c = Child(5)
    assert c.bx == 51
    assert c.cx == 105
"#,
    )
    .expect("private field shadowing allowed");
}

#[test]
fn test_inheritance_public_field_shadowing_error() {
    let result = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32)
class Child(public x: Int32) extends Base(x * 2)

function main(): Unit =
    let c = Child(5)
"#,
    );
    assert!(
        result.iter().any(|e| e.contains("shadows")),
        "expected shadowing error, got: {:?}",
        result
    );
}

#[test]
fn test_inheritance_multi_level_field_shadowing_error() {
    let result = common::compile_expecting_errors(
        r#"
package a

class A(public x: Int32)
class B(public y: Int32) extends A(y)
class C(public x: Int32) extends B(x)

function main(): Unit =
    let c = C(5)
"#,
    );
    assert!(
        result.iter().any(|e| e.contains("shadows")),
        "expected shadowing error for grandparent field, got: {:?}",
        result
    );
}

#[test]
fn test_private_field_shadowing_scope_with_methods() {
    // Each class's method should access its own scope's field, not the parent's
    common::compile_and_run(
        r#"
package a

class Base(private x: Int32) =
    public function base_x(self: Base): Int32 = self.x

class Child(private x: Int32) extends Base(x * 10) =
    public function child_x(self: Child): Int32 = self.x

function main(): Unit =
    let c = Child(5)
    assert c.base_x() == 50
    assert c.child_x() == 5
"#,
    )
    .expect("methods access their own class's field");
}

#[test]
fn test_private_field_shadowing_scope_three_levels() {
    // Three-level hierarchy: each class has private `x`, each let binding uses its own `x`
    common::compile_and_run(
        r#"
package a

class A(private x: Int32) =
    public let ax: Int32 = x * 100

class B(private x: Int32) extends A(x + 1) =
    public let bx: Int32 = x * 10

class C(private x: Int32) extends B(x + 2) =
    public let cx: Int32 = x

function main(): Unit =
    let c = C(5)
    assert c.cx == 5
    assert c.bx == 70
    assert c.ax == 800
"#,
    )
    .expect("three-level private field shadowing with correct scopes");
}

#[test]
fn test_private_field_shadowing_scope_let_bindings_same_name() {
    // Both parent and child have private let binding with the same name
    common::compile_and_run(
        r#"
package a

class Base(private x: Int32) =
    private let doubled: Int32 = x * 2
    public let base_result: Int32 = doubled + 1

class Child(private x: Int32) extends Base(x + 10) =
    private let doubled: Int32 = x * 3
    public let child_result: Int32 = doubled + 2

function main(): Unit =
    let c = Child(5)
    assert c.base_result == 31
    assert c.child_result == 17
"#,
    )
    .expect("private let binding shadowing with correct scopes");
}

// --- Abstract class tests ---

#[test]
fn test_abstract_class_with_concrete_subclass() {
    common::compile_and_run(
        r#"
package a

abstract class Shape(public sides: Int32) =
    public abstract function area(self: Shape): Int32

class Square(public side: Int32) extends Shape(4) =
    public override function area(self: Square): Int32 = self.side * self.side

function getArea(s: Shape): Int32 = s.area()

function main(): Unit =
    let sq = Square(5)
    assert getArea(sq) == 25
"#,
    )
    .expect("abstract class with concrete subclass");
}

#[test]
fn test_abstract_class_mixed_methods() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String) =
    public abstract function sound(self: Animal): Int32
    public function nameLen(self: Animal): Int32 = 42

class Dog() extends Animal("Rex") =
    public override function sound(self: Dog): Int32 = 100

function main(): Unit =
    let d = Dog()
    assert d.sound() == 100
    assert d.nameLen() == 42
"#,
    )
    .expect("abstract class with mixed methods");
}

#[test]
fn test_abstract_multi_level() {
    common::compile_and_run(
        r#"
package a

abstract class A() =
    public abstract function value(self: A): Int32

abstract class B() extends A()

class C() extends B() =
    public override function value(self: C): Int32 = 99

function getValue(a: A): Int32 = a.value()

function main(): Unit =
    let c = C()
    assert getValue(c) == 99
"#,
    )
    .expect("abstract multi-level");
}

#[test]
fn test_abstract_class_with_constructor_params() {
    common::compile_and_run(
        r#"
package a

abstract class Base(public x: Int32) =
    public abstract function doubled(self: Base): Int32

class Concrete(public y: Int32) extends Base(y * 2) =
    public override function doubled(self: Concrete): Int32 = self.x * 2

function main(): Unit =
    let c = Concrete(5)
    assert c.x == 10
    assert c.doubled() == 20
"#,
    )
    .expect("abstract class with constructor params");
}

#[test]
fn test_abstract_subtype_assignment() {
    common::compile_and_run(
        r#"
package a

abstract class Shape() =
    public abstract function area(self: Shape): Int32

class Circle(public r: Int32) extends Shape() =
    public override function area(self: Circle): Int32 = self.r * self.r * 3

function main(): Unit =
    let s: Shape = Circle(4)
    assert s.area() == 48
"#,
    )
    .expect("abstract subtype assignment");
}

// --- Abstract class error tests ---

#[test]
fn test_cannot_instantiate_abstract_class() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Shape() =
    public abstract function area(self: Shape): Int32

function main(): Unit =
    let s = Shape()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot instantiate abstract class")),
        "expected 'cannot instantiate abstract class' error, got: {:?}",
        errors
    );
}

#[test]
fn test_abstract_final_class_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract final class Foo()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot be both abstract and final")),
        "expected 'abstract and final' error, got: {:?}",
        errors
    );
}

#[test]
fn test_abstract_method_in_non_abstract_class_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo() =
    abstract function bar(self: Foo): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("abstract method") && e.contains("non-abstract class")),
        "expected 'abstract method in non-abstract class' error, got: {:?}",
        errors
    );
}

#[test]
fn test_abstract_static_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Foo() =
    abstract function bar(): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must be an instance method")),
        "expected 'must be instance method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_abstract_final_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Foo() =
    abstract final function bar(self: Foo): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot be both abstract and final")),
        "expected 'abstract and final method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_abstract_override_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Foo() =
    abstract override function bar(self: Foo): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot be both abstract and override")),
        "expected 'abstract and override method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_concrete_subclass_missing_abstract_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Shape() =
    public abstract function area(self: Shape): Int32

class Circle(public r: Int32) extends Shape()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must implement abstract method")),
        "expected 'must implement abstract method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_concrete_missing_abstract_from_grandparent_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class A() =
    public abstract function value(self: A): Int32

abstract class B() extends A()

class C() extends B()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must implement abstract method")),
        "expected 'must implement abstract method from grandparent' error, got: {:?}",
        errors
    );
}

#[test]
fn test_method_shadows_parent_without_override_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32) =
    public function greet(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(0) =
    public function greet(self: Child): Int32 = self.y

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows") && e.contains("override")),
        "expected 'shadows parent method, use override' error, got: {:?}",
        errors
    );
}

// --- Override + vtable slot replacement tests ---

#[test]
fn test_override_one_method_inherit_another() {
    // Two virtual methods on parent, child overrides only one; both dispatch correctly
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function getX(self: Base): Int32 = self.x
    public function doubled(self: Base): Int32 = self.x * 2

class Child(public y: Int32) extends Base(10) =
    public override function getX(self: Child): Int32 = self.y

function callGetX(b: Base): Int32 = b.getX()
function callDoubled(b: Base): Int32 = b.doubled()

function main(): Unit =
    let c = Child(99)
    assert callGetX(c) == 99
    assert callDoubled(c) == 20
"#,
    )
    .expect("override one method, inherit another");
}

#[test]
fn test_override_preserves_slot_order() {
    // Parent has 3 methods, child overrides the 2nd; all 3 dispatch correctly
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function first(self: Base): Int32 = 1
    public function second(self: Base): Int32 = 2
    public function third(self: Base): Int32 = 3

class Child() extends Base(0) =
    public override function second(self: Child): Int32 = 200

function callFirst(b: Base): Int32 = b.first()
function callSecond(b: Base): Int32 = b.second()
function callThird(b: Base): Int32 = b.third()

function main(): Unit =
    let c = Child()
    assert callFirst(c) == 1
    assert callSecond(c) == 200
    assert callThird(c) == 3
"#,
    )
    .expect("override preserves slot order");
}

// --- Super call tests (happy paths) ---

#[test]
fn test_super_call_basic() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function getValue(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(10) =
    public override function getValue(self: Child): Int32 = super.getValue() + self.y

function main(): Unit =
    let c = Child(5)
    assert c.getValue() == 15
"#,
    )
    .expect("basic super call");
}

#[test]
fn test_super_call_with_args() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function add(self: Base, a: Int32, b: Int32): Int32 = self.x + a + b

class Child(public y: Int32) extends Base(100) =
    public override function add(self: Child, a: Int32, b: Int32): Int32 = super.add(a, b) + self.y

function main(): Unit =
    let c = Child(1000)
    assert c.add(10, 20) == 1130
"#,
    )
    .expect("super call with args");
}

#[test]
fn test_super_call_chain() {
    // A→B→C each override calls super, verify cumulative
    common::compile_and_run(
        r#"
package a

class A() =
    public function value(self: A): Int32 = 1

class B() extends A() =
    public override function value(self: B): Int32 = super.value() + 10

class C() extends B() =
    public override function value(self: C): Int32 = super.value() + 100

function main(): Unit =
    let c = C()
    assert c.value() == 111
"#,
    )
    .expect("super call chain A->B->C");
}

#[test]
fn test_super_call_with_virtual_dispatch() {
    // Override uses super, called through parent-ref polymorphically
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function getValue(self: Base): Int32 = self.x

class Child(public y: Int32) extends Base(10) =
    public override function getValue(self: Child): Int32 = super.getValue() + self.y

function callGetValue(b: Base): Int32 = b.getValue()

function main(): Unit =
    let c = Child(5)
    assert callGetValue(c) == 15
"#,
    )
    .expect("super call with virtual dispatch");
}

#[test]
fn test_super_call_non_overridden_method() {
    // Call a different parent method via super (not the one being overridden)
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public function baseVal(self: Base): Int32 = self.x * 2
    public function other(self: Base): Int32 = 42

class Child() extends Base(5) =
    public override function baseVal(self: Child): Int32 = super.other() + 1

function main(): Unit =
    let c = Child()
    assert c.baseVal() == 43
"#,
    )
    .expect("super call to non-overridden method");
}

// --- Super call error cases ---

#[test]
fn test_super_outside_class_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = super.foo()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("super") && e.contains("class")),
        "expected 'super can only be used inside a class method' error, got: {:?}",
        errors
    );
}

#[test]
fn test_super_with_no_parent_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32) =
    public function foo(self: Base): Int32 = super.bar()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no parent class")),
        "expected 'has no parent class' error, got: {:?}",
        errors
    );
}

#[test]
fn test_super_nonexistent_method_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base(public x: Int32)

class Child() extends Base(0) =
    public function foo(self: Child): Int32 = super.nonexistent()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no method") && e.contains("parent")),
        "expected 'no method found in parent' error, got: {:?}",
        errors
    );
}

#[test]
fn test_super_call_grandparent_method() {
    // Child calls super.value() which is not on the immediate parent but on the grandparent
    common::compile_and_run(
        r#"
package a

class A() =
    public function value(self: A): Int32 = 42

class B() extends A()

class C() extends B() =
    public function getValue(self: C): Int32 = super.value()

function main(): Unit =
    let c = C()
    assert c.getValue() == 42
"#,
    )
    .expect("super call to grandparent method");
}

// ── class implements trait ────────────────────────────────────────

#[test]
fn test_class_implements_single_trait() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

class Dog() implements Greeter =
    public function greet(self: Dog): Int32 = 42

function main(): Unit =
    let d = Dog()
    assert d.greet() == 42
"#,
    )
    .expect("class implements single trait");
}

#[test]
fn test_class_implements_multiple_traits() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

trait Namer =
    function name(self: Self): Int32

class Dog() implements Greeter and Namer =
    public function greet(self: Dog): Int32 = 1
    public function name(self: Dog): Int32 = 2

function main(): Unit =
    let d = Dog()
    assert d.greet() == 1
    assert d.name() == 2
"#,
    )
    .expect("class implements multiple traits");
}

#[test]
fn test_class_implements_trait_bound_satisfaction() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

class Dog() implements Greeter =
    public function greet(self: Dog): Int32 = 42

function callGreet<T>(x: T): Int32 where T: Greeter = x.greet()

function main(): Unit =
    let d = Dog()
    assert callGreet(d) == 42
"#,
    )
    .expect("class implements trait bound satisfaction");
}

#[test]
fn test_class_implements_trait_object_coercion() {
    common::compile_and_run(
        r#"
package a

interface Greeter =
    function greet(self: Self): Int32

class Dog() implements Greeter =
    public function greet(self: Dog): Int32 = 42

function main(): Unit =
    let d = Dog()
    let g: Greeter = d
    assert g.greet() == 42
"#,
    )
    .expect("class implements trait object coercion");
}

#[test]
fn test_class_extends_and_implements() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

class Animal(public value: Int32)

class Dog() extends Animal(10) implements Greeter =
    public function greet(self: Dog): Int32 = self.value

function main(): Unit =
    let d = Dog()
    assert d.greet() == 10
    assert d.value == 10
"#,
    )
    .expect("class extends and implements");
}

#[test]
fn test_abstract_class_with_abstract_trait_method() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

abstract class Animal() implements Greeter =
    abstract function greet(self: Animal): Int32

class Dog() extends Animal() =
    public override function greet(self: Dog): Int32 = 42

function main(): Unit =
    let d = Dog()
    assert d.greet() == 42
"#,
    )
    .expect("abstract class with abstract trait method");
}

#[test]
fn test_inherited_method_satisfies_trait() {
    common::compile_and_run(
        r#"
package a

trait Greeter =
    function greet(self: Self): Int32

class Animal() =
    public function greet(self: Animal): Int32 = 42

class Dog() extends Animal() implements Greeter

function main(): Unit =
    let d = Dog()
    assert d.greet() == 42
"#,
    )
    .expect("inherited method satisfies trait");
}

// ── class implements: trait method dispatch via dot syntax ───────

#[test]
fn test_class_implements_trait_method_via_dot_syntax() {
    // resolve_trait_impl_method_for_type: calling a trait method on a class instance
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Cat(public name: String) implements Describable =
    public function describe(self: Cat): String = self.name

function main(): Unit =
    let c = Cat("Whiskers")
    assert c.describe() == "Whiskers"
"#,
    )
    .expect("trait method via dot syntax on class");
}

#[test]
fn test_class_implements_trait_method_multiple_traits_dot_syntax() {
    // resolve_trait_impl_method_for_type with multiple traits
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

trait Countable =
    function count(self: Self): Int32

class Bag(public size: Int32) implements Describable and Countable =
    public function describe(self: Bag): String = "bag"
    public function count(self: Bag): Int32 = self.size

function main(): Unit =
    let b = Bag(5)
    assert b.describe() == "bag"
    assert b.count() == 5
"#,
    )
    .expect("multiple trait methods via dot syntax on class");
}

// ── class implements: trait bounds with method calls ─────────────

#[test]
fn test_class_implements_trait_bound_method_call() {
    // Generic function with trait bound calling trait method on class
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Cat(public name: String) implements Describable =
    public function describe(self: Cat): String = self.name

function getDescription<T>(x: T): String where T: Describable = x.describe()

function main(): Unit =
    let c = Cat("Whiskers")
    assert getDescription(c) == "Whiskers"
"#,
    )
    .expect("trait bound method call on class");
}

#[test]
fn test_class_implements_separate_trait_bound_functions() {
    // Two generic functions, each with a different trait bound, both satisfied by one class
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

trait Countable =
    function count(self: Self): Int32

class Bag(public size: Int32) implements Describable and Countable =
    public function describe(self: Bag): String = "bag"
    public function count(self: Bag): Int32 = self.size

function getDescription<T>(x: T): String where T: Describable = x.describe()
function getCount<T>(x: T): Int32 where T: Countable = x.count()

function main(): Unit =
    let b = Bag(3)
    assert getDescription(b) == "bag"
    assert getCount(b) == 3
"#,
    )
    .expect("separate trait bound functions on class");
}

#[test]
fn test_class_extends_implements_trait_bound() {
    // Class with extends + implements used in trait-bounded generic
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Animal(public legs: Int32)

class Dog(public name: String) extends Animal(4) implements Describable =
    public function describe(self: Dog): String = self.name

function getDescription<T>(x: T): String where T: Describable = x.describe()

function main(): Unit =
    let d = Dog("Rex")
    assert getDescription(d) == "Rex"
    assert d.legs == 4
"#,
    )
    .expect("extends+implements with trait bound");
}

// ── class implements: trait objects ──────────────────────────────

#[test]
fn test_class_implements_trait_object_let_binding() {
    // Coerce class instance to trait object via let binding
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Cat(public name: String) implements Describable =
    public function describe(self: Cat): String = self.name

function main(): Unit =
    let c = Cat("Whiskers")
    let d: Describable = c
    assert d.describe() == "Whiskers"
"#,
    )
    .expect("class to trait object via let binding");
}

#[test]
fn test_class_implements_trait_object_function_param() {
    // Pass class instance as trait object parameter
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Cat(public name: String) implements Describable =
    public function describe(self: Cat): String = self.name

function show(d: Describable): String = d.describe()

function main(): Unit =
    let c = Cat("Whiskers")
    assert show(c) == "Whiskers"
"#,
    )
    .expect("class as trait object function param");
}

#[test]
fn test_class_implements_trait_object_multiple_classes() {
    // Multiple classes implementing same trait, used as trait objects
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Cat() implements Describable =
    public function describe(self: Cat): String = "cat"

class Dog() implements Describable =
    public function describe(self: Dog): String = "dog"

function show(d: Describable): String = d.describe()

function main(): Unit =
    assert show(Cat()) == "cat"
    assert show(Dog()) == "dog"
"#,
    )
    .expect("multiple classes as trait objects");
}

#[test]
fn test_class_implements_trait_object_return_type() {
    // Return class instance as trait object
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Cat() implements Describable =
    public function describe(self: Cat): String = "cat"

function makeDescribable(): Describable =
    Cat()

function main(): Unit =
    let d = makeDescribable()
    assert d.describe() == "cat"
"#,
    )
    .expect("class as trait object return type");
}

#[test]
fn test_class_implements_trait_object_with_inheritance() {
    // Child class with implements, parent method visible, used as trait object
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Animal() =
    public function describe(self: Animal): String = "animal"

class Dog() extends Animal() implements Describable

function show(d: Describable): String = d.describe()

function main(): Unit =
    let d = Dog()
    assert show(d) == "animal"
"#,
    )
    .expect("inherited impl as trait object");
}

// ── class implements trait error cases ───────────────────────────

#[test]
fn test_class_implements_missing_method() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src").join("a");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"package a

trait Greeter =
    function greet(self: Self): Int32

class Dog() implements Greeter
"#,
    )
    .unwrap();
    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("test".to_string()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().to_path_buf(),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };
    let errors = typecheck_project(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("must implement method 'greet' from trait 'Greeter'")),
        "expected missing method error, got: {errors:?}"
    );
}

#[test]
fn test_class_implements_unknown_trait() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src").join("a");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"package a

class Dog() implements Unknown
"#,
    )
    .unwrap();
    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("test".to_string()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().to_path_buf(),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };
    let errors = typecheck_project(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("unknown trait: 'Unknown'")),
        "expected unknown trait error, got: {errors:?}"
    );
}

#[test]
fn test_class_implements_non_abstract_with_abstract_trait_method() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src").join("a");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"package a

trait Greeter =
    function greet(self: Self): Int32

class Dog() implements Greeter =
    abstract function greet(self: Dog): Int32
"#,
    )
    .unwrap();
    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("test".to_string()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().to_path_buf(),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };
    let errors = typecheck_project(&project, dir.path());
    assert!(
        errors.iter().any(|e| e.contains("abstract method") && e.contains("non-abstract class")),
        "expected abstract-in-non-abstract error, got: {errors:?}"
    );
}

// --- Generic classes ---

#[test]
fn test_generic_class_basic() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b = Box<Int32>(42)
    assert b.value == 42
"#,
    )
    .expect("basic generic class");
}

#[test]
fn test_generic_class_inferred_type_args() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b = Box(42)
    assert b.value == 42
"#,
    )
    .expect("generic class with inferred type args");
}

#[test]
fn test_generic_class_string() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b = Box<String>("hello")
    assert b.value == "hello"
"#,
    )
    .expect("generic class with String type arg");
}

#[test]
fn test_generic_class_multiple_type_params() {
    common::compile_and_run(
        r#"
package a

class Pair<A, B>(public first: A, public second: B)

function main(): Unit =
    let p = Pair<Int32, String>(42, "hello")
    assert p.first == 42
    assert p.second == "hello"
"#,
    )
    .expect("generic class with multiple type params");
}

#[test]
fn test_generic_class_multiple_instantiations() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b1 = Box<Int32>(42)
    let b2 = Box<String>("hello")
    assert b1.value == 42
    assert b2.value == "hello"
"#,
    )
    .expect("multiple generic class instantiations");
}

#[test]
fn test_generic_class_with_method() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function get(self: Box<T>): T = self.value

function main(): Unit =
    let b = Box<Int32>(42)
    assert b.get() == 42
"#,
    )
    .expect("generic class with non-generic method");
}

#[test]
fn test_generic_class_method_uses_type_param() {
    common::compile_and_run(
        r#"
package a

class Wrapper<T>(public value: T) where T: Equatable =
    public function is_same(self: Wrapper<T>, other: T): Bool = self.value == other

function main(): Unit =
    let w = Wrapper<Int32>(42)
    assert w.is_same(42)
    assert w.is_same(99) == false
"#,
    )
    .expect("generic class method using type param");
}

#[test]
fn test_generic_class_as_function_param() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function unbox(b: Box<Int32>): Int32 = b.value

function main(): Unit =
    let b = Box<Int32>(42)
    assert unbox(b) == 42
"#,
    )
    .expect("generic class as function parameter");
}

#[test]
fn test_generic_class_as_return_type() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function make_box(x: Int32): Box<Int32> = Box<Int32>(x)

function main(): Unit =
    let b = make_box(42)
    assert b.value == 42
"#,
    )
    .expect("generic class as return type");
}

#[test]
fn test_generic_class_with_multiple_methods() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function get(self: Box<T>): T = self.value
    public function replace(self: Box<T>, new_val: T): Box<T> = Box<T>(new_val)

function main(): Unit =
    let b = Box<Int32>(42)
    let b2 = b.replace(99)
    assert b2.get() == 99
"#,
    )
    .expect("generic class with multiple methods");
}

// --- Generic methods on classes ---

#[test]
fn test_generic_method_on_non_generic_class() {
    common::compile_and_run(
        r#"
package a

class Dog(public name: String) =
    public function convert<U>(self: Dog, default: U): U = default

function main(): Unit =
    let d = Dog("Rex")
    assert d.convert<Int32>(42) == 42
    assert d.convert<String>("hello") == "hello"
"#,
    )
    .expect("generic method on non-generic class");
}

#[test]
fn test_generic_method_inferred_type_args() {
    common::compile_and_run(
        r#"
package a

class Dog(public name: String) =
    public function convert<U>(self: Dog, default: U): U = default

function main(): Unit =
    let d = Dog("Rex")
    assert d.convert(42) == 42
"#,
    )
    .expect("generic method with inferred type args");
}

#[test]
fn test_generic_method_on_generic_class() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function with_default<U>(self: Box<T>, default: U): Box<U> = Box<U>(default)

function main(): Unit =
    let b = Box<Int32>(42)
    let b2 = b.with_default<String>("hello")
    assert b2.value == "hello"
"#,
    )
    .expect("generic method on generic class");
}

#[test]
fn test_mix_generic_and_non_generic_methods() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function get(self: Box<T>): T = self.value
    public function convert<U>(self: Box<T>, default: U): U = default

function main(): Unit =
    let b = Box<Int32>(42)
    assert b.get() == 42
    assert b.convert<String>("hello") == "hello"
"#,
    )
    .expect("mix of generic and non-generic methods");
}

#[test]
fn test_generic_class_used_in_generic_function() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function get(self: Box<T>): T = self.value

function unwrap<T>(b: Box<T>): T = b.get()

function main(): Unit =
    let b = Box<Int32>(42)
    assert unwrap<Int32>(b) == 42
"#,
    )
    .expect("generic class used in generic function");
}

// --- Error cases ---

#[test]
fn test_generic_class_wrong_type_arg_count() {
    let result = common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b = Box<Int32, String>(42)
    ()
"#,
    );
    assert!(result.is_err(), "should error on wrong type arg count");
}

#[test]
fn test_generic_class_type_mismatch_in_constructor() {
    let result = common::compile_and_run(
        r#"
package a

class Box<T>(public value: T)

function main(): Unit =
    let b = Box<Int32>("not an int")
    ()
"#,
    );
    assert!(result.is_err(), "should error on type mismatch in constructor");
}

#[test]
fn test_generic_class_inferred_multiple_params() {
    common::compile_and_run(
        r#"
package a

class Pair<A, B>(public first: A, public second: B)

function main(): Unit =
    let p = Pair(10, "world")
    assert p.first == 10
    assert p.second == "world"
"#,
    )
    .expect("generic class with inferred multiple type params");
}

#[test]
fn test_generic_class_method_type_error_without_instantiation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Box<T>(public value: T) =
    function bad(self: Box<T>): Int32 =
        let x: Int32 = "hello"
        x

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type error in generic class method body without instantiation, got: {:?}",
        errors
    );
}

#[test]
fn test_non_generic_class_generic_method_type_error_without_instantiation() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Dog() =
    function convert<U>(self: Dog, default: U): U =
        let x: Int32 = "hello"
        default

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("type mismatch")),
        "expected type error in generic method body without instantiation, got: {:?}",
        errors
    );
}

// ── Phase 5b: Variance on generic classes ───────────────────────────

#[test]
fn test_generic_class_covariant_assignment() {
    common::check_no_errors(
        r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Box<out T>(public value: T)

function take_box(b: Box<Animal>): Unit = ()

function main(): Unit =
    let dog_box = Box<Dog>(Dog())
    take_box(dog_box)
"#,
    );
}

#[test]
fn test_generic_class_invariant_rejects_subtype() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Cell<T>(public value: T)

function take_cell(c: Cell<Animal>): Unit = ()

function main(): Unit =
    let dog_cell = Cell<Dog>(Dog())
    take_cell(dog_cell)
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("no matching overload")),
        "expected rejection for invariant Cell<Dog> vs Cell<Animal>, got: {:?}",
        errors
    );
}

#[test]
fn test_generic_class_covariant_never() {
    common::compile_and_run(
        r#"
package a

class Box<out T>(public value: T)

function take_box(b: Box<Int32>): Int32 = b.value

function make_never(): Box<Never> = panic "never"

function main(): Unit =
    let b: Box<Int32> = Box<Int32>(42)
    assert take_box(b) == 42
"#,
    )
    .expect("covariant generic class with concrete type");
}

// ── Phase 5b: Let bindings on generic classes ───────────────────────

#[test]
fn test_generic_class_let_binding_typed() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public let label: String = "box"

function main(): Unit =
    let b = Box<Int32>(42)
    assert b.value == 42
    assert b.label == "box"
"#,
    )
    .expect("generic class typed let binding");
}

#[test]
fn test_generic_class_let_binding_using_type_param() {
    common::compile_and_run(
        r#"
package a

class Wrapper<T>(public inner: T) =
    public let copy: T = inner

function main(): Unit =
    let w = Wrapper<Int32>(99)
    assert w.inner == 99
    assert w.copy == 99
"#,
    )
    .expect("generic class let binding using type param");
}

#[test]
fn test_generic_class_let_binding_body_expression() {
    common::compile_and_run(
        r#"
package a

class Pair<T>(public first: T, public second: T) =
    public let sum_label: String = "pair"

function main(): Unit =
    let p = Pair<Int32>(10, 20)
    assert p.first == 10
    assert p.second == 20
    assert p.sum_label == "pair"
"#,
    )
    .expect("generic class let binding with body expression");
}

// ── Phase 5b: extends on generic classes ────────────────────────────

#[test]
fn test_generic_class_extends_generic_parent() {
    common::compile_and_run(
        r#"
package a

class Container<T>(public value: T)

class LabeledContainer<T>(public label: String) extends Container<T>(label)

function main(): Unit =
    let c = LabeledContainer<String>("hello")
    assert c.label == "hello"
    assert c.value == "hello"
"#,
    )
    .expect("generic class extends generic parent");
}

#[test]
fn test_generic_class_extends_non_generic_parent() {
    common::compile_and_run(
        r#"
package a

class Base(public id: Int32)

class TypedItem<T>(public data: T) extends Base(42)

function main(): Unit =
    let item = TypedItem<String>("hello")
    assert item.data == "hello"
    assert item.id == 42
"#,
    )
    .expect("generic class extends non-generic parent");
}

#[test]
fn test_generic_class_extends_parent_field_access() {
    common::compile_and_run(
        r#"
package a

class Holder<T>(public value: T)

class NamedHolder<T>(public name: String) extends Holder<T>(name)

function main(): Unit =
    let h = NamedHolder<String>("test")
    assert h.name == "test"
    assert h.value == "test"
"#,
    )
    .expect("generic class extends parent field access");
}

// ── Phase 5b: implements on generic classes ─────────────────────────

#[test]
fn test_generic_class_implements_trait() {
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Box<T>(public value: T) implements Describable =
    public function describe(self: Box<T>): String = "a box"

function main(): Unit =
    let b = Box<Int32>(42)
    assert b.describe() == "a box"
"#,
    )
    .expect("generic class implements trait");
}

#[test]
fn test_generic_class_implements_trait_different_instantiations() {
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Box<T>(public value: T) implements Describable =
    public function describe(self: Box<T>): String = "a box"

function main(): Unit =
    let b1 = Box<Int32>(42)
    let b2 = Box<String>("hello")
    assert b1.describe() == "a box"
    assert b2.describe() == "a box"
"#,
    )
    .expect("generic class implements trait different instantiations");
}

// ── Phase 5b: Trait objects from generic classes ────────────────────

#[test]
fn test_generic_class_trait_object_coercion() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Box<T>(public value: T) implements Describable =
    public function describe(self: Box<T>): String = "a box"

function main(): Unit =
    let b = Box<Int32>(42)
    let d: Describable = b
    assert d.describe() == "a box"
"#,
    )
    .expect("generic class trait object coercion");
}

#[test]
fn test_generic_class_trait_object_different_instantiations() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): String

class Box<T>(public value: T) implements Describable =
    public function describe(self: Box<T>): String = "a box"

function take_describable(d: Describable): String = d.describe()

function main(): Unit =
    let b1 = Box<Int32>(42)
    let b2 = Box<String>("hello")
    assert take_describable(b1) == "a box"
    assert take_describable(b2) == "a box"
"#,
    )
    .expect("generic class trait object different instantiations");
}

// ── Phase 5b: Trait bounds on generic class methods ─────────────────

#[test]
fn test_generic_class_trait_bound_method() {
    common::compile_and_run(
        r#"
package a

trait Doubler =
    function double(self: Self): Int32

implement Doubler for Int32 =
    function double(self: Int32): Int32 = self * 2

class Wrapper<T>(public value: T) where T: Doubler =
    public function doubled(self: Wrapper<T>): Int32 = self.value.double()

function main(): Unit =
    let w = Wrapper<Int32>(21)
    assert w.doubled() == 42
"#,
    )
    .expect("generic class trait bound method");
}

#[test]
fn test_generic_class_implements_equatable() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) implements Equatable where T: Equatable =
    public function equals(self: Box<T>, other: Box<T>): Bool = self.value == other.value

function main(): Unit =
    let a = Box<Int32>(42)
    let b = Box<Int32>(42)
    let c = Box<Int32>(99)
    assert a == b
    assert (a == c) == false
"#,
    )
    .expect("generic class implements Equatable with == operator");
}

#[test]
fn test_generic_class_in_trait_bound() {
    common::compile_and_run(
        r#"
package a

trait Describable =
    function describe(self: Self): String

class Box<T>(public value: T) implements Describable =
    public function describe(self: Box<T>): String = "a box"

function show<T>(item: T): String where T: Describable = item.describe()

function main(): Unit =
    let b = Box<Int32>(42)
    assert show(b) == "a box"
"#,
    )
    .expect("generic class used in trait bound");
}

// --- Class bounds (subtype constraints) on type parameters ---

#[test]
fn test_class_bound_field_access() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String)

class Dog(name: String) extends Animal(name)

function get_name<T>(animal: T): String where T: Animal = animal.name

function main(): Unit = assert get_name<Dog>(Dog("Rex")) == "Rex"
"#,
    )
    .expect("class bound field access");
}

#[test]
fn test_class_bound_method_call() {
    common::compile_and_run(
        r#"
package a

abstract class Shape() =
    public function area(self: Shape): Float64 = 0.0

class Circle(public radius: Float64) extends Shape() =
    public override function area(self: Circle): Float64 = 3.14 * self.radius * self.radius

function compute_area<T>(s: T): Float64 where T: Shape = s.area()

function main(): Unit = assert compute_area<Circle>(Circle(1.0)) == 3.14
"#,
    )
    .expect("class bound method call");
}

#[test]
fn test_class_bound_on_generic_class() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String)

class Dog(name: String) extends Animal(name)

class Container<T>(public value: T) where T: Animal

function main(): Unit =
    let c = Container<Dog>(Dog("Buddy"))
    assert c.value.name == "Buddy"
"#,
    )
    .expect("class bound on generic class");
}

#[test]
fn test_class_bound_violation_error() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

abstract class Animal(public name: String)

class Unrelated()

function get_name<T>(animal: T): String where T: Animal = animal.name

function main(): Unit = get_name<Unrelated>(Unrelated())
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".into()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".into()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };

    let errors = typecheck_project(&project, dir.path());
    assert!(
        !errors.is_empty(),
        "expected error for class bound violation, got no errors"
    );
    assert!(
        errors.iter().any(|e| e.contains("not a subtype of class")),
        "expected 'not a subtype of class' error, got: {:?}",
        errors
    );
}

// --- is/as on class hierarchies ---

#[test]
fn test_class_is_type_test() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String)
class Dog(name: String) extends Animal(name)
class Cat(name: String) extends Animal(name)

function isDog(a: Animal): Bool = a is Dog

function main(): Unit =
    let d: Animal = Dog("Rex")
    let c: Animal = Cat("Whiskers")
    assert isDog(d) == true
    assert isDog(c) == false
"#,
    )
    .expect("is type test on class hierarchy");
}

#[test]
fn test_class_as_type_cast() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String)
class Dog(name: String, public tricks: Int32) extends Animal(name)

function main(): Unit =
    let a: Animal = Dog("Rex", 5)
    let d = a as Dog
    assert d.tricks == 5
"#,
    )
    .expect("as type cast on class hierarchy");
}

#[test]
fn test_class_match_type_annotated_pattern() {
    common::compile_and_run(
        r#"
package a

abstract class Shape()
class Circle(public radius: Float64) extends Shape()
class Rectangle(public width: Float64, public height: Float64) extends Shape()

function area(s: Shape): Float64 =
    match s with
        case c: Circle => 3.14 * c.radius * c.radius
        case r: Rectangle => r.width * r.height
        case _ => panic "unknown shape"

function main(): Unit =
    let s1: Shape = Circle(2.0)
    let s2: Shape = Rectangle(3.0, 4.0)
    assert area(s1) == 12.56
    assert area(s2) == 12.0
"#,
    )
    .expect("match type annotated pattern on class hierarchy");
}

#[test]
fn test_class_is_same_type_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo()

function main(): Unit =
    let f = Foo()
    let x = f is Foo
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("always true")),
        "expected 'always true' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_is_upcast_always_true_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Animal()
class Dog() extends Animal()

function main(): Unit =
    let d = Dog()
    let x = d is Animal
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("always true")),
        "expected 'always true' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_as_upcast_allowed() {
    common::compile_and_run(
        r#"
package a

abstract class Animal(public name: String)
class Dog(name: String) extends Animal(name)

function main(): Unit =
    let d = Dog("Rex")
    let a: Animal = d as Animal
    assert a.name == "Rex"
"#,
    )
    .expect("as upcast should be allowed to control inferred type");
}

#[test]
fn test_class_is_unrelated_classes_error() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("myapp").join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(
        src.join("main.dove"),
        r#"
package a

class A()
class B()

function main(): Unit =
    let a = A()
    let x = a is B
"#,
    )
    .unwrap();

    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("myapp".into()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["a".into()]),
            source_dir: src,
        }],
        project_dir: dir.path().join("myapp"),
        main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
    };

    let errors = typecheck_project(&project, dir.path());
    assert!(
        !errors.is_empty(),
        "expected error for is between unrelated classes"
    );
    assert!(
        errors.iter().any(|e| e.contains("unrelated classes")),
        "expected 'unrelated classes' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_is_non_class_subject_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo()

function main(): Unit =
    let p = 42
    let x = p is Foo
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error for is on non-class subject"
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("requires subject of type Any or a class type")),
        "expected 'requires subject of type Any or a class type' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_is_deep_hierarchy() {
    common::compile_and_run(
        r#"
package a

abstract class Base(public x: Int32)
abstract class Middle(x: Int32, public y: Int32) extends Base(x)
class Leaf(x: Int32, y: Int32, public z: Int32) extends Middle(x, y)

function main(): Unit =
    let b: Base = Leaf(1, 2, 3)
    assert b is Middle == true
    assert b is Leaf == true
    let m: Middle = Leaf(4, 5, 6)
    assert m is Leaf == true
    assert (b as Leaf).z == 3
    assert (b as Middle).y == 2
"#,
    )
    .expect("deep hierarchy is/as");
}

#[test]
fn test_class_as_wrong_type_traps() {
    common::compile_and_expect_trap(
        r#"
package a

abstract class Animal()
class Dog() extends Animal()
class Cat() extends Animal()

function main(): Unit =
    let a: Animal = Cat()
    let d = a as Dog
"#,
    );
}

#[test]
fn test_class_match_with_wildcard() {
    common::compile_and_run(
        r#"
package a

abstract class Shape()
class Circle(public radius: Float64) extends Shape()
class Rectangle(public width: Float64, public height: Float64) extends Shape()
class Triangle() extends Shape()

function describe(s: Shape): Int32 =
    match s with
        case c: Circle => 1
        case r: Rectangle => 2
        case _ => 0

function main(): Unit =
    assert describe(Circle(1.0)) == 1
    assert describe(Rectangle(2.0, 3.0)) == 2
    assert describe(Triangle()) == 0
"#,
    )
    .expect("match with wildcard on class hierarchy");
}

#[test]
fn test_class_is_then_as_idiom() {
    common::compile_and_run(
        r#"
package a

abstract class Shape()
class Circle(public radius: Float64) extends Shape()
class Rectangle(public width: Float64, public height: Float64) extends Shape()

function describe(s: Shape): String =
    if s is Circle then
        let c = s as Circle
        "circle"
    else if s is Rectangle then
        let r = s as Rectangle
        "rectangle"
    else
        "unknown"

function main(): Unit =
    let s1: Shape = Circle(2.0)
    let s2: Shape = Rectangle(3.0, 4.0)
    assert describe(s1) == "circle"
    assert describe(s2) == "rectangle"
"#,
    )
    .expect("is-then-as idiom from design doc");
}

#[test]
fn test_class_as_unrelated_classes_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class A()
class B()

function main(): Unit =
    let a: A = A()
    let b = a as B
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unrelated classes")),
        "expected 'unrelated classes' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_match_upcast_always_true_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Animal()
class Dog() extends Animal()

function test(d: Dog): Int32 =
    match d with
        case a: Animal => 1

function main(): Unit =
    let x = test(Dog())
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("always true")),
        "expected 'always true' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_match_unrelated_classes_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class A()
class B()

function test(a: A): Int32 =
    match a with
        case b: B => 1

function main(): Unit =
    let x = test(A())
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("unrelated") || e.contains("never match")),
        "expected 'unrelated' or 'never match' error, got: {:?}",
        errors
    );
}

#[test]
fn test_class_three_identical_layout_siblings() {
    common::compile_and_run(
        r#"
package a

abstract class Base()
class A() extends Base()
class B() extends Base()
class C() extends Base()

function identify(b: Base): Int32 =
    match b with
        case a: A => 1
        case b: B => 2
        case c: C => 3
        case _ => 0

function main(): Unit =
    assert identify(A()) == 1
    assert identify(B()) == 2
    assert identify(C()) == 3
"#,
    )
    .expect("three identical-layout siblings distinguished correctly");
}

// ── Class properties ───────────────────────────────────────────

#[test]
fn test_class_basic_instance_property() {
    common::compile_and_run(
        r#"
package a

class Counter(public value: Int32) =
    public property doubled(self): Int32 = self.value * 2

function main(): Unit =
    let c = Counter(5)
    assert c.doubled == 10
"#,
    )
    .expect("basic instance property");
}

#[test]
fn test_class_static_property() {
    common::compile_and_run(
        r#"
package a

class MathConstants() =
    public property zero: Int32 = 0

function main(): Unit =
    assert MathConstants.zero == 0
"#,
    )
    .expect("static property");
}

#[test]
fn test_class_abstract_property_implemented_by_subclass() {
    common::compile_and_run(
        r#"
package a

abstract class Shape() =
    public abstract property area(self): Int32

class Square(public side: Int32) extends Shape() =
    public override property area(self): Int32 = self.side * self.side

function main(): Unit =
    let s = Square(4)
    assert s.area == 16
"#,
    )
    .expect("abstract property implemented by subclass");
}

#[test]
fn test_class_override_property() {
    common::compile_and_run(
        r#"
package a

class Base() =
    public property label(self): Int32 = 1

class Child() extends Base() =
    public override property label(self): Int32 = 2

function main(): Unit =
    let b = Base()
    let c = Child()
    assert b.label == 1
    assert c.label == 2
"#,
    )
    .expect("override property");
}

#[test]
fn test_class_final_property() {
    common::compile_and_run(
        r#"
package a

class Base() =
    public final property tag(self): Int32 = 42

class Child() extends Base()

function main(): Unit =
    let c = Child()
    assert c.tag == 42
"#,
    )
    .expect("final property");
}

#[test]
fn test_class_virtual_dispatch_property() {
    common::compile_and_run(
        r#"
package a

abstract class Animal() =
    public abstract property legs(self): Int32

class Dog() extends Animal() =
    public override property legs(self): Int32 = 4

class Bird() extends Animal() =
    public override property legs(self): Int32 = 2

function count_legs(a: Animal): Int32 = a.legs

function main(): Unit =
    assert count_legs(Dog()) == 4
    assert count_legs(Bird()) == 2
"#,
    )
    .expect("virtual dispatch on property");
}

#[test]
fn test_class_property_on_generic_class() {
    common::compile_and_run(
        r#"
package a

class Wrapper<T>(public value: T) =
    public property get(self): T = self.value

function main(): Unit =
    let w = Wrapper<Int32>(42)
    let x: Int32 = w.get
    assert x == 42
"#,
    )
    .expect("property on generic class");
}

#[test]
fn test_class_property_inheritance() {
    common::compile_and_run(
        r#"
package a

class Base(public x: Int32) =
    public property double_x(self): Int32 = self.x * 2

class Child(y: Int32) extends Base(y)

function main(): Unit =
    let c = Child(5)
    assert c.double_x == 10
"#,
    )
    .expect("property inheritance");
}

#[test]
fn test_class_implements_trait_with_property() {
    common::compile_and_run(
        r#"
package a

trait Named =
    property name(self): Int32

class Person(public id: Int32) implements Named =
    public property name(self): Int32 = self.id

function main(): Unit =
    let p = Person(42)
    assert p.name == 42
"#,
    )
    .expect("class implements trait with property");
}

#[test]
fn test_class_property_abstract_in_non_abstract_class_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo() =
    public abstract property bar(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("abstract property") && e.contains("non-abstract")),
        "expected error about abstract property in non-abstract class, got: {errors:?}"
    );
}

#[test]
fn test_class_property_override_without_parent_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base()

class Child() extends Base() =
    public override property foo(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("override") && e.contains("no matching")),
        "expected error about override without parent, got: {errors:?}"
    );
}

#[test]
fn test_class_property_override_final_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base() =
    public final property tag(self): Int32 = 1

class Child() extends Base() =
    public override property tag(self): Int32 = 2

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("final")),
        "expected error about overriding final property, got: {errors:?}"
    );
}

#[test]
fn test_class_property_abstract_and_final_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Foo() =
    public abstract final property bar(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("abstract") && e.contains("final")),
        "expected error about abstract+final, got: {errors:?}"
    );
}

#[test]
fn test_class_property_abstract_static_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Foo() =
    public abstract property bar: Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("abstract property") && e.contains("instance")),
        "expected error about abstract static property, got: {errors:?}"
    );
}

// ── Generic class property tests ───────────────────────────────

#[test]
fn test_generic_class_property_multiple_instantiations() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public property unwrap(self): T = self.value

function main(): Unit =
    let b1 = Box<Int32>(42)
    let b2 = Box<String>("hello")
    assert b1.unwrap == 42
    assert b2.unwrap == "hello"
"#,
    )
    .expect("generic class property with multiple instantiations");
}

#[test]
fn test_generic_class_abstract_property_override() {
    common::compile_and_run(
        r#"
package a

abstract class Container() =
    public abstract property content(self): Int32

class Box<T>(public value: T) extends Container() =
    public override property content(self): Int32 = 42

function main(): Unit =
    let ib = Box<Int32>(1)
    let sb = Box<String>("hi")
    assert ib.content == 42
    assert sb.content == 42
"#,
    )
    .expect("generic class overrides abstract property from non-generic parent");
}

#[test]
fn test_generic_class_property_virtual_dispatch() {
    common::compile_and_run(
        r#"
package a

abstract class Container() =
    public abstract property tag(self): Int32

class BoxA<T>(public value: T) extends Container() =
    public override property tag(self): Int32 = 1

class BoxB<T>(public value: T) extends Container() =
    public override property tag(self): Int32 = 2

function get_tag(c: Container): Int32 = c.tag

function main(): Unit =
    assert get_tag(BoxA<Int32>(10)) == 1
    assert get_tag(BoxB<Int32>(20)) == 2
"#,
    )
    .expect("generic class property virtual dispatch through non-generic base");
}

#[test]
fn test_generic_class_final_property() {
    common::compile_and_run(
        r#"
package a

class Holder<T>(public value: T) =
    public final property get(self): T = self.value

class NamedHolder<T>(public name: String) extends Holder<T>(name)

function main(): Unit =
    let h = NamedHolder<String>("test")
    assert h.get == "test"
"#,
    )
    .expect("generic class final property inherited by subclass");
}

#[test]
fn test_generic_class_property_override_in_generic_subclass() {
    common::compile_and_run(
        r#"
package a

class Base() =
    public property display(self): Int32 = 0

class Child<T>(public value: T) extends Base() =
    public override property display(self): Int32 = 1

function main(): Unit =
    let b = Base()
    let c = Child<String>("hello")
    assert b.display == 0
    assert c.display == 1
"#,
    )
    .expect("generic class property override in non-generic subclass");
}

#[test]
fn test_generic_class_property_virtual_dispatch_through_generic_base() {
    common::compile_and_run(
        r#"
package a

class Base() =
    public property tag(self): Int32 = 0

class ChildA<T>(public value: T) extends Base() =
    public override property tag(self): Int32 = 1

class ChildB<T>(public value: T) extends Base() =
    public override property tag(self): Int32 = 2

function get_tag(b: Base): Int32 = b.tag

function main(): Unit =
    assert get_tag(Base()) == 0
    assert get_tag(ChildA<Int32>(10)) == 1
    assert get_tag(ChildB<String>("x")) == 2
"#,
    )
    .expect("virtual dispatch on property through non-generic base with generic children");
}

#[test]
fn test_generic_class_property_multi_level_hierarchy() {
    common::compile_and_run(
        r#"
package a

abstract class A() =
    public abstract property val(self): Int32

class B<T>(public x: T) extends A() =
    public override property val(self): Int32 = 5

function read_val(a: A): Int32 = a.val

function main(): Unit =
    assert read_val(B<Int32>(5)) == 5
    assert read_val(B<String>("x")) == 5
"#,
    )
    .expect("generic class implements abstract property via virtual dispatch");
}

#[test]
fn test_generic_class_property_inheritance_from_parent() {
    common::compile_and_run(
        r#"
package a

class Parent<T>(public value: T) =
    public final property get(self): T = self.value

class Child<T>(public name: String) extends Parent<T>(name)

function main(): Unit =
    let c = Child<String>("hello")
    assert c.get == "hello"
"#,
    )
    .expect("generic class inherits property from generic parent");
}

#[test]
fn test_generic_class_extends_non_generic_with_property() {
    common::compile_and_run(
        r#"
package a

class Base() =
    public property tag(self): Int32 = 0

class Child<T>(public value: T) extends Base() =
    public override property tag(self): Int32 = 1

function get_tag(b: Base): Int32 = b.tag

function main(): Unit =
    assert get_tag(Base()) == 0
    assert get_tag(Child<Int32>(42)) == 1
"#,
    )
    .expect("generic subclass overrides non-generic parent property");
}

#[test]
fn test_generic_class_implements_trait_with_property() {
    common::compile_and_run(
        r#"
package a

trait HasId =
    property id(self): Int32

class Entity<T>(public value: T, public eid: Int32) implements HasId =
    public property id(self): Int32 = self.eid

function main(): Unit =
    let e = Entity<String>("test", 99)
    assert e.id == 99
"#,
    )
    .expect("generic class implements trait with property");
}

#[test]
fn test_generic_class_trait_property_with_trait_object() {
    common::compile_and_run(
        r#"
package a

interface HasId =
    property id(self): Int32

class Entity<T>(public value: T, public eid: Int32) implements HasId =
    public property id(self): Int32 = self.eid

function get_id(h: HasId): Int32 = h.id

function main(): Unit =
    let e1 = Entity<Int32>(1, 10)
    let e2 = Entity<String>("x", 20)
    assert get_id(e1) == 10
    assert get_id(e2) == 20
"#,
    )
    .expect("generic class trait property via trait object dispatch");
}

#[test]
fn test_generic_class_property_with_trait_bound() {
    common::compile_and_run(
        r#"
package a

trait Doubler =
    function double(self: Self): Int32

implement Doubler for Int32 =
    function double(self: Int32): Int32 = self * 2

class Wrapper<T>(public value: T) where T: Doubler =
    public property doubled(self): Int32 = self.value.double()

function main(): Unit =
    let w = Wrapper<Int32>(21)
    assert w.doubled == 42
"#,
    )
    .expect("generic class property using trait bound on type param");
}

#[test]
fn test_generic_class_static_property() {
    common::compile_and_run(
        r#"
package a

class Holder() =
    public property tag: Int32 = 99

function main(): Unit =
    assert Holder.tag == 99
"#,
    )
    .expect("static property on class");
}

#[test]
fn test_generic_class_property_override_final_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base<T>(public value: T) =
    public final property get(self): Int32 = 0

class Child<T>(v: T) extends Base<T>(v) =
    public override property get(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("final")),
        "expected error about overriding final property on generic class, got: {errors:?}"
    );
}

#[test]
fn test_generic_class_property_override_without_parent_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base<T>(public value: T)

class Child<T>(v: T) extends Base<T>(v) =
    public override property foo(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("override") && e.contains("no matching")),
        "expected error about override without parent on generic class, got: {errors:?}"
    );
}

#[test]
fn test_generic_class_property_abstract_in_non_abstract_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Foo<T>(public value: T) =
    public abstract property bar(self): Int32

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("abstract property") && e.contains("non-abstract")),
        "expected error about abstract property in non-abstract generic class, got: {errors:?}"
    );
}

#[test]
fn test_generic_class_property_shadows_without_override_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Base<T>(public value: T) =
    public property tag(self): Int32 = 0

class Child<T>(v: T) extends Base<T>(v) =
    public property tag(self): Int32 = 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("shadows") && e.contains("override")),
        "expected error about shadowing without override on generic class, got: {errors:?}"
    );
}

#[test]
fn test_generic_class_concrete_subclass_must_implement_abstract_property() {
    let errors = common::compile_expecting_errors(
        r#"
package a

abstract class Base<T>() =
    public abstract property content(self): T

class Child<T>() extends Base<T>()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must implement") && e.contains("content")),
        "expected error about unimplemented abstract property, got: {errors:?}"
    );
}

// --- as Trait cast tests ---

#[test]
fn test_class_as_trait_call_trait_method() {
    common::compile_and_run(
        r#"
package a

interface Walkable =
    function walk(self: Self): Int32

class Dog() implements Walkable =
    public function walk(self: Dog): Int32 = 42
    public function bark(self: Dog): Int32 = 99

function main(): Unit =
    let dog = Dog()
    let w = dog as Walkable
    assert w.walk() == 42
"#,
    )
    .expect("as Trait should allow calling trait methods");
}

#[test]
fn test_class_as_trait_hides_concrete_methods() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Walkable =
    function walk(self: Self): Int32

class Dog() implements Walkable =
    public function walk(self: Dog): Int32 = 42
    public function bark(self: Dog): Int32 = 99

function main(): Unit =
    let dog = Dog()
    let w = dog as Walkable
    w.bark()
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("bark")),
        "expected error about bark not being available on trait, got: {errors:?}"
    );
}

#[test]
fn test_class_as_trait_not_implemented() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Walkable =
    function walk(self: Self): Int32

class Cat()

function main(): Unit =
    let cat = Cat()
    let w = cat as Walkable
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("does not implement")),
        "expected error about class not implementing trait, got: {errors:?}"
    );
}

#[test]
fn test_record_as_trait() {
    common::compile_and_run(
        r#"
package a

interface Describable =
    function describe(self: Self): Int32

record Point =
    x: Int32
    y: Int32

implement Describable for Point =
    function describe(self: Point): Int32 = self.x + self.y

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    let d = p as Describable
    assert d.describe() == 7
"#,
    )
    .expect("record as Trait should work");
}

#[test]
fn test_enum_as_trait() {
    common::compile_and_run(
        r#"
package a

interface Countable =
    function count(self: Self): Int32

enum Color =
    Red
    Green
    Blue

implement Countable for Color =
    function count(self: Color): Int32 =
        match self with
            case Color.Red => 1
            case Color.Green => 2
            case Color.Blue => 3

function main(): Unit =
    let c: Color = Color.Green
    let ct = c as Countable
    assert ct.count() == 2
"#,
    )
    .expect("enum as Trait should work");
}

#[test]
fn test_newtype_as_trait() {
    common::compile_and_run(
        r#"
package a

interface Measurable =
    function measure(self: Self): Int32

newtype Meters = Int32

implement Measurable for Meters =
    function measure(self: Meters): Int32 = self.value

function main(): Unit =
    let m = Meters(42)
    let ms = m as Measurable
    assert ms.measure() == 42
"#,
    )
    .expect("newtype as Trait should work");
}

#[test]
fn test_record_as_trait_not_implemented() {
    let errors = common::compile_expecting_errors(
        r#"
package a

interface Describable =
    function describe(self: Self): Int32

record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 3; y = 4 }
    let d = p as Describable
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("does not implement")),
        "expected error about record not implementing trait, got: {errors:?}"
    );
}

// --- Generic class static method tests ---

#[test]
fn test_generic_class_static_method_with_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function create(v: T): Box<T> = Box(v)

function main(): Unit =
    let b: Box<Int32> = Box<Int32>.create(42)
    assert b.value == 42
"#,
    )
    .expect("generic class static method with explicit type args");
}

#[test]
fn test_generic_class_static_method_with_inference() {
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public function create(v: T): Box<T> = Box(v)

function main(): Unit =
    let b: Box<Int32> = Box.create(42)
    assert b.value == 42
"#,
    )
    .expect("generic class static method with inference");
}

#[test]
fn test_generic_class_static_property_explicit_type_args() {
    common::compile_and_run(
        r#"
package a

class Container<T>(public value: T) =
    public property tag(): Int32 = 99

function main(): Unit =
    let t: Int32 = Container<Int32>.tag
    assert t == 99
"#,
    )
    .expect("generic class static property with explicit type args");
}

// --- Static let binding tests ---

#[test]
fn test_class_static_let_basic_read() {
    common::compile_and_run(
        r#"
package a

class Config() =
    public let static maxRetries: Int32 = 3

function main(): Unit =
    assert Config.maxRetries == 3
"#,
    )
    .expect("class static let basic read");
}

#[test]
fn test_class_static_let_mutable_assignment() {
    common::compile_and_run(
        r#"
package a

class Counter() =
    public let static mutable count: Int32 = 0

function main(): Unit =
    assert Counter.count == 0
    Counter.count = 5
    assert Counter.count == 5
"#,
    )
    .expect("class static let mutable assignment");
}

#[test]
fn test_class_static_let_type_inference() {
    common::compile_and_run(
        r#"
package a

class Config() =
    public let static defaultName = "hello"

function main(): Unit =
    assert Config.defaultName == "hello"
"#,
    )
    .expect("class static let type inference");
}

#[test]
fn test_class_static_let_immutable_assignment_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Config() =
    public let static maxRetries: Int32 = 3

function main(): Unit =
    Config.maxRetries = 5
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot assign to immutable")),
        "expected immutability error, got: {errors:?}"
    );
}

#[test]
fn test_class_static_let_private_not_accessible() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Config() =
    let static secret: Int32 = 42

function main(): Unit =
    let x = Config.secret
    ()
"#,
    );
    assert!(
        !errors.is_empty(),
        "expected error accessing private static field, got no errors"
    );
}

#[test]
fn test_generic_class_static_let() {
    common::compile_and_run(
        r#"
package a

class Container<T>(public value: T) =
    public let static label: Int32 = 42

function main(): Unit =
    let x: Int32 = Container<Int32>.label
    assert x == 42
"#,
    )
    .expect("generic class static let");
}

#[test]
fn test_generic_class_static_let_mutable() {
    common::compile_and_run(
        r#"
package a

class Container<T>(public value: T) =
    public let static mutable count: Int32 = 0

function main(): Unit =
    Container<Int32>.count = 10
    assert Container<Int32>.count == 10
"#,
    )
    .expect("generic class static let mutable");
}

#[test]
fn error_generic_class_static_references_type_param() {
    let errors = common::compile_expecting_errors(
        r#"
package a

class Box<T>(public value: T) =
    public let static default: Option<T> = None

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("references type parameter")),
        "expected type-parameter rejection, got: {:?}",
        errors,
    );
}

#[test]
fn generic_class_static_with_concrete_type_compiles() {
    // Sanity: a static on a generic class is still allowed when its type doesn't reference
    // any of the class's type parameters.
    common::compile_and_run(
        r#"
package a

class Box<T>(public value: T) =
    public let static count: Int32 = 0

function main(): Unit =
    assert Box<Int32>.count == 0
"#,
    )
    .expect("generic class static with concrete type");
}

// ── Boxed mutable fields with variance ──────────────────────────────

#[test]
fn test_covariant_class_immutable_field_variance_cast() {
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Box<out T>(public value: T)

function takeName(b: Box<Animal>): String = b.value.name

function main(): Unit =
    let dogBox = Box<Dog>(Dog())
    assert takeName(dogBox) == "Rex"
"#,
    )
    .expect("covariant class with immutable field — variance cast Dog→Animal");
}

#[test]
fn test_covariant_class_mutable_field_shared_state() {
    common::compile_and_run(
r#"
package a

class MutBox<out T>(public mutable value: Int32)

function increment(b: MutBox<String>): Unit =
    b.value = b.value + 1

function main(): Unit =
    let b = MutBox<String>(10)
    assert b.value == 10
    increment(b)
    assert b.value == 11
"#,
    )
    .expect("covariant class with mutable primitive field — shared state after cast");
}

#[test]
fn test_covariant_class_mutable_t_field_rejected() {
    // Mutable field of covariant type T must be in invariant position — rejected.
    let errors = common::compile_expecting_errors(
r#"
package a

class Holder<out T>(public mutable value: T)

function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("covariant") && e.contains("invariant position")),
        "expected variance position error for mutable covariant field, got: {:?}",
        errors
    );
}

#[test]
fn test_covariant_class_immutable_t_field_through_cast() {
    // Immutable covariant field can be read through a variance cast.
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Holder<out T>(public value: T)

function getName(h: Holder<Animal>): String = h.value.name

function main(): Unit =
    let h = Holder<Dog>(Dog())
    assert getName(h) == "Rex"
"#,
    )
    .expect("covariant class with immutable T field through variance cast");
}

#[test]
fn test_covariant_class_mixed_mutable_immutable_fields() {
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Pair<out T>(public value: T, public mutable count: Int32)

function bump(p: Pair<Animal>): Unit =
    p.count = p.count + 1

function main(): Unit =
    let p = Pair<Dog>(Dog(), 0)
    assert p.value.name == "Rex"
    assert p.count == 0
    bump(p)
    assert p.count == 1
"#,
    )
    .expect("covariant class with mixed mutable/immutable fields");
}

#[test]
fn test_covariant_class_multiple_mutable_fields() {
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Multi<out T>(public value: T, public mutable x: Int32, public mutable y: Int32)

function setX(m: Multi<Animal>): Unit =
    m.x = 100

function setY(m: Multi<Animal>): Unit =
    m.y = 200

function main(): Unit =
    let m = Multi<Dog>(Dog(), 1, 2)
    assert m.x == 1
    assert m.y == 2
    setX(m)
    setY(m)
    assert m.x == 100
    assert m.y == 200
"#,
    )
    .expect("covariant class with multiple mutable fields — independently boxed and shared");
}

#[test]
fn test_covariant_class_method_through_cast() {
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Container<out T>(public value: T) =
    public function get(self: Container<T>): T = self.value

function getName(c: Container<Animal>): String = c.get().name

function main(): Unit =
    let c = Container<Dog>(Dog())
    assert getName(c) == "Rex"
"#,
    )
    .expect("covariant class method access through variance cast");
}

#[test]
fn test_covariant_class_direct_let_shared_mutation() {
    // Direct let-binding variance cast: x and y share the same boxed mutable field
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Box<out T>(value: T, public mutable age: Int32)

function main(): Unit =
    let x = Box<Dog>(Dog(), 15)
    let y: Box<Animal> = x
    assert x.age == 15
    assert y.age == 15
    x.age = 20
    assert y.age == 20
"#,
    )
    .expect("direct let-binding variance cast — shared mutable field");
}

#[test]
fn test_covariant_class_write_through_cast_ref() {
    // Write through the cast reference, read back through the original
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Box<out T>(value: T, public mutable age: Int32)

function main(): Unit =
    let x = Box<Dog>(Dog(), 10)
    let y: Box<Animal> = x
    y.age = 99
    assert x.age == 99
"#,
    )
    .expect("write through cast ref, read through original — shared mutable state");
}

#[test]
fn test_covariant_class_invariant_no_boxing() {
    // Invariant generic class should NOT box mutable fields (no variance cast needed)
    common::compile_and_run(
r#"
package a

class Cell<T>(public mutable value: T)

function main(): Unit =
    let c = Cell<Int32>(42)
    c.value = 100
    assert c.value == 100
"#,
    )
    .expect("invariant generic class — mutable field works without boxing");
}

#[test]
fn test_covariant_class_let_binding_mutable_field() {
    // Let-binding mutable field inside a covariant class is also boxed
    common::compile_and_run(
r#"
package a

class Animal(public name: String)
class Dog() extends Animal("Rex")

class Counter<out T>(public value: T) =
    public let mutable count: Int32 = 0

function bump(c: Counter<Animal>): Unit =
    c.count = c.count + 1

function main(): Unit =
    let c = Counter<Dog>(Dog())
    assert c.count == 0
    bump(c)
    assert c.count == 1
"#,
    )
    .expect("covariant class let-binding mutable field — boxed and shared");
}

// --- Sealed abstract class tests ---

#[test]
fn test_sealed_abstract_class_exhaustive_match() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Shape()
final class Circle(public radius: Float64) extends Shape()
final class Rectangle(public width: Float64, public height: Float64) extends Shape()

function area(s: Shape): Float64 =
    match s with
        case c: Circle => 3.14 * c.radius * c.radius
        case r: Rectangle => r.width * r.height

function main(): Unit =
    let s1: Shape = Circle(2.0)
    let s2: Shape = Rectangle(3.0, 4.0)
    assert area(s1) == 12.56
    assert area(s2) == 12.0
"#,
    )
    .expect("sealed abstract class exhaustive match");
}

#[test]
fn test_sealed_abstract_class_non_exhaustive_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

sealed abstract class Shape()
final class Circle(public radius: Float64) extends Shape()
final class Rectangle(public width: Float64, public height: Float64) extends Shape()

function area(s: Shape): Float64 =
    match s with
        case c: Circle => 3.14 * c.radius * c.radius

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive") && e.contains("Rectangle")),
        "expected non-exhaustive error mentioning Rectangle, got: {:?}",
        errors
    );
}

#[test]
fn test_sealed_abstract_class_with_wildcard() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Shape()
final class Circle(public radius: Float64) extends Shape()
final class Rectangle(public width: Float64, public height: Float64) extends Shape()

function describe(s: Shape): String =
    match s with
        case c: Circle => "circle"
        case _ => "other"

function main(): Unit =
    let s: Shape = Circle(1.0)
    assert describe(s) == "circle"
"#,
    )
    .expect("sealed abstract class with wildcard pattern");
}

#[test]
fn test_sealed_abstract_class_intermediate_coverage() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Shape()
sealed abstract class Polygon() extends Shape()
final class Triangle(public base: Float64) extends Polygon()
final class Rectangle(public width: Float64) extends Polygon()
final class Circle(public radius: Float64) extends Shape()

function describe(s: Shape): String =
    match s with
        case p: Polygon => "polygon"
        case c: Circle => "circle"

function main(): Unit =
    let t: Shape = Triangle(3.0)
    let c: Shape = Circle(1.0)
    assert describe(t) == "polygon"
    assert describe(c) == "circle"
"#,
    )
    .expect("sealed abstract class intermediate covers all leaves");
}

#[test]
fn test_sealed_abstract_class_subclass_must_be_final_or_sealed() {
    let errors = common::compile_expecting_errors(
        r#"
package a

sealed abstract class Shape()
class Circle() extends Shape()

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("must be 'final' or 'sealed abstract'")),
        "expected error about final or sealed abstract, got: {:?}",
        errors
    );
}

#[test]
fn test_sealed_abstract_class_three_leaves() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Color()
final class Red() extends Color()
final class Green() extends Color()
final class Blue() extends Color()

function name(c: Color): String =
    match c with
        case r: Red => "red"
        case g: Green => "green"
        case b: Blue => "blue"

function main(): Unit =
    assert name(Red()) == "red"
    assert name(Green()) == "green"
    assert name(Blue()) == "blue"
"#,
    )
    .expect("sealed abstract class with three leaves");
}

#[test]
fn test_sealed_abstract_generic_class_exhaustive_match() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Container<out T>()
final class Box<out T>(public value: T) extends Container<T>()
final class Pair<out T>(public first: T, public second: T) extends Container<T>()

function extract(c: Container<Int32>): Int32 =
    match c with
        case b: Box<Int32> => b.value
        case p: Pair<Int32> => p.first + p.second

function main(): Unit =
    let b: Container<Int32> = Box(42)
    let p: Container<Int32> = Pair(10, 20)
    assert extract(b) == 42
    assert extract(p) == 30
"#,
    )
    .expect("sealed abstract generic class exhaustive match");
}

#[test]
fn test_sealed_abstract_generic_class_non_exhaustive_error() {
    let errors = common::compile_expecting_errors(
        r#"
package a

sealed abstract class Container<out T>()
final class Box<out T>(public value: T) extends Container<T>()
final class Pair<out T>(public first: T, public second: T) extends Container<T>()
final class Triple<out T>(public a: T, public b: T, public c: T) extends Container<T>()

function extract(c: Container<Int32>): Int32 =
    match c with
        case b: Box<Int32> => b.value
        case p: Pair<Int32> => p.first

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive") && e.contains("Triple")),
        "expected non-exhaustive error mentioning Triple, got: {:?}",
        errors
    );
}

#[test]
fn test_sealed_abstract_generic_class_with_methods() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Wrapper<out T>()
final class Value<out T>(public inner: T) extends Wrapper<T>() =
    function get(self): T = self.inner

final class Empty<out T>() extends Wrapper<T>()

function getOrDefault(w: Wrapper<Int32>, default: Int32): Int32 =
    match w with
        case v: Value<Int32> => v.get()
        case e: Empty<Int32> => default

function main(): Unit =
    let a: Wrapper<Int32> = Value(42)
    let b: Wrapper<Int32> = Empty<Int32>()
    assert getOrDefault(a, 0) == 42
    assert getOrDefault(b, 0) == 0
"#,
    )
    .expect("sealed abstract generic class with methods on subclasses");
}

#[test]
fn test_sealed_abstract_generic_class_two_type_params() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Either<out A, out B>()
final class Left<out A, out B>(public value: A) extends Either<A, B>()
final class Right<out A, out B>(public value: B) extends Either<A, B>()

function getLeft(e: Either<Int32, String>): Int32 =
    match e with
        case l: Left<Int32, String> => l.value
        case r: Right<Int32, String> => 0

function main(): Unit =
    let e: Either<Int32, String> = Left<Int32, String>(42)
    assert getLeft(e) == 42
    let e2: Either<Int32, String> = Right<Int32, String>("hello")
    assert getLeft(e2) == 0
"#,
    )
    .expect("sealed abstract generic class with two type params");
}

#[test]
fn test_sealed_abstract_generic_class_intermediate_hierarchy() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Expr<out T>()
sealed abstract class BinaryOp<out T>(public left: Expr<T>, public right: Expr<T>) extends Expr<T>()
final class Literal<out T>(public value: T) extends Expr<T>()
final class AddOp<out T>(left: Expr<T>, right: Expr<T>) extends BinaryOp<T>(left, right)
final class MulOp<out T>(left: Expr<T>, right: Expr<T>) extends BinaryOp<T>(left, right)

function describe(e: Expr<Int32>): String =
    match e with
        case l: Literal<Int32> => "literal"
        case b: BinaryOp<Int32> => "binary"

function main(): Unit =
    let lit: Expr<Int32> = Literal(42)
    let add: Expr<Int32> = AddOp<Int32>(Literal(1), Literal(2))
    let mul: Expr<Int32> = MulOp<Int32>(Literal(3), Literal(4))
    assert describe(lit) == "literal"
    assert describe(add) == "binary"
    assert describe(mul) == "binary"
"#,
    )
    .expect("sealed abstract generic class with intermediate hierarchy");
}

#[test]
fn test_non_generic_class_extends_generic_parent_concrete_args() {
    common::compile_and_run(
        r#"
package a

abstract class Base<out T>(public value: T)
final class IntHolder(v: Int32) extends Base<Int32>(v)
final class StringHolder(v: String) extends Base<String>(v)

function main(): Unit =
    let a = IntHolder(42)
    assert a.value == 42
    let b = StringHolder("hello")
    assert b.value == "hello"
"#,
    )
    .expect("non-generic class extends generic parent with concrete type args");
}

#[test]
fn test_generic_class_extends_generic_parent_mixed_args() {
    common::compile_and_run(
        r#"
package a

abstract class Computation<out T, out E>()
final class Pure<out T>(public value: T) extends Computation<T, Never>()
final class Fail<out E>(public error: E) extends Computation<Never, E>()
final class Both<out T, out E>(public value: T, public error: E) extends Computation<T, E>()

function main(): Unit =
    let p = Pure(42)
    assert p.value == 42
    let f = Fail("oops")
    assert f.error == "oops"
    let b = Both(1, "e")
    assert b.value == 1
    assert b.error == "e"
"#,
    )
    .expect("generic class extends generic parent with mixed type args");
}

#[test]
fn test_sealed_generic_class_mixed_extends_non_exhaustive() {
    let errors = common::compile_expecting_errors(
        r#"
package a

sealed abstract class IO<out T, out E>()
final class Succeed<out T>(public value: T) extends IO<T, Never>()
final class FailIO<out E>(public error: E) extends IO<Never, E>()
final class MapIO<out T, out E>(public source: IO<Any, Any>, public f: (Any) => T) extends IO<T, E>()

function describe(io: IO<Any, Any>): String =
    match io with
        case s: Succeed<Any> => "succeed"
        case f: FailIO<Any> => "fail"

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive") && e.contains("MapIO")),
        "expected non-exhaustive error mentioning MapIO, got: {:?}",
        errors
    );
}

#[test]
fn test_sealed_class_async_style_same_instantiation() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Async<out T, out E>()
final class AsyncSucceed<out T>(public value: T) extends Async<T, Never>()
final class AsyncFailCause<out E>(public cause: String) extends Async<Never, E>()
final class AsyncMap<out T, out E>(public inner: Async<Any, Any>, public f: (Any) => Any) extends Async<T, E>()

function main(): Unit =
    let s = AsyncSucceed(42)
    assert s.value == 42
    let f = AsyncFailCause<String>("err")
    assert f.cause == "err"
"#,
    )
    .expect("sealed class async-style with same instantiation (no variance cast)");
}

#[test]
fn test_sealed_class_cross_instantiation_variance_assignment() {
    // Cross-instantiation variance under full type erasure: `AsyncSucceed<Int32>` has
    // parent `Async<Int32, Never>` and assigns to `Async<Int32, String>` via covariance
    // (Never <: String). Phase 6 success criterion: this used to expect-failure on
    // WASM-GC codegen; now passes thanks to the always-erased class layout.
    common::compile_and_run(
        r#"
package a

sealed abstract class Async<out T, out E>()
final class AsyncSucceed<out T>(public value: T) extends Async<T, Never>()
final class AsyncFailCause<out E>(public cause: String) extends Async<Never, E>()

function main(): Unit =
    let s: Async<Int32, String> = AsyncSucceed(42)
    match s with
        case x: AsyncSucceed<Int32> => assert x.value == 42
        case _ => panic "expected succeed"
"#,
    )
    .expect("sealed-class cross-instantiation variance under full erasure");
}

#[test]
fn test_sealed_class_async_phase6_smoke() {
    // Validates the Phase 6 design pattern end-to-end: an Async-style sealed hierarchy
    // with three variants exercising both branches of variance (Succeed fixes E=Never,
    // FailCause fixes T=Never), exhaustive class-pattern matching on the erased
    // Async<Any, Any> position, and a closure-typed field in Thunk.
    common::compile_and_run(
        r#"
package a

sealed abstract class Async<out T, out E>()
final class Succeed<out T>(public value: T) extends Async<T, Never>()
final class FailCause<out E>(public cause: String) extends Async<Never, E>()
final class Thunk<out T>(public f: () => T) extends Async<T, Never>()

function describe(a: Async<Any, Any>): String =
    match a with
        case s: Succeed<Any> => "succeed"
        case f: FailCause<Any> => "fail"
        case t: Thunk<Any> => "thunk"

function main(): Unit =
    let s: Async<Int32, String> = Succeed(42)
    let f: Async<Int32, String> = FailCause<String>("err")
    let t: Async<Int32, String> = Thunk<Int32>(() => 7)
    assert describe(s) == "succeed"
    assert describe(f) == "fail"
    assert describe(t) == "thunk"
    match s with
        case x: Succeed<Int32> => assert x.value == 42
        case _ => panic "expected succeed"
    match t with
        case x: Thunk<Int32> => assert x.f() == 7
        case _ => panic "expected thunk"
"#,
    )
    .expect("phase 6 smoke: sealed Async with 3 variants, cross-instantiation, closure field");
}

#[test]
fn test_companion_module_on_sealed_abstract_class() {
    // Validates that the `module Foo<T> = ...` companion-module pattern (used by Promise
    // and friends for static constructors) attaches to a `sealed abstract` class, not
    // just to a concrete class. Needed so Phase 6 can keep `Async.succeed(...)` etc. as
    // module-static functions instead of moving them to top-level functions.
    common::compile_and_run(
        r#"
package a

sealed abstract class Foo<out T>()
final class Bar<out T>(public value: T) extends Foo<T>()

module Foo<T> =
    public function make(value: T): Foo<T> = Bar(value)

function main(): Unit =
    let f = Foo<Int32>.make(7)
    match f with
        case b: Bar<Int32> => assert b.value == 7
"#,
    )
    .expect("companion module on sealed abstract class");
}

#[test]
fn test_zero_typeparam_subclass_of_generic_parent() {
    // Regression: a `final class X() extends Foo<Int32, Never>()` (zero own type params)
    // must be assignable to `Foo<Int32, Never>` — needed by Phase 6's `MakeWaiter`,
    // `Wake`, `Interrupt` variants which fix the parent's type args at declaration.
    common::compile_and_run(
        r#"
package a

sealed abstract class Foo<out T, out E>()
final class MakeUnit() extends Foo<Int32, Never>()

function main(): Unit =
    let m: Foo<Int32, Never> = MakeUnit()
    match m with
        case u: MakeUnit => assert true
"#,
    )
    .expect("zero-typeparam subclass should assign to its generic parent");
}

// --- Phase 6 §9 integration tests: validate full type erasure on focused scenarios. ---

#[test]
fn test_phase6_nested_enum_matching() {
    // Nested enum patterns work after full erasure (Option's WASM type is the same
    // regardless of inner T).
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let opt: Option<Option<Int32>> = Some(Some(42))
    match opt with
    case Some(Some(v)) => assert v == 42
    case Some(None) => panic "expected inner Some"
    case None => panic "expected outer Some"
"#,
    )
    .expect("phase6 nested enum matching");
}

#[test]
fn test_phase6_is_as_with_type_args() {
    // Runtime type-check via base class succeeds; cast-back yields a typed binding.
    // Sealed-class `is`/`as` is base-only at runtime under full erasure.
    common::compile_and_run(
        r#"
package a

sealed abstract class Box<out T>()
final class IntBox(public value: Int32) extends Box<Int32>()
final class StrBox(public value: String) extends Box<String>()

function main(): Unit =
    let b: Box<Any> = IntBox(7)
    assert b is IntBox
    let ib = b as IntBox
    assert ib.value == 7
"#,
    )
    .expect("phase6 is/as with sealed-class hierarchy");
}

#[test]
fn test_phase6_function_typed_erased_field() {
    // Closure stored in a class field at an erased position, retrieved, invoked,
    // primitive return unboxes. Combines Phase 4 (always-erased closures) with
    // Phase 1-3 (erased class fields).
    common::compile_and_run(
        r#"
package a

sealed abstract class Cell<out T>()
final class Lazy<out T>(public f: () => T) extends Cell<T>()

function main(): Unit =
    let c: Cell<Int32> = Lazy<Int32>(() => 99)
    match c with
    case l: Lazy<Int32> => assert l.f() == 99
"#,
    )
    .expect("phase6 closure-typed erased field");
}

#[test]
fn test_phase6_recursive_generic_tree() {
    // Recursive self-reference resolves to the same erased struct (design-doc §11 item).
    common::compile_and_run(
        r#"
package a

sealed abstract class Tree<out T>()
final class Leaf<out T>(public value: T) extends Tree<T>()
final class Node<out T>(public left: Tree<T>, public right: Tree<T>) extends Tree<T>()

function sumInts(t: Tree<Int32>): Int32 =
    match t with
    case l: Leaf<Int32> => l.value
    case n: Node<Int32> => sumInts(n.left) + sumInts(n.right)

function main(): Unit =
    let t: Tree<Int32> = Node<Int32>(Leaf<Int32>(1), Node<Int32>(Leaf<Int32>(2), Leaf<Int32>(3)))
    assert sumInts(t) == 6
"#,
    )
    .expect("phase6 recursive Tree<T> with Leaf/Node hierarchy");
}

#[test]
fn test_phase6_nested_generics_double_box() {
    // Double-erased boxing/unboxing through two layers of generic structure across
    // all three Option<Option<Int32>> shapes.
    common::compile_and_run(
        r#"
package a

function classify(o: Option<Option<Int32>>): Int32 =
    match o with
    case Some(Some(v)) => v
    case Some(None) => -1
    case None => -2

function main(): Unit =
    let a: Option<Option<Int32>> = Some(Some(42))
    let b: Option<Option<Int32>> = Some(None)
    let c: Option<Option<Int32>> = None
    assert classify(a) == 42
    assert classify(b) == -1
    assert classify(c) == -2
"#,
    )
    .expect("phase6 nested generics Option<Option<Int32>>");
}

#[test]
fn test_sealed_class_child_with_record_field() {
    // Regression: a record field on a sealed-hierarchy child class must trigger TypeDef
    // discovery for the record. Phase 6's `Map`/`AndThen`/`Fold` variants carry a
    // `SourceLocation` field — codegen needs the topologically-correct emission order.
    common::compile_and_run(
        r#"
package a

record Loc = file: String

sealed abstract class Foo()
final class Wrap(public value: Int32, public loc: Loc) extends Foo()

function main(): Unit =
    let l = Loc { file = "main.dn" }
    let w: Foo = Wrap(42, l)
    match w with
        case x: Wrap =>
            assert x.value == 42
            assert x.loc.file == "main.dn"
"#,
    )
    .expect("sealed class child with record field");
}

#[test]
fn test_inherited_default_virtual_method_from_generic_parent_on_nongeneric_child() {
    // Regression: a NON-generic class extending a GENERIC abstract parent and
    // INHERITING (not overriding) a virtual method that has a DEFAULT body. The
    // inherited vtable slot's impl lives on the generic parent and must be
    // monomorphized at the child's binding (E -> Int32) and looked up with that
    // binding. Previously codegen skipped the child's vtable global, panicking
    // with "no entry found for key" when the child was constructed.
    common::compile_and_run(
        r#"
package a

abstract class Box<E>() =
    public abstract function tag(self): Int32
    // Virtual method WITH a default body (overridable, not abstract).
    public function describe(self): Int32 = self.tag() + 100

final class IntBox(public n: Int32) extends Box<Int32>() =
    public override function tag(self): Int32 = self.n

function main(): Unit =
    let b: Box<Int32> = IntBox(5)
    assert b.describe() == 105
    assert b.tag() == 5
"#,
    )
    .expect("inherited default virtual method from generic parent");
}

// ── Regression: closures inside a class initializer / extends-args.
//
// `emit_class_hierarchy` inlines the initializer and the extends-args at EVERY
// `ClassNew` site, but the closure prescan used to walk them exactly once, at
// the end of the module. Any closure there consumed an id the prescan had given
// to a different closure, and a second construction site consumed one more.
// Both tests put a same-shaped closure in `main` AFTER the construction so a
// desync swaps two closures with identical capture counts — the silent case.

#[test]
fn test_closure_in_class_initializer_constructed_twice() {
    common::compile_and_run(
        r#"
package a

class Adder(base: Int32) =
    public let add: (Int32) => Int32 = (x: Int32) => x + base

function main(): Unit =
    let ten = Adder(10)
    let hundred = Adder(100)
    let step = 5
    let scale: (Int32) => Int32 = (x: Int32) => x * step
    assert ten.add(1) == 11
    assert hundred.add(1) == 101
    assert scale(2) == 10
"#,
    )
    .expect("closure in a class initializer, class constructed at two sites");
}

#[test]
fn test_closure_in_extends_argument() {
    common::compile_and_run(
        r#"
package a

class Base(public apply: (Int32) => Int32)

class Doubler(k: Int32) extends Base((x: Int32) => x * k)

function main(): Unit =
    let d = Doubler(3)
    let step = 1
    let bump: (Int32) => Int32 = (x: Int32) => x + step
    assert d.apply(4) == 12
    assert bump(1) == 2
"#,
    )
    .expect("closure in an extends argument");
}

/// A `final` method declared on a GENERIC base class, called through a receiver
/// typed as the subclass.
///
/// Every generic template is instantiated by unifying its parameter types against
/// the concrete argument types at the call — and `Box<T>` does not unify with
/// `IntBox`. So this derived no type arguments, kept the template name, and
/// reached codegen as `missing function index for: a.Box.pair$Box<T>$Int32`.
/// Annotating the same binding `: Box<Int32>` always worked, which is why the
/// standard library never tripped over it: every caller there holds the abstract
/// type already.
#[test]
fn test_final_method_of_generic_base_through_subclass_receiver() {
    common::compile_and_run(
        r#"
package a

public abstract class Box<T> =
    public final function pair(self, n: Int32): Option<T> = self.pick(n)
    protected abstract function pick(self, n: Int32): Option<T>

class IntBox(unused: Bool) extends Box<Int32>() =
    protected override function pick(self, n: Int32): Option<Int32> = Some(n)

function main(): Unit =
    let b = IntBox(true)
    match b.pair(3) with
        case Some(v) => assert v == 3
        case None => assert false
"#,
    )
    .expect("final method of a generic base, subclass receiver");
}

/// The control: the same call through a receiver typed as the instantiated base.
/// This one always worked, and pins that the fix did not move the working case.
#[test]
fn test_final_method_of_generic_base_through_base_receiver() {
    common::compile_and_run(
        r#"
package a

public abstract class Box<T> =
    public final function pair(self, n: Int32): Option<T> = self.pick(n)
    protected abstract function pick(self, n: Int32): Option<T>

class IntBox(unused: Bool) extends Box<Int32>() =
    protected override function pick(self, n: Int32): Option<Int32> = Some(n)

function main(): Unit =
    let b: Box<Int32> = IntBox(true)
    match b.pair(3) with
        case Some(v) => assert v == 3
        case None => assert false
"#,
    )
    .expect("final method of a generic base, base receiver");
}

/// Two levels, with the type argument fixed in the middle. Climbing from `Leaf`
/// has to substitute the middle class's own argument into the parent type its
/// `extends` clause named — `Middle<Int32>`'s parent is `Box<Int32>`, not
/// `Box<A>` — which is the step a one-level walk gets away with skipping.
#[test]
fn test_final_method_of_generic_base_through_two_levels_of_subclass() {
    common::compile_and_run(
        r#"
package a

public abstract class Box<T> =
    public final function pair(self, n: Int32): Option<T> = self.pick(n)
    protected abstract function pick(self, n: Int32): Option<T>

abstract class Middle<A>(tag: Int32) extends Box<A>() =
    public function label(self): Int32 = self.tag

class Leaf(t: Int32) extends Middle<Int32>(t) =
    protected override function pick(self, n: Int32): Option<Int32> = Some(n + self.label())

function main(): Unit =
    let leaf = Leaf(10)
    match leaf.pair(3) with
        case Some(v) => assert v == 13
        case None => assert false
"#,
    )
    .expect("final method of a generic base, two levels down");
}

// ── Pinning test: generic-base-class struct-subtype codegen bug ─────

/// Pins the generic-base-class struct-subtype codegen bug: a `use` on a
/// Resource whose value is a generic ABSTRACT class (`AsyncInputStream<E>`)
/// actually held as its generic SUBCLASS (`WasiInputStream<E>`) emits a
/// component that fails wasm validation at load:
///
/// ```text
/// failed to compile: wasm[0]::function[N]::standard.io.WasiInputStream.checkAlive(WasiInputStream<E>)
/// WebAssembly translation error: type mismatch: expected (ref $type), found (ref $type)
/// ```
///
/// The broken emission is NONLOCAL — the invalid function is a monomorphized
/// *library* method (`checkAlive`), not the `use` site — and extremely
/// shape-sensitive, which is why the manifestation looks intermittent and the
/// real stream tests carry `useForever()` / abstract-annotation workarounds
/// (see the comments in standard-io/test/wasi_output_stream_test.dove and
/// output_stream_contract_test.dove ~51). Every one of these ingredients
/// was verified LOAD-BEARING while reducing (change any and the component
/// loads cleanly):
///
/// - the `use` block must NOT touch the value (a body that calls
///   `stream.read(...)`/`close()` shifted the emission enough to pass);
/// - the stream's error enum needs at least two variants;
/// - the project must compile in Test mode (`BuildMode::Build` + a `main`
///   running the same program passes);
/// - the real `Resource`/`Async` machinery is required — a fully
///   self-contained reduction (own generic abstract/subclass hierarchy plus a
///   constructor-based `Usable` impl mirroring `Resource`'s, 20 runs) never
///   failed.
///
/// The `WasiOutputStream.resource` twin fails identically (in
/// `WasiOutputStream.checkAlive`). With this exact shape the failure is
/// deterministic: 20/20 across fresh processes. The test builds a tiny
/// project against the repo's actual standard-io, the way `http_interop.rs`
/// builds against the real manifest.
#[test]
#[ignore = "generic-base-class struct-subtype codegen bug — owned by the concurrent workstream; un-ignore when their fix lands"]
fn test_use_of_resource_yielding_generic_abstract_class_as_generic_subclass() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dovetail crate lives under the workspace root")
        .to_path_buf();
    let mut workspace = dovetail::manifest::load_manifest(&repo_root)
        .unwrap_or_else(|errs| panic!("manifest load errors: {errs:?}"));

    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("main.dove"),
        r#"package usetest

import standard.io.Async
import standard.io.AsyncInputStream
import standard.io.WasiInputStream
import standard.wasi.ComponentStreamRead
import standard.wasi.CopyOperation
import standard.wasi.CopyResult
import standard.wasi.EventPayload
import standard.wasi.IoError
import standard.wasi.Waitable

enum ProbeError =
    Gone
    Canceled

implement IoError for ProbeError

// A stream end that is never actually read: nothing below posts a copy, so the
// made-up waitable is never handed to the host.
function quietSource(): ComponentStreamRead<Array<Uint8>, ProbeError> =
    ComponentStreamRead<Array<Uint8>, ProbeError> {
        readable = Waitable(0i32)
        ending = () => Ok(())
        start = (capacity: Int32) => panic "never read"
        cancel = () => EventPayload.ofCopy(CopyResult.Cancelled, 0i32)
        canceledError = ProbeError.Canceled
        stillOpen = () => true
        dropEnd = () => ()
        discardElements = (copy: CopyOperation, count: Int32) => ()
        lift = (copy: CopyOperation, count: Int32) => Array<Uint8>.empty()
    }

async function program(): Async<Unit, Never> =
    // The trigger: `use` (not `useForever`) on a Resource<AsyncInputStream<E>, _>
    // whose value was constructed as WasiInputStream<E>, with a body that does
    // not touch the value.
    let stream: AsyncInputStream<ProbeError> =
        use WasiInputStream<ProbeError>.resource<Never>(() => Async.succeed(quietSource()))
    ()

test "use of a stream resource held as the abstract stream type" =
    program().run()
"#,
    )
    .unwrap();

    workspace.projects.push(ResolvedProject {
        resolved_identity: None,
        name: ProjectName("use-pin-test".to_string()),
        root_package: PackagePath(vec!["usetest".to_string()]),
        depends: vec![ProjectName("standard-io".to_string())],
        packages: vec![ResolvedPackage {
            path: PackagePath(vec!["usetest".to_string()]),
            source_dir: src,
        }],
        project_dir: dir.path().to_path_buf(),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    });

    // Test mode, matching how `dovetail test` surfaced the failure — the same
    // program compiled in Build mode with a `main` loads cleanly, which is
    // part of this bug's shape-sensitivity.
    let result = dovetail::build_workspace(
        &workspace,
        Some("use-pin-test"),
        dovetail::BuildMode::Test,
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
        panic!("build_workspace failed:\n{}", errors.join("\n"));
    }
    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(name, _)| name == "use-pin-test")
        .expect("use-pin project in build results");
    let wasm_bytes = project_result.wasm.as_ref().expect("use-pin WASM output");

    // Under the bug this fails at component load with the wasm translation
    // error quoted above; once the codegen fix lands, the program acquires,
    // releases, and exits cleanly.
    let run = dovetail::test_runner::run_tests(wasm_bytes, &project_result.test_exports)
        .unwrap_or_else(|e| panic!("test component failed to run: {e}"));
    for test in &run.results {
        if let dovetail::test_runner::TestStatus::Fail { message } = &test.status {
            panic!("test '{}' failed: {message}", test.name);
        }
    }
}

#[test]
fn test_covariant_subclass_direct_method_argument() {
    common::compile_and_run(
        r#"
package a

sealed abstract class Effect<out T, out E>()
final class Success<out T>(public value: T) extends Effect<T, Never>()

class Reader() =
    public function read(self, effect: Effect<Any, Any>): Int32 =
        match effect with
            case success: Success<Int32> => success.value
            case _ => 0

function main(): Unit =
    assert Reader().read(Success(42)) == 42
"#,
    )
    .expect("subclass arguments upcast to the erased superclass representation");
}
