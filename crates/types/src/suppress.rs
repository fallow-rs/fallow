//! Inline suppression comment types and issue kind definitions.

pub use crate::issue_meta::{DEAD_CODE_FILTER_FLAGS, KNOWN_ISSUE_KIND_NAMES};

/// Issue kind for suppression matching.
///
/// # Examples
///
/// ```
/// use fallow_types::suppress::IssueKind;
///
/// let kind = IssueKind::parse("unused-export");
/// assert_eq!(kind, Some(IssueKind::UnusedExport));
///
/// // Round-trip through discriminant
/// let d = IssueKind::UnusedFile.to_discriminant();
/// assert_eq!(IssueKind::from_discriminant(d), Some(IssueKind::UnusedFile));
///
/// // Unknown strings return None
/// assert_eq!(IssueKind::parse("not-a-kind"), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// An unused file.
    UnusedFile,
    /// An unused export.
    UnusedExport,
    /// An unused type export.
    UnusedType,
    /// An exported signature that references a same-file private type.
    PrivateTypeLeak,
    /// An unused dependency.
    UnusedDependency,
    /// An unused dev dependency.
    UnusedDevDependency,
    /// An unused enum member.
    UnusedEnumMember,
    /// An unused class member.
    UnusedClassMember,
    /// An unresolved import.
    UnresolvedImport,
    /// An unlisted dependency.
    UnlistedDependency,
    /// A duplicate export name across modules.
    DuplicateExport,
    /// Code duplication.
    CodeDuplication,
    /// A circular dependency chain.
    CircularDependency,
    /// A cycle or self-loop in the re-export edge subgraph (barrel files
    /// re-exporting from each other in a loop). Structurally always a bug:
    /// chain propagation through the cycle is a no-op.
    ReExportCycle,
    /// A production dependency only imported via type-only imports.
    TypeOnlyDependency,
    /// A production dependency only imported by test files.
    TestOnlyDependency,
    /// An import that crosses an architecture boundary.
    BoundaryViolation,
    /// A runtime file or export with no test dependency path.
    CoverageGaps,
    /// A detected feature flag pattern.
    FeatureFlag,
    /// A function exceeding complexity thresholds (health command).
    Complexity,
    /// A suppression comment or JSDoc tag that no longer matches any issue.
    StaleSuppression,
    /// A pnpm catalog entry in pnpm-workspace.yaml not referenced by any workspace package.
    PnpmCatalogEntry,
    /// A named pnpm catalog group in pnpm-workspace.yaml with no entries.
    EmptyCatalogGroup,
    /// A workspace package.json reference (`catalog:` / `catalog:<name>`) pointing at
    /// a catalog that does not declare the consumed package.
    UnresolvedCatalogReference,
    /// An entry in pnpm's `overrides:` / `pnpm.overrides` whose target package
    /// is not declared in any workspace `package.json`.
    UnusedDependencyOverride,
    /// An entry in pnpm's `overrides:` / `pnpm.overrides` whose key or value
    /// cannot be parsed into a valid pnpm shape.
    MisconfiguredDependencyOverride,
    /// A `"use client"` file that transitively imports a module reading a
    /// non-public `process.env` secret (security candidate).
    SecurityClientServerLeak,
    /// A syntactic tainted-sink candidate matched against the data-driven
    /// security matcher catalogue (`security_matchers.toml`). ONE suppression
    /// token covers all catalogue categories.
    SecuritySink,
    /// A banned call or banned import matched by a declarative rule pack
    /// (`rulePacks` config). The bare token covers every pack rule; scoped
    /// tokens can target one `<pack>/<rule-id>` identity.
    PolicyViolation,
    /// A `"use client"` file that exports a Next.js server-only /
    /// route-segment config name (e.g. `metadata`, `revalidate`, `GET`).
    InvalidClientExport,
    /// A barrel file that re-exports BOTH a `"use client"` origin module AND a
    /// server-only origin module (Next.js App Router footgun: one import drags
    /// the other's directive context across the boundary).
    MixedClientServerBarrel,
    /// A `"use client"` / `"use server"` directive string written as an
    /// expression statement after a non-directive statement (an import, a
    /// const). It is no longer in the leading prologue, so the RSC bundler
    /// parses it as an ordinary string and silently ignores it.
    MisplacedDirective,
    /// A store member (Pinia `state` / `getters` / `actions` key, or a
    /// setup-store returned key) declared but never accessed by any consumer
    /// project-wide. Cross-graph: the store binding is imported (the module is
    /// reachable) yet a specific member is dead.
    UnusedStoreMember,
    /// A Vue `inject(KEY)` or Svelte `getContext(KEY)` whose symbol KEY is
    /// `provide`/`setContext`'d nowhere in the analyzed project. Cross-graph
    /// dead-half DI link: at runtime the inject returns `undefined`.
    UnprovidedInject,
    /// Two or more Next.js App Router route files that resolve to the same URL
    /// within one app-root (a guaranteed `next build` failure).
    RouteCollision,
    /// Sibling Next.js dynamic route segments at one tree position using
    /// different param spellings (`[id]` vs `[slug]`; a dev / runtime error
    /// that `next build` does NOT catch).
    DynamicSegmentNameConflict,
    /// A component defined in the project that is exported but never rendered
    /// (no JSX usage) anywhere across the analyzed project.
    UnrenderedComponent,
    /// A Vue `<script setup>` `defineProps`, Svelte 5 `$props()`, or React
    /// declared prop that is referenced NOWHERE inside its own component.
    /// Single-component dead-input direction.
    UnusedComponentProp,
    /// A Vue `<script setup>` `defineEmits` declared event that is EMITTED
    /// nowhere inside its own single-file component (no `emit('<name>')` call).
    /// Single-file dead-input direction.
    UnusedComponentEmit,
    /// An Angular `@Input()` / signal `input()` / `model()` declared input that
    /// is read NOWHERE inside its own component (neither the inline/external
    /// template nor the class body). Single-file dead-input direction; the
    /// Angular analogue of `unused-component-prop`.
    UnusedComponentInput,
    /// An Angular `@Output()` / signal `output()` declared output that is
    /// EMITTED nowhere inside its own component (no `this.<output>.emit(...)`).
    /// Single-file dead-output direction; the Angular analogue of
    /// `unused-component-emit`.
    UnusedComponentOutput,
    /// A Next.js Server Action (an export of a `"use server"` file) that no code
    /// in the project references (no import-and-call, no `action={fn}` binding,
    /// no `<form action={fn}>`). Cross-graph dead-export direction, reclassified
    /// from `unused-export` for `"use server"` files.
    UnusedServerAction,
    /// A SvelteKit `+page.{ts,server.ts,js,server.js}` `load()` return-object key
    /// that no consumer reads: not off the sibling `+page.svelte`'s `data.<key>`,
    /// nor project-wide via `page.data.<key>` / `$page.data.<key>`. A dead load
    /// key runs a real server/DB fetch cost for data nothing renders.
    UnusedLoadDataKey,
    /// A React/Preact prop forwarded unchanged through `>= N` intermediate
    /// pass-through components until a component that substantively consumes it.
    /// Health signal, rule defaults to `off` (opt-in). Cross-graph: the chain
    /// spans multiple components / files.
    PropDrilling,
    /// A React/Preact component whose entire body is `return <Child {...props}/>`
    /// (a single spread-forwarded child render, no own value-add): pure
    /// structural indirection, a candidate for inlining. Health signal, rule
    /// defaults to `off` (opt-in).
    ThinWrapper,
    /// Three or more React/Preact components across two or more files whose
    /// statically-harvested prop NAME set is identical after stripping ubiquitous
    /// DOM / passthrough names (a missing shared `Props` type). Health signal,
    /// rule defaults to `off` (opt-in). Cross-graph: the group spans multiple
    /// components / files.
    DuplicatePropShape,
    /// A Svelte component dispatching a custom event via
    /// `createEventDispatcher()` whose event name is listened to NOWHERE in the
    /// analyzed project. Cross-file dead-output direction: the component fires an
    /// event nothing handles.
    UnusedSvelteEvent,
    /// A CSS / CSS-in-JS design-token DRIFT candidate surfaced in `fallow audit`
    /// as an advisory styling finding: a hardcoded value where a design token
    /// exists (a Tailwind arbitrary value like `w-[13px]`, or a near-duplicate
    /// token). Styling-domain finding produced by the health-time css pass (not
    /// dead-code); the rule defaults to `warn` and is verdict-neutral.
    CssTokenDrift,
    /// A CSS / CSS-in-JS DUPLICATE declaration block: a copy-pasted rule body
    /// repeated across selectors, a consolidation candidate. Styling-domain
    /// advisory (rule defaults to `warn`, verdict-neutral); the audit copy of
    /// this is changed-file-local.
    CssDuplicateBlock,
    /// A CSS selector / nesting / important-density complexity finding surfaced
    /// as advisory styling feedback. Styling-domain finding produced by the
    /// health-time css pass; defaults to `warn` and is verdict-neutral.
    CssSelectorComplexity,
    /// A CSS dead-surface finding, such as unused scoped SFC classes. Styling-
    /// domain advisory surfaced in `fallow audit`; defaults to `warn` and is
    /// verdict-neutral.
    CssDeadSurface,
    /// A CSS broken-reference finding, such as a class or keyframes reference
    /// that resolves to no stylesheet definition. Styling-domain advisory
    /// surfaced by deep CSS audit mode; defaults to `warn` and is
    /// verdict-neutral.
    CssBrokenReference,
}

