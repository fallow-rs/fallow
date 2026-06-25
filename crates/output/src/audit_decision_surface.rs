//! Decision-surface output contracts.

use serde::Serialize;

/// Wire version for the `fallow decision-surface --format json` envelope.
pub const DECISION_SURFACE_SCHEMA_VERSION: u32 = 1;

/// The exactly-three shippable decision categories (the SOLID-3). No cut category
/// is representable, so confirmed-noise categories cannot ship by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DecisionCategory {
    /// A new dependency edge between modules or zones that did not depend before.
    CouplingBoundary,
    /// A new exported contract, or a changed contract consumed outside the diff.
    PublicApiContract,
    /// A new third-party dependency (new maintenance + security surface).
    ///
    /// The arm is part of the SOLID-3 surface, but its candidate source is not
    /// yet threaded onto the brief path, so the extractor never constructs it
    /// from a live signal today. Reserved, not dead.
    Dependency,
}

/// Every shippable decision category.
pub const ALL_CATEGORIES: [DecisionCategory; 3] = [
    DecisionCategory::CouplingBoundary,
    DecisionCategory::PublicApiContract,
    DecisionCategory::Dependency,
];

impl DecisionCategory {
    /// Stable lowercase tag used to namespace `signal_id` hashes and suppression
    /// comments.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::CouplingBoundary => "coupling-boundary",
            Self::PublicApiContract => "public-api-contract",
            Self::Dependency => "dependency",
        }
    }

    /// Per-category reversibility weight used by the CLI ranker.
    #[must_use]
    pub const fn reversibility_weight(self) -> u64 {
        match self {
            Self::Dependency => 5,
            Self::PublicApiContract => 3,
            Self::CouplingBoundary => 2,
        }
    }
}

/// One consequential structural decision, framed as a judgment question for a
/// human with taste, anchored to a fallow-emitted signal.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Decision {
    /// Deterministic anchor to the fallow-emitted candidate this decision frames.
    pub signal_id: String,
    /// One of the SOLID-3 categories.
    pub category: DecisionCategory,
    /// The decision framed as a judgment question for the human.
    pub question: String,
    /// Root-relative file the decision is anchored at.
    pub anchor_file: String,
    /// 1-based anchor line, or 0 when unavailable.
    pub anchor_line: u32,
    /// The raw fallow-emitted candidate key the `signal_id` hashes.
    pub signal_key: String,
    /// The pre-rename `signal_id`, when the anchor file was renamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_signal_id: Option<String>,
    /// Blast radius used for ranking.
    pub blast: u64,
    /// `blast * reversibility_weight`.
    pub consequence: u64,
    /// Routed expert(s), when ownership signals are available.
    pub expert: Vec<String>,
    /// Whether the anchor file's only qualified owner is one person.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bus_factor_one: bool,
    /// In-repo modules outside the diff that already depend on this anchor.
    pub internal_consumer_count: u64,
    /// The named structural trade-off this decision introduces.
    pub tradeoff: String,
}

/// A note for decisions collapsed below the cap.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TruncationNote {
    /// How many decisions were collapsed below the cap.
    pub collapsed: usize,
    /// Human-readable collapse reason.
    pub reason: String,
}

/// The ranked, capped decision surface plus the deterministic signal allowlist.
#[derive(Debug, Clone, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DecisionSurface {
    /// Ranked decisions, highest consequence first.
    pub decisions: Vec<Decision>,
    /// Present when more than the cap were extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<TruncationNote>,
    /// Every `signal_id` the deterministic layer emitted.
    pub emitted_signal_ids: Vec<String>,
}

impl DecisionSurface {
    /// Accept an agent-proposed `signal_id` only if fallow emitted it.
    #[must_use]
    pub fn accept_signal_id(&self, signal_id: &str) -> bool {
        self.emitted_signal_ids.iter().any(|id| id == signal_id)
    }
}

/// Independently-versioned wire-version newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DecisionSurfaceSchemaVersion(pub u32);

impl Default for DecisionSurfaceSchemaVersion {
    fn default() -> Self {
        Self(DECISION_SURFACE_SCHEMA_VERSION)
    }
}

/// A structured action attached to a surfaced decision.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DecisionAction {
    /// Stable action discriminator.
    #[serde(rename = "type")]
    pub action_type: DecisionActionType,
    /// Human-readable description of the action.
    pub description: String,
    /// Runnable command or paste-ready suppression comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Whether fallow can carry the action out automatically.
    pub auto_fixable: bool,
}

/// The discriminated action kinds a decision can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DecisionActionType {
    /// Route the decision to the named expert(s) for a judgment call.
    AskExpert,
    /// Suppress the decision with a `// fallow-ignore` comment.
    Suppress,
}

/// One decision plus its structured `actions[]`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DecisionWithActions {
    /// The underlying decision.
    #[serde(flatten)]
    pub decision: Decision,
    /// Structured actions.
    pub actions: Vec<DecisionAction>,
}

/// The separable `decision-surface` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow decision-surface --format json")
)]
pub struct DecisionSurfaceOutput {
    /// Independently-versioned schema version.
    pub schema_version: DecisionSurfaceSchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: String,
    /// Command discriminator singleton: always `"decision-surface"`.
    pub command: String,
    /// The ranked, capped decisions, each with structured actions.
    pub decisions: Vec<DecisionWithActions>,
    /// Present when more than the cap were extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<TruncationNote>,
    /// Count of fallow-emitted signal ids.
    pub signal_count: usize,
}

/// Build the suppression comment a decision's `suppress` action pastes in.
#[must_use]
pub fn suppress_comment(category: DecisionCategory) -> String {
    format!(
        "// fallow-ignore-next-line decision-surface {}",
        category.tag()
    )
}

/// Attach structured actions to one decision.
#[must_use]
pub fn decision_actions(decision: &Decision) -> Vec<DecisionAction> {
    let mut actions = Vec::new();
    if !decision.expert.is_empty() {
        actions.push(DecisionAction {
            action_type: DecisionActionType::AskExpert,
            description: format!("Ask {} to make this call", decision.expert.join(", ")),
            command: None,
            auto_fixable: false,
        });
    }
    actions.push(DecisionAction {
        action_type: DecisionActionType::Suppress,
        description: "Suppress this decision if it is settled".to_string(),
        command: Some(suppress_comment(decision.category)),
        auto_fixable: false,
    });
    actions
}

/// Project a [`DecisionSurface`] into the separable, action-bearing envelope.
#[must_use]
pub fn build_decision_surface_output(surface: &DecisionSurface) -> DecisionSurfaceOutput {
    debug_assert!(
        surface
            .decisions
            .iter()
            .all(|d| surface.accept_signal_id(&d.signal_id)
                && ALL_CATEGORIES.contains(&d.category)),
        "a surfaced decision has an unanchored signal_id or an out-of-SOLID-3 category"
    );
    let decisions = surface
        .decisions
        .iter()
        .map(|decision| DecisionWithActions {
            actions: decision_actions(decision),
            decision: decision.clone(),
        })
        .collect();
    DecisionSurfaceOutput {
        schema_version: DecisionSurfaceSchemaVersion::default(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        command: "decision-surface".to_string(),
        decisions,
        truncated: surface.truncated.clone(),
        signal_count: surface.emitted_signal_ids.len(),
    }
}
