pub mod cursor;
mod exact_number;
pub mod token;

use crate::common::diagnostics::Diagnostic;
use crate::common::span::{FilePath, Span};
use cursor::Cursor;
use token::{LiteralPart, PrefixedLiteralData, Token, TokenKind};

pub struct Lexer {
    cursor: Cursor,
    diagnostics: Vec<Diagnostic>,
    capture_source: bool,
    pub source_comments: Vec<std::ops::Range<usize>>,
    pub source_interpolations: Vec<std::ops::Range<usize>>,
}

enum InterpolationPart {
    Text(String),
    Var {
        name: String,
        line: u32,
        col: u32,
    },
    Expr {
        tokens: Vec<Token>,
        line: u32,
        col: u32,
    },
}

impl Lexer {
    pub fn new(source: &str, file: FilePath) -> Self {
        Self {
            cursor: Cursor::new(source, file),
            diagnostics: Vec::new(),
            capture_source: false,
            source_comments: Vec::new(),
            source_interpolations: Vec::new(),
        }
    }

    /// A lexer over an excerpt, reporting spans relative to the excerpt's real
    /// position in `file`. See `Cursor::new_at`.
    pub fn new_at(source: &str, file: FilePath, line: u32, column: u32) -> Self {
        Self {
            cursor: Cursor::new_at(source, file, line, column),
            diagnostics: Vec::new(),
            capture_source: false,
            source_comments: Vec::new(),
            source_interpolations: Vec::new(),
        }
    }

    pub fn capturing_source(source: &str, file: FilePath) -> Self {
        let mut lexer = Self::new(source, file);
        lexer.capture_source = true;
        lexer
    }

    /// Tokenizes the entire source, returning all tokens (including Newline, Eof).
    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace();

            if self.cursor.is_eof() {
                tokens.push(self.make_token(TokenKind::Eof, ""));
                break;
            }

            let source_start = self.cursor.byte_offset();
            let token_start = tokens.len();
            let ch = self.cursor.peek().unwrap();

