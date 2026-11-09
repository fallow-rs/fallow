//! Mutation operators and their stable kebab-case names.

use serde::Serialize;

/// One mutation operator.
///
/// The default set ([`Operator::DEFAULT`]) comes from selective-mutation
/// research: it keeps the operators that give most of the test-strength
/// signal and drops redundant ones. The other operators of [`Operator::ALL`]
/// are opt-in: they make more mutants, many of them equivalent or killed by
/// any test, so they run only when a run names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operator {
    /// `+` and `-` swap, `*` and `/` swap, `%` becomes `*`. The compound
    /// assignments `+=` and `-=`, `*=` and `/=`, `<<=` and `>>=`, `&=` and
    /// `|=` swap, and `%=` becomes `*=`.
    ArithmeticOperator,
    /// The non-redundant relational subset (Kaminski), and the constant
    /// result of the comparison.
    RelationalOperator,
    /// `&&` and `||` swap, `??` becomes `&&`. `&&=` and `||=` swap, `??=`
    /// becomes `&&=`.
    LogicalOperator,
    /// A branch or loop test becomes `true` and `false`, and a boolean
    /// operand of `&&` or `||` becomes the constant that does not decide
    /// the whole expression.
    NegateCondition,
    /// `!x`, `-x` and `~x` become `x`. `-x` and `+x` swap.
    UnaryOperator,
    /// `++` and `--` swap.
    UpdateOperator,
    /// A call statement becomes a no-op.
    RemovedCall,
    /// `return expr` and the concise body of `x => expr` become `undefined`.
    ReturnValue,
    /// `true` and `false` swap.
    BooleanLiteral,
    /// A non-empty string or template becomes empty. An empty one becomes
    /// `"fallow"` or `` `fallow` ``.
    StringLiteral,
    /// `a?.b` becomes `a.b`, `a?.()` becomes `a()`.
    OptionalChaining,
    /// One token of a regular expression literal changes: an anchor or a
    /// quantifier is removed, a `{n,m}` range becomes `{n}`, a character
    /// class is negated, or a class escape gets the opposite case.
    RegexLiteral,
    /// One call link of a fluent chain in a module-level `const`
    /// initializer is removed: `r.m(args)` becomes `r`, when `r` is a call.
    RemovedChainLink,
    /// Opt-in. The statements of a non-empty block or function body are
    /// removed.
    BlockStatement,
    /// Opt-in. `[x]` becomes `[]`, and `[]` becomes `["fallow"]`.
    ArrayLiteral,
    /// Opt-in. A non-empty object literal becomes `{}`.
    ObjectLiteral,
    /// Opt-in. A string, array or date method call becomes its pair
    /// (`startsWith` and `endsWith`), or the call is removed (`s.trim()`
    /// becomes `s`).
    MethodExpression,
    /// Opt-in. The statements of a `case` or `default` clause are removed,
    /// so the clause falls through.
    SwitchCase,
    /// Opt-in. A call in an expression position becomes `void 0`.
    CallExpression,
    /// Opt-in. A `throw new X(...)` statement is removed.
    ThrowStatement,
    /// Opt-in. The `regex-literal` and `regex-assertion` changes, applied
    /// to the string pattern of `new RegExp("...")` or `RegExp("...")`.
    RegexConstructor,
    /// Opt-in. A lookaround of a regular expression literal is negated
    /// (`(?=` and `(?!` swap, `(?<=` and `(?<!` swap), or `\b` and `\B`
    /// swap.
    RegexAssertion,
}

impl Operator {
    /// The operators of a run that names no operator.
    pub const DEFAULT: [Self; 13] = [
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
        Self::RemovedChainLink,
    ];

    /// The operators that run only when a run names them.
    pub const OPT_IN: [Self; 9] = [
        Self::BlockStatement,
        Self::ArrayLiteral,
        Self::ObjectLiteral,
        Self::MethodExpression,
        Self::SwitchCase,
        Self::CallExpression,
        Self::ThrowStatement,
        Self::RegexConstructor,
        Self::RegexAssertion,
    ];

    /// Every operator, in a stable order: the default set, then the opt-in
    /// set.
    pub const ALL: [Self; 22] = [
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
        Self::RemovedChainLink,
        Self::BlockStatement,
        Self::ArrayLiteral,
        Self::ObjectLiteral,
        Self::MethodExpression,
        Self::SwitchCase,
        Self::CallExpression,
        Self::ThrowStatement,
        Self::RegexConstructor,
        Self::RegexAssertion,
    ];

    /// True for an operator of [`Operator::DEFAULT`].
    pub fn is_default(self) -> bool {
        Self::DEFAULT.contains(&self)
    }

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
            Self::RemovedChainLink => "removed-chain-link",
            Self::BlockStatement => "block-statement",
            Self::ArrayLiteral => "array-literal",
            Self::ObjectLiteral => "object-literal",
            Self::MethodExpression => "method-expression",
            Self::SwitchCase => "switch-case",
            Self::CallExpression => "call-expression",
            Self::ThrowStatement => "throw-statement",
            Self::RegexConstructor => "regex-constructor",
            Self::RegexAssertion => "regex-assertion",
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
