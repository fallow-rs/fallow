use serde::{Deserialize, Serialize};

/// Schema discriminator serialized into [`PrDecisionSurface::schema`].
pub const PR_DECISION_SCHEMA: &str = "fallow-pr-decision/v1";

/// Provider-neutral PR gate decision, consumed by CI integrations to publish
/// a check run or equivalent status surface.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionSurface {
    /// Schema discriminator; always [`PR_DECISION_SCHEMA`].
    pub schema: String,
    /// Display title for the check surface.
    pub title: String,
    /// Overall conclusion aggregated across `gates`.
    pub conclusion: PrDecisionConclusion,
    /// Individual quality-gate outcomes.
    pub gates: Vec<PrDecisionGate>,
    /// File-anchored annotations for inline display.
    pub annotations: Vec<PrDecisionAnnotation>,
    /// Longer-form report content and links.
    pub details: PrDecisionDetails,
}

/// Outcome of the overall decision or a single gate; values mirror GitHub
/// check-run conclusions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrDecisionConclusion {
    /// Gate passed.
    Success,
    /// Gate breached its threshold.
    Failure,
    /// Gate produced findings without failing.
    Neutral,
    /// Gate did not run.
    Skipped,
}

/// One quality-gate outcome inside [`PrDecisionSurface::gates`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionGate {
    /// Stable gate identifier, e.g. `duplication`.
    pub id: String,
    /// Display label for the gate.
    pub label: String,
    /// Gate outcome.
    pub status: PrDecisionConclusion,
    /// Observed value text, e.g. "9.1% on changed code".
    pub observed: String,
    /// Configured threshold text, e.g. "<= 3%", when the gate has one.
    pub threshold: Option<String>,
    /// Scope the gate evaluated, e.g. "new code".
    pub scope: String,
}

/// File-anchored annotation inside [`PrDecisionSurface::annotations`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionAnnotation {
    /// File path relative to the repository root.
    pub path: String,
    /// 1-based line the annotation points at.
    pub line: u32,
    /// Annotation display level.
    pub level: PrDecisionAnnotationLevel,
    /// Short annotation title.
    pub title: String,
    /// Annotation body text.
    pub message: String,
    /// Extra unrendered context, when available.
    pub raw_details: Option<String>,
}

/// Display level for a [`PrDecisionAnnotation`]; values mirror GitHub
/// check-run annotation levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrDecisionAnnotationLevel {
    /// Informational annotation.
    Notice,
    /// Advisory annotation.
    Warning,
    /// Gating annotation.
    Failure,
}

/// Report content and links inside [`PrDecisionSurface::details`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionDetails {
    /// Markdown body summarizing the run.
    pub summary_markdown: String,
    /// Local path to the full report artifact, when one was written.
    pub full_report_path: Option<String>,
    /// Link to hosted report output, when available.
    pub details_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_surface_serializes_stable_schema() {
        let surface = PrDecisionSurface {
            schema: PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Failure,
            gates: vec![PrDecisionGate {
                id: "duplication".to_owned(),
                label: "Duplication".to_owned(),
                status: PrDecisionConclusion::Failure,
                observed: "9.1% on changed code".to_owned(),
                threshold: Some("<= 3%".to_owned()),
                scope: "new code".to_owned(),
            }],
            annotations: vec![PrDecisionAnnotation {
                path: "src/app.ts".to_owned(),
                line: 42,
                level: PrDecisionAnnotationLevel::Warning,
                title: "Duplication".to_owned(),
                message: "Clone group found".to_owned(),
                raw_details: Some("fallow/code-duplication".to_owned()),
            }],
            details: PrDecisionDetails {
                summary_markdown: "Quality gate failed".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let json = serde_json::to_value(surface).expect("serializes");
        assert_eq!(json["schema"], PR_DECISION_SCHEMA);
        assert_eq!(json["conclusion"], "failure");
        assert_eq!(json["annotations"][0]["level"], "warning");
    }
}
