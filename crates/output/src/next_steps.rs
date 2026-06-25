//! Pure builders for JSON `next_steps[]` entries.
//!
//! Runtime probes stay with callers. This module owns the stable command,
//! ordering, capping, and read-only contracts once a caller has already decided
//! which signals apply.

use fallow_types::output::NextStep;

const MAX_NEXT_STEPS: usize = 3;
const MUTATING_VERBS: [&str; 5] = ["fix", "init", "hooks", "migrate", "setup-hooks"];

/// Local impact digest counters used to render the `impact-report` next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactDigestCounts {
    pub containment_count: usize,
    pub resolved_total: usize,
}

/// Runtime-independent inputs for standalone health next steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthNextStepsInput {
    pub suggestions_enabled: bool,
    pub has_findings: bool,
    pub offer_setup: bool,
    pub impact_digest: Option<ImpactDigestCounts>,
    pub audit_changed: bool,
}

/// Render the human-readable impact counter summary shared by JSON and human
/// output surfaces.
#[must_use]
pub fn impact_digest_summary(digest: ImpactDigestCounts) -> String {
    let mut parts = Vec::new();
    if digest.containment_count > 0 {
        parts.push(format!(
            "{} commit{} contained at the gate",
            digest.containment_count,
            if digest.containment_count == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if digest.resolved_total > 0 {
        parts.push(format!(
            "{} finding{} resolved",
            digest.resolved_total,
            if digest.resolved_total == 1 { "" } else { "s" }
        ));
    }
    parts.join(", ")
}

/// Next-steps for standalone `fallow health`.
#[must_use]
pub fn build_health_next_steps(input: HealthNextStepsInput) -> Vec<NextStep> {
    if !input.suggestions_enabled {
        return Vec::new();
    }
    if !input.has_findings {
        return impact_digest_step(input.impact_digest)
            .into_iter()
            .collect();
    }

    let mut steps: Vec<NextStep> = [
        setup_pointer(input.offer_setup),
        impact_digest_step(input.impact_digest),
        complexity_breakdown(input.has_findings),
        audit_changed(input.audit_changed),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

fn next_step(id: &str, command: String, reason: &str) -> NextStep {
    debug_assert!(
        !command.contains('<') && !command.contains('>'),
        "next-step command must be runnable (no placeholder): {command}"
    );
    debug_assert!(
        !command
            .split_whitespace()
            .any(|token| MUTATING_VERBS.contains(&token)),
        "next-step command must be read-only (no mutating verb): {command}"
    );
    NextStep {
        id: id.to_string(),
        command,
        reason: reason.to_string(),
    }
}

fn setup_pointer(offer_setup: bool) -> Option<NextStep> {
    if !offer_setup {
        return None;
    }
    Some(next_step(
        "setup",
        "fallow schema".to_string(),
        "fallow has no config here; the manifest lists guided-setup commands (agent guide, commit gate) to offer the user",
    ))
}

fn impact_digest_step(digest: Option<ImpactDigestCounts>) -> Option<NextStep> {
    let digest = digest?;
    Some(next_step(
        "impact-report",
        "fallow impact".to_string(),
        &format!(
            "local value report: {}; share the non-zero numbers with the user",
            impact_digest_summary(digest)
        ),
    ))
}

fn complexity_breakdown(has_findings: bool) -> Option<NextStep> {
    if !has_findings {
        return None;
    }
    Some(next_step(
        "complexity-breakdown",
        "fallow health --complexity-breakdown".to_string(),
        "see per-decision-point contributions for a hotspot",
    ))
}

fn audit_changed(applicable: bool) -> Option<NextStep> {
    if !applicable {
        return None;
    }
    Some(next_step(
        "audit-changed",
        "fallow audit".to_string(),
        "gate only the files your branch changed (auto-detects the base)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest(containment_count: usize, resolved_total: usize) -> ImpactDigestCounts {
        ImpactDigestCounts {
            containment_count,
            resolved_total,
        }
    }

    fn dirty_input() -> HealthNextStepsInput {
        HealthNextStepsInput {
            suggestions_enabled: true,
            has_findings: true,
            offer_setup: false,
            impact_digest: None,
            audit_changed: false,
        }
    }

    fn assert_valid(step: &NextStep) {
        assert!(
            !step.command.contains('<') && !step.command.contains('>'),
            "command must be placeholder-free: {}",
            step.command
        );
        assert!(
            !step
                .command
                .split_whitespace()
                .any(|token| MUTATING_VERBS.contains(&token)),
            "command must be read-only: {}",
            step.command
        );
    }

    #[test]
    fn health_steps_are_empty_when_suggestions_are_disabled() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            suggestions_enabled: false,
            has_findings: true,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
        });

        assert!(steps.is_empty());
    }

    #[test]
    fn clean_health_run_emits_only_due_impact_digest() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            suggestions_enabled: true,
            has_findings: false,
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
        });

        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].id, "impact-report");
        assert_valid(&steps[0]);
    }

    #[test]
    fn dirty_health_run_orders_setup_impact_complexity_then_audit() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            offer_setup: true,
            impact_digest: Some(digest(2, 1)),
            audit_changed: true,
            ..dirty_input()
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["setup", "impact-report", "complexity-breakdown"]);
        for step in &steps {
            assert_valid(step);
        }
    }

    #[test]
    fn dirty_health_run_uses_complexity_when_setup_and_impact_are_absent() {
        let steps = build_health_next_steps(HealthNextStepsInput {
            audit_changed: true,
            ..dirty_input()
        });
        let ids = steps
            .iter()
            .map(|step| step.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["complexity-breakdown", "audit-changed"]);
    }

    #[test]
    fn impact_digest_summary_pluralizes_real_counters() {
        assert_eq!(
            impact_digest_summary(digest(1, 1)),
            "1 commit contained at the gate, 1 finding resolved"
        );
        assert_eq!(
            impact_digest_summary(digest(2, 3)),
            "2 commits contained at the gate, 3 findings resolved"
        );
    }
}
