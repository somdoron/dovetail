mod common;

#[test]
fn trait_collection_preserves_associated_bound_declaration_order() {
    common::check_no_errors(r#"
package a

trait ZProducer =
    type Output
    function produce(self): Output

trait AConsumer =
    function consume<T>(self, value: T): Int32 where T: ZProducer<Output = Int32>

trait Child extends AConsumer =
    function extra(self): Int32

function main(): Unit = ()
"#);
}

#[test]
fn trait_default_bodies_cannot_shadow_enclosing_type_parameters() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Defaults<T> =
    function value(self): Int32 =
        let T = 1
        T
    property size(self): Int32 =
        let T = 2
        T
function main(): Unit = ()
"#,
    );
    assert_eq!(
        errors
            .iter()
            .filter(|error| error.contains("variable 'T' shadows type parameter 'T'"))
            .count(),
        2,
        "{errors:?}"
    );
}

#[test]
fn class_default_applications_must_not_share_incompatible_signatures() {
    let errors = common::compile_expecting_errors(
        r#"
package a
interface Chooser<T> =
    function choose(self, value: T): Int32 = 1
class Thing(x: Int32) implements Chooser<Int32> and Chooser<String> =
    function own(self): Int32 = self.x
function main(): Unit =
    let c: Chooser<String> = Thing(1)
    assert c.choose("x") == 1
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("different applications of trait 'Chooser'")),
        "{errors:?}"
    );
}

#[test]
fn class_default_property_applications_are_checked_through_supers() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait Sized<T> =
    property size(self): Int32 = 1
trait Left extends Sized<Int32> =
    function left(self): Int32 = 0
trait Right extends Sized<String> =
    function right(self): Int32 = 0
class Thing(x: Int32) implements Left and Right =
    function own(self): Int32 = self.x
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("different applications of trait 'Sized'")),
        "{errors:?}"
    );
}

#[test]
fn class_can_share_the_same_generic_default_application_through_supers() {
    common::compile_and_run(
        r#"
package a
trait Sized<T> =
    function choose(self, value: T): Int32 = 1
    property size(self): Int32 = 2
trait Left extends Sized<Int32> =
    function left(self): Int32 = 0
class Thing(x: Int32) implements Sized<Int32> and Left =
    function own(self): Int32 = self.x
function main(): Unit =
    let t = Thing(1)
    assert t.choose(3) == 1
    assert t.size == 2
"#,
    )
    .expect("the same generic default application reached twice remains one class member");
}

#[test]
fn child_signatures_can_use_inherited_associated_types() {
    common::compile_and_run(
        r#"
package a

trait Parent =
    type Item
    function get(self): Item

trait Child extends Parent =
    function other(self): Item
    property item(self): Item

record Rec =
    x: Int32

implement Child for Rec =
    type Item = Int32
    function get(self): Int32 = self.x
    function other(self): Int32 = self.x + 1
    property item(self): Int32 = self.x + 2

function main(): Unit =
    let r = Rec { x = 10 }
    assert r.other() == 11
    assert r.item == 12
"#,
    )
    .expect("child method and property signatures use the inherited associated type");
}

#[test]
fn forward_transitive_supers_expose_associated_types_and_gats() {
    common::compile_and_run(
        r#"
package a

trait Child extends Middle =
    function other(self): Item
    function wrap<U>(self, value: U): Wrapped<U>

trait Middle extends Parent =
    function middle(self): Int32

trait Parent =
    type Item
    type Wrapped<U>
    function get(self): Item

record Rec =
    x: Int32

implement Child for Rec =
    type Item = Int32
    type Wrapped<U> = List<U>
    function get(self): Int32 = self.x
    function middle(self): Int32 = self.x
    function other(self): Int32 = self.x + 1
    function wrap<U>(self, value: U): List<U> = [value]

function main(): Unit =
    let r = Rec { x = 10 }
    assert r.other() == 11
    assert r.wrap(42) == [42]
"#,
    )
    .expect("forward and transitive inheritance brings associated types and GATs into scope");
}

#[test]
fn inherited_types_preserve_each_files_import_aliases() {
    let sources = [
        (
            "child.dove",
            r#"
package a
import a.Middle as Ancestor
trait Child extends Ancestor =
    function other(self): Item
"#,
        ),
        (
            "middle.dove",
            r#"
package a
import a.Parent as Ancestor
trait Middle extends Ancestor =
    function middle(self): Item
"#,
        ),
        (
            "parent.dove",
            r#"
package a
trait Parent =
    type Item
    function get(self): Item
"#,
        ),
    ];
    let files = sources
        .into_iter()
        .map(|(path, source)| {
            let (file, diagnostics) =
                dovetail::discovery::parse_source(source, std::sync::Arc::from(path));
            assert!(!diagnostics.has_errors());
            file
        })
        .collect();
    let package = dovetail::parser::ast::PackageAst {
        package_path: dovetail::common::types::PackagePath(vec!["a".into()]),
        files,
    };
    let result =
        dovetail::typechecker::typecheck(&package, &dovetail::typechecker::registry::Registry::new());
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    assert!(!result.has_errors(), "{errors:?}");
}
