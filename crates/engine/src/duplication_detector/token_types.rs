//! Token type definitions for clone detection tokenization.
//!
//! Contains the normalized token types (`TokenKind`, `KeywordType`, `OperatorType`,
//! `PunctuationType`), the `SourceToken` wrapper, and `FileTokens` result struct.

use bitcode::{Decode, Encode};
use oxc_span::Span;

/// A single token extracted from the AST with its source location.
#[derive(Debug, Clone)]
pub struct SourceToken {
    /// The kind of token.
    pub kind: TokenKind,
    /// Byte offset into the source file.
    pub span: Span,
}

/// Normalized token types for clone detection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Encode, Decode)]
pub enum TokenKind {
    Keyword(KeywordType),
    Identifier(String),
    StringLiteral(String),
    NumericLiteral(String),
    BooleanLiteral(bool),
    NullLiteral,
    TemplateLiteral,
    RegExpLiteral,
    Operator(OperatorType),
    Punctuation(PunctuationType),
    /// Logical separator between independently tokenized regions in the same file.
    ///
    /// Duplicate detection must not report a clone that starts in a script block
    /// and continues into template or style markup just because the token streams
    /// were concatenated. Boundary tokens hash to their own stable value and are
    /// excluded from source fragments by virtue of their zero-width span.
    Boundary(String),
}

/// TypeScript/JavaScript keyword types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum KeywordType {
    Var,
    Let,
    Const,
    Function,
    Return,
    If,
    Else,
    For,
    While,
    Do,
    Switch,
    Case,
    Break,
    Continue,
    Default,
    Throw,
    Try,
    Catch,
    Finally,
    New,
    Delete,
    Typeof,
    Instanceof,
    In,
    Of,
    Void,
    This,
    Super,
    Class,
    Extends,
    Import,
    Export,
    From,
    As,
    Async,
    Await,
    Yield,
    Static,
    Get,
    Set,
    Type,
    Interface,
    Enum,
    Implements,
    Abstract,
    Declare,
    Readonly,
    Keyof,
    Satisfies,
}

/// Operator categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum OperatorType {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Exp,
    Eq,
    NEq,
    StrictEq,
    StrictNEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Not,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    BitwiseNot,
    ShiftLeft,
    ShiftRight,
    UnsignedShiftRight,
    NullishCoalescing,
    OptionalChaining,
    Spread,
    Ternary,
    Arrow,
    Comma,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    ExpAssign,
    AndAssign,
    OrAssign,
    NullishAssign,
    BitwiseAndAssign,
    BitwiseOrAssign,
    BitwiseXorAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    UnsignedShiftRightAssign,
    Increment,
    Decrement,
    Instanceof,
    In,
}

/// Punctuation / delimiter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum PunctuationType {
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Semicolon,
    Colon,
    Dot,
}

/// Result of tokenizing a source file.
#[derive(Debug, Clone)]
pub struct FileTokens {
    /// The extracted token sequence.
    pub tokens: Vec<SourceToken>,
    /// Source spans for invocation-shaped expressions that should not be
    /// reported as actionable duplicate code when the whole clone fits inside
    /// one of these spans.
    pub atomic_invocation_spans: Vec<Span>,
    /// Source text (needed for extracting fragments).
    pub source: String,
    /// Total number of lines in the source.
    pub line_count: usize,
}

/// Create a 1-byte span at the given byte position.
///
/// Used for synthetic punctuation tokens (`(`, `)`, `,`, `.`) that don't
/// have their own AST span. Using the parent expression's full span would
/// inflate clone line ranges, especially in chained method calls.
pub(super) const fn point_span(pos: u32) -> Span {
    Span::new(pos, pos + 1)
}
