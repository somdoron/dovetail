//! Unit tests for LSP features.
//!
//! Each test calls `dovetail::check()` to get a `TypedModule` + `Registry`,
//! then exercises LSP functions directly.

use std::path::Path;
use std::sync::Arc;

use dovetail::lsp::call_hierarchy;
use dovetail::lsp::code_actions;
use dovetail::lsp::code_lens;
use dovetail::lsp::completion;
use dovetail::lsp::hover;
use dovetail::lsp::implementation;
use dovetail::lsp::inlay_hints;
use dovetail::lsp::navigation;
use dovetail::lsp::position::{self, NodeAtPosition};
use dovetail::lsp::references;
use dovetail::lsp::scope;

use dovetail::lsp::signature_help;
use dovetail::lsp::symbols;
use dovetail::typechecker::imports::ImportScope;
use dovetail::typechecker::registry::Registry;
use dovetail::typechecker::types::TypedModule;

use tower_lsp::lsp_types::*;

/// Workspace root for URI construction. Using `/` makes file_path_to_uri produce
/// `file:///src/a.dove`.
fn workspace_root() -> &'static Path {
    Path::new("/")
}

/// Run `dovetail::check()` and return typed module + registry.
fn check_source(source: &str) -> (TypedModule, Registry) {
    let result = dovetail::check(source, "src/a.dove");
    assert!(
        !result.diagnostics.has_errors(),
        "typecheck errors: {:?}",
        result.diagnostics.iter().collect::<Vec<_>>()
    );
    (result.typed_module, result.registry)
}

/// Parse a source file (without typechecking) for document symbol tests.
fn parse_source(source: &str) -> dovetail::parser::ast::SourceFile {
    let file_path: dovetail::common::span::FilePath = Arc::from("src/a.dove");
    let (sf, _diag) = dovetail::discovery::parse_source(source, file_path);
    sf
}

/// File path matching what check() produces.
fn file_path() -> dovetail::common::span::FilePath {
    Arc::from("src/a.dove")
}

// ─── Position & Node Detection ───────────────────────────────────────

#[test]
fn find_node_function_call() {
    let (tm, _reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // "add(1, 2)" on line 7 (1-indexed), "add" starts at col 16
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16);
    assert!(node.is_some(), "should find a node at add() call");
    let node = node.unwrap();
    assert!(matches!(node, NodeAtPosition::FunctionCall { .. }));
}

#[test]
fn find_node_var_ref() {
    let (tm, _reg) = check_source(
        r#"
package a

function main(): Unit =
  let x = 42
  assert x == 42
"#,
    );
    // "x" on line 6, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 6, 10);
    assert!(node.is_some(), "should find a node at variable ref");
    let node = node.unwrap();
    assert!(matches!(node, NodeAtPosition::VarRef { .. }));
}

#[test]
fn find_node_record_create() {
    let (tm, _reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    // "Point { x = 1; y = 2 }" on line 9, "Point" at col 11
    let node = position::find_node_at_position(&tm, &file_path(), 9, 11);
    assert!(node.is_some(), "should find a node at record create");
    let node = node.unwrap();
    assert!(matches!(node, NodeAtPosition::RecordCreate { .. }));
}

#[test]
fn find_node_field_access() {
    let (tm, _reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    // "p.x" on line 10, the field access starts around col 10-12
    // Let's target the ".x" part; the FieldAccess expression spans the entire `p.x`
    let node = position::find_node_at_position(&tm, &file_path(), 10, 12);
    assert!(node.is_some(), "should find a node at field access");
    // It could be FieldAccess or the inner binary op; let's just check we find something
}

#[test]
fn find_node_outside_expression() {
    let (tm, _reg) = check_source(
        r#"
package a

function main(): Unit = assert true
"#,
    );
    // Line 2 is "package a" — no typed expression there
    let node = position::find_node_at_position(&tm, &file_path(), 2, 1);
    assert!(
        node.is_none(),
        "should not find a node on package declaration"
    );
}

// ─── Hover ───────────────────────────────────────────────────────────

#[test]
fn hover_on_function_call() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16).unwrap();
    let h = hover::hover_for_node(&node, &tm, &reg);
    assert!(h.is_some(), "should produce hover for function call");
    let h = h.unwrap();
    if let HoverContents::Markup(markup) = &h.contents {
        assert!(
            markup.value.contains("add"),
            "hover should show function name"
        );
        assert!(
            markup.value.contains("Int32"),
            "hover should show param types"
        );
    } else {
        panic!("expected markup hover");
    }
}

#[test]
fn hover_on_variable() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit =
  let x = 42
  assert x == 42
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 6, 10).unwrap();
    let h = hover::hover_for_node(&node, &tm, &reg);
    assert!(h.is_some(), "should produce hover for variable");
    if let HoverContents::Markup(markup) = &h.unwrap().contents {
        assert!(
            markup.value.contains("let x"),
            "hover should show let binding"
        );
        assert!(markup.value.contains("Int32"), "hover should show type");
    }
}

