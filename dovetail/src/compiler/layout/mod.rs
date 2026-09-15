use std::collections::VecDeque;

use crate::common::span::Span;
use crate::lexer::token::{Token, TokenKind};

/// Represents an indentation context on the stack.
struct OffsideContext {
    /// Column number where this block starts (1-indexed).
    column: u32,
    /// True if this is a sequence block (allows Sep tokens).
    is_seq_block: bool,
    /// True if this context was created by `{` (brace-delimited).
    /// Brace contexts are NOT closed by outdentation — only by `}`.
    is_brace_delimited: bool,
}

/// Pending context info — set when a layout opener is seen.
struct PendingContext {
    /// Line number where the opener appeared.
    line: u32,
    /// True if the opener was `{`.
    is_brace: bool,
}

/// The layout filter: consumes raw tokens from the lexer and inserts
/// virtual Begin/End/Sep tokens according to the offside rule.
pub struct LayoutFilter {
    tokens: Vec<Token>,
    pos: usize,
    context_stack: Vec<OffsideContext>,
    pending_tokens: VecDeque<Token>,
    pending_context: Option<PendingContext>,
    last_span: Span,
}

impl LayoutFilter {
    pub fn new(tokens: Vec<Token>) -> Self {
        use std::sync::Arc;
        let last_span = tokens
            .first()
            .map(|t| Span::point(t.span.file.clone(), 1, 1))
            .unwrap_or_else(|| Span::point(Arc::from(""), 1, 1));
        Self {
            tokens,
            pos: 0,
            context_stack: Vec::new(),
            pending_tokens: VecDeque::new(),
            pending_context: None,
            last_span,
        }
    }

    /// Process all tokens, returning the layout-aware token stream.
    pub fn filter(&mut self) -> Vec<Token> {
        let mut output = Vec::new();
        loop {
            let token = self.next_token();
            let is_eof = token.kind == TokenKind::Eof;
            output.push(token);
            if is_eof {
                break;
            }
        }
        output
    }

    fn next_token(&mut self) -> Token {
        // 1. Return pending tokens first
        if let Some(token) = self.pending_tokens.pop_front() {
            self.last_span = token.span.clone();
            return token;
        }

        loop {
            let token = self.read_next_raw_token();

            // 2. Skip newlines
            if token.kind == TokenKind::Newline {
                continue;
            }

            // 3. Handle EOF — close all open contexts
            if token.kind == TokenKind::Eof {
                while let Some(ctx) = self.context_stack.pop() {
                    if !ctx.is_brace_delimited {
                        self.pending_tokens.push_back(self.make_end_token());
                    }
                }
                self.pending_tokens.push_back(token);
                return self.pending_tokens.pop_front().unwrap();
            }

            let col = token.span.column;
            let line = token.span.line;

            // 3b. Handle RBrace — close brace-delimited context
            if token.kind == TokenKind::RBrace {
                if self.pending_context.as_ref().is_some_and(|pending| pending.is_brace) {
                    // Empty braces have not pushed a context yet. In particular,
                    // do not pop the enclosing match/function blocks for `Case {}`.
                    self.pending_context = None;
                } else {
                    self.close_brace_context();
                }
                self.last_span = token.span.clone();
                return token;
            }

            // 4. Establish pending context if any
            if let Some(pending) = self.pending_context.take() {
                let is_opener = token.kind.is_layout_opener();
                self.establish_new_context(&token, pending.line, pending.is_brace);

                // Check if this token is also a layout opener
                if is_opener {
                    self.pending_context = Some(PendingContext {
                        line,
                        is_brace: token.kind == TokenKind::LBrace,
                    });
                }

                return self.pending_tokens.pop_front().unwrap();
            }

            // 5. Dedicated `else` handling — closes all offside contexts, opens else-block
            if token.kind == TokenKind::Else {
                while let Some(ctx) = self.context_stack.last() {
                    if ctx.is_brace_delimited {
                        break;
                    }
                    let should_close = if ctx.is_seq_block {
                        col < ctx.column
                    } else {
                        col <= ctx.column
                    };

                    if should_close {
                        self.context_stack.pop();
                        self.pending_tokens.push_back(self.make_end_token());
                    } else {
                        break;
                    }
                }
                // Open a new context for the else-block
                self.pending_context = Some(PendingContext {
                    line,
                    is_brace: false,
                });
                self.pending_tokens.push_back(token);
                return self.pending_tokens.pop_front().unwrap();
            }

            // 6. Check offside rules against current context
            if let Some(ctx) = self.context_stack.last() {
                if self.should_close_block(col, ctx) {
                    self.close_offside_contexts(col, &token.kind);

                    // Check if queued token is a layout opener
                    if token.kind.is_layout_opener() {
                        self.pending_context = Some(PendingContext {
                            line,
                            is_brace: token.kind == TokenKind::LBrace,
                        });
                    }

                    self.pending_tokens.push_back(token);
                    return self.pending_tokens.pop_front().unwrap();
                } else if self.should_insert_sep(col, ctx, &token.kind) {
                    // Check if queued token is a layout opener
                    if token.kind.is_layout_opener() {
                        self.pending_context = Some(PendingContext {
                            line,
                            is_brace: token.kind == TokenKind::LBrace,
                        });
                    }

                    self.pending_tokens.push_back(token);
                    return self.make_sep_token();
                }
            }

            // 7. Normal path — check if token is a layout opener
            if token.kind.is_layout_opener() {
                self.pending_context = Some(PendingContext {
                    line,
                    is_brace: token.kind == TokenKind::LBrace,
                });
            }

            self.last_span = token.span.clone();
            return token;
        }
    }

