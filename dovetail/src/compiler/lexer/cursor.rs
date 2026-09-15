use crate::common::span::FilePath;

/// Character-by-character cursor with line/column tracking.
pub struct Cursor {
    source: Vec<char>,
    pos: usize,
    byte_offset: usize,
    line: u32,
    column: u32,
    file: FilePath,
}

impl Cursor {
    pub fn new(source: &str, file: FilePath) -> Self {
        Self {
            source: source.chars().collect(),
            pos: 0,
            byte_offset: 0,
            line: 1,
            column: 1,
            file,
        }
    }

    /// A cursor whose first character is reported at `line`/`column` rather
    /// than at 1:1.
    ///
    /// Used when lexing an interpolation expression that was carved out of a
    /// larger file: seeding the position here is all the span arithmetic that
    /// is needed. Characters on the expression's first line land at
    /// `column + k`, and `advance` resets to column 1 after each newline —
    /// which is also correct, because the carved-out text is a verbatim copy
    /// including leading whitespace, so column 1 of the excerpt is column 1 of
    /// the file.
    pub fn new_at(source: &str, file: FilePath, line: u32, column: u32) -> Self {
        Self {
            source: source.chars().collect(),
            pos: 0,
            byte_offset: 0,
            line,
            column,
            file,
        }
    }

    pub fn byte_offset(&self) -> usize {
        self.byte_offset
    }

    pub fn file(&self) -> FilePath {
        self.file.clone()
    }

    pub fn line(&self) -> u32 {
        self.line
    }

    pub fn column(&self) -> u32 {
        self.column
    }

    /// Returns the current character without advancing.
    pub fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    /// Returns the character at offset from current position.
    pub fn peek_at(&self, offset: usize) -> Option<char> {
        self.source.get(self.pos + offset).copied()
    }

    /// Returns true if the cursor is at the end of input.
    pub fn is_eof(&self) -> bool {
        self.pos >= self.source.len()
    }

    /// Advances the cursor by one character and returns it.
    pub fn advance(&mut self) -> Option<char> {
        let ch = self.source.get(self.pos).copied()?;
        self.pos += 1;
        self.byte_offset += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(ch)
    }

    /// Skip characters while the predicate holds.
    pub fn skip_while(&mut self, pred: impl Fn(char) -> bool) {
        while let Some(ch) = self.peek() {
            if pred(ch) {
                self.advance();
            } else {
                break;
            }
        }
    }
}
