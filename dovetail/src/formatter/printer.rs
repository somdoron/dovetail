//! Printing rules for the source syntax captured by the compiler parser.
use super::document::Document as D;
use super::syntax::{Parsed, is_modifier};
use crate::lexer::token::TokenKind as K;
use crate::parser::syntax::{SyntaxKind as S, SyntaxNode};
use std::ops::Range;

pub(super) fn document(
    parsed: &Parsed,
    source: &str,
    literals: std::collections::HashMap<usize, D>,
) -> D {
    let mut builder = Builder::new(parsed, source);
    builder.literals = literals;
    builder.prepare_comments();
    builder.range(0..parsed.tokens.len(), &[])
}

struct Sequence {
    parts: Vec<D>,
    index: usize,
    previous: Option<K>,
    previous_item: Option<S>,
}

impl Sequence {
    fn new(index: usize) -> Self {
        Self {
            parts: vec![],
            index,
            previous: None,
            previous_item: None,
        }
    }
}

struct Builder<'a> {
    parsed: &'a Parsed,
    source: &'a str,
    leading: Vec<Vec<Range<usize>>>,
    trailing: Vec<Vec<Range<usize>>>,
    dangling: Vec<Vec<Range<usize>>>,
    block_indents: Vec<Option<usize>>,
    literals: std::collections::HashMap<usize, D>,
    nodes_at: Vec<Vec<usize>>,
}

impl<'a> Builder<'a> {
    fn new(parsed: &'a Parsed, source: &'a str) -> Self {
        let mut nodes_at = vec![vec![]; parsed.tokens.len()];
        for (id, node) in parsed.nodes.iter().enumerate() {
            if node.tokens.start < nodes_at.len() {
                nodes_at[node.tokens.start].push(id);
            }
        }
        let mut block_indents = vec![None; parsed.tokens.len()];
        let mut blocks = vec![];
        for (index, token) in parsed.tokens.iter().enumerate() {
            if token.kind == K::Begin {
                blocks.push(index);
            }
            if token.kind == K::End
                && let Some(start) = blocks.pop()
            {
                block_indents[index] = parsed.tokens[start + 1..index]
                    .iter()
                    .find(|token| token.source_range.is_some())
                    .map(|token| token.span.column as usize - 1);
            }
        }
        Self {
            block_indents,
            nodes_at,
            parsed,
            source,
            literals: Default::default(),
            leading: vec![vec![]; parsed.tokens.len()],
            trailing: vec![vec![]; parsed.tokens.len()],
            dangling: vec![vec![]; parsed.tokens.len()],
        }
    }