impl IssueKind {
    /// Stable inventory of all issue kinds.
    pub const ALL: &'static [Self] = &[
        Self::UnusedFile,
        Self::UnusedExport,
        Self::UnusedType,
        Self::PrivateTypeLeak,
        Self::UnusedDependency,
        Self::UnusedDevDependency,
        Self::UnusedEnumMember,
        Self::UnusedClassMember,
        Self::UnresolvedImport,
        Self::UnlistedDependency,
        Self::DuplicateExport,
        Self::CodeDuplication,
        Self::CircularDependency,
        Self::ReExportCycle,
        Self::TypeOnlyDependency,
        Self::TestOnlyDependency,
        Self::BoundaryViolation,
        Self::CoverageGaps,
        Self::FeatureFlag,
        Self::Complexity,
        Self::StaleSuppression,
        Self::PnpmCatalogEntry,
        Self::EmptyCatalogGroup,
        Self::UnresolvedCatalogReference,
        Self::UnusedDependencyOverride,
        Self::MisconfiguredDependencyOverride,
        Self::SecurityClientServerLeak,
        Self::SecuritySink,
        Self::PolicyViolation,
        Self::InvalidClientExport,
        Self::MixedClientServerBarrel,
        Self::MisplacedDirective,
        Self::UnusedStoreMember,
        Self::UnprovidedInject,
        Self::RouteCollision,
        Self::DynamicSegmentNameConflict,
        Self::UnrenderedComponent,
        Self::UnusedComponentProp,
        Self::UnusedComponentEmit,
        Self::UnusedComponentInput,
        Self::UnusedComponentOutput,
        Self::UnusedServerAction,
        Self::UnusedLoadDataKey,
        Self::PropDrilling,
        Self::ThinWrapper,
        Self::DuplicatePropShape,
        Self::UnusedSvelteEvent,
        Self::CssTokenDrift,
        Self::CssDuplicateBlock,
        Self::CssSelectorComplexity,
        Self::CssDeadSurface,
        Self::CssBrokenReference,
    ];

    /// Parse an issue kind from the string tokens used in CLI output and suppression comments.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        crate::issue_meta::issue_meta_for_token(s).and_then(|meta| meta.kind)
    }

    /// Convert to a u8 discriminant for compact cache storage.
    #[must_use]
    pub const fn to_discriminant(self) -> u8 {
        match self {
            Self::UnusedFile => 1,
            Self::UnusedExport => 2,
            Self::UnusedType => 3,
            Self::PrivateTypeLeak => 4,
            Self::UnusedDependency => 5,
            Self::UnusedDevDependency => 6,
            Self::UnusedEnumMember => 7,
            Self::UnusedClassMember => 8,
            Self::UnresolvedImport => 9,
            Self::UnlistedDependency => 10,
            Self::DuplicateExport => 11,
            Self::CodeDuplication => 12,
            Self::CircularDependency => 13,
            Self::TypeOnlyDependency => 14,
            Self::TestOnlyDependency => 15,
            Self::BoundaryViolation => 16,
            Self::CoverageGaps => 17,
            Self::FeatureFlag => 18,
            Self::Complexity => 19,
            Self::StaleSuppression => 20,
            Self::PnpmCatalogEntry => 21,
            Self::UnresolvedCatalogReference => 22,
            Self::UnusedDependencyOverride => 23,
            Self::MisconfiguredDependencyOverride => 24,
            Self::EmptyCatalogGroup => 25,
            Self::ReExportCycle => 26,
            Self::SecurityClientServerLeak => 27,
            Self::SecuritySink => 28,
            Self::PolicyViolation => 29,
            Self::InvalidClientExport => 30,
            Self::MixedClientServerBarrel => 31,
            Self::MisplacedDirective => 32,
            Self::UnusedStoreMember => 33,
            Self::UnprovidedInject => 34,
            Self::RouteCollision => 35,
            Self::DynamicSegmentNameConflict => 36,
            Self::UnrenderedComponent => 37,
            Self::UnusedComponentProp => 38,
            Self::UnusedComponentEmit => 39,
            Self::UnusedServerAction => 40,
            Self::UnusedLoadDataKey => 41,
            Self::PropDrilling => 42,
            Self::ThinWrapper => 43,
            Self::DuplicatePropShape => 44,
            Self::UnusedComponentInput => 45,
            Self::UnusedComponentOutput => 46,
            Self::UnusedSvelteEvent => 47,
            Self::CssTokenDrift => 48,
            Self::CssDuplicateBlock => 49,
            Self::CssSelectorComplexity => 50,
            Self::CssDeadSurface => 51,
            Self::CssBrokenReference => 52,
        }
    }

    /// Reconstruct from a cache discriminant.
    #[must_use]
    pub const fn from_discriminant(d: u8) -> Option<Self> {
        match d {
            1 => Some(Self::UnusedFile),
            2 => Some(Self::UnusedExport),
            3 => Some(Self::UnusedType),
            4 => Some(Self::PrivateTypeLeak),
            5 => Some(Self::UnusedDependency),
            6 => Some(Self::UnusedDevDependency),
            7 => Some(Self::UnusedEnumMember),
            8 => Some(Self::UnusedClassMember),
            9 => Some(Self::UnresolvedImport),
            10 => Some(Self::UnlistedDependency),
            11 => Some(Self::DuplicateExport),
            12 => Some(Self::CodeDuplication),
            13 => Some(Self::CircularDependency),
            14 => Some(Self::TypeOnlyDependency),
            15 => Some(Self::TestOnlyDependency),
            16 => Some(Self::BoundaryViolation),
            17 => Some(Self::CoverageGaps),
            18 => Some(Self::FeatureFlag),
            19 => Some(Self::Complexity),
            20 => Some(Self::StaleSuppression),
            21 => Some(Self::PnpmCatalogEntry),
            22 => Some(Self::UnresolvedCatalogReference),
            23 => Some(Self::UnusedDependencyOverride),
            24 => Some(Self::MisconfiguredDependencyOverride),
            25 => Some(Self::EmptyCatalogGroup),
            26 => Some(Self::ReExportCycle),
            27 => Some(Self::SecurityClientServerLeak),
            28 => Some(Self::SecuritySink),
            29 => Some(Self::PolicyViolation),
            30 => Some(Self::InvalidClientExport),
            31 => Some(Self::MixedClientServerBarrel),
            32 => Some(Self::MisplacedDirective),
            33 => Some(Self::UnusedStoreMember),
            34 => Some(Self::UnprovidedInject),
            35 => Some(Self::RouteCollision),
            36 => Some(Self::DynamicSegmentNameConflict),
            37 => Some(Self::UnrenderedComponent),
            38 => Some(Self::UnusedComponentProp),
            39 => Some(Self::UnusedComponentEmit),
            40 => Some(Self::UnusedServerAction),
            41 => Some(Self::UnusedLoadDataKey),
            42 => Some(Self::PropDrilling),
            43 => Some(Self::ThinWrapper),
            44 => Some(Self::DuplicatePropShape),
            45 => Some(Self::UnusedComponentInput),
            46 => Some(Self::UnusedComponentOutput),
            47 => Some(Self::UnusedSvelteEvent),
            48 => Some(Self::CssTokenDrift),
            49 => Some(Self::CssDuplicateBlock),
            50 => Some(Self::CssSelectorComplexity),
            51 => Some(Self::CssDeadSurface),
            52 => Some(Self::CssBrokenReference),
            _ => None,
        }
    }
}

