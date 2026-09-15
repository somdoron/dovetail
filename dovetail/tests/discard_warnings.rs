use dovetail::common::diagnostics::{Diagnostic, Severity};

fn warnings(source: &str) -> Vec<Diagnostic> {
    let result = dovetail::check(source, "src/discard.dove");
    assert!(!result.has_errors(), "{:?}", result.diagnostics);
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .cloned()
        .collect()
}

#[test]
fn result_discard_warns_and_compilation_succeeds() {
    let source = r#"
package a
function attempt(): Result<Int32, String> = Error("failed")
function main(): Unit =
    attempt()
    ()
"#;
    let diagnostics = warnings(source);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("discarded Result"));
    assert!(diagnostics[0].message.contains("let _ ="));
    assert_eq!(diagnostics[0].span.line, 5);
    assert_eq!(diagnostics[0].span.column, 5);
    assert_eq!(diagnostics[0].span.end_column, 13);
    let compiled = dovetail::compile(source, "src/discard.dove");
    assert!(
        !compiled.diagnostics.has_errors(),
        "{:?}",
        compiled.diagnostics
    );
    assert!(compiled.wasm.is_some());
    assert_eq!(
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.message.contains("discarded Result"))
            .count(),
        1
    );
}

#[test]
fn aliases_and_never_errors_keep_the_result_identity() {
    let diagnostics = warnings(
        r#"
package a
type Outcome<T, E> = Result<T, E>
type Answer = Outcome<Int32, Never>
function attempt(): Answer = Ok(1)
function main(): Unit =
    attempt()
    ()
"#,
    );
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("discarded Result"));
}

#[test]
fn unrelated_names_and_ordinary_discards_are_valid() {
    assert!(
        warnings(
            r#"
package a
record Result =
    value: Int32
record Async =
    value: Int32
newtype Resource = Int32
function main(): Unit =
    Result { value = 1 }
    Async { value = 2 }
    Resource(3)
    42
    let mutable counter = 0
    counter = counter + 1
    "builder".length
    ()
"#
        )
        .is_empty()
    );
}

#[test]
fn consumption_and_acknowledgement_only_suppress_the_final_value() {
    let diagnostics = warnings(
        r#"
package a
function attempt(): Result<Int32, String> = Ok(1)
function accept(value: Result<Int32, String>): Unit = ()
function returned(): Result<Int32, String> = attempt()
function main(): Unit =
    let mutable value = attempt()
    value = attempt()
    accept(attempt())
    let _ = attempt()
    let _ =
        attempt()
        attempt()
    let (ignored, _) = (attempt(), attempt())
    ()
"#,
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].span.line, 12);
}

#[test]
fn nested_control_flow_reports_each_discarded_producer_once() {
    let diagnostics = warnings(
        r#"
package a
function attempt(): Result<Int32, String> = Ok(1)
function main(): Unit =
    if true then
        attempt()
        attempt()
    else
        match false with
        case true => attempt()
        case false => attempt()
    let consumed =
        if true then attempt() else attempt()
    let closure = () =>
        attempt()
        ()
    ()
"#,
    );
    let lines: Vec<_> = diagnostics.iter().map(|d| d.span.line).collect();
    assert_eq!(lines, [6, 7, 10, 11, 15]);
}

#[test]
fn lsp_delivers_warning_severity_and_expression_range() {
    let result = dovetail::check(
        r#"
package a
function main(): Unit =
    let result: Result<Int32, String> = Ok(1)
    result
    ()
"#,
        "src/discard.dove",
    );
    assert!(!result.has_errors());
    let grouped = dovetail::lsp::diagnostics::group_diagnostics_by_file(
        &result.diagnostics,
        std::path::Path::new("/"),
    );
    let uri = tower_lsp::lsp_types::Url::parse("file:///src/discard.dove").unwrap();
    let diagnostics = &grouped[&uri];
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].severity,
        Some(tower_lsp::lsp_types::DiagnosticSeverity::WARNING)
    );
    assert_eq!(
        diagnostics[0].range,
        tower_lsp::lsp_types::Range::new(
            tower_lsp::lsp_types::Position::new(4, 4),
            tower_lsp::lsp_types::Position::new(4, 10)
        )
    );
}