    fn prepare_comments(&mut self) {
        for comment in &self.parsed.comments {
            let previous = self
                .parsed
                .tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| {
                    token
                        .source_range
                        .as_ref()
                        .is_some_and(|range| range.end <= comment.start)
                })
                .max_by_key(|(_, token)| token.source_range.as_ref().unwrap().end);
            if let Some((index, token)) = previous {
                let end = token.source_range.as_ref().unwrap().end;
                if !self.source[end..comment.start].contains(['\n', '\r']) {
                    self.trailing[index].push(comment.clone());
                    continue;
                }
            }
            let next = self.parsed.tokens.iter().position(|token| {
                token
                    .source_range
                    .as_ref()
                    .is_some_and(|range| range.start >= comment.end)
            });
            let index = next.unwrap_or(self.parsed.tokens.len() - 1);
            let after = previous.map_or(0, |(index, _)| index + 1);
            if let Some(boundary) = self.comment_boundary(comment, after..index + 1) {
                self.dangling[boundary].push(comment.clone());
            } else {
                self.leading[index].push(comment.clone());
            }
        }
    }

    fn comment_boundary(&self, comment: &Range<usize>, candidates: Range<usize>) -> Option<usize> {
        let text = &self.source[comment.clone()];
        if text.starts_with("///") && !text.starts_with("////") {
            return None;
        }
        let column = self.source[..comment.start]
            .rsplit('\n')
            .next()
            .unwrap_or("")
            .chars()
            .count();
        candidates.into_iter().find(|&index| {
            self.block_indents[index].is_some_and(|indent| column >= indent)
                || (matches!(
                    self.parsed.tokens[index].kind,
                    K::RBrace | K::RParen | K::RBracket | K::PipeRBracket
                ) && column >= self.parsed.tokens[index].span.column as usize)
        })
    }

    fn dangling_comments(&self, index: usize) -> D {
        if index >= self.dangling.len() {
            return D::text("");
        }
        D::concat(self.dangling[index].iter().flat_map(|range| {
            [
                D::HardLine,
                D::text(&self.source[range.clone()]),
                D::HardLine,
            ]
        }))
    }

    fn range(&self, range: Range<usize>, excluded: &[usize]) -> D {
        self.sequence(range, excluded, false, false)
    }

    fn sequence(
        &self,
        range: Range<usize>,
        excluded: &[usize],
        continuation: bool,
        brace_fields: bool,
    ) -> D {
        let mut state = Sequence::new(range.start);
        while state.index < range.end {
            if self.append_node(&mut state, range.end, excluded)
                || self.append_layout(&mut state, range.end, excluded, brace_fields)
                || self.append_delimiter(&mut state, range.end, excluded)
            {
                continue;
            }
            self.append_token(&mut state, continuation, brace_fields);
        }
        D::concat(state.parts)
    }

    fn append_node(&self, state: &mut Sequence, end: usize, excluded: &[usize]) -> bool {
        let token = &self.parsed.tokens[state.index];
        if let Some((id, node)) = self.node_at(state.index, end, excluded) {
            let item = matches!(node.kind, S::Item | S::Package | S::Import);
            if item && !state.parts.is_empty() && !state.previous.is_some_and(is_modifier) {
                state.parts.push(
                    if state.previous_item == Some(S::Attribute)
                        || (state.previous_item == Some(S::Import)
                            && node.kind == S::Import
                            && !self.blank_line_before(state.index))
                    {
                        D::HardLine
                    } else {
                        D::BlankLine
                    },
                );
            } else if !matches!(
                node.kind,
                S::Body | S::Members | S::TypeParameters | S::WhereClause | S::Type
            ) && needs_space(state.previous, Some(token.kind))
            {
                state.parts.push(D::text(" "));
            }
            let mut excluded = excluded.to_vec();
            excluded.push(id);
            state.parts.push(self.node(node, &excluded));
            state.previous = self.last_kind(node.tokens.clone());
            state.previous_item = (item || node.kind == S::Attribute).then_some(node.kind);
            state.index = node.tokens.end;
            return true;
        }
        false
    }

    fn blank_line_before(&self, index: usize) -> bool {
        let previous = self.parsed.tokens[..index]
            .iter()
            .rev()
            .find_map(|token| token.source_range.as_ref());
        let next = self.parsed.tokens[index..]
            .iter()
            .find_map(|token| token.source_range.as_ref());
        let (Some(previous), Some(next)) = (previous, next) else {
            return false;
        };
        if previous.end > next.start {
            return false;
        }
        let mut lines = self.source[previous.end..next.start]
            .split('\n')
            .skip(1)
            .peekable();
        while let Some(line) = lines.next() {
            if lines.peek().is_some() && line.trim().is_empty() {
                return true;
            }
        }
        false
    }

    fn append_layout(
        &self,
        state: &mut Sequence,
        end: usize,
        excluded: &[usize],
        brace_fields: bool,
    ) -> bool {
        let token = &self.parsed.tokens[state.index];
        if matches!(token.kind, K::Begin) {
            let closing_index = self.matching(state.index, end, K::Begin, K::End);
            state.parts.push(
                D::concat([
                    D::HardLine,
                    self.range(state.index + 1..closing_index, excluded),
                    self.dangling_comments(closing_index),
                ])
                .indent(),
            );
            state.index = (closing_index + 1).min(end);
            state.previous = Some(K::End);
            return true;
        }
        if matches!(token.kind, K::End | K::Sep | K::Semicolon) {
            state.parts.push(self.comments_before(state.index));
            if brace_fields && matches!(token.kind, K::Sep | K::Semicolon) {
                state.parts.push(D::FlatText(";"));
            }
            state.parts.extend(
                self.trailing[state.index]
                    .iter()
                    .map(|range| D::Suffix(self.source[range.clone()].into())),
            );
            state.parts.push(
                if brace_fields && matches!(token.kind, K::Sep | K::Semicolon) {
                    D::Line(" ")
                } else if matches!(token.kind, K::Sep | K::Semicolon)
                    && self.blank_line_before(state.index + 1)
                {
                    D::BlankLine
                } else {
                    D::HardLine
                },
            );
            state.previous = None;
            state.index += 1;
            return true;
        }
        if token.kind == K::Eof {
            state.parts.push(self.comments_before(state.index));
            state.index += 1;
            return true;
        }
        false
    }

    fn append_delimiter(&self, state: &mut Sequence, limit: usize, excluded: &[usize]) -> bool {
        let token = &self.parsed.tokens[state.index];
        if let Some(close) = closing(token.kind) {
            let end = self.matching(state.index, limit, token.kind, close);
            if end < limit {
                if needs_space(state.previous, Some(token.kind)) {
                    state.parts.push(D::text(" "));
                }
                let inner = D::concat([
                    self.sequence(
                        state.index + 1..end,
                        excluded,
                        false,
                        token.kind == K::LBrace && !self.in_pattern(state.index),
                    ),
                    self.dangling_comments(end),
                ]);
                let space =
                    if matches!(token.kind, K::LBrace | K::LBracketPipe) && end > state.index + 1 {
                        " "
                    } else {
                        ""
                    };
                let content = if end == state.index + 1 && self.dangling[end].is_empty() {
                    D::text("")
                } else {
                    D::concat([D::concat([D::Line(space), inner]).indent(), D::Line(space)])
                };
                state
                    .parts
                    .push(D::concat([self.leaf(state.index), content, self.leaf(end)]).group());
                state.previous = Some(close);
                state.index = end + 1;
                return true;
            }
        }
        false
    }

    fn append_token(&self, state: &mut Sequence, continuation: bool, brace_fields: bool) {
        let kind = self.parsed.tokens[state.index].kind;
        if kind == K::Comma {
            state
                .parts
                .push(self.comma(state.index, continuation, brace_fields));
            state.previous = None;
        } else {
            let binary =
                continuation && is_binary(kind) && state.previous.is_some_and(ends_operand);
            state
                .parts
                .push(self.token_prefix(state, continuation, binary));
            state.parts.push(self.leaf(state.index));
            if binary {
                state.parts.push(D::text(" "));
            }
            state.previous = (!binary).then_some(kind);
        }
        state.index += 1;
    }

    fn token_prefix(&self, state: &Sequence, continuation: bool, binary: bool) -> D {
        let kind = self.parsed.tokens[state.index].kind;
        if kind == K::Dot && self.follows_multiline_literal(state.index) {
            return D::HardLine.indent();
        }
        if continuation && kind == K::Dot && state.previous.is_some() {
            return D::Line("").indent();
        }
        if kind == K::Where || binary {
            return D::Line(" ").indent();
        }
        if kind == K::At && !state.parts.is_empty() {
            return D::HardLine;
        }
        if kind == K::Else {
            return D::ConditionalLine;
        }
        D::text(if needs_space(state.previous, Some(kind)) {
            " "
        } else {
            ""
        })
    }

    fn follows_multiline_literal(&self, index: usize) -> bool {
        index > 0
            && self.parsed.tokens[index - 1]
                .source_range
                .as_ref()
                .is_some_and(|range| self.source[range.clone()].contains('\n'))
    }

    fn comma(&self, index: usize, continuation: bool, brace_fields: bool) -> D {
        let mut parts = if brace_fields {
            let mut parts = vec![self.comments_before(index), D::FlatText(";")];
            parts.extend(
                self.trailing[index]
                    .iter()
                    .map(|range| D::Suffix(self.source[range.clone()].into())),
            );
            parts
        } else {
            vec![self.leaf(index)]
        };
        parts.push(if continuation {
            D::Line(" ").indent()
        } else {
            D::Line(" ")
        });
        D::concat(parts)
    }

    fn node_at(
        &self,
        start: usize,
        end: usize,
        excluded: &[usize],
    ) -> Option<(usize, &SyntaxNode)> {
        self.nodes_at[start]
            .iter()
            .map(|&id| (id, &self.parsed.nodes[id]))
            .filter(|(id, node)| {
                !excluded.contains(id)
                    && node.tokens.start == start
                    && node.tokens.end <= end
                    && matches!(
                        node.kind,
                        S::Item
                            | S::Package
                            | S::Import
                            | S::Body
                            | S::Members
                            | S::TypeParameters
                            | S::Type
                            | S::WhereClause
                            | S::Conditional
                            | S::Match
                            | S::Expression
                            | S::Closure
                            | S::Attribute
                            | S::Binary
                            | S::Update
                            | S::Receiver
                    )
                    && (node.kind != S::Type
                        || (start > 0 && self.parsed.tokens[start - 1].kind == K::Colon))
                    && self.complete_literal_boundary(node.tokens.clone())
            })
            .max_by_key(|(id, node)| (node.tokens.end, *id))
    }

    fn complete_literal_boundary(&self, range: Range<usize>) -> bool {
        let tokens = &self.parsed.tokens;
        let start_complete = range.start == 0
            || tokens[range.start].source_range.is_none()
            || tokens[range.start].source_range != tokens[range.start - 1].source_range;
        let end_complete = range.end == tokens.len()
            || tokens[range.end - 1].source_range.is_none()
            || tokens[range.end - 1].source_range != tokens[range.end].source_range;
        start_complete && end_complete
    }

    fn node(&self, node: &SyntaxNode, excluded: &[usize]) -> D {
        let mut range = node.tokens.clone();
        match node.kind {
            S::Item => {
                let where_clause = self.parsed.nodes.iter().find(|child| {
                    child.kind == S::WhereClause
                        && child.tokens.start >= range.start
                        && child.tokens.end < range.end
                        && self.parsed.tokens[child.tokens.end].kind == K::Equals
                        && !self.parsed.tokens[range.start..child.tokens.start]
                            .iter()
                            .any(|token| token.kind == K::Begin)
                });
                if let Some(where_clause) = where_clause {
                    let body_start = where_clause.tokens.end + 1;
                    return D::concat([
                        self.range(range.start..body_start, excluded).group(),
                        self.range(body_start..range.end, excluded),
                    ]);
                }
                self.range(range, excluded).group()
            }
            S::Body | S::Members => {
                let was_block = self.parsed.tokens[range.start].kind == K::Begin;
                if was_block {
                    range.start += 1;
                    if self.parsed.tokens[range.end - 1].kind == K::End {
                        range.end -= 1;
                    }
                }
                let inner = D::concat([
                    self.range(range.clone(), excluded),
                    if was_block {
                        self.dangling_comments(range.end)
                    } else {
                        D::text("")
                    },
                ]);
                if node.tokens.start > 0
                    && self.parsed.tokens[node.tokens.start - 1].kind == K::Else
                    && self.parsed.tokens[range.start].kind == K::If
                    && (node.tokens.start - 1..=node.tokens.end).all(|index| {
                        self.leading[index].is_empty()
                            && self.trailing[index].is_empty()
                            && self.dangling[index].is_empty()
                    })
                    && !self.multiple_statements(range.clone())
                {
                    return D::concat([D::text(" "), inner]).group();
                }
                let scoped = matches!(self.parsed.tokens[range.start].kind, K::Use | K::Let);
                if scoped && !was_block {
                    return D::concat([D::text(" "), inner]).group();
                }
                let forced = (scoped && was_block)
                    || node.kind == S::Members
                    || (node.tokens.start > 0
                        && self.parsed.tokens[node.tokens.start - 1].kind == K::Then
                        && self.parsed.tokens[range.clone()]
                            .iter()
                            .any(|token| token.kind == K::If))
                    || (node.tokens.start > 0
                        && self.parsed.tokens[node.tokens.start - 1].kind == K::Do)
                    || self.multiple_statements(range);
                let body =
                    D::concat([if forced { D::HardLine } else { D::Line(" ") }, inner]).indent();
                if node.tokens.start > 0
                    && matches!(
                        self.parsed.tokens[node.tokens.start - 1].kind,
                        K::Then | K::Else
                    )
                {
                    D::Branch(Box::new(body))
                } else {
                    body.group()
                }
            }
            S::WhereClause => D::concat([
                D::Line(" "),
                self.leaf(range.start),
                D::text(" "),
                D::Align(Box::new(self.range(range.start + 1..range.end, excluded))),
            ])
            .indent(),
            S::Conditional => {
                let continuation = self.parsed.tokens[..range.start]
                    .iter()
                    .rev()
                    .find(|token| token.kind != K::Begin)
                    .is_some_and(|token| token.kind == K::Else);
                D::Conditional {
                    inner: Box::new(self.range(range, excluded)),
                    continuation,
                }
            }
            S::Update => {
                let start = range.start;
                range.start += 1;
                let was_block = self.parsed.tokens[range.start].kind == K::Begin;
                if was_block {
                    range.start += 1;
                    range.end -= 1;
                }
                D::concat([
                    self.leaf(start),
                    D::concat([
                        D::Line(" "),
                        self.sequence(range.clone(), excluded, false, true),
                        if was_block {
                            self.dangling_comments(range.end)
                        } else {
                            D::text("")
                        },
                    ])
                    .indent(),
                ])
                .group()
            }
            S::Match => {
                let with = (range.start..range.end)
                    .find(|&index| self.parsed.tokens[index].kind == K::With)
                    .unwrap();
                let mut body = with + 1..range.end;
                let was_block = self.parsed.tokens[body.start].kind == K::Begin;
                if was_block {
                    body.start += 1;
                    body.end -= 1;
                }
                D::concat([
                    self.range(range.start..with + 1, excluded),
                    D::concat([
                        D::HardLine,
                        self.range(body.clone(), excluded),
                        if was_block {
                            self.dangling_comments(body.end)
                        } else {
                            D::text("")
                        },
                    ])
                    .indent(),
                ])
            }
            S::Receiver if self.needs_receiver_group(range.clone()) => D::concat([
                D::text("("),
                D::concat([D::Line(""), self.range(range, excluded)]).indent(),
                D::Line(""),
                D::text(")"),
            ])
            .group(),
            S::Binary if self.needs_argument_group(range.clone()) => D::concat([
                D::BrokenText("("),
                D::concat([D::Line(""), self.range(range, excluded)]).indent(),
                D::Line(""),
                D::BrokenText(")"),
            ])
            .group(),
            S::Attribute => D::concat([self.range(range, excluded).group(), D::HardLine]),
            S::Expression | S::Binary => self.sequence(range, excluded, true, false).group(),
            S::Type => D::concat([D::Line(" "), self.range(range, excluded)])
                .indent()
                .group(),
            S::TypeParameters => self.type_parameters(range, excluded),
            _ => self.range(range, excluded).group(),
        }
    }

    fn type_parameters(&self, range: Range<usize>, excluded: &[usize]) -> D {
        let start = range.start;
        let end = range.end - 1;
        if self.parsed.tokens[start].kind != K::Lt || self.parsed.tokens[end].kind != K::Gt {
            return self.range(range, excluded).group();
        }
        D::concat([
            self.leaf(start),
            D::concat([D::Line(""), self.range(start + 1..end, excluded)]).indent(),
            self.leaf(end),
        ])
        .group()
    }

    fn multiple_statements(&self, range: Range<usize>) -> bool {
        let mut depth = 0usize;
        for index in range {
            let token = &self.parsed.tokens[index];
            match token.kind {
                K::Begin | K::LBrace | K::LParen | K::LBracket | K::LBracketPipe => depth += 1,
                K::End | K::RBrace | K::RParen | K::RBracket | K::PipeRBracket => {
                    depth = depth.saturating_sub(1)
                }
                K::Sep | K::Semicolon
                    if depth == 0
                        && !self
                            .parsed
                            .nodes
                            .iter()
                            .any(|node| node.kind == S::Update && node.tokens.contains(&index)) =>
                {
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    fn needs_receiver_group(&self, range: Range<usize>) -> bool {
        self.parsed
            .tokens
            .get(range.end)
            .is_some_and(|token| matches!(token.kind, K::Dot | K::LBracket | K::LParen))
            && !(self.parsed.tokens[range.start].kind == K::LParen
                && self.matching(range.start, range.end, K::LParen, K::RParen) == range.end - 1)
    }

    fn needs_argument_group(&self, range: Range<usize>) -> bool {
        if self
            .parsed
            .tokens
            .get(range.end)
            .is_none_or(|token| token.kind != K::Comma)
        {
            return false;
        }
        if self.parsed.tokens[range.start].kind == K::LParen
            && self.matching(range.start, range.end, K::LParen, K::RParen) == range.end - 1
        {
            return false;
        }
        matches!(
            self.parsed.tokens[range.start].kind,
            K::Async | K::If | K::Match
        ) || self
            .parsed
            .nodes
            .iter()
            .any(|node| node.kind == S::Closure && node.tokens == range)
    }

    fn in_pattern(&self, index: usize) -> bool {
        self.parsed
            .nodes
            .iter()
            .any(|node| node.kind == S::Pattern && node.tokens.contains(&index))
    }

    fn matching(&self, start: usize, end: usize, open: K, close: K) -> usize {
        let mut depth = 0;
        for index in start..end {
            match self.parsed.tokens[index].kind {
                kind if kind == open => depth += 1,
                kind if kind == close => {
                    depth -= 1;
                    if depth == 0 {
                        return index;
                    }
                }
                _ => {}
            }
        }
        end
    }

    fn last_kind(&self, range: Range<usize>) -> Option<K> {
        self.parsed.tokens[range]
            .iter()
            .rev()
            .find(|token| !matches!(token.kind, K::Begin | K::End | K::Sep))
            .map(|token| token.kind)
    }

    fn comments_before(&self, index: usize) -> D {
        D::concat(
            self.leading[index]
                .iter()
                .flat_map(|range| [D::text(&self.source[range.clone()]), D::HardLine]),
        )
    }

    fn leaf(&self, index: usize) -> D {
        let token = &self.parsed.tokens[index];
        let text = token
            .source_range
            .as_ref()
            .map_or(token.text.as_str(), |range| &self.source[range.clone()]);
        let mut parts = vec![
            self.comments_before(index),
            self.literals
                .get(&index)
                .cloned()
                .unwrap_or_else(|| D::text(text)),
        ];
        for range in &self.trailing[index] {
            parts.push(D::Suffix(self.source[range.clone()].to_string()));
        }
        D::concat(parts)
    }
}

fn closing(kind: K) -> Option<K> {
    match kind {
        K::LParen => Some(K::RParen),
        K::LBracket => Some(K::RBracket),
        K::LBracketPipe => Some(K::PipeRBracket),
        K::LBrace => Some(K::RBrace),
        _ => None,
    }
}

fn needs_space(previous: Option<K>, current: Option<K>) -> bool {
    let (Some(previous), Some(current)) = (previous, current) else {
        return false;
    };
    if matches!(
        current,
        K::RParen
            | K::RBracket
            | K::PipeRBracket
            | K::RBrace
            | K::Comma
            | K::Colon
            | K::Dot
            | K::DotDot
            | K::DotDotEq
    ) {
        return false;
    }
    if matches!(
        previous,
        K::LParen
            | K::LBracket
            | K::LBracketPipe
            | K::Dot
            | K::At
            | K::DotDot
            | K::DotDotEq
            | K::Bang
            | K::Minus
            | K::Tilde
    ) {
        return false;
    }
    if current == K::LParen && matches!(previous, K::Ident | K::Gt | K::GtGt | K::RParen) {
        return false;
    }
    if current == K::LBracket && matches!(previous, K::Ident | K::RParen | K::RBracket) {
        return false;
    }
    true
}

fn is_binary(kind: K) -> bool {
    matches!(
        kind,
        K::Plus
            | K::PlusPlus
            | K::Minus
            | K::Star
            | K::Slash
            | K::Percent
            | K::EqEq
            | K::BangEq
            | K::Lt
            | K::Gt
            | K::LtEq
            | K::GtEq
            | K::Tilde
            | K::Ampersand
            | K::Pipe
            | K::Caret
            | K::LtLt
            | K::GtGt
            | K::AmpAmp
            | K::PipePipe
            | K::ColonColon
            | K::Is
            | K::As
    )
}

fn ends_operand(kind: K) -> bool {
    matches!(
        kind,
        K::Ident
            | K::IntLiteral
            | K::FloatLiteral
            | K::ExactNumberLiteral
            | K::StringLiteral
            | K::PrefixedStringLiteral
            | K::CharLiteral
            | K::True
            | K::False
            | K::RParen
            | K::RBracket
            | K::PipeRBracket
            | K::RBrace
    )
}