/// One scoped rule-pack policy suppression target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PolicyRuleSuppression {
    /// Rule-pack name.
    pub pack: String,
    /// Rule id within the pack.
    pub rule_id: String,
}

impl PolicyRuleSuppression {
    /// Build a scoped policy suppression target.
    #[must_use]
    pub fn new(pack: impl Into<String>, rule_id: impl Into<String>) -> Self {
        Self {
            pack: pack.into(),
            rule_id: rule_id.into(),
        }
    }

    /// Canonical suppression token.
    #[must_use]
    pub fn token(&self) -> String {
        format!("policy-violation:{}/{}", self.pack, self.rule_id)
    }
}

/// A specific suppression target parsed from a comment token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressionTarget {
    /// A regular issue-kind token such as `unused-export` or bare
    /// `policy-violation`.
    Issue(IssueKind),
    /// A scoped rule-pack policy token such as
    /// `policy-violation:team-policy/no-child-process`.
    PolicyRule(PolicyRuleSuppression),
}

impl SuppressionTarget {
    /// Return the regular issue kind when this target is a bare issue-kind
    /// token.
    #[must_use]
    pub const fn issue_kind(&self) -> Option<IssueKind> {
        match self {
            Self::Issue(kind) => Some(*kind),
            Self::PolicyRule(_) => None,
        }
    }

