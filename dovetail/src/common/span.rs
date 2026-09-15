use std::sync::Arc;

/// A file path for source locations (relative to workspace root).
pub type FilePath = Arc<str>;

/// A source location range. Line and column are 1-indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub file: FilePath,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

impl Span {
    pub fn new(file: FilePath, line: u32, column: u32, end_line: u32, end_column: u32) -> Self {
        Self {
            file,
            line,
            column,
            end_line,
            end_column,
        }
    }

    /// Create a span covering a single position.
    pub fn point(file: FilePath, line: u32, column: u32) -> Self {
        Self {
            file,
            line,
            column,
            end_line: line,
            end_column: column,
        }
    }

    /// Merge two spans into one covering both.
    pub fn merge(&self, other: &Span) -> Span {
        Span {
            file: self.file.clone(),
            line: self.line.min(other.line),
            column: if self.line <= other.line {
                if self.line == other.line {
                    self.column.min(other.column)
                } else {
                    self.column
                }
            } else {
                other.column
            },
            end_line: self.end_line.max(other.end_line),
            end_column: if self.end_line >= other.end_line {
                if self.end_line == other.end_line {
                    self.end_column.max(other.end_column)
                } else {
                    self.end_column
                }
            } else {
                other.end_column
            },
        }
    }
}

/// A value with an attached source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}
