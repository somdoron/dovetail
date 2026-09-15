mod common;

const TYPES: &str = r#"
package a

@derive(Equatable)
record Account private =
    balance: Int32

module Account =
    public function make(n: Int32): Account = Account { balance = n }
    public function add(self, n: Int32): Account = self with balance = self.balance + n

record Box<T> private =
    item: T

module Box<T> =
    public function make(item: T): Box<T> = Box { item = item }
    public function explicit(item: T): Box<T> = Box<T> { item = item }
    public function replace(self, item: T): Box<T> = self with item = item

record Token private
module Token =
    public function make(): Token = Token {}

@derive(Equatable)
enum Status private =
    Open
    Number(Int32)
    Named { n: Int32 }

module Status =
    public function open(): Status = Status.Open
    public function number(n: Int32): Status = Status.Number(n)
    public function named(n: Int32): Status = Status.Named { n = n }

enum Choice<T> private =
    Empty
    Item(T)
    Named { item: T }

module Choice<T> =
    public function empty(): Choice<T> = Choice<T>.Empty
    public function item(item: T): Choice<T> = Choice.Item(item)
    public function named(item: T): Choice<T> = Choice.Named { item = item }

newtype Amount private = Int32
module Amount =
    public function make(n: Int32): Amount = Amount(n)
    public function read(self): Int32 = self.value

newtype Secret<T> private = T
module Secret<T> =
    public function make(item: T): Secret<T> = Secret(item)
    public function read(self): T =
        match self with
            case Secret(item) => item
"#;

fn assert_errors(source: &str, expected: &[&str]) {
    let result = dovetail::check(source, "test.dove");
    let messages: Vec<_> = result
        .diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect();
    for expected in expected {
        assert!(
            messages.iter().any(|message| message.contains(expected)),
            "missing {expected:?} in {messages:?} for source:\n{source}"
        );
    }
}

#[test]
fn module_construction_and_external_inspection() {
    common::compile_and_run(&format!(
        r#"{TYPES}
function read(status: Status): Int32 =
    match status with
        case Status.Open => 0
        case Status.Number(n) => n
        case Status.Named {{ n }} => n

function readChoice(choice: Choice<Int32>): Int32 =
    match choice with
        case Choice.Empty => 0
        case Choice.Item(n) => n
        case Choice.Named {{ item }} => item

function main(): Unit =
    let account = Account.make(4).add(3)
    assert account.balance == 7
    assert account == Account.make(7)
    let balance = match account with
        case Account {{ balance }} => balance
    assert balance == 7
    let box = Box.make(4).replace(8)
    assert box.item == 8
    assert Box.explicit(9).item == 9
    let item = match box with
        case Box {{ item }} => item
    assert item == 8
    let token = Token.make()
    assert read(Status.open()) == 0
    assert read(Status.number(4)) == 4
    assert read(Status.named(5)) == 5
    assert Status.number(4) == Status.number(4)
    assert readChoice(Choice<Int32>.empty()) == 0
    assert readChoice(Choice.item(6)) == 6
    assert readChoice(Choice.named(7)) == 7
    assert Amount.make(9).read() == 9
    assert Secret.make(10).read() == 10
"#
    ))
    .expect("module construction and public inspection");
}

#[test]
fn external_record_construction_and_updates_are_rejected() {
    for expression in [
        "Account { balance = 0 }",
        "Account.make(1) with balance = 0",
        "Box { item = 0 }",
        "Box<Int32> { item = 0 }",
        "Box.make(1) with item = 0",
        "Token {}",
    ] {
        assert_errors(
            &format!("{TYPES}\nfunction main(): Unit =\n    let value = {expression}\n    ()\n"),
            &["private record", "outside its associated module"],
        );
    }
}

#[test]
fn external_enum_construction_is_rejected() {
    for (ty, expression) in [
        ("Status", "Status.Open"),
        ("Status", "Status.Number(1)"),
        ("Status", "Status.Named { n = 1 }"),
        ("Choice<Int32>", "Choice.Empty"),
        ("Choice<Int32>", "Choice<Int32>.Empty"),
        ("Choice<Int32>", "Choice.Item(1)"),
        ("Choice<Int32>", "Choice<Int32>.Item(1)"),
        ("Choice<Int32>", "Choice.Named { item = 1 }"),
    ] {
        assert_errors(
            &format!(
                "{TYPES}\nfunction main(): Unit =\n    let value: {ty} = {expression}\n    ()\n"
            ),
            &[
                "cannot construct private enum",
                "outside its associated module",
            ],
        );
    }
}