    /// Canonical suppression token for output and active-suppression capture.
    #[must_use]
    pub fn token(&self) -> String {
        match self {
            Self::Issue(kind) => issue_kind_to_kebab(*kind).to_owned(),
            Self::PolicyRule(rule) => rule.token(),
        }
    }
}

/// Convert an [`IssueKind`] to its canonical suppression token.
#[must_use]
pub fn issue_kind_to_kebab(kind: IssueKind) -> &'static str {
    let Some(meta) = crate::issue_meta::issue_meta_by_kind(kind) else {
        unreachable!("IssueKind {kind:?} has no metadata row");
    };
    meta.suppress_token.unwrap_or(meta.code)
}

/// Parse a suppression token into a structured target.
#[must_use]
pub fn parse_suppression_target(token: &str) -> Option<SuppressionTarget> {
    parse_policy_rule_suppression_token(token)
        .map(SuppressionTarget::PolicyRule)
        .or_else(|| IssueKind::parse(token).map(SuppressionTarget::Issue))
}

/// Parse canonical scoped policy suppression tokens.
///
/// The plural prefix is accepted for consistency with the bare legacy alias,
/// but output always uses singular `policy-violation:`.
#[must_use]
pub fn parse_policy_rule_suppression_token(token: &str) -> Option<PolicyRuleSuppression> {
    let identity = token
        .strip_prefix("policy-violation:")
        .or_else(|| token.strip_prefix("policy-violations:"))?;
    let (pack, rule_id) = identity.split_once('/')?;
    if rule_id.contains('/') {
        return None;
    }
    if !is_valid_policy_identifier(pack) || !is_valid_policy_identifier(rule_id) {
        return None;
    }
    Some(PolicyRuleSuppression::new(pack, rule_id))
}