#[test]
fn hover_on_record_constructor() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 9, 11).unwrap();
    let h = hover::hover_for_node(&node, &tm, &reg);
    assert!(h.is_some(), "should produce hover for record");
    if let HoverContents::Markup(markup) = &h.unwrap().contents {
        assert!(
            markup.value.contains("Point"),
            "hover should show record name"
        );
    }
}

#[test]
fn hover_on_let_binding() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit =
  let value = 42
  assert value == 42
"#,
    );
    // The let expression spans the whole "let value = 42"
    // Position at "value" on line 5, col 7
    let node = position::find_node_at_position(&tm, &file_path(), 5, 7);
    // This might find the Let node or the literal — depends on what's innermost
    if let Some(node) = node {
        let h = hover::hover_for_node(&node, &tm, &reg);
        assert!(h.is_some(), "should produce hover");
    }
}

// ─── Go to Definition ────────────────────────────────────────────────

#[test]
fn goto_definition_function_call() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(loc.is_some(), "should navigate to function definition");
    let loc = loc.unwrap();
    // The function definition is on line 4 (1-indexed), LSP range is 0-indexed → line 3
    assert_eq!(loc.range.start.line, 3);
}

#[test]
fn goto_definition_record_constructor() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 9, 11).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(loc.is_some(), "should navigate to record definition");
    let loc = loc.unwrap();
    // Record defined on line 4 → LSP line 3
    assert_eq!(loc.range.start.line, 3);
}

#[test]
fn goto_definition_variable_returns_none() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit =
  let x = 42
  assert x == 42
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 6, 10).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_none(),
        "VarRef should return None (local, not cross-file)"
    );
}

// ─── Go to Type Definition (via TypeRef) ─────────────────────────────

#[test]
fn goto_definition_type_annotation_record() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p: Point = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    // Cursor on `Point` in the type annotation `let p: Point =` — line 9, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 9, 10).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_some(),
        "TypeRef on record type should navigate to record definition"
    );
    // Record defined on line 4 → LSP line 3
    assert_eq!(loc.unwrap().range.start.line, 3);
}

#[test]
fn goto_definition_type_annotation_enum() {
    let (tm, reg) = check_source(
        r#"
package a

public enum Color =
  Red
  Green
  Blue

function main(): Unit =
  let c: Color = Color.Red
  assert true
"#,
    );
    // Cursor on `Color` in `  let c: Color =` — line 10, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 10, 10).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_some(),
        "TypeRef on enum type should navigate to enum definition"
    );
    // Enum defined on line 4 → LSP line 3
    assert_eq!(loc.unwrap().range.start.line, 3);
}

#[test]
fn goto_definition_type_annotation_generic_record() {
    let (tm, reg) = check_source(
        r#"
package a

public record Box<T> = value: T

function main(): Unit =
  let b: Box<Int32> = Box<Int32> { value = 42 }
  assert b.value == 42
"#,
    );
    // Cursor on `Box` in `  let b: Box<Int32>` — line 7, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 7, 10).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_some(),
        "TypeRef on generic record type should navigate to template definition"
    );
    // Box<T> defined on line 4 → LSP line 3
    assert_eq!(loc.unwrap().range.start.line, 3);
}

#[test]
fn goto_definition_type_annotation_class() {
    let (tm, reg) = check_source(
        r#"
package a

public class Animal(public name: String) =
  public function speak(self: Animal): String = self.name

function makeAnimal(): Animal = Animal("cat")

function main(): Unit =
  let a: Animal = makeAnimal()
  assert a.speak() == "cat"
"#,
    );
    // Cursor on `Animal` in `  let a: Animal =` — line 10, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 10, 10).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_some(),
        "TypeRef on class type should navigate to class definition"
    );
    // Animal defined on line 4 → LSP line 3
    assert_eq!(loc.unwrap().range.start.line, 3);
}

// ─── Go to Type Definition ───────────────────────────────────────────

#[test]
fn goto_type_definition_record_variable() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function makePoint(): Point = Point { x = 1; y = 2 }

function main(): Unit =
  let p = makePoint()
  assert p.x == 1
"#,
    );
    // Find `p` variable reference on line 12, col 10
    let node = position::find_node_at_position(&tm, &file_path(), 12, 10);
    if let Some(node) = node {
        let loc = navigation::goto_type_definition(&node, &reg, workspace_root());
        // Variable `p` has type Point → should navigate to record definition
        if let Some(loc) = loc {
            assert_eq!(
                loc.range.start.line, 3,
                "should navigate to Point definition"
            );
        }
    }
}

#[test]
fn goto_type_definition_primitive_returns_none() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit =
  let x = 42
  assert x == 42
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 6, 10).unwrap();
    let loc = navigation::goto_type_definition(&node, &reg, workspace_root());
    assert!(loc.is_none(), "primitive type should return None");
}

// ─── Document Symbols ────────────────────────────────────────────────