#[test]
fn traits_and_extensions_have_no_private_privileges() {
    for (ty, operation, error) in [
        (
            "Account",
            "Account { balance = 0 }",
            "cannot construct private record",
        ),
        (
            "Account",
            "self with balance = 0",
            "update with 'with' on private record",
        ),
        ("Status", "Status.Open", "cannot construct private enum"),
        ("Amount", "Amount(0)", "cannot construct private newtype"),
        (
            "Amount",
            "self.value",
            "cannot access .value on private newtype",
        ),
        (
            "Amount",
            "match self with\n            case Amount(n) => n",
            "cannot pattern match on private newtype",
        ),
    ] {
        for declaration in [
            format!("trait Probe =\n    function probe(self): Unit\n\nimplement Probe for {ty}"),
            format!("extension ProbeExtension for {ty}"),
        ] {
            assert_errors(
                &format!(
                    "{TYPES}\n{declaration} =\n    public function probe(self): Unit =\n        let value = {operation}\n        ()\n"
                ),
                &[error],
            );
        }
    }
}

#[test]
fn generic_traits_have_no_private_privileges() {
    for (ty, operation, error) in [
        (
            "Box<T>",
            "Box<T> { item = self.item }",
            "cannot construct private record",
        ),
        (
            "Box<T>",
            "self with item = self.item",
            "update with 'with' on private record",
        ),
        (
            "Choice<T>",
            "Choice<T>.Empty",
            "cannot construct private enum",
        ),
        (
            "Secret<T>",
            "self.value",
            "cannot access .value on private newtype",
        ),
        (
            "Secret<T>",
            "Secret<T>(self.read())",
            "cannot construct private newtype",
        ),
        (
            "Secret<T>",
            "match self with\n            case Secret(item) => item",
            "cannot pattern match on private newtype",
        ),
    ] {
        assert_errors(
            &format!(
                "{TYPES}\ntrait Probe =\n    function probe(self): Unit\n\nimplement <T> Probe for {ty} =\n    public function probe(self): Unit =\n        let value = {operation}\n        ()\n"
            ),
            &[error],
        );
    }
}

#[test]
fn traits_can_delegate_to_module_functions() {
    common::compile_and_run(&format!(
        r#"{TYPES}
trait Read =
    function readValue(self): Int32

implement Read for Amount =
    public function readValue(self): Int32 = Amount.make(self.read()).read()

implement <T> Read for Secret<T> =
    public function readValue(self): Int32 =
        let copy = Secret.make(self.read())
        12

implement Read for Account =
    public function readValue(self): Int32 = Account.make(self.balance).add(1).balance

function main(): Unit =
    let amount = Amount.make(7)
    let secret = Secret.make(8)
    let account = Account.make(9)
    assert amount.readValue() == 7
    assert secret.readValue() == 12
    assert account.readValue() == 10
"#
    ))
    .expect("trait implementations delegate to module functions");
}

#[test]
fn generated_newtype_inspection_has_no_exemption() {
    assert_errors(
        r#"
package a
@derive(Equatable)
newtype Amount private = Int32
"#,
        &["cannot access .value on private newtype"],
    );
}