/// Whether a rule-pack name or rule id can be used inside
/// `policy-violation:<pack>/<rule-id>` without escaping.
#[must_use]
pub fn is_valid_policy_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A suppression directive parsed from a source comment.
///
/// # Examples
///
/// ```
/// use fallow_types::suppress::{Suppression, IssueKind};
///
/// // File-wide suppression (line 0, no specific kind)
/// let file_wide = Suppression::all(0, 1);
/// assert_eq!(file_wide.line, 0);
///
/// // Line-specific suppression for unused exports
/// let line_suppress = Suppression::issue(42, 41, IssueKind::UnusedExport);
/// assert_eq!(line_suppress.issue_kind_target(), Some(IssueKind::UnusedExport));
/// ```
#[derive(Debug, Clone)]
pub struct Suppression {
    /// 1-based line this suppression applies to. 0 = file-wide suppression.
    pub line: u32,
    /// 1-based line where the suppression comment itself appears.
    /// For `fallow-ignore-next-line`, this is `line - 1`.
    /// For `fallow-ignore-file`, this is the actual line of the comment in the source.
    pub comment_line: u32,
    /// None = suppress all issue kinds on this line or file.
    pub target: Option<SuppressionTarget>,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

impl Suppression {
    /// Build a blanket suppression.
    #[must_use]
    pub const fn all(line: u32, comment_line: u32) -> Self {
        Self {
            line,
            comment_line,
            target: None,
            reason: None,
        }
    }

    /// Build a regular issue-kind suppression.
    #[must_use]
    pub const fn issue(line: u32, comment_line: u32, kind: IssueKind) -> Self {
        Self {
            line,
            comment_line,
            target: Some(SuppressionTarget::Issue(kind)),
            reason: None,
        }
    }

    /// Build a scoped rule-pack policy suppression.
    #[must_use]
    pub fn policy_rule(
        line: u32,
        comment_line: u32,
        pack: impl Into<String>,
        rule_id: impl Into<String>,
    ) -> Self {
        Self {
            line,
            comment_line,
            target: Some(SuppressionTarget::PolicyRule(PolicyRuleSuppression::new(
                pack, rule_id,
            ))),
            reason: None,
        }
    }

    /// Return a copy with a parsed suppression reason attached.
    #[must_use]
    pub fn with_reason(mut self, reason: Option<String>) -> Self {
        self.reason = reason;
        self
    }

    /// The bare issue kind if this suppression targets one.
    #[must_use]
    pub const fn issue_kind_target(&self) -> Option<IssueKind> {
        match &self.target {
            Some(SuppressionTarget::Issue(kind)) => Some(*kind),
            Some(SuppressionTarget::PolicyRule(_)) | None => None,
        }
    }

    /// The scoped policy target if this suppression targets one rule-pack rule.
    #[must_use]
    pub const fn policy_rule_target(&self) -> Option<&PolicyRuleSuppression> {
        match &self.target {
            Some(SuppressionTarget::PolicyRule(rule)) => Some(rule),
            Some(SuppressionTarget::Issue(_)) | None => None,
        }
    }

