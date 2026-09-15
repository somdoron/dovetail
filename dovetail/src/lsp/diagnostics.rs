use std::collections::HashMap;
use std::path::Path;

use tower_lsp::lsp_types::{self, DiagnosticSeverity, Position, Range, Url};

use crate::common::diagnostics::{DiagnosticTag, Diagnostics, Severity};
use crate::common::span::Span;

/// Convert an inclusive, 1-indexed compiler Span to a half-open, 0-indexed LSP Range.
pub fn span_to_range(span: &Span) -> Range {
    Range {
        start: Position {
            line: span.line.saturating_sub(1),
            character: span.column.saturating_sub(1),
        },
        end: Position {
            line: span.end_line.saturating_sub(1),
            character: span.end_column,
        },
    }
}

/// Clamp `inner` so it is fully contained within `outer`.
/// LSP requires selectionRange to be contained in fullRange.
pub fn clamp_range(inner: Range, outer: Range) -> Range {
    let start = if inner.start.line < outer.start.line
        || (inner.start.line == outer.start.line && inner.start.character < outer.start.character)
    {
        outer.start
    } else {
        inner.start
    };
    let end = if inner.end.line > outer.end.line
        || (inner.end.line == outer.end.line && inner.end.character > outer.end.character)
    {
        outer.end
    } else {
        inner.end
    };
    // Ensure start <= end after clamping
    if start.line > end.line || (start.line == end.line && start.character > end.character) {
        Range {
            start: outer.start,
            end: outer.start,
        }
    } else {
        Range { start, end }
    }
}

/// Convert a compiler Diagnostic to an LSP Diagnostic.
fn convert_diagnostic(diag: &crate::common::diagnostics::Diagnostic) -> lsp_types::Diagnostic {
    let severity = match diag.severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
    };
    let tags = diag.tag.map(|t| match t {
        DiagnosticTag::Unnecessary => vec![lsp_types::DiagnosticTag::UNNECESSARY],
    });
    lsp_types::Diagnostic {
        range: span_to_range(&diag.span),
        severity: Some(severity),
        source: Some("dovetail".to_string()),
        message: diag.message.clone(),
        tags,
        ..Default::default()
    }
}

/// Convert a compiler FilePath to an LSP Url, relative to the workspace root.
/// Returns None for synthetic files like `<prelude>`, `<discovery>`, `<codegen>`.
pub fn file_path_to_uri(workspace_root: &Path, file_path: &str) -> Option<Url> {
    if let Some(rest) = file_path.strip_prefix("<prelude>/") {
        let abs = workspace_root
            .join(".dovetail")
            .join("dependencies")
            .join("prelude")
            .join("src")
            .join(rest);
        return Url::from_file_path(abs).ok();
    }
    if file_path.starts_with('<') {
        return None;
    }
    let abs = workspace_root.join(file_path);
    Url::from_file_path(abs).ok()
}

/// Group compiler diagnostics by file URI, converting each to an LSP Diagnostic.
/// Diagnostics with synthetic file paths are silently skipped.
pub fn group_diagnostics_by_file(
    diagnostics: &Diagnostics,
    workspace_root: &Path,
) -> HashMap<Url, Vec<lsp_types::Diagnostic>> {
    let mut map: HashMap<Url, Vec<lsp_types::Diagnostic>> = HashMap::new();
    for diag in diagnostics.iter() {
        if let Some(uri) = file_path_to_uri(workspace_root, &diag.span.file) {
            map.entry(uri).or_default().push(convert_diagnostic(diag));
        }
    }
    map
}