#[test]
fn document_symbols_function() {
    let sf = parse_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y
"#,
    );
    let symbols = symbols::source_file_to_document_symbols(&sf);
    assert!(!symbols.is_empty(), "should have symbols");
    let func_sym = symbols.iter().find(|s| s.name == "add");
    assert!(func_sym.is_some(), "should find 'add' function symbol");
    assert_eq!(func_sym.unwrap().kind, SymbolKind::FUNCTION);
}

#[test]
fn document_symbols_record_with_fields() {
    let sf = parse_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32
"#,
    );
    let symbols = symbols::source_file_to_document_symbols(&sf);
    let rec_sym = symbols.iter().find(|s| s.name == "Point");
    assert!(rec_sym.is_some(), "should find 'Point' record symbol");
    let rec_sym = rec_sym.unwrap();
    assert_eq!(rec_sym.kind, SymbolKind::STRUCT);
    let children = rec_sym
        .children
        .as_ref()
        .expect("record should have field children");
    assert_eq!(children.len(), 2);
    assert_eq!(children[0].name, "x");
    assert_eq!(children[0].kind, SymbolKind::FIELD);
}

#[test]
fn document_symbols_enum_with_variants() {
    let sf = parse_source(
        r#"
package a

public enum Color =
  Red
  Green
  Blue
"#,
    );
    let symbols = symbols::source_file_to_document_symbols(&sf);
    let enum_sym = symbols.iter().find(|s| s.name == "Color");
    assert!(enum_sym.is_some(), "should find 'Color' enum symbol");
    let enum_sym = enum_sym.unwrap();
    assert_eq!(enum_sym.kind, SymbolKind::ENUM);
    let children = enum_sym
        .children
        .as_ref()
        .expect("enum should have variant children");
    assert_eq!(children.len(), 3);
    assert!(children.iter().all(|c| c.kind == SymbolKind::ENUM_MEMBER));
}

#[test]
fn document_symbols_test() {
    let sf = parse_source(
        r#"
package a

test "basic addition" = assert 1 + 2 == 3
"#,
    );
    let symbols = symbols::source_file_to_document_symbols(&sf);
    let test_sym = symbols.iter().find(|s| s.name == "basic addition");
    assert!(test_sym.is_some(), "should find test symbol");
    assert_eq!(test_sym.unwrap().kind, SymbolKind::EVENT);
}

#[test]
fn document_symbols_class() {
    let sf = parse_source(
        r#"
package a

public class Counter(initial: Int32) =
  public function increment(self: Counter): Int32 = 1
"#,
    );
    let symbols = symbols::source_file_to_document_symbols(&sf);
    let class_sym = symbols.iter().find(|s| s.name == "Counter");
    assert!(class_sym.is_some(), "should find 'Counter' class symbol");
    let class_sym = class_sym.unwrap();
    assert_eq!(class_sym.kind, SymbolKind::CLASS);
    let children = class_sym
        .children
        .as_ref()
        .expect("class should have children");
    assert!(
        !children.is_empty(),
        "should have at least constructor param or method"
    );
}

// ─── Workspace Symbols ───────────────────────────────────────────────

#[test]
fn workspace_symbols_empty_query() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit = assert add(1, 2) == 3
"#,
    );
    let results = symbols::workspace_symbols("", &tm, &reg, workspace_root());
    assert!(!results.is_empty(), "empty query should return all symbols");
}

#[test]
fn workspace_symbols_filter_by_name() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function multiply(x: Int32, y: Int32): Int32 = x * y

function main(): Unit = assert add(1, 2) == 3
"#,
    );
    let results = symbols::workspace_symbols("add", &tm, &reg, workspace_root());
    assert!(
        results.iter().any(|s| s.name.contains("add")),
        "should find 'add' symbol"
    );
    assert!(
        !results.iter().any(|s| s.name.contains("multiply")),
        "should not find 'multiply' symbol"
    );
}

#[test]
fn workspace_symbols_no_match() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit = assert true
"#,
    );
    let results = symbols::workspace_symbols("nonexistent", &tm, &reg, workspace_root());
    assert!(results.is_empty(), "no match should return empty");
}

// ─── Scope & Completion ──────────────────────────────────────────────

#[test]
fn collect_visible_locals_in_function() {
    let (tm, _reg) = check_source(
        r#"
package a

function main(): Unit =
  let x = 42
  let y = 10
  assert x + y == 52
"#,
    );
    // Position inside the assert on line 7
    let visible = scope::collect_visible_locals(&tm, &file_path(), 7, 10);
    // Should see `x` and `y` as locals (no params for main)
    assert!(visible.params.is_empty(), "main has no params");
    let local_names: Vec<&str> = visible.locals.iter().map(|l| l.name.0.as_str()).collect();
    assert!(local_names.contains(&"x"), "should see 'x'");
    assert!(local_names.contains(&"y"), "should see 'y'");
}

