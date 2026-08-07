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
    /// A language keyword.
    Keyword(KeywordType),
    /// An identifier with its name; normalization may erase the name.
    Identifier(String),
    /// A string literal with its raw text; normalization may erase the value.
    StringLiteral(String),
    /// A numeric literal with its raw text; normalization may erase the value.
    NumericLiteral(String),
    /// A `true` / `false` literal.
    BooleanLiteral(bool),
    /// The `null` literal.
    NullLiteral,
    /// A template literal, collapsed to one token regardless of its parts.
    TemplateLiteral,
    /// A regular expression literal, collapsed to one token.
    RegExpLiteral,
    /// An operator.
    Operator(OperatorType),
    /// A punctuation / delimiter token.
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
    /// `var`
    Var,
    /// `let`
    Let,
    /// `const`
    Const,
    /// `function`
    Function,
    /// `return`
    Return,
    /// `if`
    If,
    /// `else`
    Else,
    /// `for`
    For,
    /// `while`
    While,
    /// `do`
    Do,
    /// `switch`
    Switch,
    /// `case`
    Case,
    /// `break`
    Break,
    /// `continue`
    Continue,
    /// `default`
    Default,
    /// `throw`
    Throw,
    /// `try`
    Try,
    /// `catch`
    Catch,
    /// `finally`
    Finally,
    /// `new`
    New,
    /// `delete`
    Delete,
    /// `typeof`
    Typeof,
    /// `instanceof`
    Instanceof,
    /// `in`
    In,
    /// `of`
    Of,
    /// `void`
    Void,
    /// `this`
    This,
    /// `super`
    Super,
    /// `class`
    Class,
    /// `extends`
    Extends,
    /// `import`
    Import,
    /// `export`
    Export,
    /// `from`
    From,
    /// `as`
    As,
    /// `async`
    Async,
    /// `await`
    Await,
    /// `yield`
    Yield,
    /// `static`
    Static,
    /// `get`
    Get,
    /// `set`
    Set,
    /// `type`
    Type,
    /// `interface`
    Interface,
    /// `enum`
    Enum,
    /// `implements`
    Implements,
    /// `abstract`
    Abstract,
    /// `declare`
    Declare,
    /// `readonly`
    Readonly,
    /// `keyof`
    Keyof,
    /// `satisfies`
    Satisfies,
}

/// Operator categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum OperatorType {
    /// `=`
    Assign,
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `%`
    Mod,
    /// `**`
    Exp,
    /// `==`
    Eq,
    /// `!=`
    NEq,
    /// `===`
    StrictEq,
    /// `!==`
    StrictNEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `&&`
    And,
    /// `||`
    Or,
    /// `!`
    Not,
    /// `&`
    BitwiseAnd,
    /// `|`
    BitwiseOr,
    /// `^`
    BitwiseXor,
    /// `~`
    BitwiseNot,
    /// `<<`
    ShiftLeft,
    /// `>>`
    ShiftRight,
    /// `>>>`
    UnsignedShiftRight,
    /// `??`
    NullishCoalescing,
    /// `?.`
    OptionalChaining,
    /// `...`
    Spread,
    /// `? :`
    Ternary,
    /// `=>`
    Arrow,
    /// `,`
    Comma,
    /// `+=`
    AddAssign,
    /// `-=`
    SubAssign,
    /// `*=`
    MulAssign,
    /// `/=`
    DivAssign,
    /// `%=`
    ModAssign,
    /// `**=`
    ExpAssign,
    /// `&&=`
    AndAssign,
    /// `||=`
    OrAssign,
    /// `??=`
    NullishAssign,
    /// `&=`
    BitwiseAndAssign,
    /// `|=`
    BitwiseOrAssign,
    /// `^=`
    BitwiseXorAssign,
    /// `<<=`
    ShiftLeftAssign,
    /// `>>=`
    ShiftRightAssign,
    /// `>>>=`
    UnsignedShiftRightAssign,
    /// `++`
    Increment,
    /// `--`
    Decrement,
    /// `instanceof` in operator position.
    Instanceof,
    /// `in` in operator position.
    In,
}

/// Punctuation / delimiter types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Encode, Decode)]
pub enum PunctuationType {
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `{`
    OpenBrace,
    /// `}`
    CloseBrace,
    /// `[`
    OpenBracket,
    /// `]`
    CloseBracket,
    /// `;`
    Semicolon,
    /// `:`
    Colon,
    /// `.`
    Dot,
}

/// Result of tokenizing a source file.
#[derive(Debug, Clone)]
pub struct FileTokens {
    /// The extracted token sequence.
    pub tokens: Vec<SourceToken>,
    /// Source spans for function-like regions eligible for near-miss clone
    /// detection. Includes declarations, expressions, arrows, and methods.
    pub function_spans: Vec<Span>,
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
