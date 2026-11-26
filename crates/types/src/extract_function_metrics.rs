//! Branch counts of a file, the kinds of anonymous functions, and the
//! complexity of a function.

use super::*;

/// Branching totals for one file: the quantity that survives extraction, and
/// the number of units now holding it.
///
/// A per-function cyclomatic ceiling constrains a partition, not a quantity.
/// `McCabe` gives a function `1 + one increment per decision point`, so across
/// a set of units the summed cyclomatic score is `functions + branch_points`.
/// Moving an `if` from one function into a new one removes an increment from
/// the first and adds it to the second: `branch_points` is unchanged and
/// `functions` rises. Reporting the two terms separately is what distinguishes
/// branching that left from branching that only moved.
///
/// Module scope is accounted for. Extraction pushes a root frame per program
/// and emits it as a synthetic `<module>` unit whenever it branches, so a
/// branch hoisted out of a function to the top level of the module keeps its
/// increment in `branch_points` and adds one to `functions`, exactly as moving
/// it into a new function would. A fall in `branch_points` is therefore a
/// statement about the file's measured decision points, not about a partition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct FileBranching {
    /// Summed weight of `Cyclomatic` contributions. The conserved quantity.
    pub branch_points: u32,
    /// Number of accounted units. The tax a split adds.
    pub functions: u32,
    /// Highest single-unit cyclomatic score. Reported, never a verdict input:
    /// a split lowers it by construction.
    pub peak_cyclomatic: u16,
    /// Summed weight of `Cognitive` contributions, excluding `PropCount` and
    /// `HookDensity`. Both are cognitive-only with no cyclomatic counterpart,
    /// and `PropCount` records the excess over a floor, so it is superlinear in
    /// a split: one 14-prop component contributes `+10` while the same props
    /// across two 7-prop components contribute `+3` and `+3`. Including them
    /// would move this number with both `branch_points` and nesting flat.
    /// This therefore does NOT equal the sum of `FunctionComplexity::cognitive`.
    pub cognitive: u32,
    /// Summed `nesting` over the same cognitive contributions. Extraction
    /// rebases nesting to zero on every new frame, so a cognitive improvement
    /// that shows up here and not in `branch_points` came from repartitioning,
    /// not from removing branching.
    pub cognitive_nesting_weight: u32,
    /// Whether one of the counted units is the module body. Its branching is
    /// real and belongs in `branch_points`, but it is not a function anyone
    /// split into, so a consumer judging whether a file was split must not
    /// count it towards the function tax.
    pub has_module_unit: bool,
    /// Whether the file carried a synthetic template unit that these counts
    /// exclude. When it did, the numbers describe the file's script only, so a
    /// consumer must not present them as describing the whole file. The
    /// synthetic `<module>` unit is NOT a template unit and does not set this
    /// flag: it is counted like any other unit, because module-scope branching
    /// is part of the script the numbers describe.
    pub has_synthetic_units: bool,
}

impl FileBranching {
    /// Aggregate one file's units.
    ///
    /// Synthetic template units are excluded: they are suppressed entirely when
    /// trivial, which would silently move any denominator that counted them.
    /// Suppression is deliberately not consulted, so a
    /// `fallow-ignore-next-line complexity` comment cannot remove a unit's
    /// branches from the total.
    ///
    /// The synthetic `<module>` unit IS counted. It carries the file's
    /// module-scope decision points, and leaving it out would restore the blind
    /// spot that let a branch disappear from the total by being hoisted out of
    /// every function.
    #[must_use]
    pub fn from_units(units: &[FunctionComplexity]) -> Self {
        let mut totals = Self {
            has_module_unit: units
                .iter()
                .any(|unit| is_synthetic_module_unit(&unit.name)),
            has_synthetic_units: units
                .iter()
                .any(|unit| is_synthetic_template_unit(&unit.name)),
            ..Self::default()
        };
        for unit in units
            .iter()
            .filter(|unit| !is_synthetic_template_unit(&unit.name))
        {
            totals.functions += 1;
            totals.peak_cyclomatic = totals.peak_cyclomatic.max(unit.cyclomatic);
            for contribution in &unit.contributions {
                match contribution.metric {
                    ComplexityMetric::Cyclomatic => {
                        totals.branch_points += u32::from(contribution.weight);
                    }
                    ComplexityMetric::Cognitive => {
                        if matches!(
                            contribution.kind,
                            ComplexityContributionKind::PropCount
                                | ComplexityContributionKind::HookDensity
                        ) {
                            continue;
                        }
                        totals.cognitive += u32::from(contribution.weight);
                        totals.cognitive_nesting_weight += u32::from(contribution.nesting);
                    }
                }
            }
        }
        totals
    }