    /// Canonical token for this suppression, or `None` for blanket comments.
    #[must_use]
    pub fn target_token(&self) -> Option<String> {
        self.target.as_ref().map(SuppressionTarget::token)
    }

    /// Whether the comment applies to `line`.
    #[must_use]
    pub const fn applies_to_line(&self, line: u32) -> bool {
        self.line == 0 || self.line == line
    }

    /// Whether this suppression covers a regular issue kind on a line.
    ///
    /// Scoped policy-rule targets intentionally do not match this generic
    /// predicate. Policy detection uses [`Self::matches_policy_rule`] so the
    /// exact pack and rule id are available.
    #[must_use]
    pub fn matches_issue_kind(&self, line: u32, kind: IssueKind) -> bool {
        self.applies_to_line(line)
            && match &self.target {
                None => true,
                Some(SuppressionTarget::Issue(target_kind)) => *target_kind == kind,
                Some(SuppressionTarget::PolicyRule(_)) => false,
            }
    }

    /// Whether this suppression covers a policy finding on a line.
    #[must_use]
    pub fn matches_policy_rule(&self, line: u32, pack: &str, rule_id: &str) -> bool {
        self.applies_to_line(line)
            && match &self.target {
                None | Some(SuppressionTarget::Issue(IssueKind::PolicyViolation)) => true,
                Some(SuppressionTarget::Issue(_)) => false,
                Some(SuppressionTarget::PolicyRule(target)) => {
                    target.pack == pack && target.rule_id == rule_id
                }
            }
    }
}

/// Check if a specific issue at a given line should be suppressed.
#[must_use]
pub fn is_suppressed(suppressions: &[Suppression], line: u32, kind: IssueKind) -> bool {
    suppressions
        .iter()
        .any(|suppression| suppression.matches_issue_kind(line, kind))
}

/// Check if the entire file is suppressed for issue types that do not have line numbers.
#[must_use]
pub fn is_file_suppressed(suppressions: &[Suppression], kind: IssueKind) -> bool {
    suppressions
        .iter()
        .any(|suppression| suppression.line == 0 && suppression.matches_issue_kind(0, kind))
}

/// A suppression token that did not parse to any known `IssueKind`.
///
/// Emitted alongside `Suppression` when a `// fallow-ignore-*` marker contains
/// a typo or an obsolete issue-kind name. The known tokens on the same marker
/// are recorded as normal `Suppression` entries; this struct preserves the
/// unknown token so the downstream `find_stale` pass can surface it as a
/// `StaleSuppression` finding with `kind_known: false`. Without this, the
/// entire suppression line would be discarded silently. See issue #449.
#[derive(Debug, Clone)]
pub struct UnknownSuppressionKind {
    /// 1-based line where the suppression comment itself appears.
    pub comment_line: u32,
    /// Whether the marker was `fallow-ignore-file` (`true`) or
    /// `fallow-ignore-next-line` (`false`).
    pub is_file_level: bool,
    /// The verbatim token from the marker that did not parse.
    pub token: String,
    /// Human-authored reason after `--`, when present.
    pub reason: Option<String>,
}

/// Levenshtein edit distance between two ASCII-leaning strings.
///
/// Local duplicate of the config-crate helper (see
/// `crates/config/src/config/rules.rs::levenshtein`) so `fallow-types` can
/// compute "did you mean?" suggestions for unknown suppression tokens without
/// taking a dependency on `fallow-config`. Issue-kind names are short
/// (max ~33 chars) so allocation cost is negligible.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let (a_len, b_len) = (a_bytes.len(), b_bytes.len());

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the closest known issue-kind name to `input` when it is plausibly a typo.
///
/// Returns the best match when the Levenshtein distance is at most 2 AND
/// the input is long enough that the match is not coincidental
/// (`input.len() / 2 > distance`). Returns `None` for completely novel
/// strings where a suggestion would be misleading.
#[must_use]
pub fn closest_known_kind_name(input: &str) -> Option<&'static str> {
    let input_lower = input.to_ascii_lowercase();
    let mut best: Option<(&'static str, usize)> = None;

    for &candidate in KNOWN_ISSUE_KIND_NAMES.iter() {
        let d = levenshtein(&input_lower, candidate);
        if best.is_none_or(|(_, b_dist)| d < b_dist) {
            best = Some((candidate, d));
        }
    }

    best.filter(|&(_, d)| d > 0 && d <= 2 && input_lower.len() / 2 > d)
        .map(|(name, _)| name)
}

const _: () = assert!(std::mem::size_of::<IssueKind>() == 1);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_kind_parse_accepts_registry_codes_and_aliases() {
        for meta in crate::issue_meta::ISSUE_KIND_META
            .iter()
            .filter(|meta| meta.kind.is_some())
        {
            let expected = meta.kind;
            assert_eq!(
                IssueKind::parse(meta.code),
                expected,
                "canonical registry token {} must parse",
                meta.code
            );
            for alias in meta.aliases {
                assert_eq!(
                    IssueKind::parse(alias),
                    expected,
                    "registry alias {alias} must parse as {}",
                    meta.code
                );
            }
        }
    }

