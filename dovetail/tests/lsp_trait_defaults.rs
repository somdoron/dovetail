use std::path::Path;
use std::sync::Arc;

use dovetail::common::span::FilePath;
use dovetail::lsp::{
    call_hierarchy, code_actions, hover, inlay_hints, navigation, position, references, scope,
    signature_help,
};
use dovetail::typechecker::TypeCheckerResult;
use dovetail::typechecker::types::Type;
use tower_lsp::lsp_types::{Position, Range, Url};

const SOURCE: &str = "package a
function increment(input: Int32): Int32 = input + 1
trait Defaults =
    function answer(self): Int32 =
        let local = increment(41)
        local
    property size(self): Int32 =
        let count = increment(1)
        count
";

fn check() -> TypeCheckerResult {
    let result = dovetail::check(SOURCE, "/src/defaults.dove");
    let errors: Vec<_> = result.diagnostics.iter().map(|d| &d.message).collect();
    assert!(!result.diagnostics.has_errors(), "{errors:?}");
    result
}

fn file() -> FilePath {
    Arc::from("/src/defaults.dove")
}

fn cursor(text: &str, offset: usize) -> (u32, u32) {
    let index = SOURCE.find(text).expect("cursor text exists") + offset;
    let prefix = &SOURCE[..index];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32 + 1;
    let column = prefix.rsplit('\n').next().unwrap().len() as u32 + 1;
    (line, column)
}

#[test]
fn default_method_and_property_locals_support_editor_queries() {
    let result = check();
    let module = &result.typed_module;
    for name in ["local", "count"] {
        let (line, column) = cursor(&format!("        {name}\n"), 8);
        let node = position::find_node_at_position(module, &file(), line, column)
            .expect("find local use in default body");
        assert!(matches!(
            &node,
            position::NodeAtPosition::VarRef {
                ty: Type::Int32,
                ..
            }
        ));
        assert!(hover::hover_for_node(&node, module, &result.registry).is_some());
        assert_eq!(
            position::find_expression_type_at_position(module, &file(), line, column),
            Some(Type::Int32)
        );

        let locals = scope::collect_visible_locals(module, &file(), line, column);
        assert!(locals.params.iter().any(|param| param.name == "self"));
        assert_eq!(locals.locals.len(), 1);
        assert_eq!(locals.locals[0].ty, Type::Int32);

        let locations = references::find_references(
            &node,
            module,
            &result.registry,
            &file(),
            Path::new("/"),
            true,
        );
        assert_eq!(locations.len(), 2, "declaration and use for {name}");
        assert!(
            locations
                .iter()
                .any(|location| location.range.start.line == line - 1)
        );
    }
}

#[test]
fn calls_inside_defaults_support_navigation_signatures_and_hierarchy() {
    let result = check();
    let module = &result.typed_module;
    for call in ["increment(41)", "increment(1)"] {
        let (line, column) = cursor(call, 1);
        let node = position::find_node_at_position(module, &file(), line, column).unwrap();
        let definition =
            navigation::goto_definition(&node, module, &result.registry, Path::new("/"))
                .expect("navigate from a default to the helper");
        assert_eq!(definition.range.start.line, 1);
        let signature = signature_help::signature_help_at_position(
            module,
            &result.registry,
            &file(),
            line,
            column + 9,
        )
        .expect("signature help inside default body call");
        assert!(signature.signatures[0].label.contains("increment"));

        let items =
            call_hierarchy::prepare_call_hierarchy(module, &file(), line, column, Path::new("/"))
                .unwrap();
        let incoming = call_hierarchy::incoming_calls(module, &items[0], Path::new("/"));
        assert_eq!(incoming.len(), 2, "both defaults call the helper");
        for caller in incoming {
            let outgoing = call_hierarchy::outgoing_calls(module, &caller.from, Path::new("/"));
            assert_eq!(
                outgoing.len(),
                1,
                "default templates resolve as hierarchy callers"
            );
            assert!(outgoing[0].to.name.contains("increment"));
        }
    }
}

#[test]
fn default_bodies_offer_type_hints_and_annotation_actions() {
    let result = check();
    let range = Range::new(Position::new(0, 0), Position::new(20, 0));
    let hints =
        inlay_hints::inlay_hints_for_file(&result.typed_module, &file(), range, Some(SOURCE));
    let actions = code_actions::add_type_annotation_actions(
        &result.typed_module,
        &file(),
        &range,
        SOURCE,
        &Url::from_file_path("/src/defaults.dove").unwrap(),
    );
    for text in ["let local", "let count"] {
        let (line, _) = cursor(text, 0);
        assert!(hints.iter().any(|hint| hint.position.line == line - 1));
    }
    assert_eq!(
        actions.len(),
        2,
        "both default bodies offer inferred type annotations"
    );
}