    /// Summed cyclomatic score implied by the identity `functions + branch_points`.
    ///
    /// Equals the direct sum of `FunctionComplexity::cyclomatic` over the same
    /// units unless a unit saturated `u16`, which is the one way the identity
    /// can break.
    #[must_use]
    pub const fn implied_cyclomatic(&self) -> u32 {
        self.functions + self.branch_points
    }
}

/// The kind of function that has no name of its own, which selects its
/// placeholder name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
pub enum AnonymousFunctionKind {
    /// A function expression: `<anonymous>`.
    Function,
    /// An arrow function: `<arrow>`.
    Arrow,
}

impl AnonymousFunctionKind {
    /// The placeholder name of this kind.
    #[must_use]
    pub const fn placeholder(self) -> &'static str {
        match self {
            Self::Function => ANONYMOUS_FUNCTION_NAME,
            Self::Arrow => ANONYMOUS_ARROW_NAME,
        }
    }
}

/// Complexity metrics for a single function/method/arrow.
#[derive(Debug, Clone, serde::Serialize, bitcode::Encode, bitcode::Decode)]
pub struct FunctionComplexity {
    /// Function name, or a placeholder (`"<anonymous>"`, `"<arrow>"`) for a
    /// function that has no name.
    pub name: String,
    /// Whether this function is an ECMAScript `#`-private class member.
    ///
    /// Kept separately from `name` because a public string-named method may
    /// legally begin with `#` and remains eligible for runtime coverage.
    pub is_private_member: bool,
    /// `Some` when `name` comes from the call or the function that holds this
    /// function, not from its own declaration, binding, or key. Examples:
    /// `Command.make("setup")` for a callback argument, the `Effect.fn` span
    /// name, or the name of the function that returns `Effect.gen(...)`. The
    /// value is the kind of placeholder that the function had before.
    ///
    /// Coverage producers and runtime identities record such a function
    /// without a name, so they use [`Self::identity_name`].
    pub context_named: Option<AnonymousFunctionKind>,
    /// 1-based line number where the function starts.
    pub line: u32,
    /// 0-based byte column where the function starts.
    pub col: u32,
    /// `McCabe` cyclomatic complexity (1 + decision points).
    pub cyclomatic: u16,
    /// `SonarSource` cognitive complexity (structural + nesting penalty).
    pub cognitive: u16,
    /// Number of lines in the function body.
    pub line_count: u32,
    /// Number of parameters (excluding TypeScript's `this` parameter).
    pub param_count: u8,
    /// Number of React hook calls (`useState` / `useEffect` / `useMemo` /
    /// `useCallback` / custom `use*`) made directly in this function's body.
    /// Non-zero only for React components/hooks; descriptive context surfaced in
    /// the hotspot drill-down, never a tunable threshold (anti-numerology).
    pub react_hook_count: u16,
    /// Maximum JSX element nesting depth reached in this function's body (the
    /// deepest chain of element-inside-element). `0` when the function renders
    /// no JSX. Descriptive context surfaced in the hotspot drill-down, never a
    /// tunable threshold (anti-numerology).
    pub react_jsx_max_depth: u16,
    /// Number of props destructured from this component's first parameter (the
    /// `{ a, b, c }` props object). `0` for non-component functions and for
    /// components taking a bare `props` identifier (not statically countable).
    /// Descriptive context surfaced in the hotspot drill-down, never a tunable
    /// threshold (anti-numerology).
    pub react_prop_count: u16,
    /// Content digest of the function's full-span source slice.
    pub source_hash: Option<String>,
    /// Per-decision-point breakdown explaining WHICH constructs drove the
    /// cyclomatic and cognitive scores. One entry per increment event (an `if`
    /// emits one cyclomatic and one cognitive entry at the same line, because
    /// the two metrics accrue at different granularities). Always computed and
    /// cached; surfaced in JSON only behind `health --complexity-breakdown`.
    pub contributions: Vec<ComplexityContribution>,
}

impl FunctionComplexity {
    /// The name that coverage producers and runtime identities use: the
    /// placeholder for a function with a context name, else `name`.
    #[must_use]
    pub fn identity_name(&self) -> &str {
        match self.context_named {
            Some(kind) => kind.placeholder(),
            None => &self.name,
        }
    }
}