    #[test]
    fn issue_kind_parse_accepts_registry_suppression_tokens() {
        for meta in crate::issue_meta::ISSUE_KIND_META {
            let (Some(kind), Some(token)) = (meta.kind, meta.suppress_token) else {
                continue;
            };
            assert_eq!(
                IssueKind::parse(token),
                Some(kind),
                "registry suppression token {token} must parse as {}",
                meta.code
            );
        }
    }

    #[test]
    fn issue_kind_from_str_unknown() {
        assert_eq!(IssueKind::parse("foo"), None);
        assert_eq!(IssueKind::parse(""), None);
    }

    #[test]
    fn issue_kind_from_str_near_misses() {
        assert_eq!(IssueKind::parse("Unused-File"), None);
        assert_eq!(IssueKind::parse("UNUSED-EXPORT"), None);
        assert_eq!(IssueKind::parse("unused_file"), None);
        assert_eq!(IssueKind::parse("unused-files"), None);
    }

    #[test]
    fn discriminant_out_of_range() {
        assert_eq!(IssueKind::from_discriminant(0), None);
        assert_eq!(
            IssueKind::from_discriminant(29),
            Some(IssueKind::PolicyViolation)
        );
        assert_eq!(
            IssueKind::from_discriminant(30),
            Some(IssueKind::InvalidClientExport)
        );
        assert_eq!(
            IssueKind::from_discriminant(31),
            Some(IssueKind::MixedClientServerBarrel)
        );
        assert_eq!(
            IssueKind::from_discriminant(32),
            Some(IssueKind::MisplacedDirective)
        );
        assert_eq!(
            IssueKind::from_discriminant(33),
            Some(IssueKind::UnusedStoreMember)
        );
        assert_eq!(
            IssueKind::from_discriminant(34),
            Some(IssueKind::UnprovidedInject)
        );
        assert_eq!(
            IssueKind::from_discriminant(35),
            Some(IssueKind::RouteCollision)
        );
        assert_eq!(
            IssueKind::from_discriminant(36),
            Some(IssueKind::DynamicSegmentNameConflict)
        );
        assert_eq!(
            IssueKind::from_discriminant(37),
            Some(IssueKind::UnrenderedComponent)
        );
        assert_eq!(
            IssueKind::from_discriminant(38),
            Some(IssueKind::UnusedComponentProp)
        );
        assert_eq!(
            IssueKind::from_discriminant(39),
            Some(IssueKind::UnusedComponentEmit)
        );
        assert_eq!(
            IssueKind::from_discriminant(40),
            Some(IssueKind::UnusedServerAction)
        );
        assert_eq!(
            IssueKind::from_discriminant(41),
            Some(IssueKind::UnusedLoadDataKey)
        );
        assert_eq!(
            IssueKind::from_discriminant(42),
            Some(IssueKind::PropDrilling)
        );
        assert_eq!(
            IssueKind::from_discriminant(43),
            Some(IssueKind::ThinWrapper)
        );
        assert_eq!(
            IssueKind::from_discriminant(44),
            Some(IssueKind::DuplicatePropShape)
        );
        assert_eq!(
            IssueKind::from_discriminant(45),
            Some(IssueKind::UnusedComponentInput)
        );
        assert_eq!(
            IssueKind::from_discriminant(46),
            Some(IssueKind::UnusedComponentOutput)
        );
        assert_eq!(
            IssueKind::from_discriminant(47),
            Some(IssueKind::UnusedSvelteEvent)
        );
        let max_discriminant = IssueKind::ALL
            .iter()
            .map(|kind| kind.to_discriminant())
            .max()
            .expect("IssueKind::ALL should not be empty");
        assert_eq!(IssueKind::from_discriminant(max_discriminant + 1), None);
        assert_eq!(IssueKind::from_discriminant(u8::MAX), None);
    }

    #[test]
    fn discriminant_roundtrip() {
        for &kind in IssueKind::ALL {
            assert_eq!(
                IssueKind::from_discriminant(kind.to_discriminant()),
                Some(kind)
            );
        }
        assert_eq!(IssueKind::from_discriminant(0), None);
        let max_discriminant = IssueKind::ALL
            .iter()
            .map(|kind| kind.to_discriminant())
            .max()
            .expect("IssueKind::ALL should not be empty");
        assert_eq!(IssueKind::from_discriminant(max_discriminant + 1), None);
    }

