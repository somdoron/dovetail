use crate::common::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTag {
    Unnecessary,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Span,
    pub message: String,
    pub tag: Option<DiagnosticTag>,
}

/// Accumulates diagnostics across all pipeline stages.
#[derive(Debug, Default)]
pub struct Diagnostics {
    diagnostics: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn error(&mut self, span: Span, message: impl Into<String>) {
        let message = message.into();
        // Collect and inference both resolve some annotations (e.g. function
        // signatures), which can produce the exact same diagnostic twice —
        // an identical (span, message) pair is always noise, drop it.
        if self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error && d.span == span && d.message == message)
        {
            return;
        }
        self.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            span,
            message,
            tag: None,
        });
    }

    pub fn warning(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            span,
            message: message.into(),
            tag: None,
        });
    }

    pub fn warning_with_tag(&mut self, span: Span, message: impl Into<String>, tag: DiagnosticTag) {
        self.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            span,
            message: message.into(),
            tag: Some(tag),
        });
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    /// Returns the number of diagnostics accumulated so far.
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns true if no diagnostics have been accumulated.
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Truncate diagnostics to the given length, discarding any added after that point.
    pub fn truncate(&mut self, len: usize) {
        self.diagnostics.truncate(len);
    }

    pub fn extend(&mut self, diagnostics: &[Diagnostic]) {
        self.diagnostics.extend_from_slice(diagnostics);
    }

    pub fn extend_from(&mut self, other: &Diagnostics) {
        self.diagnostics.extend_from_slice(&other.diagnostics);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }
}