    fn read_next_raw_token(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            token
        } else {
            // Return EOF if past end
            Token::new(TokenKind::Eof, self.last_span.clone(), "")
        }
    }

    fn establish_new_context(&mut self, token: &Token, opener_line: u32, is_brace: bool) {
        let token_line = token.span.line;
        let token_col = token.span.column;

        // Brace-delimited contexts are always created (even same-line),
        // but do NOT emit Begin/End tokens.
        if is_brace {
            self.context_stack.push(OffsideContext {
                column: token_col,
                is_seq_block: true,
                is_brace_delimited: true,
            });
            self.pending_tokens.push_back(token.clone());
            return;
        }

        let is_multi_line = token_line > opener_line;

        // For same-line expressions (no newline after opener), don't create a block.
        if !is_multi_line {
            self.pending_tokens.push_back(token.clone());
            return;
        }

        // Multi-line block: create a seq-block context
        self.context_stack.push(OffsideContext {
            column: token_col,
            is_seq_block: true,
            is_brace_delimited: false,
        });

        // Emit Begin token before the current token
        self.pending_tokens.push_back(self.make_begin_token());
        self.pending_tokens.push_back(token.clone());
    }

    fn should_close_block(&self, col: u32, ctx: &OffsideContext) -> bool {
        if ctx.is_brace_delimited {
            return false;
        }
        if ctx.is_seq_block {
            col < ctx.column
        } else {
            col <= ctx.column
        }
    }

    fn should_insert_sep(&self, col: u32, ctx: &OffsideContext, token_kind: &TokenKind) -> bool {
        // Don't insert Sep before closing delimiters
        if token_kind.is_closing_delimiter() {
            return false;
        }
        col == ctx.column && ctx.is_seq_block
    }

    fn close_offside_contexts(&mut self, col: u32, token_kind: &TokenKind) {
        while let Some(ctx) = self.context_stack.last() {
            // Never close brace-delimited contexts via outdentation
            if ctx.is_brace_delimited {
                break;
            }

            let should_close = if ctx.is_seq_block {
                col < ctx.column
            } else {
                col <= ctx.column
            };

            if should_close {
                self.context_stack.pop();
                self.pending_tokens.push_back(self.make_end_token());
            } else {
                break;
            }
        }

        // After closing, check if we need Sep at the remaining context.
        // Suppress Sep before closing delimiters.
        if !token_kind.is_closing_delimiter() {
            if let Some(ctx) = self.context_stack.last() {
                if col == ctx.column && ctx.is_seq_block {
                    self.pending_tokens.push_back(self.make_sep_token());
                }
            }
        }
    }

    /// Close a brace-delimited context when `}` is encountered.
    /// First closes any implicit (non-brace) contexts nested inside, then pops the brace context.
    fn close_brace_context(&mut self) {
        // Close any implicit contexts nested inside the brace context
        while let Some(ctx) = self.context_stack.last() {
            if ctx.is_brace_delimited {
                break;
            }
            self.context_stack.pop();
            self.pending_tokens.push_back(self.make_end_token());
        }
        // Pop the brace context itself (no End token — `}` serves as the delimiter)
        if let Some(ctx) = self.context_stack.last() {
            if ctx.is_brace_delimited {
                self.context_stack.pop();
            }
        }
        // Discard any pending context (e.g. from `=` inside the brace block)
        self.pending_context = None;
    }

    fn make_begin_token(&self) -> Token {
        Token::new(TokenKind::Begin, self.last_span.clone(), "")
    }

    fn make_end_token(&self) -> Token {
        Token::new(TokenKind::End, self.last_span.clone(), "")
    }

    fn make_sep_token(&self) -> Token {
        Token::new(TokenKind::Sep, self.last_span.clone(), "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::FilePath;
    use crate::lexer::Lexer;
    use crate::lexer::attach_doc_comments;

    fn layout_filter(source: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        let tokens = attach_doc_comments(tokens);
        let mut filter = LayoutFilter::new(tokens);
        filter.filter()
    }

    fn kinds(tokens: &[Token]) -> Vec<TokenKind> {
        tokens.iter().map(|t| t.kind).collect()
    }

    #[test]
    fn test_same_line_no_block() {
        // `function main(): Unit = ()` — () is on same line as =, no Begin/End
        let tokens = layout_filter("function main(): Unit = ()");
        assert_eq!(
            kinds(&tokens),
            vec![
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
    }

    #[test]
    fn test_minimal_program() {
        // Full program with package declaration + same-line function
        let tokens = layout_filter("package a\n\nfunction main(): Unit = ()");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Package,
                TokenKind::Ident, // a
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
    }

    #[test]
    fn test_multi_line_block() {
        // function body on next line → Begin/End inserted
        let tokens = layout_filter("function foo() =\n    ()");
        assert_eq!(
            kinds(&tokens),
            vec![
                TokenKind::Function,
                TokenKind::Ident, // foo
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Equals,
                TokenKind::Begin,
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::End,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_strip_newlines() {
        let tokens = layout_filter("package a\n\n\n");
        // No Newline tokens in output
        for tok in &tokens {
            assert_ne!(tok.kind, TokenKind::Newline);
        }
    }

    #[test]
    fn test_eof_closes_contexts() {
        // Multi-line block left open → EOF closes it
        let tokens = layout_filter("function foo() =\n    ()");
        // Should have End before Eof
        let k = kinds(&tokens);
        let end_pos = k.iter().position(|k| *k == TokenKind::End).unwrap();
        let eof_pos = k.iter().position(|k| *k == TokenKind::Eof).unwrap();
        assert!(end_pos < eof_pos);
    }

    #[test]
    fn test_match_layout() {
        let tokens = layout_filter(
            "package a\n\nfunction main(): Unit =\n    match true with\n        case true => 1\n        case false => 0\n",
        );
        let k = kinds(&tokens);
        // Should not infinite loop, and should have proper structure
        assert!(k.contains(&TokenKind::Match));
    }

    #[test]
    fn test_match_in_function_arg_layout() {
        let tokens = layout_filter("f(match x with\n    case 3 => 10\n    case _ => 0\n)");
        let k = kinds(&tokens);
        assert!(k.contains(&TokenKind::Match));
    }

    #[test]
    fn empty_braces_preserve_enclosing_layout_contexts() {
        let tokens = layout_filter(
            "function f(x) =\n    match x with\n        case Value {} => 1\n        case Empty => 0\n",
        );
        let k = kinds(&tokens);
        let brace = k.iter().position(|kind| *kind == TokenKind::RBrace).unwrap();
        assert_eq!(k[brace + 1], TokenKind::FatArrow);
        let cases: Vec<_> = k.iter().enumerate()
            .filter_map(|(index, kind)| (*kind == TokenKind::Case).then_some(index))
            .collect();
        assert_eq!(cases.len(), 2);
        assert_eq!(k[cases[1] - 1], TokenKind::Sep);
        assert!(!k[cases[0]..cases[1]].contains(&TokenKind::End));
    }
}
