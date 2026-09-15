//! Optional concrete syntax annotations for source tools. Ranges refer to the
//! parser's token stream, including virtual layout tokens.
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxKind {
    Item,
    Attribute,
    Import,
    Package,
    Body,
    Members,
    Update,
    TypeParameters,
    WhereClause,
    Type,
    Pattern,
    Primary,
    Conditional,
    Expression,
    Binary,
    Closure,
    Receiver,
    Match,
}

#[derive(Debug, Clone)]
pub struct SyntaxNode {
    pub kind: SyntaxKind,
    pub tokens: Range<usize>,
}
