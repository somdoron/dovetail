use super::document::Document as D;
use super::{FormatError, validate};
use crate::common::span::FilePath;
use crate::layout::LayoutFilter;
use crate::lexer::token::{Token, TokenKind as K};
use crate::lexer::{Lexer, attach_doc_comments};
use crate::parser::{
    Parser,
    ast::SourceFile,
    syntax::{SyntaxKind as S, SyntaxNode},
};
use std::ops::Range;

pub(super) struct Parsed {
    pub ast: Option<SourceFile>,
    pub(super) tokens: Vec<Token>,
    pub(super) nodes: Vec<SyntaxNode>,
    pub(super) comments: Vec<Range<usize>>,
    interpolations: Vec<Range<usize>>,
}

pub(super) fn parse(source: &str, file: FilePath) -> Result<Parsed, FormatError> {
    let mut lexer = Lexer::capturing_source(source, file.clone());
    let raw = lexer.tokenize();
    let interpolations = lexer.source_interpolations.clone();
    let mut comments = lexer.source_comments.clone();
    comments.extend(
        raw.iter()
            .filter(|token| token.kind == K::DocComment)
            .filter_map(|token| token.source_range.clone()),
    );
    comments.sort_by_key(|range| range.start);
    if !lexer.diagnostics().is_empty() {
        return Err(FormatError(
            lexer
                .diagnostics()
                .iter()
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let empty = raw
        .iter()
        .all(|token| matches!(token.kind, K::Newline | K::Eof | K::DocComment));
    if empty {
        return Ok(Parsed {
            ast: None,
            tokens: vec![],
            nodes: vec![],
            comments,
            interpolations,
        });
    }
    let mut layout = LayoutFilter::new(attach_doc_comments(raw));
    let mut parser = Parser::capturing_source(layout.filter());
    let captured_ast = parser.parse_source_file();
    let ast = validate(source, file)?;
    if super::structure(&captured_ast)? != super::structure(&ast)? {
        return Err(FormatError(
            "source capture disagrees with compiler parsing".into(),
        ));
    }
    let (tokens, nodes) = parser.into_syntax();
    let (tokens, nodes) = collapse_literals(tokens, nodes, source);
    Ok(Parsed {
        ast: Some(ast),
        tokens,
        nodes,
        comments,
        interpolations,
    })
}

impl Parsed {
    pub fn comments<'a>(&self, source: &'a str) -> Vec<&'a str> {
        self.comments
            .iter()
            .map(|range| &source[range.clone()])
            .collect()
    }

    pub fn document(&self, source: &str) -> Result<D, FormatError> {
        if self.tokens.is_empty() {
            return Ok(D::concat(
                self.comments
                    .iter()
                    .flat_map(|range| [D::text(&source[range.clone()]), D::HardLine]),
            ));
        }
        let mut literals = std::collections::HashMap::new();
        for (index, token) in self.tokens.iter().enumerate() {
            if let Some(range) = &token.source_range {
                let interpolations: Vec<_> = self
                    .interpolations
                    .iter()
                    .filter(|inner| range.start <= inner.start && inner.end <= range.end)
                    .collect();
                if interpolations.is_empty() {
                    continue;
                }
                let mut parts = vec![];
                let mut cursor = range.start;
                for inner in interpolations {
                    parts.push(D::text(&source[cursor..inner.start]));
                    parts.push(expression_document(&source[inner.clone()])?);
                    cursor = inner.end;
                }
                parts.push(D::text(&source[cursor..range.end]));
                literals.insert(index, D::concat(parts));
            }
        }
        Ok(super::printer::document(self, source, literals))
    }
}

// The compiler expands an ordinary interpolation into several tokens. Source
// tools see its original literal as one leaf instead.
fn collapse_literals(
    tokens: Vec<Token>,
    nodes: Vec<SyntaxNode>,
    source: &str,
) -> (Vec<Token>, Vec<SyntaxNode>) {
    let mut compact: Vec<Token> = Vec::new();
    let mut boundaries = vec![None; tokens.len() + 1];
    let mut index = 0;
    while index < tokens.len() {
        boundaries[index] = Some(compact.len());
        let mut token = tokens[index].clone();
        let mut end = index + 1;
        if let Some(range) = &token.source_range {
            while end < tokens.len() && tokens[end].source_range.as_ref() == Some(range) {
                end += 1;
            }
            if source[range.clone()].starts_with('"') {
                token.kind = K::StringLiteral;
            }
        }
        compact.push(token);
        index = end;
    }
    boundaries[tokens.len()] = Some(compact.len());
    let mut nodes: Vec<_> = nodes
        .into_iter()
        .filter_map(|node| {
            Some(SyntaxNode {
                kind: node.kind,
                tokens: boundaries[node.tokens.start]?..boundaries[node.tokens.end]?,
            })
        })
        .collect();
    let attributes: Vec<_> = nodes
        .iter()
        .filter(|node| node.kind == S::Attribute)
        .map(|node| node.tokens.clone())
        .collect();
    for node in &mut nodes {
        if node.kind == S::Item {
            while node.tokens.start > 0 && is_modifier(compact[node.tokens.start - 1].kind) {
                node.tokens.start -= 1;
            }
            while let Some(attribute) = attributes
                .iter()
                .find(|attribute| attribute.end == node.tokens.start)
            {
                node.tokens.start = attribute.start;
            }
        }
    }
    (compact, nodes)
}

fn expression_document(source: &str) -> Result<D, FormatError> {
    let mut lexer = Lexer::capturing_source(source, "<interpolation>".into());
    let raw = lexer.tokenize();
    let mut comments = lexer.source_comments.clone();
    comments.extend(
        raw.iter()
            .filter(|token| token.kind == K::DocComment)
            .filter_map(|token| token.source_range.clone()),
    );
    comments.sort_by_key(|range| range.start);
    let mut layout = LayoutFilter::new(attach_doc_comments(raw));
    let mut parser = Parser::capturing_source(layout.filter());
    let expression = parser.parse_source_expression();
    if !parser.diagnostics().is_empty() {
        return Err(FormatError(
            "could not parse interpolation for formatting".into(),
        ));
    }
    let (tokens, nodes) = parser.into_syntax();
    let (tokens, nodes) = collapse_literals(tokens, nodes, source);
    let parsed = Parsed {
        ast: None,
        tokens,
        nodes,
        comments,
        interpolations: lexer.source_interpolations,
    };
    let document = parsed.document(source)?;
    let rendered = super::document::render(&document);
    if super::structure(&expression)? != expression_structure(&rendered)? {
        return Err(FormatError(
            "formatter changed interpolation structure".into(),
        ));
    }
    Ok(document)
}

fn expression_structure(source: &str) -> Result<serde_json::Value, FormatError> {
    let mut lexer = Lexer::new(source, "<interpolation>".into());
    let mut layout = LayoutFilter::new(attach_doc_comments(lexer.tokenize()));
    let mut parser = Parser::new(layout.filter());
    let expression = parser.parse_source_expression();
    if !lexer.diagnostics().is_empty() || !parser.diagnostics().is_empty() {
        return Err(FormatError("invalid interpolation".into()));
    }
    super::structure(&expression)
}

pub(super) fn is_modifier(kind: K) -> bool {
    matches!(
        kind,
        K::Public
            | K::Private
            | K::Internal
            | K::Protected
            | K::Static
            | K::Abstract
            | K::Final
            | K::Override
            | K::Sealed
            | K::Async
    )
}
