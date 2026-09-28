use dovetail::manifest::ResolveOptions;
use dovetail::query::{QueryArgs, QueryCommand, QueryResult, execute};
use std::path::Path;

fn copy_directory(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn workspace() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    copy_directory(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/query"),
        directory.path(),
    );
    let manifest = directory.path().join("Dovetail.toml");
    let source = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(manifest, source.replace("0.1.4", env!("CARGO_PKG_VERSION"))).unwrap();
    directory
}

fn query(root: &Path, command: QueryCommand) -> QueryResult {
    execute(
        root,
        &QueryArgs {
            project: None,
            all: false,
            command,
        },
        &ResolveOptions {
            offline: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn definition(root: &Path, name: &str) -> QueryResult {
    query(
        root,
        QueryCommand::Definition {
            name: name.to_owned(),
        },
    )
}

fn assert_success(result: &QueryResult) {
    assert!(
        result.success,
        "{}\n{:?}",
        result.output, result.diagnostics
    );
}

#[test]
fn definition_pairs_source_type_and_module_and_keeps_related_contracts() {
    let workspace = workspace();
    let result = definition(workspace.path(), "example.api.Connection");
    assert_success(&result);
    for expected in [
        "public record Connection",
        "module Connection",
        "@derive(Equatable)",
        "implement Source for Connection",
        "type Item = Int32",
        "ready supplied by",
        "import example.api.ConnectionText",
        "application-defined identifier",
        "implementationDetail",
    ] {
        assert!(
            result.output.contains(expected),
            "missing {expected}: {}",
            result.output
        );
    }
    assert!(!result.output.contains("secret body"));
    assert!(!result.output.contains("self.id"));
    assert!(!result.output.contains("implement Equatable for Descriptor"));
    assert!(!result.output.contains("id: 0"));
}

#[test]
fn member_lookup_keeps_overloads_docs_and_container() {
    let workspace = workspace();
    let result = definition(workspace.path(), "example.api.Connection.open");
    assert_success(&result);
    assert!(result.output.contains("open(id: Int32): Connection"));
    assert!(result.output.contains("open(): Connection"));
    assert!(result.output.contains("module Connection"));
    assert!(result.output.contains("Open the default connection"));
    assert!(!result.output.contains("implementationDetail"));
}

#[test]
fn inferred_types_aliases_and_private_construction_keep_their_meaning() {
    let workspace = workspace();
    for (name, expected) in [
        ("inferred", "let inferred: String"),
        ("answer", "let answer: Int32"),
        ("Identifier", "type Identifier = Int32"),
        ("Secret", "newtype Secret private = String"),
    ] {
        let result = definition(workspace.path(), &format!("example.api.{name}"));
        assert_success(&result);
        assert!(result.output.contains(expected), "{}", result.output);
    }
}

#[test]
fn package_browsing_is_shallow_and_search_is_ranked_and_paginated() {
    let workspace = workspace();
    let result = query(
        workspace.path(),
        QueryCommand::Package {
            name: Some("example.api".to_owned()),
        },
    );
    assert_success(&result);
    assert!(result.output.contains("example.api.child  [package]"));
    assert!(!result.output.contains("Connection.open"));
    let command = |offset| QueryCommand::Search {
        text: "CONNECTION".to_owned(),
        package: Some("example.api".to_owned()),
        limit: 1,
        offset,
    };
    let first = query(workspace.path(), command(0));
    assert_success(&first);
    assert!(
        first
            .output
            .starts_with("example.api.Connection  [module + record]")
    );
    assert!(first.output.contains("more results"));
    let second = query(workspace.path(), command(1));
    assert_ne!(first.output, second.output);
    assert_eq!(first.output, query(workspace.path(), command(0)).output);
}

#[test]
fn dependency_definition_ignores_broken_consumers_and_partial_results_fail() {
    let workspace = workspace();
    let app = workspace.path().join("app/src/main.dove");
    let source = std::fs::read_to_string(&app)
        .unwrap()
        .replace(" = ()", " = missingValue");
    std::fs::write(&app, source).unwrap();
    assert_success(&definition(workspace.path(), "example.api.Connection"));
    let result = definition(workspace.path(), "example.app.main");
    assert!(!result.success);
    assert!(result.output.contains("INCOMPLETE"));
    assert!(result.output.contains("function main"));
    assert!(result.diagnostics.has_errors());
}

#[test]
fn embedded_prelude_works_without_or_with_an_invalid_manifest() {
    let workspace = tempfile::tempdir().unwrap();
    for invalid in [false, true] {
        if invalid {
            std::fs::write(workspace.path().join("Dovetail.toml"), "invalid").unwrap();
        }
        let result = definition(workspace.path(), "standard.prelude.Array");
        assert_success(&result);
        assert!(result.output.contains("Compiler-built-in type Array<T>"));
        assert!(result.output.contains("module Array<T>"));
        assert!(result.output.contains("<prelude>/Array.dove"));
    }
}

#[test]
fn missing_targets_fail_but_empty_searches_succeed() {
    let workspace = tempfile::tempdir().unwrap();
    assert!(!definition(workspace.path(), "standard.prelude.DoesNotExist").success);
    assert_success(&query(
        workspace.path(),
        QueryCommand::Search {
            text: "doesNotExist".to_owned(),
            package: None,
            limit: 50,
            offset: 0,
        },
    ));
    assert!(
        !query(
            workspace.path(),
            QueryCommand::Package {
                name: Some("absent".to_owned())
            }
        )
        .success
    );
}

#[test]
fn class_fields_inheritance_and_associated_bounds_are_visible() {
    let workspace = workspace();
    for (name, expected) in [
        ("Counter", "private let label: String"),
        ("echo", "function echo(self: Int32): Int32"),
        ("Child.label", "private let label: Int32"),
        (
            "Box.get",
            "Enclosing type bounds: T: standard.prelude.Display",
        ),
        ("Outcome.Metadata.code", "code: Int32"),
        ("Counter.total", "public let static mutable total: Int32"),
        (
            "Connection.item",
            "public function item(self: Connection): Int32",
        ),
        ("Counter.value", "mutable value: Int32"),
        (
            "Child",
            "Inherited members: dovetail query definition example.api.Counter",
        ),
        (
            "NamedReadable",
            "Inherited members: dovetail query definition example.api.Readable",
        ),
        ("add", "where L: Add<R, Output = O>"),
        ("defaultReturn", "function defaultReturn(): Unit"),
    ] {
        let result = definition(workspace.path(), &format!("example.api.{name}"));
        assert_success(&result);
        assert!(result.output.contains(expected), "{}", result.output);
    }
    assert!(!workspace.path().join("build").exists());
}

#[test]
fn parse_errors_preserve_recoverable_source_declarations() {
    let workspace = workspace();
    let file = workspace.path().join("library/src/types.dove");
    let source = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, format!("{source}\n@\n")).unwrap();
    let result = definition(workspace.path(), "example.api.Identifier");
    assert!(!result.success);
    assert!(result.output.contains("INCOMPLETE"));
    assert!(result.output.contains("type Identifier = Int32"));
}

#[test]
fn project_selection_narrows_package_discovery() {
    let workspace = workspace();
    let args = QueryArgs {
        project: Some("library".to_owned()),
        all: false,
        command: QueryCommand::Package { name: None },
    };
    let result = execute(workspace.path(), &args, &ResolveOptions::default()).unwrap();
    assert_success(&result);
    assert!(result.output.contains("example.api"));
    assert!(!result.output.contains("example.app"));
}

#[test]
fn cli_returns_results_on_stdout_and_errors_with_failure_status() {
    let workspace = workspace();
    let run = |arguments: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_dovetail"))
            .current_dir(workspace.path())
            .args(arguments)
            .output()
            .unwrap()
    };
    let result = run(&[
        "--offline",
        "--locked",
        "query",
        "definition",
        "example.api.Connection.open",
        "--project",
        "app",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("open(id: Int32)"));
    assert!(
        !run(&["query", "definition", "example.api.absent"])
            .status
            .success()
    );
    let invalid = run(&["query", "search", "Connection", "--limit", "0"]);
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
}