            match ch {
                '\n' => {
                    let tok = self.make_token(TokenKind::Newline, "\n");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '\r' => {
                    let tok = self.make_token(TokenKind::Newline, "\n");
                    self.cursor.advance();
                    // Consume \n after \r
                    if self.cursor.peek() == Some('\n') {
                        self.cursor.advance();
                    }
                    tokens.push(tok);
                }
                '/' if self.cursor.peek_at(1) == Some('/')
                    && self.cursor.peek_at(2) == Some('/')
                    && self.cursor.peek_at(3) != Some('/') =>
                {
                    let tok = self.scan_doc_comment();
                    tokens.push(tok);
                }
                '/' if self.cursor.peek_at(1) == Some('/') => {
                    self.skip_line_comment();
                    if self.capture_source {
                        self.source_comments
                            .push(source_start..self.cursor.byte_offset());
                    }
                }
                '/' => {
                    let tok = self.make_token(TokenKind::Slash, "/");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                c if c.is_ascii_digit() => {
                    let tok = self.scan_number_literal();
                    tokens.push(tok);
                }
                '+' => {
                    if self.cursor.peek_at(1) == Some('+') {
                        let tok = self.make_token(TokenKind::PlusPlus, "++");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Plus, "+");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '-' => {
                    let tok = self.make_token(TokenKind::Minus, "-");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '*' => {
                    let tok = self.make_token(TokenKind::Star, "*");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '%' => {
                    let tok = self.make_token(TokenKind::Percent, "%");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '~' => {
                    let tok = self.make_token(TokenKind::Tilde, "~");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '(' => {
                    let tok = self.make_token(TokenKind::LParen, "(");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                ')' => {
                    let tok = self.make_token(TokenKind::RParen, ")");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                ':' => {
                    if self.cursor.peek_at(1) == Some(':') {
                        let tok = self.make_token(TokenKind::ColonColon, "::");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Colon, ":");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '=' => {
                    if self.cursor.peek_at(1) == Some('=') {
                        let tok = self.make_token(TokenKind::EqEq, "==");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else if self.cursor.peek_at(1) == Some('>') {
                        let tok = self.make_token(TokenKind::FatArrow, "=>");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Equals, "=");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '!' => {
                    if self.cursor.peek_at(1) == Some('=') {
                        let tok = self.make_token(TokenKind::BangEq, "!=");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Bang, "!");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '<' => {
                    if self.cursor.peek_at(1) == Some('<') {
                        let tok = self.make_token(TokenKind::LtLt, "<<");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else if self.cursor.peek_at(1) == Some('=') {
                        let tok = self.make_token(TokenKind::LtEq, "<=");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Lt, "<");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '>' => {
                    if self.cursor.peek_at(1) == Some('>') {
                        let tok = self.make_token(TokenKind::GtGt, ">>");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else if self.cursor.peek_at(1) == Some('=') {
                        let tok = self.make_token(TokenKind::GtEq, ">=");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Gt, ">");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '&' => {
                    if self.cursor.peek_at(1) == Some('&') {
                        let tok = self.make_token(TokenKind::AmpAmp, "&&");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Ampersand, "&");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '|' => {
                    // `]` is checked before `|`: the two are mutually exclusive
                    // on a single peek, but closing an array literal must win
                    // over any future `|`-prefixed operator.
                    if self.cursor.peek_at(1) == Some(']') {
                        let tok = self.make_token(TokenKind::PipeRBracket, "|]");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else if self.cursor.peek_at(1) == Some('|') {
                        let tok = self.make_token(TokenKind::PipePipe, "||");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::Pipe, "|");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                '^' => {
                    let tok = self.make_token(TokenKind::Caret, "^");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '.' => {
                    let (kind, text) = if self.cursor.peek_at(1) == Some('.') {
                        if self.cursor.peek_at(2) == Some('=') {
                            (TokenKind::DotDotEq, "..=")
                        } else {
                            (TokenKind::DotDot, "..")
                        }
                    } else {
                        (TokenKind::Dot, ".")
                    };
                    let tok = self.make_token(kind, text);
                    for _ in 0..text.len() {
                        self.cursor.advance();
                    }
                    tokens.push(tok);
                }
                ',' => {
                    let tok = self.make_token(TokenKind::Comma, ",");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '[' => {
                    if self.cursor.peek_at(1) == Some('|') {
                        let tok = self.make_token(TokenKind::LBracketPipe, "[|");
                        self.cursor.advance();
                        self.cursor.advance();
                        tokens.push(tok);
                    } else {
                        let tok = self.make_token(TokenKind::LBracket, "[");
                        self.cursor.advance();
                        tokens.push(tok);
                    }
                }
                ']' => {
                    let tok = self.make_token(TokenKind::RBracket, "]");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '{' => {
                    let tok = self.make_token(TokenKind::LBrace, "{");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '}' => {
                    let tok = self.make_token(TokenKind::RBrace, "}");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                ';' => {
                    let tok = self.make_token(TokenKind::Semicolon, ";");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '@' => {
                    let tok = self.make_token(TokenKind::At, "@");
                    self.cursor.advance();
                    tokens.push(tok);
                }
                '"' => {
                    if self.cursor.peek_at(1) == Some('"') && self.cursor.peek_at(2) == Some('"') {
                        let toks = self.scan_multiline_string_literal();
                        tokens.extend(toks);
                    } else {
                        let toks = self.scan_string_literal();
                        tokens.extend(toks);
                    }
                }
                '\'' => {
                    let tok = self.scan_char_literal();
                    tokens.push(tok);
                }
                c if is_ident_start(c) => {
                    let tok = self.scan_identifier();
                    tokens.push(tok);
                }
                _ => {
                    let span =
                        Span::point(self.cursor.file(), self.cursor.line(), self.cursor.column());
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: format!("unexpected character: '{}'", ch),
                        tag: None,
                    });
                    self.cursor.advance();
                }
            }
            if self.capture_source {
                let range = source_start..self.cursor.byte_offset();
                for token in &mut tokens[token_start..] {
                    token.source_range = Some(range.clone());
                }
            }
        }

        tokens
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    fn skip_whitespace(&mut self) {
        // Skip spaces and tabs, but NOT newlines (they are tokens).
        self.cursor.skip_while(|c| c == ' ' || c == '\t');
    }

    fn skip_line_comment(&mut self) {
        // Skip `//` and everything until end of line.
        self.cursor.skip_while(|c| c != '\n' && c != '\r');
    }

    fn scan_doc_comment(&mut self) -> Token {
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        // Skip `///`
        self.cursor.advance();
        self.cursor.advance();
        self.cursor.advance();

        // Skip one optional leading space
        if self.cursor.peek() == Some(' ') {
            self.cursor.advance();
        }

        // Collect remaining text until newline
        let mut text = String::new();
        while let Some(ch) = self.cursor.peek() {
            if ch == '\n' || ch == '\r' {
                break;
            }
            text.push(ch);
            self.cursor.advance();
        }

        let span = Span::new(file, start_line, start_col, start_line, start_col + 3);
        Token::new(TokenKind::DocComment, span, text)
    }

    fn scan_identifier(&mut self) -> Token {
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        let mut text = String::new();
        while let Some(ch) = self.cursor.peek() {
            if is_ident_continue(ch) {
                text.push(ch);
                self.cursor.advance();
            } else {
                break;
            }
        }

        let end_col = start_col + text.len() as u32 - 1;
        let span = Span::new(file, start_line, start_col, start_line, end_col);

        let kind = TokenKind::keyword(&text).unwrap_or(TokenKind::Ident);

        // `ident"` with no space between them is a prefixed string literal.
        // `skip_whitespace` runs only at the top of the tokenize loop, never
        // inside this function, so the cursor sitting on `"` is an exact
        // "nothing between them" test. Keywords are excluded, so `case "x"`
        // and friends are untouched — as is anything with a space.
        // `test` is a soft keyword — `test "name" = ...` declares a test — so it
        // is excluded even though it is not a `TokenKind` keyword. Without this
        // a space-less `test"name"` would silently become a literal.
        if kind == TokenKind::Ident && text != "test" && self.cursor.peek() == Some('"') {
            return self.scan_prefixed_string_literal(text, span);
        }

        Token::new(kind, span, text)
    }

    /// Scan `prefix"..."` or `prefix"""..."""` into a single token carrying its
    /// structured parts.
    ///
    /// Unlike an ordinary interpolated string, nothing is assembled into a
    /// `+`/`.format()` token soup here: `$x` must become a `value(x)` call on
    /// the prefix's builder, which only the typechecker can construct. Escapes
    /// are processed exactly as in ordinary strings. (A future `raw = true`
    /// registration would flip `process_escapes` below; the scanner is shaped
    /// so that is a parameter, not a rewrite.)
    fn scan_prefixed_string_literal(&mut self, prefix: String, prefix_span: Span) -> Token {
        let file = self.cursor.file();
        let start_line = prefix_span.line;
        let start_col = prefix_span.column;
        let process_escapes = true;

        let multiline = self.cursor.peek_at(1) == Some('"') && self.cursor.peek_at(2) == Some('"');
        if multiline {
            self.cursor.advance();
            self.cursor.advance();
            self.cursor.advance();
            // Strip one leading newline, as `"""` does.
            if self.cursor.peek() == Some('\r') && self.cursor.peek_at(1) == Some('\n') {
                self.cursor.advance();
                self.cursor.advance();
            } else if self.cursor.peek() == Some('\n') {
                self.cursor.advance();
            }
        } else {
            self.cursor.advance();
        }

        let mut parts: Vec<LiteralPart> = Vec::new();
        let mut text = String::new();
        loop {
            match self.cursor.peek() {
                Some('"')
                    if multiline
                        && self.cursor.peek_at(1) == Some('"')
                        && self.cursor.peek_at(2) == Some('"') =>
                {
                    self.cursor.advance();
                    self.cursor.advance();
                    self.cursor.advance();
                    break;
                }
                Some('"') if !multiline => {
                    self.cursor.advance();
                    break;
                }
                Some('\\') if process_escapes => {
                    self.cursor.advance();
                    self.scan_escape_sequence(&mut text, &file);
                }
                Some('$') => {
                    self.handle_prefixed_interpolation(&mut text, &mut parts, &file);
                }
                Some('\n') | Some('\r') if !multiline => {
                    self.unterminated_prefixed_literal(&prefix, &file, multiline);
                    break;
                }
                None => {
                    self.unterminated_prefixed_literal(&prefix, &file, multiline);
                    break;
                }
                Some(c) => {
                    text.push(c);
                    self.cursor.advance();
                }
            }
        }

        if !text.is_empty() {
            parts.push(LiteralPart::Text(text));
        }

        let end_col = self.cursor.column().saturating_sub(1).max(start_col);
        let span = Span::new(file, start_line, start_col, self.cursor.line(), end_col);
        Token::with_literal(
            span,
            PrefixedLiteralData {
                prefix,
                prefix_span,
                parts,
            },
        )
    }

    fn unterminated_prefixed_literal(&mut self, prefix: &str, file: &FilePath, multiline: bool) {
        let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
        let form = if multiline { "multi-line " } else { "" };
        self.diagnostics.push(Diagnostic {
            severity: crate::common::diagnostics::Severity::Error,
            span,
            message: format!("unterminated {form}`{prefix}\"...\"` literal"),
            tag: None,
        });
    }

    /// Interpolation inside a prefixed literal: `$ident`, `${expr}`,
    /// `$..ident`, `$..{expr}`.
    ///
    /// Deliberately a sibling of `handle_interpolation` rather than a shared
    /// implementation: that one assembles `.format()` calls, which is a
    /// different meaning, and it is covered by its own tests. The one thing not
    /// copied from it is the blanket span remap — these sub-tokens live in the
    /// token payload and never enter the main stream, so the layout filter
    /// never sees them and real spans are safe to keep.
    fn handle_prefixed_interpolation(
        &mut self,
        text: &mut String,
        parts: &mut Vec<LiteralPart>,
        file: &FilePath,
    ) {
        let dollar_line = self.cursor.line();
        let dollar_col = self.cursor.column();

        let spread = self.cursor.peek_at(1) == Some('.') && self.cursor.peek_at(2) == Some('.');
        let head = if spread { 3 } else { 1 };

        match self.cursor.peek_at(head) {
            Some(c) if is_ident_start(c) => {
                if !text.is_empty() {
                    parts.push(LiteralPart::Text(std::mem::take(text)));
                }
                for _ in 0..head {
                    self.cursor.advance();
                }

                let ident_line = self.cursor.line();
                let ident_col = self.cursor.column();
                let mut ident = String::new();
                while let Some(ch) = self.cursor.peek() {
                    if is_ident_continue(ch) {
                        ident.push(ch);
                        self.cursor.advance();
                    } else {
                        break;
                    }
                }

                if TokenKind::keyword(&ident).is_some() {
                    let span = Span::point(file.clone(), dollar_line, dollar_col);
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: format!("cannot use keyword '{}' in string interpolation", ident),
                        tag: None,
                    });
                    text.push('$');
                    if spread {
                        text.push_str("..");
                    }
                    text.push_str(&ident);
                    return;
                }

                let ident_end = ident_col + ident.len() as u32 - 1;
                let ident_span =
                    Span::new(file.clone(), ident_line, ident_col, ident_line, ident_end);
                let part_span =
                    Span::new(file.clone(), dollar_line, dollar_col, ident_line, ident_end);
                let tokens = vec![Token::new(TokenKind::Ident, ident_span, ident)];
                parts.push(if spread {
                    LiteralPart::Spread {
                        tokens,
                        span: part_span,
                    }
                } else {
                    LiteralPart::Value {
                        tokens,
                        span: part_span,
                    }
                });
            }
            Some('{') => {
                if !text.is_empty() {
                    parts.push(LiteralPart::Text(std::mem::take(text)));
                }
                for _ in 0..=head {
                    self.cursor.advance();
                }

                let open_line = self.cursor.line();
                let open_col = self.cursor.column();
                let expr_text = self.scan_interpolation_expr_text(file);
                let close_line = self.cursor.line();
                let close_col = self.cursor.column();
                self.cursor.advance(); // consume '}'

                let mut sub_lexer = Lexer::new_at(&expr_text, file.clone(), open_line, open_col);
                let mut tokens = sub_lexer.tokenize();
                if tokens.last().is_some_and(|t| t.kind == TokenKind::Eof) {
                    tokens.pop();
                }
                self.diagnostics.extend(sub_lexer.diagnostics);

                let part_span =
                    Span::new(file.clone(), dollar_line, dollar_col, close_line, close_col);
                parts.push(if spread {
                    LiteralPart::Spread {
                        tokens,
                        span: part_span,
                    }
                } else {
                    LiteralPart::Value {
                        tokens,
                        span: part_span,
                    }
                });
            }
            _ => {
                let span = Span::point(file.clone(), dollar_line, dollar_col);
                let message = if spread {
                    "expected identifier or '{' after '$..' in string interpolation".to_string()
                } else {
                    "expected identifier or '{' after '$' in string interpolation".to_string()
                };
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message,
                    tag: None,
                });
                text.push('$');
                self.cursor.advance();
            }
        }
    }

    fn scan_string_literal(&mut self) -> Vec<Token> {
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        self.cursor.advance(); // consume opening '"'

        let mut parts: Vec<InterpolationPart> = Vec::new();
        let mut text = String::new();
        loop {
            match self.cursor.peek() {
                Some('"') => {
                    self.cursor.advance(); // consume closing '"'
                    break;
                }
                Some('\\') => {
                    self.cursor.advance(); // consume '\'
                    self.scan_escape_sequence(&mut text, &file);
                }
                Some('$') => {
                    self.handle_interpolation(&mut text, &mut parts, &file);
                }
                Some('\n') | Some('\r') | None => {
                    let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: "unterminated string literal".to_string(),
                        tag: None,
                    });
                    break;
                }
                Some(c) => {
                    text.push(c);
                    self.cursor.advance();
                }
            }
        }

        // Flush remaining text
        if !text.is_empty() {
            parts.push(InterpolationPart::Text(text));
        }

        let end_col = self.cursor.column().saturating_sub(1).max(start_col);
        let span = Span::new(
            file.clone(),
            start_line,
            start_col,
            self.cursor.line(),
            end_col,
        );
        self.assemble_interpolation_parts(parts, span)
    }

    fn scan_multiline_string_literal(&mut self) -> Vec<Token> {
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        // Consume opening """
        self.cursor.advance();
        self.cursor.advance();
        self.cursor.advance();

        // Strip leading newline (if present)
        if self.cursor.peek() == Some('\r') && self.cursor.peek_at(1) == Some('\n') {
            self.cursor.advance();
            self.cursor.advance();
        } else if self.cursor.peek() == Some('\n') {
            self.cursor.advance();
        }

        let mut parts: Vec<InterpolationPart> = Vec::new();
        let mut text = String::new();
        loop {
            match self.cursor.peek() {
                Some('"')
                    if self.cursor.peek_at(1) == Some('"')
                        && self.cursor.peek_at(2) == Some('"') =>
                {
                    // Consume closing """
                    self.cursor.advance();
                    self.cursor.advance();
                    self.cursor.advance();
                    break;
                }
                Some('\\') => {
                    self.cursor.advance(); // consume '\'
                    self.scan_escape_sequence(&mut text, &file);
                }
                Some('$') => {
                    self.handle_interpolation(&mut text, &mut parts, &file);
                }
                None => {
                    let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: "unterminated multi-line string literal".to_string(),
                        tag: None,
                    });
                    break;
                }
                Some(c) => {
                    text.push(c);
                    self.cursor.advance();
                }
            }
        }

        // Flush remaining text
        if !text.is_empty() {
            parts.push(InterpolationPart::Text(text));
        }

        let end_col = self.cursor.column().saturating_sub(1).max(start_col);
        let span = Span::new(
            file.clone(),
            start_line,
            start_col,
            self.cursor.line(),
            end_col,
        );
        self.assemble_interpolation_parts(parts, span)
    }

    fn handle_interpolation(
        &mut self,
        text: &mut String,
        parts: &mut Vec<InterpolationPart>,
        file: &FilePath,
    ) {
        let dollar_line = self.cursor.line();
        let dollar_col = self.cursor.column();

        // Peek at what follows '$'
        match self.cursor.peek_at(1) {
            Some(c) if is_ident_start(c) => {
                // $ident — simple variable interpolation
                // Flush accumulated text
                if !text.is_empty() {
                    parts.push(InterpolationPart::Text(std::mem::take(text)));
                }

                self.cursor.advance(); // consume '$'

                // Scan identifier
                let mut ident = String::new();
                while let Some(ch) = self.cursor.peek() {
                    if is_ident_continue(ch) {
                        ident.push(ch);
                        self.cursor.advance();
                    } else {
                        break;
                    }
                }

                // Check if it's a keyword
                if TokenKind::keyword(&ident).is_some() {
                    let span = Span::point(file.clone(), dollar_line, dollar_col);
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: format!("cannot use keyword '{}' in string interpolation", ident),
                        tag: None,
                    });
                    // Error recovery: push as literal text
                    text.push('$');
                    text.push_str(&ident);
                } else {
                    parts.push(InterpolationPart::Var {
                        name: ident,
                        line: dollar_line,
                        col: dollar_col,
                    });
                }
            }
            Some('{') => {
                // ${expr} — expression interpolation
                // Flush accumulated text
                if !text.is_empty() {
                    parts.push(InterpolationPart::Text(std::mem::take(text)));
                }

                self.cursor.advance(); // consume '$'
                self.cursor.advance(); // consume '{'

                let expr_text = self.scan_interpolation_expr_text(file);
                self.cursor.advance(); // consume closing '}'

                // Create a sub-lexer to tokenize the expression
                let mut sub_lexer = Lexer::new(&expr_text, file.clone());
                let mut sub_tokens = sub_lexer.tokenize();
                // Remove trailing Eof
                if sub_tokens.last().is_some_and(|t| t.kind == TokenKind::Eof) {
                    sub_tokens.pop();
                }
                // Remap sub-token spans to the $ position so the layout filter
                // doesn't get confused by column 1 from the sub-lexer
                let remap_span = Span::point(file.clone(), dollar_line, dollar_col);
                for tok in &mut sub_tokens {
                    tok.span = remap_span.clone();
                }
                // Propagate diagnostics
                self.diagnostics.extend(sub_lexer.diagnostics);

                parts.push(InterpolationPart::Expr {
                    tokens: sub_tokens,
                    line: dollar_line,
                    col: dollar_col,
                });
            }
            Some('.') if self.cursor.peek_at(2) == Some('.') => {
                // `$..` is spread interpolation, which only a prefixed literal
                // knows what to do with — an ordinary string has no builder to
                // call `spread` on.
                let span = Span::point(file.clone(), dollar_line, dollar_col);
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "`$..` spread interpolation is only allowed in a prefixed string literal (e.g. `sql\"... IN ($..ids)\"`)"
                        .to_string(),
                    tag: None,
                });
                text.push_str("$..");
                self.cursor.advance();
                self.cursor.advance();
                self.cursor.advance();
            }
            _ => {
                // '$' not followed by ident or '{'
                let span = Span::point(file.clone(), dollar_line, dollar_col);
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "expected identifier or '{' after '$' in string interpolation"
                        .to_string(),
                    tag: None,
                });
                text.push('$');
                self.cursor.advance(); // consume '$'
            }
        }
    }

    fn scan_interpolation_expr_text(&mut self, file: &FilePath) -> String {
        let source_start = self.cursor.byte_offset();
        let mut expr_text = String::new();
        let mut depth: u32 = 1;

        loop {
            match self.cursor.peek() {
                None => {
                    let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                    self.diagnostics.push(Diagnostic {
                        severity: crate::common::diagnostics::Severity::Error,
                        span,
                        message: "unterminated string interpolation expression".to_string(),
                        tag: None,
                    });
                    break;
                }
                Some('{') => {
                    depth += 1;
                    expr_text.push('{');
                    self.cursor.advance();
                }
                Some('}') => {
                    depth -= 1;
                    if depth == 0 {
                        // Don't consume closing '}' — caller does it
                        break;
                    }
                    expr_text.push('}');
                    self.cursor.advance();
                }
                Some('"') => {
                    // Collect entire string literal to avoid counting its braces
                    expr_text.push('"');
                    self.cursor.advance();
                    loop {
                        match self.cursor.peek() {
                            Some('"') => {
                                expr_text.push('"');
                                self.cursor.advance();
                                break;
                            }
                            Some('\\') => {
                                expr_text.push('\\');
                                self.cursor.advance();
                                if let Some(c) = self.cursor.peek() {
                                    expr_text.push(c);
                                    self.cursor.advance();
                                }
                            }
                            Some(c) => {
                                expr_text.push(c);
                                self.cursor.advance();
                            }
                            None => break,
                        }
                    }
                }
                Some('\'') => {
                    // Collect entire char literal
                    expr_text.push('\'');
                    self.cursor.advance();
                    loop {
                        match self.cursor.peek() {
                            Some('\'') => {
                                expr_text.push('\'');
                                self.cursor.advance();
                                break;
                            }
                            Some('\\') => {
                                expr_text.push('\\');
                                self.cursor.advance();
                                if let Some(c) = self.cursor.peek() {
                                    expr_text.push(c);
                                    self.cursor.advance();
                                }
                            }
                            Some(c) => {
                                expr_text.push(c);
                                self.cursor.advance();
                            }
                            None => break,
                        }
                    }
                }
                Some(c) => {
                    expr_text.push(c);
                    self.cursor.advance();
                }
            }
        }

        if self.capture_source {
            self.source_interpolations
                .push(source_start..self.cursor.byte_offset());
        }
        expr_text
    }

    fn assemble_interpolation_parts(
        &self,
        parts: Vec<InterpolationPart>,
        span: Span,
    ) -> Vec<Token> {
        // Filter out empty Text parts
        let parts: Vec<_> = parts
            .into_iter()
            .filter(|p| !matches!(p, InterpolationPart::Text(s) if s.is_empty()))
            .collect();

        // If empty after filtering, return a single empty StringLiteral
        if parts.is_empty() {
            return vec![Token::new(TokenKind::StringLiteral, span, "")];
        }

        // If single Text part, return a single StringLiteral (unchanged behavior)
        if parts.len() == 1
            && let InterpolationPart::Text(s) = &parts[0]
        {
            return vec![Token::new(TokenKind::StringLiteral, span, s.clone())];
        }

        let mut tokens = Vec::new();
        let point_span = Span::point(span.file.clone(), span.line, span.column);

        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                tokens.push(Token::new(TokenKind::PlusPlus, point_span.clone(), "++"));
            }
            match part {
                InterpolationPart::Text(s) => {
                    tokens.push(Token::new(TokenKind::StringLiteral, span.clone(), s));
                }
                InterpolationPart::Var { name, line, col } => {
                    let var_span = Span::point(span.file.clone(), line, col);
                    tokens.push(Token::new(TokenKind::Ident, var_span.clone(), name));
                    // Append .format() so the typechecker sees a Display method call
                    tokens.push(Token::new(TokenKind::Dot, point_span.clone(), "."));
                    tokens.push(Token::new(TokenKind::Ident, point_span.clone(), "format"));
                    tokens.push(Token::new(TokenKind::LParen, point_span.clone(), "("));
                    tokens.push(Token::new(TokenKind::RParen, point_span.clone(), ")"));
                }
                InterpolationPart::Expr {
                    tokens: sub_tokens,
                    line,
                    col,
                } => {
                    let expr_span = Span::point(span.file.clone(), line, col);
                    tokens.push(Token::new(TokenKind::LParen, expr_span.clone(), "("));
                    tokens.extend(sub_tokens);
                    tokens.push(Token::new(TokenKind::RParen, expr_span, ")"));
                    // Append .format() so the typechecker sees a Display method call
                    tokens.push(Token::new(TokenKind::Dot, point_span.clone(), "."));
                    tokens.push(Token::new(TokenKind::Ident, point_span.clone(), "format"));
                    tokens.push(Token::new(TokenKind::LParen, point_span.clone(), "("));
                    tokens.push(Token::new(TokenKind::RParen, point_span.clone(), ")"));
                }
            }
        }

        tokens
    }

    fn scan_char_literal(&mut self) -> Token {
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        self.cursor.advance(); // consume opening '\''

        let mut text = String::new();
        let mut had_error = false;

        match self.cursor.peek() {
            Some('\'') => {
                // Empty character literal
                let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "empty character literal".to_string(),
                    tag: None,
                });
                self.cursor.advance(); // consume closing '\''
                had_error = true;
            }
            Some('\\') => {
                self.cursor.advance(); // consume '\'
                self.scan_escape_sequence(&mut text, &file);
            }
            None | Some('\n') | Some('\r') => {
                let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "unterminated character literal".to_string(),
                    tag: None,
                });
                had_error = true;
            }
            Some(c) => {
                text.push(c);
                self.cursor.advance();
            }
        }

        if !had_error {
            // Expect closing '\''
            if self.cursor.peek() == Some('\'') {
                self.cursor.advance();
            } else {
                let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "unterminated character literal".to_string(),
                    tag: None,
                });
            }
        }

        let end_col = self.cursor.column().saturating_sub(1).max(start_col);
        let span = Span::new(
            file.clone(),
            start_line,
            start_col,
            self.cursor.line(),
            end_col,
        );
        Token::new(TokenKind::CharLiteral, span, text)
    }

    fn scan_escape_sequence(&mut self, text: &mut String, file: &FilePath) {
        match self.cursor.peek() {
            Some('n') => {
                text.push('\n');
                self.cursor.advance();
            }
            Some('t') => {
                text.push('\t');
                self.cursor.advance();
            }
            Some('r') => {
                text.push('\r');
                self.cursor.advance();
            }
            Some('\\') => {
                text.push('\\');
                self.cursor.advance();
            }
            Some('"') => {
                text.push('"');
                self.cursor.advance();
            }
            Some('\'') => {
                text.push('\'');
                self.cursor.advance();
            }
            Some('0') => {
                text.push('\0');
                self.cursor.advance();
            }
            Some('$') => {
                text.push('$');
                self.cursor.advance();
            }
            Some('u') => {
                self.cursor.advance(); // consume 'u'
                self.scan_unicode_escape(text);
            }
            Some(c) => {
                let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: format!("unknown escape sequence: '\\{}'", c),
                    tag: None,
                });
                text.push(c);
                self.cursor.advance();
            }
            None => {
                let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "unterminated string literal".to_string(),
                    tag: None,
                });
            }
        }
    }

    fn scan_unicode_escape(&mut self, text: &mut String) {
        let file = self.cursor.file();

        // Expect '{'
        match self.cursor.peek() {
            Some('{') => {
                self.cursor.advance();
            }
            _ => {
                let span = Span::point(file, self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "expected '{' after '\\u'".to_string(),
                    tag: None,
                });
                return;
            }
        }

        // Read 1-6 hex digits
        let mut hex = String::new();
        while let Some(c) = self.cursor.peek() {
            if c.is_ascii_hexdigit() && hex.len() < 6 {
                hex.push(c);
                self.cursor.advance();
            } else {
                break;
            }
        }

        if hex.is_empty() {
            let span = Span::point(file.clone(), self.cursor.line(), self.cursor.column());
            self.diagnostics.push(Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span,
                message: "expected hex digit in unicode escape".to_string(),
                tag: None,
            });
            // Skip to closing '}'
            while let Some(c) = self.cursor.peek() {
                if c == '}' {
                    self.cursor.advance();
                    break;
                }
                self.cursor.advance();
            }
            return;
        }

        // Expect '}'
        match self.cursor.peek() {
            Some('}') => {
                self.cursor.advance();
            }
            _ => {
                let span = Span::point(file, self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "expected '}' to close unicode escape".to_string(),
                    tag: None,
                });
                return;
            }
        }

        // Parse hex value and validate code point
        let code_point = u32::from_str_radix(&hex, 16).unwrap();
        if code_point > 0x10FFFF {
            let span = Span::point(file, self.cursor.line(), self.cursor.column());
            self.diagnostics.push(Diagnostic {
                severity: crate::common::diagnostics::Severity::Error,
                span,
                message: "unicode escape out of range (max U+10FFFF)".to_string(),
                tag: None,
            });
            return;
        }

        match char::from_u32(code_point) {
            Some(c) => text.push(c),
            None => {
                let span = Span::point(file, self.cursor.line(), self.cursor.column());
                self.diagnostics.push(Diagnostic {
                    severity: crate::common::diagnostics::Severity::Error,
                    span,
                    message: "invalid unicode code point".to_string(),
                    tag: None,
                });
            }
        }
    }

    fn scan_number_literal(&mut self) -> Token {
        if let Some(token) = self.scan_exact_number() {
            return token;
        }
        let start_line = self.cursor.line();
        let start_col = self.cursor.column();
        let file = self.cursor.file();

        let mut text = String::new();
        let mut is_float = false;

        // Check for base prefix
        if self.cursor.peek() == Some('0') {
            match self.cursor.peek_at(1) {
                Some('x') | Some('X') => {
                    text.push('0');
                    self.cursor.advance();
                    text.push(self.cursor.peek().unwrap());
                    self.cursor.advance();
                    while let Some(ch) = self.cursor.peek() {
                        if ch.is_ascii_hexdigit() {
                            text.push(ch);
                            self.cursor.advance();
                        } else {
                            break;
                        }
                    }
                    return self.finish_int_literal(text, start_line, start_col, file);
                }
                Some('b') | Some('B') => {
                    text.push('0');
                    self.cursor.advance();
                    text.push(self.cursor.peek().unwrap());
                    self.cursor.advance();
                    while let Some(ch) = self.cursor.peek() {
                        if ch == '0' || ch == '1' {
                            text.push(ch);
                            self.cursor.advance();
                        } else {
                            break;
                        }
                    }
                    return self.finish_int_literal(text, start_line, start_col, file);
                }
                Some('o') | Some('O') => {
                    text.push('0');
                    self.cursor.advance();
                    text.push(self.cursor.peek().unwrap());
                    self.cursor.advance();
                    while let Some(ch) = self.cursor.peek() {
                        if ch.is_ascii_digit() && ch < '8' {
                            text.push(ch);
                            self.cursor.advance();
                        } else {
                            break;
                        }
                    }
                    return self.finish_int_literal(text, start_line, start_col, file);
                }
                _ => {}
            }
        }

        // Decimal digits
        while let Some(ch) = self.cursor.peek() {
            if ch.is_ascii_digit() {
                text.push(ch);
                self.cursor.advance();
            } else {
                break;
            }
        }

        // Check for decimal point (only if followed by a digit to avoid `42.method`)
        if self.cursor.peek() == Some('.')
            && self.cursor.peek_at(1).is_some_and(|c| c.is_ascii_digit())
        {
            is_float = true;
            text.push('.');
            self.cursor.advance();
            while let Some(ch) = self.cursor.peek() {
                if ch.is_ascii_digit() {
                    text.push(ch);
                    self.cursor.advance();
                } else {
                    break;
                }
            }
        }

        // Check for exponent
        if self.cursor.peek() == Some('e') || self.cursor.peek() == Some('E') {
            is_float = true;
            text.push(self.cursor.peek().unwrap());
            self.cursor.advance();
            if self.cursor.peek() == Some('+') || self.cursor.peek() == Some('-') {
                text.push(self.cursor.peek().unwrap());
                self.cursor.advance();
            }
            while let Some(ch) = self.cursor.peek() {
                if ch.is_ascii_digit() {
                    text.push(ch);
                    self.cursor.advance();
                } else {
                    break;
                }
            }
        }

        // Check for suffix
        if self.cursor.peek() == Some('f') {
            // Float suffix: f32 or f64
            is_float = true;
            text.push('f');
            self.cursor.advance();
            // Consume suffix digits (32 or 64)
            while let Some(ch) = self.cursor.peek() {
                if ch.is_ascii_digit() {
                    text.push(ch);
                    self.cursor.advance();
                } else {
                    break;
                }
            }
        }

        if is_float {
            let end_col = self.cursor.column().saturating_sub(1).max(start_col);
            let span = Span::new(file, start_line, start_col, start_line, end_col);
            Token::new(TokenKind::FloatLiteral, span, text)
        } else {
            self.finish_int_literal(text, start_line, start_col, file)
        }
    }

    fn finish_int_literal(
        &mut self,
        mut text: String,
        start_line: u32,
        start_col: u32,
        file: FilePath,
    ) -> Token {
        // Check for integer suffix: i8, i16, i32, i64, u8, u16, u32, u64
        if let Some(ch) = self.cursor.peek()
            && (ch == 'i' || ch == 'u')
        {
            text.push(ch);
            self.cursor.advance();
            while let Some(d) = self.cursor.peek() {
                if d.is_ascii_digit() {
                    text.push(d);
                    self.cursor.advance();
                } else {
                    break;
                }
            }
        }
        let end_col = self.cursor.column().saturating_sub(1).max(start_col);
        let span = Span::new(file, start_line, start_col, start_line, end_col);
        Token::new(TokenKind::IntLiteral, span, text)
    }

    fn make_token(&self, kind: TokenKind, text: &str) -> Token {
        let span = Span::point(self.cursor.file(), self.cursor.line(), self.cursor.column());
        Token::new(kind, span, text)
    }
}

