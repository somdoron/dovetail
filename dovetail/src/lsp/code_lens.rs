use tower_lsp::lsp_types::{CodeLens, Command};

use crate::common::span::FilePath;
use crate::typechecker::types::TypedModule;

use super::diagnostics::span_to_range;

/// Generate code lenses for test declarations in a file.
///
/// Each test gets a "Run Test" lens, and if there are multiple tests,
/// a "Run All Tests" lens is added at the first test.
pub fn test_code_lenses(typed_module: &TypedModule, file: &FilePath) -> Vec<CodeLens> {
    let mut lenses = Vec::new();

    let file_tests: Vec<_> = typed_module
        .tests
        .iter()
        .filter(|t| t.span.file == *file)
        .collect();

    if file_tests.is_empty() {
        return lenses;
    }

    // "Run All Tests" lens at the first test, only if there are multiple
    if file_tests.len() > 1 {
        let first = &file_tests[0];
        lenses.push(CodeLens {
            range: span_to_range(&first.span),
            command: Some(Command {
                title: format!("Run All Tests ({})", file_tests.len()),
                command: "dovetail.runTestFile".to_string(),
                arguments: Some(vec![serde_json::Value::String(
                    first.span.file.to_string(),
                )]),
            }),
            data: None,
        });
    }

    // Individual "Run Test" lenses
    for test in &file_tests {
        lenses.push(CodeLens {
            range: span_to_range(&test.span),
            command: Some(Command {
                title: "Run Test".to_string(),
                command: "dovetail.runTest".to_string(),
                arguments: Some(vec![serde_json::Value::String(test.fqtn.clone())]),
            }),
            data: None,
        });
    }

    lenses
}