fn check_project(files: &[(&str, &str, &str)]) -> Vec<String> {
    use dovetail::common::types::PackagePath;
    use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject};
    use std::collections::BTreeMap;

    let dir = tempfile::tempdir().unwrap();
    let mut packages = BTreeMap::new();
    for (package, name, source) in files {
        let source_dir = dir.path().join(package);
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::write(source_dir.join(name), source).unwrap();
        packages.insert(*package, source_dir);
    }
    let project = ResolvedProject {
        resolved_identity: None,
        name: ProjectName("privacy".into()),
        root_package: PackagePath(vec!["a".into()]),
        depends: vec![],
        packages: packages
            .into_iter()
            .map(|(package, source_dir)| ResolvedPackage {
                path: PackagePath(vec![package.into()]),
                source_dir,
            })
            .collect(),
        project_dir: dir.path().to_path_buf(),
        main_function: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    };
    let result = dovetail::build_project(
        &project,
        dir.path(),
        &dovetail::typechecker::registry::Registry::new(),
        dovetail::typechecker::types::TypedModule::empty(),
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

const EXPORTED_TYPES: &str = r#"
package a
public record Account private =
    balance: Int32
public record Box<T> private =
    item: T
public enum Status private =
    Open
    Number(Int32)
    Named { n: Int32 }
public newtype Amount private = Int32
"#;

const EXPORTED_MODULES: &str = r#"
package a
module Box<T> =
    public function make(item: T): Box<T> = Box { item = item }
    public function replace(self, item: T): Box<T> = self with item = item
module Status =
    public function open(): Status = Status.Open
    public function number(n: Int32): Status = Status.Number(n)
    public function named(n: Int32): Status = Status.Named { n = n }
module Amount =
    public function make(n: Int32): Amount = Amount(n)
    public function read(self): Int32 = self.value
"#;

const ACCOUNT_MODULE: &str = r#"
module a.Account
public function make(balance: Int32): Account = Account { balance = balance }
public function add(self, n: Int32): Account = self with balance = self.balance + n
"#;

#[test]
fn separate_files_and_external_packages_can_use_module_apis_and_patterns() {
    let errors = check_project(&[
        ("a", "types.dove", EXPORTED_TYPES),
        ("a", "modules.dove", EXPORTED_MODULES),
        ("a", "Account.dove", ACCOUNT_MODULE),
        (
            "b",
            "caller.dove",
            r#"
package b
import a.Account
import a.Box
import a.Status
import a.Amount

function read(): Int32 =
    let account = Account.make(2).add(3)
    let box = Box.make(4).replace(5)
    let balance = match account with
        case Account { balance } => balance
    let item = match box with
        case Box { item } => item
    let n = match Status.number(7) with
        case Status.Open => 0
        case Status.Number(n) => n
        case Status.Named { n } => n
    balance + item + n + account.balance + box.item + Amount.make(8).read()
"#,
        ),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn same_named_foreign_modules_cannot_access_private_operations() {
    for (ty, operation, error) in [
        (
            "Account",
            "Account { balance = 0 }",
            "cannot construct private record",
        ),
        (
            "Account",
            "Account.make(1) with balance = 0",
            "update with 'with' on private record",
        ),
        (
            "Box",
            "Box<Int32> { item = 0 }",
            "cannot construct private record",
        ),
        (
            "Box",
            "Box.make(1) with item = 0",
            "update with 'with' on private record",
        ),
        ("Status", "Status.Open", "cannot construct private enum"),
        (
            "Status",
            "Status.Number(1)",
            "cannot construct private enum",
        ),
        (
            "Status",
            "Status.Named { n = 1 }",
            "cannot construct private enum",
        ),
        ("Amount", "Amount(0)", "cannot construct private newtype"),
        (
            "Amount",
            "Amount.make(1).value",
            "cannot access .value on private newtype",
        ),
        (
            "Amount",
            "match Amount.make(1) with\n            case Amount(n) => n",
            "cannot pattern match on private newtype",
        ),
    ] {
        let caller = format!(
            "package b\nimport a.{ty}\n\nmodule {ty} =\n    public function probe(): Unit =\n        let value = {operation}\n        ()\n"
        );
        let errors = check_project(&[
            ("a", "types.dove", EXPORTED_TYPES),
            ("a", "modules.dove", EXPORTED_MODULES),
            ("a", "Account.dove", ACCOUNT_MODULE),
            ("b", "caller.dove", &caller),
        ]);
        assert!(
            errors.iter().any(|message| message.contains(error)),
            "{operation}: {errors:?}"
        );
    }
}

#[test]
fn shorthand_enum_constructors_respect_privacy() {
    for expression in ["None", "Some(1)"] {
        assert_errors(
            &format!(
                r#"
package a

enum Option<T> private =
    None
    Some(T)

function main(): Unit =
    let value: Option<Int32> = {expression}
    ()
"#
            ),
            &["cannot construct private enum"],
        );
    }
}

#[test]
fn generated_record_and_enum_construction_has_no_exemption() {
    for (declaration, expression, error) in [
        (
            "record Item private = n: Int32",
            "Item { n = 0 }",
            "private record",
        ),
        ("enum Item private = One", "Item.One", "private enum"),
    ] {
        let source = format!(
            r#"
package a
trait Factory =
    function copy(self): Self

@derive(Factory)
{declaration}
"#
        );
        let script = format!(
            r#"`implement Factory for Item =
    public function copy(self: Item): Item = {expression}
`"#
        );
        let result = dovetail::compile_for_test_with_derives(
            &source,
            "test.dove",
            &[("a.Factory", &script)],
        );
        let errors: Vec<_> = result
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert!(
            errors.iter().any(|message| message.contains(error)),
            "{errors:?}"
        );
    }
}

#[test]
fn a_class_name_does_not_grant_associated_module_privileges() {
    assert_errors(
        r#"
package a
record Account private = balance: Int32
class Account() =
    public function probe(self): Unit =
        let account = Account { balance = 0 }
        ()
"#,
        &["cannot construct private record"],
    );
}

#[test]
fn shorthand_probe_does_not_reject_unrelated_class_constructors() {
    for (enum_name, constructor) in [("Option", "Some"), ("Result", "Ok"), ("Result", "Error")] {
        for variant in [
            "Missing".to_owned(),
            constructor.to_owned(),
            format!("{constructor} {{ value: Int32 }}"),
        ] {
            common::check_no_errors(&format!(
                r#"
package a
enum {enum_name} private = {variant}
class {constructor}(value: Int32)
function probe(): {constructor} = {constructor}(1)
"#
            ));
        }
    }
}

#[test]
fn private_prelude_list_literals_and_cons_require_the_module() {
    for expression in ["[]", "[1]", "1 :: tail"] {
        assert_errors(
            &format!(
                r#"
package standard.prelude

enum List<out T> private =
    Nil
    Cons(T, List<T>)

function probe(tail: List<Int32>): List<Int32> = {expression}
"#
            ),
            &["cannot construct private enum 'List'"],
        );
    }
}

#[test]
fn private_prelude_list_module_can_construct_and_callers_can_match() {
    common::check_no_errors(
        r#"
package standard.prelude

enum List<out T> private =
    Nil
    Cons(T, List<T>)

module List<T> =
    public function privacyEmpty(): List<T> = []
    public function privacySingleton(value: T): List<T> = [value]
    public function privacyPrepend(value: T, tail: List<T>): List<T> = value :: tail

function isEmpty(values: List<Int32>): Bool =
    match values with
        case [] => true
        case _ :: _ => false

function probe(): Bool = isEmpty(List<Int32>.privacyEmpty())
"#,
    );
}

#[test]
fn an_empty_record_does_not_widen_the_next_declarations_visibility() {
    let errors = check_project(&[
        (
            "a",
            "types.dove",
            r#"
package a
record Token
private function secret(): Int32 = 1
"#,
        ),
        (
            "a",
            "caller.dove",
            r#"
package a
function probe(): Int32 =
    let token = Token {}
    secret()
"#,
        ),
    ]);
    assert!(
        !errors.is_empty(),
        "private function must not be accessible from another file"
    );
    assert!(
        !errors.iter().any(|e| e.contains("private record")),
        "{errors:?}"
    );
}

#[test]
fn record_payload_braces_preserve_private_construction_and_inspection() {
    common::compile_and_run(
        r#"
package a

record Detail private =
    count: Int32

module Detail =
    public function make(count: Int32): Detail = Detail { count = count }
    public function wrap(count: Int32): Event = Event.Value { count = count }

enum Event =
    Value(Detail)

enum Hidden private =
    Value(Detail)

module Hidden =
    public function make(detail: Detail): Hidden = Hidden.Value(detail)

function main(): Unit =
    let event = Event.Value(Detail.make(3))
    match event with
        case Value { count } => assert count == 3
    match Detail.wrap(4) with
        case Event.Value { count } => assert count == 4
    match Hidden.make(Detail.make(5)) with
        case Hidden.Value { count } => assert count == 5
"#,
    )
    .expect("record payload sugar respects construction boundaries");
}

#[test]
fn record_payload_braces_check_both_constructor_owners() {
    assert_errors(
        r#"
package a
record Detail private =
    count: Int32

enum Event private =
    Value(Detail)

module Event =
    public function make(): Event = Event.Value { count = 1 }

module Detail =
    public function wrap(): Event = Event.Value { count = 2 }

function bad(): Event = Event.Value { count = 3 }
"#,
        &[
            "cannot construct private record 'Detail'",
            "cannot construct private enum 'Event'",
        ],
    );
}

#[test]
fn record_payload_braces_resolve_cross_package_without_payload_import() {
    let errors = check_project(&[
        (
            "a",
            "types.dove",
            r#"
package a
public record Detail<T> =
    count: T
public enum Event<T> =
    Value(Detail<T>)
"#,
        ),
        (
            "b",
            "consumer.dove",
            r#"
package b
import a.Event

record Detail =
    wrong: Bool

function read(value: Event<Int32>): Int32 =
    match value with
        case Value { count } => count

function make(): Event<Int32> = Event.Value { count = 7 }
"#,
        ),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}
