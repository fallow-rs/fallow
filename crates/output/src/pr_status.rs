use serde::{Deserialize, Serialize};

use crate::{PrDecisionConclusion, PrDecisionSurface};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrStatusContext {
    pub name: String,
    pub conclusion: PrDecisionConclusion,
    pub summary: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrStatusMode {
    Aggregate,
    Split,
    AggregateAndSplit,
}

pub fn pr_status_contexts(surface: &PrDecisionSurface) -> Vec<PrStatusContext> {
    pr_status_contexts_with_mode(surface, PrStatusMode::Split)
}

pub fn pr_status_contexts_with_mode(
    surface: &PrDecisionSurface,
    mode: PrStatusMode,
) -> Vec<PrStatusContext> {
    match mode {
        PrStatusMode::Aggregate => vec![aggregate_status_context(surface)],
        PrStatusMode::Split => split_status_contexts(surface),
        PrStatusMode::AggregateAndSplit => {
            let mut contexts = vec![aggregate_status_context(surface)];
            contexts.extend(split_status_contexts(surface));
            contexts
        }
    }
}

fn split_status_contexts(surface: &PrDecisionSurface) -> Vec<PrStatusContext> {
    surface
        .gates
        .iter()
        .map(|gate| PrStatusContext {
            name: format!("Fallow / {}", gate.id),
            conclusion: gate.status,
            summary: match &gate.threshold {
                Some(threshold) => {
                    format!("{}: {} (threshold {threshold})", gate.label, gate.observed)
                }
                None => format!("{}: {}", gate.label, gate.observed),
            },
        })
        .collect()
}

fn aggregate_status_context(surface: &PrDecisionSurface) -> PrStatusContext {
    let total = surface.gates.len();
    let failing = surface
        .gates
        .iter()
        .filter(|gate| gate.status == PrDecisionConclusion::Failure)
        .count();
    let neutral = surface
        .gates
        .iter()
        .filter(|gate| gate.status == PrDecisionConclusion::Neutral)
        .count();
    let summary = if total == 0 {
        "No PR gates reported.".to_owned()
    } else if failing > 0 {
        format!("{failing} of {total} PR gates failed.")
    } else if neutral > 0 {
        format!("{neutral} of {total} PR gates need review.")
    } else {
        format!("All {total} PR gates passed.")
    };
    PrStatusContext {
        name: "Fallow".to_owned(),
        conclusion: surface.conclusion,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PR_DECISION_SCHEMA, PrDecisionDetails, PrDecisionGate, PrDecisionSurface};

    #[test]
    fn builds_status_contexts_from_decision_gates() {
        let surface = PrDecisionSurface {
            schema: PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Neutral,
            gates: vec![PrDecisionGate {
                id: "duplication".to_owned(),
                label: "Duplication".to_owned(),
                status: PrDecisionConclusion::Neutral,
                observed: "2 clone groups".to_owned(),
                threshold: Some("configured rules".to_owned()),
                scope: "new code".to_owned(),
            }],
            annotations: vec![],
            details: PrDecisionDetails {
                summary_markdown: "Review recommended.".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let contexts = pr_status_contexts(&surface);

        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].name, "Fallow / duplication");
        assert_eq!(contexts[0].conclusion, PrDecisionConclusion::Neutral);
        assert_eq!(
            contexts[0].summary,
            "Duplication: 2 clone groups (threshold configured rules)"
        );
    }

    #[test]
    fn aggregate_and_split_status_contexts_keep_one_consolidated_context() {
        let surface = PrDecisionSurface {
            schema: PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Neutral,
            gates: vec![PrDecisionGate {
                id: "health".to_owned(),
                label: "Health".to_owned(),
                status: PrDecisionConclusion::Neutral,
                observed: "1 finding".to_owned(),
                threshold: None,
                scope: "new code".to_owned(),
            }],
            annotations: vec![],
            details: PrDecisionDetails {
                summary_markdown: "Review recommended.".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let contexts = pr_status_contexts_with_mode(&surface, PrStatusMode::AggregateAndSplit);

        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].name, "Fallow");
        assert_eq!(contexts[0].summary, "1 of 1 PR gates need review.");
        assert_eq!(contexts[1].name, "Fallow / health");
    }
}