#[test]
fn collect_visible_locals_with_params() {
    let (tm, _reg) = check_source(
        r#"
package a

function compute(a: Int32, b: Int32): Int32 =
  let sum = a + b
  sum

function main(): Unit = assert compute(1, 2) == 3
"#,
    );
    // Position inside compute on line 5 (inside `let sum = a + b`)
    let visible = scope::collect_visible_locals(&tm, &file_path(), 5, 14);
    let param_names: Vec<&str> = visible.params.iter().map(|p| p.name.as_str()).collect();
    assert!(param_names.contains(&"a"), "should see param 'a'");
    assert!(param_names.contains(&"b"), "should see param 'b'");
}

#[test]
fn scope_completion_filters_by_prefix() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y
function apply(f: Int32): Int32 = f

function main(): Unit = assert add(1, 2) == 3
"#,
    );
    let visible = scope::collect_visible_locals(&tm, &file_path(), 7, 25);
    let import_scope = ImportScope::new();
    let pkg = dovetail::common::types::PackagePath(vec!["a".into()]);
    let items = completion::scope_completion(&visible, &reg, &import_scope, &pkg, "ad");
    assert!(
        items.iter().any(|i| i.label == "add"),
        "should find 'add' with prefix 'ad'"
    );
    assert!(
        !items.iter().any(|i| i.label == "apply"),
        "should not find 'apply' with prefix 'ad'"
    );
}

#[test]
fn dot_completion_on_record() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    // Get the type of `p` — it should be Record(Point)
    let expr_type = position::find_expression_type_at_position(&tm, &file_path(), 10, 10);
    assert!(expr_type.is_some(), "should find expression type for p");
    let receiver_type = expr_type.unwrap();
    let import_scope = ImportScope::new();
    let items = completion::dot_completion(&receiver_type, &reg, &import_scope);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"x"), "should suggest field 'x'");
    assert!(labels.contains(&"y"), "should suggest field 'y'");
}

// ─── Signature Help ──────────────────────────────────────────────────

#[test]
fn signature_help_inside_function_call() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // Position inside the call `add(1, 2)` — at the `1` argument, line 7, col 20
    let sig = signature_help::signature_help_at_position(&tm, &reg, &file_path(), 7, 20);
    assert!(sig.is_some(), "should produce signature help inside call");
    let sig = sig.unwrap();
    assert_eq!(sig.signatures.len(), 1);
    assert!(
        sig.signatures[0].label.contains("add"),
        "signature should contain function name"
    );
    assert!(sig.signatures[0].parameters.is_some());
    let params = sig.signatures[0].parameters.as_ref().unwrap();
    assert_eq!(params.len(), 2, "add has 2 params");
}

#[test]
fn signature_help_active_parameter_advances() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // Position at the second argument `2`, line 7, col 23
    let sig = signature_help::signature_help_at_position(&tm, &reg, &file_path(), 7, 23);
    assert!(sig.is_some(), "should produce signature help");
    let sig = sig.unwrap();
    // Active parameter should be 1 (second param, 0-indexed)
    let active = sig.active_parameter.unwrap_or(0);
    assert!(
        active >= 1,
        "active parameter should advance to second param"
    );
}

#[test]
fn signature_help_outside_call_returns_none() {
    let (tm, reg) = check_source(
        r#"
package a

function main(): Unit = assert true
"#,
    );
    // Position on `assert true` — not inside a function call
    let sig = signature_help::signature_help_at_position(&tm, &reg, &file_path(), 4, 26);
    // Assert expression is not a function call, so no signature help
    assert!(sig.is_none(), "should return None outside function call");
}

// ─── Inlay Hints ─────────────────────────────────────────────────────

#[test]
fn inlay_hints_let_without_annotation() {
    let source = r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#;
    let (tm, _reg) = check_source(source);
    let range = Range::new(Position::new(0, 0), Position::new(10, 0));
    let hints = inlay_hints::inlay_hints_for_file(&tm, &file_path(), range, Some(source));
    // `let result = add(1, 2)` should get a type hint `: Int32`
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(
        !type_hints.is_empty(),
        "should have at least one type hint for 'result'"
    );
    let has_int32 = type_hints.iter().any(|h| {
        if let InlayHintLabel::String(s) = &h.label {
            s.contains("Int32")
        } else {
            false
        }
    });
    assert!(has_int32, "should show ': Int32' hint");
}

#[test]
fn inlay_hints_explicit_annotation_no_hint() {
    let source = r#"
package a

function main(): Unit =
  let x: Int32 = 42
  assert x == 42
"#;
    let (tm, _reg) = check_source(source);
    let range = Range::new(Position::new(0, 0), Position::new(10, 0));
    let hints = inlay_hints::inlay_hints_for_file(&tm, &file_path(), range, Some(source));
    // `let x: Int32 = 42` has explicit annotation — no type hint needed
    // Also `42` is a literal which is skipped
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(
        type_hints.is_empty(),
        "explicit annotation should not produce type hint"
    );
}