/// Remove `DocComment` tokens from the stream and attach their text
/// to the next meaningful (non-Newline, non-DocComment) token via its
/// `doc_comment` field. Consecutive doc-comment lines are joined with `\n`.
pub fn attach_doc_comments(tokens: Vec<Token>) -> Vec<Token> {
    let mut result = Vec::with_capacity(tokens.len());
    let mut pending_doc: Option<String> = None;

    for token in tokens {
        match token.kind {
            TokenKind::DocComment => {
                if let Some(ref mut doc) = pending_doc {
                    doc.push('\n');
                    doc.push_str(&token.text);
                } else {
                    pending_doc = Some(token.text);
                }
            }
            TokenKind::Newline => {
                // Preserve newlines in output; don't attach doc comment to them
                result.push(token);
            }
            _ => {
                let mut token = token;
                if let Some(doc) = pending_doc.take() {
                    token.doc_comment = Some(doc);
                }
                result.push(token);
            }
        }
    }
    result
}

pub(crate) fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

pub(crate) fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        lexer.tokenize()
    }

    fn lex_with_diagnostics(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        let diagnostics = lexer.diagnostics().to_vec();
        (tokens, diagnostics)
    }

    /// The sole prefixed literal in `source`, with its payload.
    fn prefixed(source: &str) -> PrefixedLiteralData {
        let tokens = lex(source);
        let token = tokens
            .iter()
            .find(|t| t.kind == TokenKind::PrefixedStringLiteral)
            .expect("expected a prefixed string literal");
        (*token.literal.as_ref().expect("payload").clone()).clone()
    }

    fn part_text(part: &LiteralPart) -> &str {
        match part {
            LiteralPart::Text(t) => t.as_str(),
            _ => panic!("expected a text part"),
        }
    }

    fn kinds(tokens: &[Token]) -> Vec<TokenKind> {
        tokens.iter().map(|t| t.kind).collect()
    }

    #[test]
    fn test_lex_minimal_program() {
        let tokens = lex("package a\n\nfunction main(): Unit = ()");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Package,
                TokenKind::Ident, // a
                TokenKind::Newline,
                TokenKind::Newline,
                TokenKind::Function,
                TokenKind::Ident, // main
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Colon,
                TokenKind::Ident, // Unit
                TokenKind::Equals,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "package");
        assert_eq!(tokens[1].text, "a");
        assert_eq!(tokens[5].text, "main");
        assert_eq!(tokens[9].text, "Unit");
    }

    #[test]
    fn test_lex_empty() {
        let tokens = lex("");
        assert_eq!(kinds(&tokens), vec![TokenKind::Eof]);
    }

    #[test]
    fn test_lex_comment() {
        let tokens = lex("// this is a comment\npackage a");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Newline,
                TokenKind::Package,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_lex_unknown_char() {
        let mut lexer = Lexer::new("\\", FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        assert_eq!(kinds(&tokens), vec![TokenKind::Eof]);
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("unexpected character")
        );
    }

    #[test]
    fn test_lex_at_symbol() {
        let tokens = lex("@skip");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::At, TokenKind::Ident, TokenKind::Eof]
        );
    }

    #[test]
    fn test_lex_dotted_path() {
        let tokens = lex("com.example.myapp");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_lex_bool_literals() {
        let tokens = lex("true false");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::True, TokenKind::False, TokenKind::Eof]
        );
    }

    #[test]
    fn test_lex_panic_assert() {
        let tokens = lex("panic assert");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::Panic, TokenKind::Assert, TokenKind::Eof]
        );
    }

    #[test]
    fn test_lex_comma() {
        let tokens = lex("a, b");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Comma,
                TokenKind::Ident,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_lex_string_literal() {
        let tokens = lex("\"hello\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_lex_string_with_escapes() {
        let tokens = lex("\"hello\\nworld\\t\\\\\\\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello\nworld\t\\\"");
    }

    #[test]
    fn test_lex_unterminated_string() {
        let mut lexer = Lexer::new("\"hello", FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("unterminated string")
        );
    }

    #[test]
    fn test_lex_string_null_escape() {
        let tokens = lex("\"\\0\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "\0");
        assert_eq!(tokens[0].text.len(), 1);
    }

    #[test]
    fn test_lex_string_dollar_escape() {
        let tokens = lex("\"\\$\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "$");
    }

    #[test]
    fn test_lex_string_unicode_escape() {
        let tokens = lex("\"\\u{41}\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "A");
    }

    #[test]
    fn test_lex_string_unicode_escape_multi_digit() {
        let tokens = lex("\"\\u{2764}\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "❤");
    }

    #[test]
    fn test_lex_string_unicode_escape_max() {
        let tokens = lex("\"\\u{10FFFF}\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text.len(), 4); // max code point is 4 bytes in UTF-8
    }

    #[test]
    fn test_lex_string_unicode_escape_error_missing_brace() {
        let mut lexer = Lexer::new("\"\\u41}\"", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("expected '{' after '\\u'")
        );
    }

    #[test]
    fn test_lex_string_unicode_escape_error_out_of_range() {
        let mut lexer = Lexer::new("\"\\u{110000}\"", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("unicode escape out of range")
        );
    }

    #[test]
    fn test_lex_string_unicode_escape_error_surrogate() {
        let mut lexer = Lexer::new("\"\\u{D800}\"", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("invalid unicode code point")
        );
    }

    #[test]
    fn test_span_tracking() {
        let tokens = lex("package a");
        // "package" starts at line 1, col 1
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.column, 1);
        // "a" starts at line 1, col 9
        assert_eq!(tokens[1].span.line, 1);
        assert_eq!(tokens[1].span.column, 9);
    }

    #[test]
    fn test_lex_multiline_string_basic() {
        let tokens = lex("\"\"\"\nhello\n\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello\n");
    }

    #[test]
    fn test_lex_multiline_string_strips_leading_newline() {
        let tokens = lex("\"\"\"\nhello\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_lex_multiline_string_strips_leading_crlf() {
        let tokens = lex("\"\"\"\r\nhello\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_lex_multiline_string_no_leading_newline() {
        let tokens = lex("\"\"\"hello\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_lex_multiline_string_embedded_quote() {
        let tokens = lex("\"\"\"\n\"hi\"\n\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "\"hi\"\n");
    }

    #[test]
    fn test_lex_multiline_string_embedded_double_quote() {
        let tokens = lex("\"\"\"\na\"\"b\n\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "a\"\"b\n");
    }

    #[test]
    fn test_lex_multiline_string_with_escapes() {
        let tokens = lex("\"\"\"\nhello\\tworld\n\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "hello\tworld\n");
    }

    #[test]
    fn test_lex_multiline_string_empty() {
        let tokens = lex("\"\"\"\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "");
    }

    #[test]
    fn test_lex_multiline_string_unterminated() {
        let mut lexer = Lexer::new("\"\"\"hello", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(lexer.diagnostics()[0].message.contains("unterminated"));
    }

    #[test]
    fn test_lex_char_literal() {
        let tokens = lex("'A'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "A");
    }

    #[test]
    fn test_lex_char_escape_newline() {
        let tokens = lex("'\\n'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "\n");
    }

    #[test]
    fn test_lex_char_escape_tab() {
        let tokens = lex("'\\t'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "\t");
    }

    #[test]
    fn test_lex_char_escape_backslash() {
        let tokens = lex("'\\\\'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "\\");
    }

    #[test]
    fn test_lex_char_escape_single_quote() {
        let tokens = lex("'\\''");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "'");
    }

    #[test]
    fn test_lex_char_unicode_escape() {
        let tokens = lex("'\\u{41}'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "A");
    }

    #[test]
    fn test_lex_char_unicode_escape_emoji() {
        let tokens = lex("'\\u{2764}'");
        assert_eq!(kinds(&tokens), vec![TokenKind::CharLiteral, TokenKind::Eof]);
        assert_eq!(tokens[0].text, "❤");
    }

    #[test]
    fn test_lex_char_empty_error() {
        let mut lexer = Lexer::new("''", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(
            lexer.diagnostics()[0]
                .message
                .contains("empty character literal")
        );
    }

    #[test]
    fn test_lex_char_unterminated() {
        let mut lexer = Lexer::new("'A", FilePath::from("test.dove"));
        let _tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(lexer.diagnostics()[0].message.contains("unterminated"));
    }

    #[test]
    fn test_interpolation_simple_var() {
        let tokens = lex("\"hello $name\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "hello ");
        assert_eq!(tokens[2].text, "name");
        assert_eq!(tokens[4].text, "format");
    }

    #[test]
    fn test_interpolation_var_middle() {
        let tokens = lex("\"hi $x bye\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::PlusPlus,
                TokenKind::StringLiteral,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "hi ");
        assert_eq!(tokens[2].text, "x");
        assert_eq!(tokens[8].text, " bye");
    }

    #[test]
    fn test_interpolation_expr() {
        let tokens = lex("\"val: ${x}\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::LParen,
                TokenKind::Ident,
                TokenKind::RParen,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "val: ");
        assert_eq!(tokens[3].text, "x");
    }

    #[test]
    fn test_interpolation_only_var() {
        let tokens = lex("\"$a\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "a");
        assert_eq!(tokens[2].text, "format");
    }

    #[test]
    fn test_interpolation_consecutive_vars() {
        let tokens = lex("\"$a$b\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::PlusPlus,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "a");
        assert_eq!(tokens[6].text, "b");
    }

    #[test]
    fn test_interpolation_escaped_dollar() {
        let tokens = lex("\"price: \\$5\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "price: $5");
    }

    #[test]
    fn test_interpolation_no_interpolation() {
        let tokens = lex("\"plain\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "plain");
    }

    #[test]
    fn test_interpolation_empty_string() {
        let tokens = lex("\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "");
    }

    #[test]
    fn test_interpolation_dollar_error() {
        let mut lexer = Lexer::new("\"test $\"", FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(lexer.diagnostics()[0].message.contains("$"));
        // Error recovery: '$' treated as literal text
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::StringLiteral, TokenKind::Eof]
        );
        assert_eq!(tokens[0].text, "test $");
    }

    #[test]
    fn test_interpolation_expr_nested_braces() {
        let tokens = lex("\"${f(a)}\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::LParen,
                TokenKind::Ident, // f
                TokenKind::LParen,
                TokenKind::Ident, // a
                TokenKind::RParen,
                TokenKind::RParen,
                TokenKind::Dot,
                TokenKind::Ident, // format
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_interpolation_multiline() {
        let tokens = lex("\"\"\"\nhello $x world\n\"\"\"");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::PlusPlus,
                TokenKind::StringLiteral,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "hello ");
        assert_eq!(tokens[2].text, "x");
        assert_eq!(tokens[8].text, " world\n");
    }

    #[test]
    fn test_interpolation_keyword_error() {
        let mut lexer = Lexer::new("\"hello $if\"", FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        assert_eq!(lexer.diagnostics().len(), 1);
        assert!(lexer.diagnostics()[0].message.contains("keyword"));
        // Error recovery: keyword treated as literal text, but split into two parts
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::StringLiteral,
                TokenKind::Eof,
            ]
        );
        assert_eq!(tokens[0].text, "hello ");
        assert_eq!(tokens[2].text, "$if");
    }

    #[test]
    fn test_lex_ampamp() {
        let tokens = lex("true && false");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::True,
                TokenKind::AmpAmp,
                TokenKind::False,
                TokenKind::Eof
            ]
        );
        assert_eq!(tokens[1].text, "&&");
    }

    #[test]
    fn test_lex_pipepipe() {
        let tokens = lex("true || false");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::True,
                TokenKind::PipePipe,
                TokenKind::False,
                TokenKind::Eof
            ]
        );
        assert_eq!(tokens[1].text, "||");
    }

    #[test]
    fn test_lex_single_amp_still_works() {
        let tokens = lex("a & b");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Ampersand,
                TokenKind::Ident,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_lex_single_pipe_still_works() {
        let tokens = lex("a | b");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Ident,
                TokenKind::Pipe,
                TokenKind::Ident,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_doc_comment_produces_token() {
        let tokens = lex("/// hello");
        assert_eq!(tokens[0].kind, TokenKind::DocComment);
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_doc_comment_strips_leading_space() {
        let tokens = lex("/// hello world");
        assert_eq!(tokens[0].kind, TokenKind::DocComment);
        assert_eq!(tokens[0].text, "hello world");
    }

    #[test]
    fn test_doc_comment_no_space() {
        let tokens = lex("///hello");
        assert_eq!(tokens[0].kind, TokenKind::DocComment);
        assert_eq!(tokens[0].text, "hello");
    }

    #[test]
    fn test_four_slashes_is_regular_comment() {
        let tokens = lex("////not a doc comment\nx");
        // //// is a regular comment, should be skipped
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::Newline, TokenKind::Ident, TokenKind::Eof]
        );
    }

    #[test]
    fn test_regular_comment_still_skipped() {
        let tokens = lex("// regular\nx");
        assert_eq!(
            kinds(&tokens),
            vec![TokenKind::Newline, TokenKind::Ident, TokenKind::Eof]
        );
    }

    #[test]
    fn test_empty_doc_comment() {
        let tokens = lex("///");
        assert_eq!(tokens[0].kind, TokenKind::DocComment);
        assert_eq!(tokens[0].text, "");
    }

    #[test]
    fn test_attach_doc_comments_single() {
        let tokens = lex("/// A function\nfunction");
        let tokens = attach_doc_comments(tokens);
        let func = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Function)
            .unwrap();
        assert_eq!(func.doc_comment.as_deref(), Some("A function"));
    }

    #[test]
    fn test_attach_doc_comments_multiline() {
        let tokens = lex("/// Line 1\n/// Line 2\nfunction");
        let tokens = attach_doc_comments(tokens);
        let func = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Function)
            .unwrap();
        assert_eq!(func.doc_comment.as_deref(), Some("Line 1\nLine 2"));
    }

    #[test]
    fn test_attach_doc_comments_removes_doc_tokens() {
        let tokens = lex("/// doc\nfunction");
        let tokens = attach_doc_comments(tokens);
        assert!(!tokens.iter().any(|t| t.kind == TokenKind::DocComment));
    }

    #[test]
    fn test_no_doc_comment_means_none() {
        let tokens = lex("function");
        let tokens = attach_doc_comments(tokens);
        let func = tokens
            .iter()
            .find(|t| t.kind == TokenKind::Function)
            .unwrap();
        assert!(func.doc_comment.is_none());
    }

    // ---- prefixed string literals -------------------------------------

    #[test]
    fn prefixed_literal_splits_text_and_values() {
        let data = prefixed(r#"let q = sql"a $b c""#);
        assert_eq!(data.prefix, "sql");
        assert_eq!(data.parts.len(), 3);
        assert_eq!(part_text(&data.parts[0]), "a ");
        assert_eq!(part_text(&data.parts[2]), " c");
        match &data.parts[1] {
            LiteralPart::Value { tokens, span } => {
                assert_eq!(tokens.len(), 1);
                assert_eq!(tokens[0].text, "b");
                // `$` is at column 15 (1-based), `b` at 16.
                assert_eq!(span.column, 15);
                assert_eq!(span.end_column, 16);
                assert_eq!(tokens[0].span.column, 16);
            }
            other => panic!("expected a value part, got {other:?}"),
        }
    }

    #[test]
    fn prefixed_literal_recognizes_spread() {
        let data = prefixed(r#"sql"x IN ($..ids)""#);
        let spread = data
            .parts
            .iter()
            .find(|p| matches!(p, LiteralPart::Spread { .. }))
            .expect("expected a spread part");
        match spread {
            LiteralPart::Spread { tokens, .. } => {
                assert_eq!(tokens.len(), 1);
                assert_eq!(tokens[0].text, "ids");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn prefixed_literal_braced_expression_keeps_absolute_columns() {
        // The payoff of seeding the sub-lexer instead of remapping spans:
        // a type error inside `${a.b}` can point at `a` or at `b`.
        let source = r#"let q = sql"${a.b}""#;
        let data = prefixed(source);
        match &data.parts[0] {
            LiteralPart::Value { tokens, .. } => {
                assert_eq!(
                    kinds(tokens),
                    vec![TokenKind::Ident, TokenKind::Dot, TokenKind::Ident]
                );
                let a_col = source.find("a.b").unwrap() as u32 + 1;
                assert_eq!(tokens[0].span.column, a_col);
                assert_eq!(tokens[1].span.column, a_col + 1);
                assert_eq!(tokens[2].span.column, a_col + 2);
                assert!(tokens.iter().all(|t| t.span.line == 1));
            }
            other => panic!("expected a value part, got {other:?}"),
        }
    }

    #[test]
    fn prefixed_literal_spread_accepts_a_braced_expression() {
        let data = prefixed(r#"sql"IN ($..{user.ids})""#);
        let spread = data
            .parts
            .iter()
            .find(|p| matches!(p, LiteralPart::Spread { .. }))
            .expect("expected a spread part");
        match spread {
            LiteralPart::Spread { tokens, .. } => {
                assert_eq!(
                    kinds(tokens),
                    vec![TokenKind::Ident, TokenKind::Dot, TokenKind::Ident]
                );
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn prefixed_multiline_literal_spans_real_lines() {
        let source = "let q = sql\"\"\"\nSELECT *\nFROM t WHERE id = $id\n\"\"\"";
        let data = prefixed(source);
        let value = data
            .parts
            .iter()
            .find(|p| matches!(p, LiteralPart::Value { .. }))
            .expect("expected a value part");
        match value {
            LiteralPart::Value { tokens, span } => {
                // Line 1 is `let q = sql"""`; the opener's newline is stripped,
                // so `SELECT *` is line 2 and the interpolation is on line 3.
                assert_eq!(span.line, 3);
                assert_eq!(tokens[0].span.line, 3);
                assert_eq!(tokens[0].text, "id");
                // `FROM t WHERE id = $id` — `$` is column 19, `id` column 20.
                assert_eq!(span.column, 19);
                assert_eq!(tokens[0].span.column, 20);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn prefixed_literal_processes_escapes() {
        let data = prefixed(r#"sql"a\$b""#);
        assert_eq!(data.parts.len(), 1);
        assert_eq!(part_text(&data.parts[0]), "a$b");
    }

    #[test]
    fn prefixed_literal_rejects_a_keyword_interpolation() {
        let (_, diagnostics) = lex_with_diagnostics(r#"sql"x = $match""#);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("cannot use keyword 'match'"))
        );
    }

    #[test]
    fn prefixed_literal_reports_unterminated() {
        let (_, diagnostics) = lex_with_diagnostics(r#"sql"abc"#);
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("unterminated"))
        );
    }

    #[test]
    fn prefixed_literal_nests_inside_a_braced_interpolation() {
        let data = prefixed(r#"sql"${re"x"}""#);
        match &data.parts[0] {
            LiteralPart::Value { tokens, .. } => {
                assert_eq!(kinds(tokens), vec![TokenKind::PrefixedStringLiteral]);
                assert_eq!(tokens[0].literal.as_ref().unwrap().prefix, "re");
            }
            other => panic!("expected a value part, got {other:?}"),
        }
    }

    #[test]
    fn a_space_before_the_quote_is_not_a_prefixed_literal() {
        let tokens = lex(r#"test "name""#);
        assert_eq!(
            kinds(&tokens)[..2],
            [TokenKind::Ident, TokenKind::StringLiteral]
        );
    }

    #[test]
    fn plus_plus_lexes_as_one_token() {
        assert_eq!(
            kinds(&lex("a ++ b"))[..3],
            [TokenKind::Ident, TokenKind::PlusPlus, TokenKind::Ident]
        );
        // A single `+` is unaffected, and `+ +` stays two tokens.
        assert_eq!(
            kinds(&lex("a + b"))[..3],
            [TokenKind::Ident, TokenKind::Plus, TokenKind::Ident]
        );
        assert_eq!(
            kinds(&lex("a + +b"))[..4],
            [
                TokenKind::Ident,
                TokenKind::Plus,
                TokenKind::Plus,
                TokenKind::Ident
            ]
        );
    }

    #[test]
    fn test_is_reserved_so_a_test_declaration_still_parses() {
        // `test` is a soft keyword, not a TokenKind keyword, so it needs its
        // own exclusion — otherwise `test"name" = ...` would silently become a
        // prefixed literal instead of a test declaration.
        let tokens = lex(r#"test"name" = ()"#);
        assert_eq!(
            kinds(&tokens)[..2],
            [TokenKind::Ident, TokenKind::StringLiteral]
        );
    }

    #[test]
    fn a_keyword_before_the_quote_is_not_a_prefixed_literal() {
        let tokens = lex(r#"case"x""#);
        assert_eq!(
            kinds(&tokens)[..2],
            [TokenKind::Case, TokenKind::StringLiteral]
        );
    }

    #[test]
    fn spread_in_a_plain_string_is_rejected_with_a_hint() {
        let (_, diagnostics) = lex_with_diagnostics(r#""IN ($..ids)""#);
        assert!(diagnostics.iter().any(|d| {
            d.message
                .contains("only allowed in a prefixed string literal")
        }));
    }

    #[test]
    fn ordinary_interpolation_is_unchanged() {
        let tokens = lex(r#""a $b c""#);
        assert_eq!(
            kinds(&tokens)[..8],
            [
                TokenKind::StringLiteral,
                TokenKind::PlusPlus,
                TokenKind::Ident,
                TokenKind::Dot,
                TokenKind::Ident,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::PlusPlus,
            ]
        );
    }

    // ── `[|` / `|]` / `::` ────────────────────────────────────────────

    /// The empty array literal. `[|` being one token is what keeps this from
    /// lexing as `[` `||` `]`.
    #[test]
    fn empty_array_literal_lexes_as_two_tokens() {
        assert_eq!(
            kinds(&lex("[||]"))[..2],
            [TokenKind::LBracketPipe, TokenKind::PipeRBracket]
        );
    }

    #[test]
    fn array_literal_with_elements() {
        assert_eq!(
            kinds(&lex("[|1, 2|]"))[..6],
            [
                TokenKind::LBracketPipe,
                TokenKind::IntLiteral,
                TokenKind::Comma,
                TokenKind::IntLiteral,
                TokenKind::PipeRBracket,
                TokenKind::Eof,
            ]
        );
    }

    /// A single element that is itself a bitwise-or. `|]` needs adjacency, so
    /// the inner `|` stays a `Pipe`.
    #[test]
    fn bitwise_or_inside_an_array_literal() {
        assert_eq!(
            kinds(&lex("[| a | b |]"))[..5],
            [
                TokenKind::LBracketPipe,
                TokenKind::Ident,
                TokenKind::Pipe,
                TokenKind::Ident,
                TokenKind::PipeRBracket,
            ]
        );
    }

    #[test]
    fn logical_or_inside_an_array_literal() {
        assert_eq!(
            kinds(&lex("[| a || b |]"))[..5],
            [
                TokenKind::LBracketPipe,
                TokenKind::Ident,
                TokenKind::PipePipe,
                TokenKind::Ident,
                TokenKind::PipeRBracket,
            ]
        );
    }

    #[test]
    fn cons_operator() {
        assert_eq!(
            kinds(&lex("a :: b"))[..3],
            [TokenKind::Ident, TokenKind::ColonColon, TokenKind::Ident]
        );
    }

    /// A type annotation is still a single colon — `::` must not swallow it.
    #[test]
    fn single_colon_is_unchanged() {
        assert_eq!(
            kinds(&lex("x: Int32"))[..3],
            [TokenKind::Ident, TokenKind::Colon, TokenKind::Ident]
        );
    }

    /// Indexing is untouched: a bare `[` after an expression still opens an
    /// index, and an inner `|` is still bitwise-or.
    #[test]
    fn indexing_is_unaffected() {
        assert_eq!(
            kinds(&lex("xs[i | j]"))[..6],
            [
                TokenKind::Ident,
                TokenKind::LBracket,
                TokenKind::Ident,
                TokenKind::Pipe,
                TokenKind::Ident,
                TokenKind::RBracket,
            ]
        );
    }

    /// `|]` is a closing delimiter, so the layout filter must not insert a
    /// `Sep` before it when it sits on its own line. Guards the
    /// `is_closing_delimiter` registration.
    #[test]
    fn multiline_array_literal_has_no_sep_before_close() {
        let source = "package a\n\nfunction f(): Unit =\n    let xs = [|\n        1,\n        2\n    |]\n    ()\n";
        let tokens = crate::compiler::layout::LayoutFilter::new(lex(source)).filter();
        let kinds = kinds(&tokens);
        let close = kinds
            .iter()
            .position(|k| *k == TokenKind::PipeRBracket)
            .expect("expected a `|]`");
        assert_ne!(
            kinds[close - 1],
            TokenKind::Sep,
            "layout inserted a Sep before `|]`: {kinds:?}"
        );
    }
}
