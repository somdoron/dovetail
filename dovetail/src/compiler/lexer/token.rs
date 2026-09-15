use crate::common::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Package,
    Function,
    Public,
    Internal,
    Private,
    Let,
    Mutable,
    True,
    False,
    Panic,
    Assert,
    If,
    Then,
    Else,
    While,
    Do,
    Break,
    Continue,
    Match,
    Case,
    With,
    Import,
    As,
    Is,
    Record,
    Enum,
    Trait,
    Interface,
    Newtype,
    Type,
    Extension,
    Implement,
    Module,
    For,
    Intrinsic,
    Property,
    Where,
    And,
    In,
    Out,
    Class,
    Static,
    Protected,
    Try,
    Extends,
    Final,
    Override,
    Super,
    Abstract,
    Implements,
    Async,
    Await,
    Use,
    Sealed,

    // Identifiers
    Ident,

    // Literals
    StringLiteral,
    /// `ident"..."` / `ident"""..."""` — a prefixed string literal. The prefix
    /// and the parsed interpolation parts live in `Token::literal`; `text`
    /// holds the prefix so debug output stays legible.
    PrefixedStringLiteral,
    CharLiteral,
    IntLiteral,
    FloatLiteral,
    ExactNumberLiteral,

    // Operators
    Plus,      // +
    PlusPlus,  // ++
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    EqEq,      // ==
    BangEq,    // !=
    Lt,        // <
    Gt,        // >
    LtEq,      // <=
    GtEq,      // >=
    Bang,      // !
    Tilde,     // ~
    Ampersand, // &
    Pipe,      // |
    Caret,     // ^
    LtLt,      // <<
    GtGt,      // >>
    AmpAmp,    // &&
    PipePipe,  // ||
    /// `::` — cons. Right-associative; prepends to a `List`.
    ColonColon,

    // Punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// `[|` — opens an array literal. Lexed as one token so `[||]` (the empty
    /// array) cannot be mistaken for `[` `||` `]`.
    LBracketPipe,
    /// `|]` — closes an array literal. Requires adjacency, like `++` or `==`.
    PipeRBracket,
    Colon,
    Semicolon,
    Equals,
    FatArrow, // =>
    Dot,
    DotDot,
    DotDotEq,
    Comma,
    At, // @

    // Layout (virtual, inserted by LayoutFilter)
    Begin,
    End,
    Sep,

    // Structural
    DocComment,
    Newline,
    Eof,
}

impl TokenKind {
    /// Returns the keyword kind for a given string, or `None` if it's not a keyword.
    pub fn keyword(s: &str) -> Option<TokenKind> {
        match s {
            "package" => Some(TokenKind::Package),
            "function" => Some(TokenKind::Function),
            "public" => Some(TokenKind::Public),
            "internal" => Some(TokenKind::Internal),
            "private" => Some(TokenKind::Private),
            "let" => Some(TokenKind::Let),
            "mutable" => Some(TokenKind::Mutable),
            "true" => Some(TokenKind::True),
            "false" => Some(TokenKind::False),
            "panic" => Some(TokenKind::Panic),
            "assert" => Some(TokenKind::Assert),
            "if" => Some(TokenKind::If),
            "then" => Some(TokenKind::Then),
            "else" => Some(TokenKind::Else),
            "while" => Some(TokenKind::While),
            "do" => Some(TokenKind::Do),
            "break" => Some(TokenKind::Break),
            "continue" => Some(TokenKind::Continue),
            "match" => Some(TokenKind::Match),
            "case" => Some(TokenKind::Case),
            "with" => Some(TokenKind::With),
            "import" => Some(TokenKind::Import),
            "as" => Some(TokenKind::As),
            "is" => Some(TokenKind::Is),
            "record" => Some(TokenKind::Record),
            "enum" => Some(TokenKind::Enum),
            "trait" => Some(TokenKind::Trait),
            "interface" => Some(TokenKind::Interface),
            "newtype" => Some(TokenKind::Newtype),
            "type" => Some(TokenKind::Type),
            "extension" => Some(TokenKind::Extension),
            "implement" => Some(TokenKind::Implement),
            "module" => Some(TokenKind::Module),
            "for" => Some(TokenKind::For),
            "intrinsic" => Some(TokenKind::Intrinsic),
            "property" => Some(TokenKind::Property),
            "where" => Some(TokenKind::Where),
            "and" => Some(TokenKind::And),
            "in" => Some(TokenKind::In),
            "out" => Some(TokenKind::Out),
            "class" => Some(TokenKind::Class),
            "static" => Some(TokenKind::Static),
            "protected" => Some(TokenKind::Protected),
            "try" => Some(TokenKind::Try),
            "extends" => Some(TokenKind::Extends),
            "final" => Some(TokenKind::Final),
            "override" => Some(TokenKind::Override),
            "super" => Some(TokenKind::Super),
            "abstract" => Some(TokenKind::Abstract),
            "implements" => Some(TokenKind::Implements),
            "async" => Some(TokenKind::Async),
            "await" => Some(TokenKind::Await),
            "use" => Some(TokenKind::Use),
            "sealed" => Some(TokenKind::Sealed),
            _ => None,
        }
    }

