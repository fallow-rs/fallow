//! Per-construct complexity increments that explain the cyclomatic and
//! cognitive scores of a function.

/// Which complexity metric a [`ComplexityContribution`] adds to.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ComplexityMetric {
    /// `McCabe` cyclomatic complexity (independent execution paths).
    Cyclomatic,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    Cognitive,
}

/// The syntactic construct that produced a single complexity increment.
///
/// Mirrors `SonarSource` cognitive-complexity vocabulary where it overlaps.
/// `Case` means a `case` label carrying a test; a bare `default` adds nothing
/// to cyclomatic complexity and so produces no contribution.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ComplexityContributionKind {
    /// An `if` condition.
    If,
    /// A bare `else` branch (cognitive only).
    Else,
    /// An `else if` continuation (both metrics: cyclomatic +1, cognitive flat
    /// +1 with no nesting penalty).
    ElseIf,
    /// A `?:` conditional (ternary) expression.
    Ternary,
    /// A logical `&&` operator.
    LogicalAnd,
    /// A logical `||` operator.
    LogicalOr,
    /// A `??` nullish-coalescing operator.
    NullishCoalescing,
    /// A logical assignment operator (`&&=`, `||=`, `??=`); cyclomatic only.
    LogicalAssignment,
    /// An optional-chaining link (`?.`); cyclomatic only.
    OptionalChain,
    /// A `for` loop.
    For,
    /// A `for...in` loop.
    ForIn,
    /// A `for...of` loop.
    ForOf,
    /// A `while` loop.
    While,
    /// A `do...while` loop.
    DoWhile,
    /// A `switch` statement (cognitive only; each `case` adds cyclomatic).
    Switch,
    /// A `case` label carrying a test (cyclomatic only).
    Case,
    /// A `catch` clause.
    Catch,
    /// A labeled `break` (cognitive only).
    LabeledBreak,
    /// A labeled `continue` (cognitive only).
    LabeledContinue,
    /// Legacy JSX-depth contribution kind kept for schema compatibility. Current
    /// extraction records JSX nesting as descriptive `react_jsx_max_depth`
    /// context and does not emit this kind for layout depth.
    JsxDepth,
    /// React hook density (cognitive only). One contribution per hook call in a
    /// component body (`useState` / `useEffect` / `useMemo` / `useCallback` /
    /// custom `use*`); a hook-heavy component accrues cognitive load the same way
    /// branching does.
    HookDensity,
    /// React prop count past the comfortable floor (cognitive only). A component
    /// destructuring many props is doing many things; the props beyond the floor
    /// fold into cognitive so a wide-interface component surfaces as a hotspot.
    PropCount,
    /// A Svelte `{#await}` block.
    Await,
    /// A Svelte `{:then}` continuation.
    Then,
}

/// A single complexity increment, located at its source line/column.
///
/// `weight` is the amount this construct added to `metric`; for nested
/// cognitive increments `weight == 1 + nesting`. Consumers that render inline
/// (the VS Code editor breakdown) group contributions by `line` and sum the
/// weights, deferring the per-kind list to a hover.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplexityContribution {
    /// 1-based line number where the construct begins.
    pub line: u32,
    /// 0-based byte column where the construct begins.
    pub col: u32,
    /// Which metric this increment contributes to.
    pub metric: ComplexityMetric,
    /// The syntactic construct responsible for the increment.
    pub kind: ComplexityContributionKind,
    /// The amount added to `metric` at this site (`1 + nesting` for nested
    /// cognitive increments, otherwise `1`).
    pub weight: u16,
    /// The nesting depth at the increment site (`0` when not nested). Lets a
    /// consumer explain a cognitive `+3` as "+1 base, +2 nesting".
    pub nesting: u16,
    /// This cyclomatic `||` increment is part of a literal membership chain:
    /// strict equality checks of one identifier against primitive literals
    /// (`kind === "a" || kind === "b"`). The `literal-membership` value of
    /// `health.cyclomaticPolicy` removes these increments from the effective
    /// cyclomatic value. Omitted when `false`, and always `false` under the
    /// default `standard` policy.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub literal_membership: bool,
}
