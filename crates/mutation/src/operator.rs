//! Mutation operators and their stable kebab-case names.

use serde::Serialize;

/// One mutation operator.
///
/// The set comes from selective-mutation research: it keeps the operators
/// that give most of the test-strength signal and drops redundant ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operator {
    /// `+` and `-` swap, `*` and `/` swap, `%` becomes `*`, `+=` and `-=` swap.
    ArithmeticOperator,
    /// The non-redundant relational subset (Kaminski).
    RelationalOperator,
    /// `&&` and `||` swap, `??` becomes `&&`.
    LogicalOperator,
    /// A plain condition becomes `true` and `false`.
    NegateCondition,
    /// `!x` and `-x` become `x`.
    UnaryOperator,
    /// `++` and `--` swap.
    UpdateOperator,
    /// A call statement becomes a no-op.
    RemovedCall,
    /// `return expr` becomes `return undefined`.
    ReturnValue,
    /// `true` and `false` swap.
    BooleanLiteral,
    /// A non-empty string becomes empty, an empty string becomes `"fallow"`.
    StringLiteral,
    /// `a?.b` becomes `a.b`, `a?.()` becomes `a()`.
    OptionalChaining,
    /// One token of a regular expression literal changes: an anchor is
    /// removed, a character class is negated, or a class escape gets the
    /// opposite case.
    RegexLiteral,
}

impl Operator {
    /// Every operator, in a stable order.
    pub const ALL: [Self; 12] = [
        Self::ArithmeticOperator,
        Self::RelationalOperator,
        Self::LogicalOperator,
        Self::NegateCondition,
        Self::UnaryOperator,
        Self::UpdateOperator,
        Self::RemovedCall,
        Self::ReturnValue,
        Self::BooleanLiteral,
        Self::StringLiteral,
        Self::OptionalChaining,
        Self::RegexLiteral,
    ];

    /// The stable kebab-case name of the operator.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArithmeticOperator => "arithmetic-operator",
            Self::RelationalOperator => "relational-operator",
            Self::LogicalOperator => "logical-operator",
            Self::NegateCondition => "negate-condition",
            Self::UnaryOperator => "unary-operator",
            Self::UpdateOperator => "update-operator",
            Self::RemovedCall => "removed-call",
            Self::ReturnValue => "return-value",
            Self::BooleanLiteral => "boolean-literal",
            Self::StringLiteral => "string-literal",
            Self::OptionalChaining => "optional-chaining",
            Self::RegexLiteral => "regex-literal",
        }
    }

    /// Parses a kebab-case operator name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|operator| operator.as_str() == name)
    }
}

impl std::fmt::Display for Operator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