    pub fn is_layout_opener(&self) -> bool {
        matches!(
            self,
            TokenKind::Equals
                | TokenKind::Then
                | TokenKind::Else
                | TokenKind::Do
                | TokenKind::With
                | TokenKind::FatArrow
                | TokenKind::LBrace
        )
    }

    pub fn is_closing_delimiter(&self) -> bool {
        matches!(
            self,
            TokenKind::RParen
                | TokenKind::RBrace
                | TokenKind::RBracket
                | TokenKind::PipeRBracket
        )
    }
}

/// One part of a prefixed string literal, as the lexer produces it.
///
/// The interpolation forms carry *tokens*, not source text: only the lexer can
/// scan an interpolation correctly (the brace matching in
/// `scan_interpolation_expr_text` is string- and char-literal aware), and lex
/// errors inside `${...}` must surface as lexer diagnostics — the pipeline
/// aborts on those before parsing. The parser sub-parses these vectors into
/// expressions.
#[derive(Debug, Clone)]
pub enum LiteralPart {
    /// Literal text between interpolations, with escapes already processed.
    Text(String),
    /// `$ident` or `${expr}`. `span` covers `$` through the last character of
    /// the interpolation — it becomes the span of the synthesized `value(...)`
    /// call, and therefore the span a trait-bound failure is reported at.
    Value { tokens: Vec<Token>, span: Span },
    /// `$..ident` or `$..{expr}`, spanning `$` through the interpolation's end.
    Spread { tokens: Vec<Token>, span: Span },
}

/// The payload of a `PrefixedStringLiteral` token.
#[derive(Debug, Clone)]
pub struct PrefixedLiteralData {
    /// The prefix identifier, e.g. `"sql"` for `sql"..."`.
    pub prefix: String,
    /// Span of the prefix identifier alone.
    pub prefix_span: Span,
    /// Literal text and interpolations, in source order.
    pub parts: Vec<LiteralPart>,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    /// Original bytes, populated only when source capture is enabled.
    pub source_range: Option<std::ops::Range<usize>>,
    pub span: Span,
    pub text: String,
    pub doc_comment: Option<String>,
    /// Set only on `PrefixedStringLiteral`. `Arc` so the layout filter's token
    /// clones stay O(1).
    pub literal: Option<std::sync::Arc<PrefixedLiteralData>>,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span, text: impl Into<String>) -> Self {
        Self {
            kind,
            source_range: None,
            span,
            text: text.into(),
            doc_comment: None,
            literal: None,
        }
    }

    /// A `PrefixedStringLiteral` token carrying its parsed parts.
    pub fn with_literal(span: Span, data: PrefixedLiteralData) -> Self {
        Self {
            kind: TokenKind::PrefixedStringLiteral,
            source_range: None,
            span,
            text: data.prefix.clone(),
            doc_comment: None,
            literal: Some(std::sync::Arc::new(data)),
        }
    }
}
