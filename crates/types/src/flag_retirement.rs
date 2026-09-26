//! Flag retirement report types.
//!
//! `fallow flags --retirement` groups the per-site flag findings into one row
//! per flag and attaches the evidence that the flag can be retired. A person
//! makes the decision. Every action is `auto_fixable: false`, and Fallow never
//! removes code for this report.

use std::collections::BTreeMap;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

/// Why a flag is a retirement candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RetirementReason {
    /// The flag has exactly one read site.
    SingleReadSite,
    /// Every read site is in a test, story or mock file.
    TestOnly,
    /// The flag is a `const` bound to a literal and used as a guard.
    LiteralConstant,
    /// The guarded branch and the other branch are the same code.
    IdenticalBranches,
    /// No branch of the guard holds code, so the flag does nothing.
    EmptyBranch,
    /// The guarded block holds unused exports.
    GuardsDeadCode,
    /// The flag is defined, but no code reads it.
    DefinedNeverRead,
}

impl RetirementReason {
    /// Every reason, in report order.
    pub const ALL: [Self; 7] = [
        Self::SingleReadSite,
        Self::TestOnly,
        Self::LiteralConstant,
        Self::IdenticalBranches,
        Self::EmptyBranch,
        Self::GuardsDeadCode,
        Self::DefinedNeverRead,
    ];

    /// The wire code of the reason.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SingleReadSite => "single-read-site",
            Self::TestOnly => "test-only",
            Self::LiteralConstant => "literal-constant",
            Self::IdenticalBranches => "identical-branches",
            Self::EmptyBranch => "empty-branch",
            Self::GuardsDeadCode => "guards-dead-code",
            Self::DefinedNeverRead => "defined-never-read",
        }
    }
}

/// How a retirement row's flag was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RetirementFlagKind {
    /// Environment-variable read used as a toggle.
    EnvironmentVariable,
    /// Feature-flag SDK evaluation call or definition.
    SdkCall,
    /// Flag key in a configuration object.
    ConfigObject,
    /// A `const` binding with a flag-style name and a literal value. It is
    /// in the retirement block only, not in `feature_flags[]`.
    Constant,
}

/// What a site does with the flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FlagSiteRole {
    /// The site reads the flag value.
    Read,
    /// The site defines the flag.
    Definition,
}

/// How the report measures the age of a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FlagAgeMode {
    /// `git blame` of the flag sites. The age is a lower bound: it is the age
    /// of the oldest line that still holds the flag.
    #[default]
    Blame,
    /// `git log -S` per flag name. The age is the date of the first commit
    /// that added the name.
    Pickaxe,
    /// No age.
    Off,
}

/// One site of a flag in the retirement report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetirementSite {
    /// File path relative to the analysed root.
    pub path: String,
    /// 1-based line.
    pub line: u32,
    /// 0-based byte column.
    pub col: u32,
    /// What the site does with the flag.
    pub role: FlagSiteRole,
    /// Whether the file is a test, story or mock file.
    pub in_test: bool,
}

/// A commit that git history links to a flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FlagCommit {
    /// Abbreviated commit hash.
    pub commit: String,
    /// Commit date in UTC, as `YYYY-MM-DD`.
    pub date: String,
}

/// One piece of evidence for a retirement reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetirementEvidence {
    /// The reason this evidence supports.
    pub reason: RetirementReason,
    /// File path relative to the analysed root.
    pub path: String,
    /// 1-based line.
    pub line: u32,
    /// What the evidence shows.
    pub detail: String,
}

/// Action discriminants for a retirement row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum RetirementActionType {
    /// A person reviews the flag for retirement.
    ReviewRetirement,
}

/// A follow-up action for a retirement candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetirementAction {
    /// Action discriminator, serialized as `type`.
    #[serde(rename = "type")]
    pub kind: RetirementActionType,
    /// Always `false`: Fallow never removes a flag.
    pub auto_fixable: bool,
    /// Human-readable action description.
    pub description: String,
}

/// One flag in the retirement report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetirementFlag {
    /// Flag identifier.
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: RetirementFlagKind,
    /// Flag SDK, for SDK flags with a known provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    /// Workspace root relative to the analysed root, when the project has
    /// workspaces and the flag is inside one. Part of the flag identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// Every site of the flag, sorted by path, line and column.
    pub sites: Vec<RetirementSite>,
    /// Number of sites in this row that read the flag.
    pub read_sites: usize,
    /// Whether every read site is in a test, story or mock file. Read sites
    /// of the same flag in other workspaces count too.
    pub test_only: bool,
    /// First commit that added the flag name. Set in `pickaxe` mode only.
    pub first_seen: Option<FlagCommit>,
    /// Oldest commit among the lines that still hold the flag.
    pub oldest_surviving_site: Option<FlagCommit>,
    /// Newest commit among the lines that still hold the flag.
    pub last_touched: Option<FlagCommit>,
    /// Days between the flag's oldest known commit and the analysis clock.
    /// In `blame` mode this is a lower bound.
    pub age_days: Option<u64>,
    /// Retirement reasons, in report order. Empty for a flag that is not a
    /// candidate.
    pub reasons: Vec<RetirementReason>,
    /// Evidence for each reason.
    pub evidence: Vec<RetirementEvidence>,
    /// Follow-up actions. Empty for a flag that is not a candidate.
    pub actions: Vec<RetirementAction>,
}

/// Totals of the retirement report.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RetirementSummary {
    /// Distinct flags in scope, before `--min-age` and `--reason`.
    pub distinct_flags: usize,
    /// Flags in scope with at least one reason.
    pub candidates: usize,
    /// Number of flags in scope per reason.
    pub by_reason: BTreeMap<RetirementReason, usize>,
}

/// The `retirement` block of `fallow flags --retirement --format json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct FlagRetirementReport {
    /// The analysis clock that ages count from, as an RFC 3339 UTC
    /// timestamp. `null` when the age mode is `off`.
    pub generated_at_clock: Option<String>,
    /// How the report measured flag age.
    pub age_mode: FlagAgeMode,
    /// Totals for the flags in scope.
    pub summary: RetirementSummary,
    /// One row per flag after the `--min-age`, `--reason`, `--sort` and
    /// `--top` options.
    pub flags: Vec<RetirementFlag>,
}