    #[test]
    fn discriminant_values_are_unique() {
        let discriminants: Vec<u8> = IssueKind::ALL
            .iter()
            .map(|kind| kind.to_discriminant())
            .collect();
        let mut sorted = discriminants.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            discriminants.len(),
            sorted.len(),
            "discriminant values must be unique"
        );
    }

    #[test]
    fn discriminant_starts_at_one() {
        assert_eq!(IssueKind::UnusedFile.to_discriminant(), 1);
    }

    #[test]
    fn issue_kind_to_kebab_uses_registry_suppression_token() {
        for &kind in IssueKind::ALL {
            let meta = crate::issue_meta::issue_meta_by_kind(kind)
                .unwrap_or_else(|| panic!("IssueKind {kind:?} has no metadata row"));
            let token = issue_kind_to_kebab(kind);
            assert_eq!(token, meta.suppress_token.unwrap_or(meta.code));
            assert_eq!(IssueKind::parse(token), Some(kind));
        }
    }

    #[test]
    fn suppression_line_zero_is_file_wide() {
        let s = Suppression::all(0, 1);
        assert_eq!(s.line, 0);
        assert!(s.issue_kind_target().is_none());
    }

    #[test]
    fn suppression_with_specific_kind_and_line() {
        let s = Suppression::issue(42, 41, IssueKind::UnusedExport);
        assert_eq!(s.line, 42);
        assert_eq!(s.comment_line, 41);
        assert_eq!(s.issue_kind_target(), Some(IssueKind::UnusedExport));
    }

    #[test]
    fn suppression_predicates_match_lines_and_file_wide_markers() {
        let suppressions = vec![
            Suppression::issue(42, 41, IssueKind::UnusedExport),
            Suppression::all(0, 1),
        ];

        assert!(is_suppressed(&suppressions, 42, IssueKind::UnusedExport));
        assert!(is_suppressed(&suppressions, 10, IssueKind::UnusedType));
        assert!(is_file_suppressed(&suppressions, IssueKind::UnusedFile));
    }

    #[test]
    fn parses_scoped_policy_suppression_token() {
        let target =
            parse_policy_rule_suppression_token("policy-violation:team-policy/no-child-process")
                .expect("scoped token should parse");
        assert_eq!(target.pack, "team-policy");
        assert_eq!(target.rule_id, "no-child-process");
        assert_eq!(
            target.token(),
            "policy-violation:team-policy/no-child-process"
        );
    }

    #[test]
    fn rejects_malformed_scoped_policy_suppression_tokens() {
        for token in [
            "policy-violation:",
            "policy-violation:team-policy",
            "policy-violation:/no-child-process",
            "policy-violation:team-policy/",
            "policy-violation:team-policy/no/child-process",
            "policy-violation:team policy/no-child-process",
            "policy-violation:team-policy/no:child-process",
        ] {
            assert!(
                parse_policy_rule_suppression_token(token).is_none(),
                "{token} should be rejected"
            );
        }
    }

    #[test]
    fn scoped_policy_suppression_matches_exact_policy_rule_only() {
        let suppression = Suppression::policy_rule(7, 6, "team-policy", "no-child-process");
        assert!(suppression.matches_policy_rule(7, "team-policy", "no-child-process"));
        assert!(!suppression.matches_policy_rule(7, "team-policy", "no-fs"));
        assert!(!suppression.matches_policy_rule(8, "team-policy", "no-child-process"));
        assert!(!suppression.matches_issue_kind(7, IssueKind::PolicyViolation));
    }

    #[test]
    fn known_issue_kind_names_parses_each_entry() {
        for &name in KNOWN_ISSUE_KIND_NAMES.iter() {
            assert!(
                IssueKind::parse(name).is_some(),
                "KNOWN_ISSUE_KIND_NAMES contains '{name}' but IssueKind::parse rejects it"
            );
        }
    }

    #[test]
    fn closest_known_kind_name_finds_near_misses() {
        assert_eq!(
            closest_known_kind_name("unused-exports"),
            Some("unused-export")
        );
        assert_eq!(closest_known_kind_name("unused-files"), Some("unused-file"));
        assert_eq!(closest_known_kind_name("complxity"), Some("complexity"));
    }

    #[test]
    fn closest_known_kind_name_rejects_novel_strings() {
        assert_eq!(closest_known_kind_name("xyzzy"), None);
        assert_eq!(closest_known_kind_name("foo"), None);
        assert_eq!(closest_known_kind_name(""), None);
    }

    #[test]
    fn closest_known_kind_name_skips_exact_match() {
        assert_eq!(closest_known_kind_name("unused-export"), None);
    }
}