#[test]
fn real_async_and_resource_types_warn_through_workspace_and_lsp() {
    use std::collections::HashMap;
    use std::path::Path;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let workspace = dovetail::manifest::load_manifest(root).expect("real workspace manifest");
    // Overlay an existing test-only file: use actual standard.io definitions
    // without modifying the workspace or maintaining imitation async types.
    let path = "standard-io/src/asyncFunctionTest.dove";
    let source = include_str!("fixtures/discardValues.dove");
    let overlays = HashMap::from([(path.to_owned(), source.to_owned())]);
    let result = dovetail::build_workspace(
        &workspace,
        Some("standard-io"),
        dovetail::BuildMode::Check,
        &overlays,
        false,
        None,
    );
    assert!(!result.diagnostics.has_errors(), "{:?}", result.diagnostics);
    let diagnostics: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.span.file.as_ref() == path)
        .collect();
    let mut expected_count = 0;
    for (index, line) in source.lines().enumerate() {
        let Some((_, expectation)) = line.split_once("// warning: ") else {
            continue;
        };
        let expected = if expectation.contains("twice") { 2 } else { 1 };
        expected_count += expected;
        let found: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.span.line == index as u32 + 1)
            .collect();
        assert_eq!(found.len(), expected, "line {}: {diagnostics:?}", index + 1);
        for diagnostic in found {
            assert_eq!(diagnostic.severity, Severity::Warning);
            assert!(
                diagnostic
                    .message
                    .contains(expectation.split_whitespace().next().unwrap())
            );
            if expectation.starts_with("Async ") {
                assert_eq!(
                    diagnostic.message.contains("await"),
                    expectation.contains(" async")
                );
            }
        }
    }
    assert_eq!(diagnostics.len(), expected_count, "{diagnostics:?}");
    let grouped = dovetail::lsp::diagnostics::group_diagnostics_by_file(&result.diagnostics, root);
    let uri = tower_lsp::lsp_types::Url::from_file_path(root.join(path)).unwrap();
    assert_eq!(grouped[&uri].len(), expected_count);
    for diagnostic in &grouped[&uri] {
        assert_eq!(
            diagnostic.severity,
            Some(tower_lsp::lsp_types::DiagnosticSeverity::WARNING)
        );
        assert!(diagnostic.range.end.character > diagnostic.range.start.character);
    }
}

#[test]
fn declaration_bodies_and_copied_defaults_are_checked_once() {
    let diagnostics = warnings(
        r#"
package a
function attempt(): Result<Int32, String> = Ok(1)
let global =
    attempt()
    1
trait Discarding =
    function work(self): Unit =
        attempt()
        ()
record First =
    value: Int32
record Second =
    value: Int32
implement Discarding for First
implement Discarding for Second
class Worker() =
    attempt()
    public function work(self): Unit =
        attempt()
        ()
    public property value(self): Int32 =
        attempt()
        1
module Helpers =
    function work(): Unit =
        attempt()
        ()
extension DiscardHelper for Int32 =
    function work(self): Unit =
        attempt()
        ()
test "discard in test" =
    attempt()
    ()
function main(): Unit = ()
"#,
    );
    assert_eq!(diagnostics.len(), 8, "{diagnostics:?}");
    let unique: std::collections::BTreeSet<_> = diagnostics.iter().map(|d| d.span.line).collect();
    assert_eq!(unique.len(), diagnostics.len());
}

#[test]
fn cli_prints_warnings_without_failing_check_or_build() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("example/src")).unwrap();
    std::fs::write(
        directory.path().join("Dovetail.toml"),
        r#"
compiler-version = "0.1.0"
[[project]]
name = "example"
root_package = "example"
packages = ["."]
"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("example/src/main.dove"),
        r#"
package example
function attempt(): Result<Int32, String> = Error("failure")
function main(): Unit =
    attempt()
    ()
"#,
    )
    .unwrap();
    for command in ["check", "build"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_dovetail"))
            .current_dir(directory.path())
            .arg(command)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{command}: {stderr}");
        assert_eq!(
            stderr.matches("warning: discarded Result").count(),
            1,
            "{stderr}"
        );
    }
}