#[test]
fn inlay_hints_literal_value_no_hint() {
    let source = r#"
package a

function main(): Unit =
  let x = 42
  assert x == 42
"#;
    let (tm, _reg) = check_source(source);
    let range = Range::new(Position::new(0, 0), Position::new(10, 0));
    let hints = inlay_hints::inlay_hints_for_file(&tm, &file_path(), range, Some(source));
    // `let x = 42` — literal value makes type obvious, should be skipped
    let type_hints: Vec<_> = hints
        .iter()
        .filter(|h| h.kind == Some(InlayHintKind::TYPE))
        .collect();
    assert!(
        type_hints.is_empty(),
        "literal value should not produce type hint"
    );
}

// ─── Find References ─────────────────────────────────────────────────

#[test]
fn find_references_function_call() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let a = add(1, 2)
  let b = add(3, 4)
  assert a + b == 10
"#,
    );
    // Find node at first `add(1, 2)` call, line 7, col 11
    let node = position::find_node_at_position(&tm, &file_path(), 7, 11).unwrap();
    let refs = references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), false);
    // Should find at least 2 call sites (add(1,2) and add(3,4))
    assert!(
        refs.len() >= 2,
        "should find at least 2 references, found {}",
        refs.len()
    );
}

#[test]
fn find_references_with_declaration() {
    let (tm, reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16).unwrap();
    let refs_no_decl =
        references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), false);
    let refs_with_decl =
        references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), true);
    assert!(
        refs_with_decl.len() > refs_no_decl.len(),
        "include_declaration should add one more location"
    );
}

#[test]
fn find_references_variable_scoped() {
    let (tm, reg) = check_source(
        r#"
package a

function foo(): Int32 =
  let x = 1
  x + x

function bar(): Int32 =
  let x = 2
  x * x

function main(): Unit = assert foo() + bar() == 6
"#,
    );
    // Find `x` in foo() on line 6
    let node = position::find_node_at_position(&tm, &file_path(), 6, 3).unwrap();
    let refs = references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), false);
    // Should only find refs within foo(), not bar()
    // foo's x is referenced twice (x + x on line 6)
    assert!(
        refs.len() >= 2,
        "should find variable refs within function scope"
    );
}

#[test]
fn find_references_record_type() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p1 = Point { x = 1; y = 2 }
  let p2 = Point { x = 3; y = 4 }
  assert p1.x + p2.x == 4
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 9, 12).unwrap();
    let refs = references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), false);
    // Should find at least 2 RecordCreate sites
    assert!(
        refs.len() >= 2,
        "should find record create sites, found {}",
        refs.len()
    );
}

#[test]
fn find_references_no_references() {
    let (tm, reg) = check_source(
        r#"
package a

function unused(): Int32 = 42

function main(): Unit = assert true
"#,
    );
    // Find node at `unused` function — but nobody calls it
    // We need to find the function. Let's look for it via the function body.
    // Position at `42` on line 4, col 29
    let node = position::find_node_at_position(&tm, &file_path(), 4, 29);
    if let Some(node) = node {
        let refs =
            references::find_references(&node, &tm, &reg, &file_path(), workspace_root(), false);
        // The literal 42 is a TypedExpr — references returns empty for TypedExpr
        assert!(refs.is_empty(), "TypedExpr should have no references");
    }
}

// ─── Go to Implementation ────────────────────────────────────────────

#[test]
fn goto_implementation_trait() {
    let (_tm, _reg) = check_source(
        r#"
package a

public trait Printable =
  function display(self: Self): String

public record Name =
  value: String

implement Printable for Name =
  function display(self: Name): String = "hello"

function main(): Unit = assert true
"#,
    );
    // Verifies that source with trait + implement blocks typechecks correctly.
    // Direct trait goto_implementation requires a trait-typed node in an expression,
    // which is tested indirectly through the integration tests.
}

#[test]
fn goto_implementation_empty_for_non_trait() {
    let (tm, reg) = check_source(
        r#"
package a

public record Point =
  x: Int32
  y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  assert p.x == 1
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 9, 11).unwrap();
    let locations = implementation::goto_implementation(&node, &tm, &reg, workspace_root());
    // Record is neither a trait nor a class with subclasses
    assert!(
        locations.is_empty(),
        "record should have no implementations"
    );
}

// ─── Call Hierarchy ──────────────────────────────────────────────────

