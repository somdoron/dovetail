use super::{Lexer, Span, Token, TokenKind};

impl Lexer {
    /// Recognize the complete suffix before the ordinary scanner consumes hex digits.
    pub(super) fn scan_exact_number(&mut self) -> Option<Token> {
        let mut text = String::new();
        let mut offset = 0;
        let based = self.cursor.peek() == Some('0')
            && matches!(
                self.cursor.peek_at(1),
                Some('x' | 'X' | 'b' | 'B' | 'o' | 'O')
            );
        while let Some(ch) = self.cursor.peek_at(offset) {
            let exponent_sign = !based && matches!(ch, '+' | '-') && text.ends_with(['e', 'E']);
            let decimal_point = ch == '.'
                && self
                    .cursor
                    .peek_at(offset + 1)
                    .is_some_and(|c| c.is_ascii_digit());
            if !ch.is_ascii_alphanumeric() && ch != '_' && !exponent_sign && !decimal_point {
                break;
            }
            text.push(ch);
            offset += 1;
        }
        let hex = text.starts_with("0x") || text.starts_with("0X");
        if !text.contains("big") && (hex || !text.contains("dec")) {
            return None;
        }
        let line = self.cursor.line();
        let column = self.cursor.column();
        for _ in 0..offset {
            self.cursor.advance();
        }
        let span = Span::new(
            self.cursor.file(),
            line,
            column,
            line,
            self.cursor.column() - 1,
        );
        Some(Token::new(TokenKind::ExactNumberLiteral, span, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::FilePath;

    #[test]
    fn suffixes_bases_and_member_boundaries() {
        let source = "0xFFbig 0b10big 0o77big 1.25e-3dec 42dec.format() 0xdec 1i64 1.5f32";
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        let exact: Vec<_> = tokens
            .iter()
            .filter(|t| t.kind == TokenKind::ExactNumberLiteral)
            .map(|t| t.text.as_str())
            .collect();
        assert_eq!(
            exact,
            ["0xFFbig", "0b10big", "0o77big", "1.25e-3dec", "42dec"]
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::IntLiteral && t.text == "0xdec")
        );
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::FloatLiteral && t.text == "1.5f32")
        );
    }
}