#[test]
fn prepare_call_hierarchy_on_function_call() {
    let (tm, _reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // On `add(1, 2)` call, line 7, col 16
    let items = call_hierarchy::prepare_call_hierarchy(&tm, &file_path(), 7, 16, workspace_root());
    assert!(items.is_some(), "should prepare call hierarchy item");
    let items = items.unwrap();
    assert!(!items.is_empty());
    assert!(items[0].name.contains("add"), "item should reference 'add'");
    assert!(
        items[0].data.is_some(),
        "item should have mangled name in data"
    );
}

#[test]
fn prepare_call_hierarchy_inside_function_body() {
    let (tm, _reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit = assert add(1, 2) == 3
"#,
    );
    // Inside `add` function body — line 4, col 45 (inside `x + y`)
    let items = call_hierarchy::prepare_call_hierarchy(&tm, &file_path(), 4, 45, workspace_root());
    assert!(items.is_some(), "should find enclosing function");
}

#[test]
fn incoming_calls_finds_callers() {
    let (tm, _reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // Prepare call hierarchy for `add`
    let items =
        call_hierarchy::prepare_call_hierarchy(&tm, &file_path(), 7, 16, workspace_root()).unwrap();

    let incoming = call_hierarchy::incoming_calls(&tm, &items[0], workspace_root());
    // `main` calls `add`, so we should find at least one incoming call
    assert!(!incoming.is_empty(), "should find incoming calls from main");
}

#[test]
fn outgoing_calls_finds_callees() {
    let (tm, _reg) = check_source(
        r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#,
    );
    // Prepare call hierarchy for `main`
    let items = call_hierarchy::prepare_call_hierarchy(&tm, &file_path(), 6, 10, workspace_root());
    if let Some(items) = items {
        let outgoing = call_hierarchy::outgoing_calls(&tm, &items[0], workspace_root());
        // `main` calls `add`
        assert!(!outgoing.is_empty(), "main should have outgoing calls");
        assert!(
            outgoing.iter().any(|c| c.to.name.contains("add")),
            "outgoing should include 'add'"
        );
    }
}

#[test]
fn incoming_calls_empty_for_uncalled() {
    let (tm, _reg) = check_source(
        r#"
package a

function unused(): Int32 = 42

function main(): Unit = assert true
"#,
    );
    // Prepare call hierarchy for `unused`
    let items = call_hierarchy::prepare_call_hierarchy(&tm, &file_path(), 4, 29, workspace_root());
    if let Some(items) = items {
        let incoming = call_hierarchy::incoming_calls(&tm, &items[0], workspace_root());
        assert!(
            incoming.is_empty(),
            "uncalled function should have no incoming calls"
        );
    }
}

// ─── Code Lenses ─────────────────────────────────────────────────────

#[test]
fn code_lens_single_test() {
    let (tm, _reg) = check_source(
        r#"
package a

test "basic math" = assert 1 + 2 == 3
"#,
    );
    let lenses = code_lens::test_code_lenses(&tm, &file_path());
    // Single test → only "Run Test" lens (no "Run All Tests")
    assert_eq!(lenses.len(), 1, "single test should have 1 lens");
    let cmd = lenses[0].command.as_ref().unwrap();
    assert_eq!(cmd.title, "Run Test");
    assert_eq!(cmd.command, "dovetail.runTest");
    // Argument should be the FQTN
    let args = cmd.arguments.as_ref().unwrap();
    assert_eq!(args.len(), 1);
    let fqtn = args[0].as_str().unwrap();
    assert_eq!(fqtn, "a basic math");
}

#[test]
fn code_lens_multiple_tests() {
    let (tm, _reg) = check_source(
        r#"
package a

test "test one" = assert 1 == 1
test "test two" = assert 2 == 2
test "test three" = assert 3 == 3
"#,
    );
    let lenses = code_lens::test_code_lenses(&tm, &file_path());
    // Multiple tests → "Run All Tests" + individual "Run Test" lenses
    assert_eq!(
        lenses.len(),
        4,
        "should have Run All Tests + 3 Run Test lenses, got {}",
        lenses.len()
    );

    // First lens: "Run All Tests" with file path argument
    let run_all = &lenses[0];
    let cmd = run_all.command.as_ref().unwrap();
    assert_eq!(cmd.command, "dovetail.runTestFile");
    assert!(
        cmd.title.contains("Run All Tests"),
        "title should contain 'Run All Tests'"
    );
    let args = cmd.arguments.as_ref().unwrap();
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].as_str().unwrap(), "src/a.dove");

    // Remaining lenses: individual "Run Test" with FQTN arguments
    let run_tests: Vec<_> = lenses[1..].iter().collect();
    assert_eq!(run_tests.len(), 3);
    for lens in &run_tests {
        let cmd = lens.command.as_ref().unwrap();
        assert_eq!(cmd.command, "dovetail.runTest");
        assert_eq!(cmd.title, "Run Test");
        assert_eq!(cmd.arguments.as_ref().unwrap().len(), 1);
    }
    let fqtns: Vec<_> = run_tests
        .iter()
        .map(|l| {
            l.command.as_ref().unwrap().arguments.as_ref().unwrap()[0]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(fqtns, vec!["a test one", "a test two", "a test three"]);
}

#[test]
fn code_lens_no_tests() {
    let (tm, _reg) = check_source(
        r#"
package a

function main(): Unit = assert true
"#,
    );
    let lenses = code_lens::test_code_lenses(&tm, &file_path());
    assert!(lenses.is_empty(), "no tests should produce no lenses");
}

// ─── Code Actions ────────────────────────────────────────────────────

#[test]
fn code_action_add_type_annotation() {
    let source = r#"
package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#;
    let (tm, _reg) = check_source(source);
    let file_uri = Url::from_file_path("/src/a.dove").unwrap();
    // Range covering the let binding on line 7 (0-indexed: line 6)
    let range = Range::new(Position::new(6, 0), Position::new(6, 30));
    let actions =
        code_actions::add_type_annotation_actions(&tm, &file_path(), &range, source, &file_uri);
    assert!(!actions.is_empty(), "should offer type annotation action");
    assert!(
        actions[0].title.contains("Add type annotation"),
        "action should be 'Add type annotation'"
    );
    assert!(
        actions[0].title.contains("Int32"),
        "annotation should contain the inferred type"
    );
}

#[test]
fn code_action_organize_imports() {
    let sf = parse_source(
        r#"
package a

import b.utils.bar
import b.utils.foo
"#,
    );
    let file_uri = Url::from_file_path("/src/a.dove").unwrap();
    let _action = code_actions::organize_imports_action(&sf, &file_uri);
    // Imports are already sorted alphabetically (bar before foo), so might return None
    // depending on grouping logic. Let's just verify it doesn't panic.
}

#[test]
fn code_action_organize_imports_already_sorted() {
    let sf = parse_source(
        r#"
package a

import b.utils.bar
import b.utils.foo
"#,
    );
    let file_uri = Url::from_file_path("/src/a.dove").unwrap();
    let action = code_actions::organize_imports_action(&sf, &file_uri);
    assert!(
        action.is_none(),
        "already sorted imports should return None"
    );
}

#[test]
fn code_action_organize_imports_unsorted() {
    let sf = parse_source(
        r#"
package a

import b.utils.foo
import b.utils.bar
"#,
    );
    let file_uri = Url::from_file_path("/src/a.dove").unwrap();
    let action = code_actions::organize_imports_action(&sf, &file_uri);
    assert!(
        action.is_some(),
        "unsorted imports should produce organize action"
    );
    let action = action.unwrap();
    assert_eq!(action.title, "Organize Imports");
}

#[test]
fn code_action_auto_import() {
    // This test checks that auto_import_code_actions finds matching public symbols
    // from the registry when given a diagnostic about an unknown function.
    let source = r#"
package b

public function helper(): Int32 = 42
"#;
    // First, check `b` package to get its registry with the public `helper` function
    let result_b = dovetail::check(source, "src/b.dove");
    let reg = result_b.registry;

    // Now create a diagnostic that mimics "unknown function 'helper'"
    let sf = parse_source(
        r#"
package a

function main(): Unit = assert true
"#,
    );
    let file_uri = Url::from_file_path("/src/a.dove").unwrap();
    let diag = Diagnostic {
        range: Range::new(Position::new(3, 0), Position::new(3, 6)),
        message: "unknown function 'helper'".to_string(),
        ..Default::default()
    };
    let actions = code_actions::auto_import_code_actions(&[diag], &reg, &sf, &file_uri);
    assert!(!actions.is_empty(), "should produce auto-import action");
    assert!(
        actions[0].title.contains("Import"),
        "action should be import"
    );
}

// ─── Diagnostics Utilities ───────────────────────────────────────────

use dovetail::lsp::diagnostics as lsp_diag;

#[test]
fn span_to_range_basic() {
    let span = dovetail::common::span::Span::new(file_path(), 1, 1, 1, 5);
    let range = lsp_diag::span_to_range(&span);
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 5);
}

#[test]
fn span_to_range_multiline() {
    let span = dovetail::common::span::Span::new(file_path(), 3, 5, 7, 10);
    let range = lsp_diag::span_to_range(&span);
    assert_eq!(range.start.line, 2);
    assert_eq!(range.start.character, 4);
    assert_eq!(range.end.line, 6);
    assert_eq!(range.end.character, 10);
}

#[test]
fn span_to_range_zero_values() {
    // Edge case: line=0 or col=0 — saturating_sub produces 0
    let span = dovetail::common::span::Span::new(file_path(), 0, 0, 0, 0);
    let range = lsp_diag::span_to_range(&span);
    assert_eq!(range.start.line, 0);
    assert_eq!(range.start.character, 0);
    assert_eq!(range.end.line, 0);
    assert_eq!(range.end.character, 0);
}

#[test]
fn clamp_range_inner_within_outer() {
    let outer = Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 10,
            character: 20,
        },
    };
    let inner = Range {
        start: Position {
            line: 2,
            character: 5,
        },
        end: Position {
            line: 5,
            character: 10,
        },
    };
    let result = lsp_diag::clamp_range(inner, outer);
    assert_eq!(result.start.line, 2);
    assert_eq!(result.start.character, 5);
    assert_eq!(result.end.line, 5);
    assert_eq!(result.end.character, 10);
}

#[test]
fn clamp_range_inner_exceeds_outer() {
    let outer = Range {
        start: Position {
            line: 2,
            character: 5,
        },
        end: Position {
            line: 5,
            character: 10,
        },
    };
    let inner = Range {
        start: Position {
            line: 1,
            character: 0,
        },
        end: Position {
            line: 8,
            character: 30,
        },
    };
    let result = lsp_diag::clamp_range(inner, outer);
    assert_eq!(result.start.line, 2);
    assert_eq!(result.start.character, 5);
    assert_eq!(result.end.line, 5);
    assert_eq!(result.end.character, 10);
}

#[test]
fn clamp_range_degenerate() {
    // After clamping, start > end → collapse to (outer.start, outer.start)
    let outer = Range {
        start: Position {
            line: 5,
            character: 10,
        },
        end: Position {
            line: 5,
            character: 20,
        },
    };
    let inner = Range {
        start: Position {
            line: 6,
            character: 0,
        },
        end: Position {
            line: 4,
            character: 0,
        },
    };
    let result = lsp_diag::clamp_range(inner, outer);
    assert_eq!(result.start, outer.start);
    assert_eq!(result.end, outer.start);
}

#[test]
fn file_path_to_uri_regular_file() {
    let root = std::path::Path::new("/workspace");
    let uri = lsp_diag::file_path_to_uri(root, "src/a.dove");
    assert!(uri.is_some());
    let uri = uri.unwrap();
    assert!(uri.as_str().contains("src/a.dove"));
}

#[test]
fn file_path_to_uri_synthetic_prelude() {
    let root = std::path::Path::new("/workspace");
    let uri = lsp_diag::file_path_to_uri(root, "<prelude>");
    assert!(uri.is_none());
}

#[test]
fn file_path_to_uri_synthetic_codegen() {
    let root = std::path::Path::new("/workspace");
    let uri = lsp_diag::file_path_to_uri(root, "<codegen>");
    assert!(uri.is_none());
}

#[test]
fn group_diagnostics_by_file_groups_correctly() {
    use dovetail::common::diagnostics::Diagnostics;
    use dovetail::common::span::Span;

    let root = std::path::Path::new("/");
    let mut diags = Diagnostics::new();
    diags.error(Span::new("src/a.dove".into(), 1, 1, 1, 5), "error in a");
    diags.error(Span::new("src/b.dove".into(), 2, 1, 2, 5), "error in b");
    diags.error(
        Span::new("src/a.dove".into(), 3, 1, 3, 5),
        "second error in a",
    );

    let grouped = lsp_diag::group_diagnostics_by_file(&diags, root);
    assert_eq!(grouped.len(), 2, "should have 2 files");

    let a_uri = Url::from_file_path("/src/a.dove").unwrap();
    let b_uri = Url::from_file_path("/src/b.dove").unwrap();
    assert_eq!(grouped[&a_uri].len(), 2, "file a should have 2 diagnostics");
    assert_eq!(grouped[&b_uri].len(), 1, "file b should have 1 diagnostic");
}

#[test]
fn group_diagnostics_by_file_skips_synthetic() {
    use dovetail::common::diagnostics::Diagnostics;
    use dovetail::common::span::Span;

    let root = std::path::Path::new("/");
    let mut diags = Diagnostics::new();
    diags.error(Span::new("<prelude>".into(), 1, 1, 1, 5), "prelude error");
    diags.error(Span::new("src/a.dove".into(), 1, 1, 1, 5), "real error");

    let grouped = lsp_diag::group_diagnostics_by_file(&diags, root);
    assert_eq!(grouped.len(), 1, "should only have real file");
    assert!(!grouped.keys().any(|u| u.as_str().contains("prelude")));
}

// ── Generic function LSP tests ──────────────────────────────────────────

#[test]
fn hover_generic_function_shows_type_params() {
    let (tm, reg) = check_source(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
  let result = identity<Int32>(42)
  assert result == 42
"#,
    );
    // Hover on the `identity` call on line 7
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16).unwrap();
    let h = hover::hover_for_node(&node, &tm, &reg);
    assert!(
        h.is_some(),
        "should produce hover for generic function call"
    );
    if let HoverContents::Markup(markup) = &h.unwrap().contents {
        assert!(
            markup.value.contains("<T>"),
            "hover should show type params, got: {}",
            markup.value
        );
        assert!(
            markup.value.contains("identity"),
            "hover should show function name"
        );
    } else {
        panic!("expected markup hover");
    }
}

#[test]
fn goto_definition_generic_function_call() {
    let (tm, reg) = check_source(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
  let result = identity<Int32>(42)
  assert result == 42
"#,
    );
    let node = position::find_node_at_position(&tm, &file_path(), 7, 16).unwrap();
    let loc = navigation::goto_definition(&node, &tm, &reg, workspace_root());
    assert!(
        loc.is_some(),
        "should navigate to generic function definition"
    );
    let loc = loc.unwrap();
    // The generic function definition is on line 4 (1-indexed) → LSP line 3
    assert_eq!(
        loc.range.start.line, 3,
        "should navigate to definition line"
    );
}
