//! Comparison of the head run with the base snapshot, and the verdict.

use std::path::Path;

use fallow_config::{AuditGate, RulesConfig, Severity};

use super::{AuditAnalysesView, AuditKeySnapshot};
use crate::audit_keys::{
    AuditComparison, AuditDomainLedger, DeadCodeAuditLedger, dead_code_audit_ledger,
    dupe_group_key, health_finding_key, preexisting_dupe_group_keys, styling_finding_key,
};
use crate::{AuditAttribution, AuditSummary, AuditVerdict};

/// Which diff decided the new-only duplication demotion, so output can name
/// where a demotion came from (issue #2220).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DupeDemotionDiffSource {
    /// The opt-in shared diff index decided. Carries the user-facing source
    /// label (`--diff-file <path>`, `--diff-stdin`, or
    /// `$FALLOW_DIFF_FILE <path>`).
    Shared(String),
    /// The merge-base worktree diff against the resolved base ref decided.
    Worktree,
    /// No diff was available. The demotion check did not run, and every
    /// introduced clone group kept its gate.
    Skipped,
}

impl DupeDemotionDiffSource {
    /// User-facing label for the diff that decided the demotion.
    #[must_use]
    pub fn label(&self, base_ref: &str) -> String {
        match self {
            Self::Shared(label) => label.clone(),
            Self::Worktree => format!("merge-base worktree diff vs {base_ref}"),
            Self::Skipped => "skipped: no diff available".to_string(),
        }
    }
}

/// An opt-in shared diff that the run already applied, with its source label.
#[derive(Clone, Copy)]
pub struct SharedDiff<'a> {
    /// The parsed diff.
    pub index: &'a fallow_output::DiffIndex,
    /// The user-facing source label, for example `--diff-file <path>`.
    pub label: &'a str,
}

/// Classify every head finding once against the base snapshot.
///
/// With `syntactic_fallback` set (the degraded type-aware path), dead code
/// compares against the syntactic keys of the base, and a head finding that
/// only semantic evidence produced stays advisory: it has no syntactic base
/// counterpart to compare with.
#[must_use]
pub fn compare(
    view: &AuditAnalysesView<'_>,
    base: Option<&AuditKeySnapshot>,
    syntactic_fallback: bool,
) -> AuditComparison {
    let dead_code =
        view.dead_code
            .as_ref()
            .map_or_else(DeadCodeAuditLedger::default, |dead_code| {
                // A base that ran without type-aware analysis is already
                // syntactic, so its refined set is also its fallback set.
                let base_keys = base.map(|snapshot| {
                    if syntactic_fallback {
                        snapshot
                            .syntactic_dead_code
                            .as_ref()
                            .unwrap_or(&snapshot.dead_code)
                    } else {
                        &snapshot.dead_code
                    }
                });
                let mut ledger = dead_code_audit_ledger(
                    dead_code.results,
                    dead_code.root,
                    dead_code.config,
                    base_keys,
                );
                if syntactic_fallback && let Some(head_syntactic) = dead_code.syntactic_keys {
                    ledger.demote_unattributable_introductions(head_syntactic);
                }
                ledger
            });
    let health = AuditDomainLedger::compare(
        view.health.iter().flat_map(|health| {
            health
                .report
                .findings
                .iter()
                .map(|finding| health_finding_key(finding, health.root))
        }),
        base.map(|snapshot| &snapshot.health),
    );
    let dupes = AuditDomainLedger::compare(
        view.duplication.iter().flat_map(|duplication| {
            duplication
                .clone_groups
                .iter()
                .map(|group| dupe_group_key(group, duplication.root))
        }),
        base.map(|snapshot| &snapshot.dupes),
    );
    let styling = AuditDomainLedger::compare(
        view.health.iter().flat_map(|health| {
            health
                .report
                .styling_findings
                .iter()
                .map(|finding| styling_finding_key(finding, health.root))
        }),
        base.map(|snapshot| &snapshot.styling),
    );
    AuditComparison {
        dead_code,
        health,
        dupes,
        styling,
    }
}

/// Demote introduced clone groups that hold no added line of the run's diff.
///
/// No instance range holds an added line, so the changeset did not write the
/// duplicated text. Only the attribution key of the group changed, because the
/// changeset removed code in another place. Without this step, a clone-removal
/// refactor fails the new-only gate on duplication that it did not write
/// (issue #2164). The opt-in shared diff decides when it is present; the
/// merge-base worktree diff decides in the other cases.
pub fn demote_preexisting_dupe_introductions(
    comparison: &mut AuditComparison,
    view: &AuditAnalysesView<'_>,
    root: &Path,
    base_ref: &str,
    shared: Option<SharedDiff<'_>>,
) -> Option<DupeDemotionDiffSource> {
    if comparison.dupes.introduced_count() == 0 {
        return None;
    }
    let duplication = view.duplication.as_ref()?;
    let worktree_index;
    let (index, source) = if let Some(shared) = shared {
        (
            shared.index,
            DupeDemotionDiffSource::Shared(shared.label.to_owned()),
        )
    } else if let Ok(diff) = fallow_engine::changed_files::try_get_changed_diff(root, base_ref) {
        worktree_index = fallow_output::DiffIndex::from_unified_diff(&diff);
        (&worktree_index, DupeDemotionDiffSource::Worktree)
    } else {
        return Some(DupeDemotionDiffSource::Skipped);
    };
    let demote = preexisting_dupe_group_keys(
        duplication.clone_groups.iter().copied(),
        duplication.root,
        index,
    );
    comparison.dupes.demote_introductions(&demote);
    Some(source)
}

/// The severity of the rule that owns a styling finding `code`. Styling is
/// verdict-neutral by default (rule `warn`).
#[must_use]
pub fn styling_rule_severity(rules: &RulesConfig, code: &str) -> Severity {
    match code {
        "css-token-drift" => rules.css_token_drift,
        "css-duplicate-block" => rules.css_duplicate_block,
        "css-selector-complexity" => rules.css_selector_complexity,
        "css-dead-surface" => rules.css_dead_surface,
        "css-broken-reference" => rules.css_broken_reference,
        _ => Severity::Warn,
    }
}

/// Whether a styling finding escalates to `error` and so gates the verdict.
#[must_use]
pub fn styling_finding_gates(rules: &RulesConfig, code: &str) -> bool {
    styling_rule_severity(rules, code) == Severity::Error
}

/// Attribution counts, verdict and summary of one comparison.
#[must_use]
pub fn outcome(
    gate: AuditGate,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
    has_base: bool,
) -> (AuditAttribution, AuditVerdict, AuditSummary) {
    let summary = summary(view, comparison);
    (
        attribution(gate, comparison, has_base),
        verdict(gate, view, comparison, &summary),
        summary,
    )
}

fn verdict(
    gate: AuditGate,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
    summary: &AuditSummary,
) -> AuditVerdict {
    let new_only = matches!(gate, AuditGate::NewOnly);
    let dead_code_errors = if new_only {
        comparison.dead_code.has_introduced_errors()
    } else {
        comparison.dead_code.has_errors()
    };
    let dead_code_warnings = if new_only {
        comparison.dead_code.has_introduced_warnings()
    } else {
        comparison
            .dead_code
            .records()
            .iter()
            .any(|record| record.effective_severity == Severity::Warn)
    };
    // The `complexity-*` rules decide if a finding blocks: `error` fails the
    // verdict and `warn` gives `warn`. The `new-only` gate reads only the
    // introduced findings.
    let (complexity_errors, complexity_warnings) =
        view.health.as_ref().map_or((false, false), |health| {
            health
                .report
                .findings
                .iter()
                .zip(comparison.health.introduced())
                .filter(|(_, introduced)| !new_only || *introduced)
                .fold((false, false), |(errors, warnings), (finding, _)| {
                    if finding.blocks() {
                        (true, warnings)
                    } else {
                        (errors, true)
                    }
                })
        });
    let styling_errors = view.health.as_ref().is_some_and(|health| {
        health
            .report
            .styling_findings
            .iter()
            .zip(comparison.styling.introduced())
            .any(|(finding, introduced)| {
                (!new_only || introduced) && styling_finding_gates(health.rules, &finding.code)
            })
    });
    let duplication_findings = if new_only {
        comparison.dupes.introduced_count()
    } else {
        summary.duplication_clone_groups
    };
    let duplication_errors = view.duplication.as_ref().is_some_and(|duplication| {
        duplication_findings > 0
            && duplication.threshold > 0.0
            && duplication.duplication_percentage > duplication.threshold
    });
    if dead_code_errors || complexity_errors || styling_errors || duplication_errors {
        AuditVerdict::Fail
    } else if dead_code_warnings || complexity_warnings || duplication_findings > 0 {
        AuditVerdict::Warn
    } else {
        AuditVerdict::Pass
    }
}

fn attribution(gate: AuditGate, comparison: &AuditComparison, has_base: bool) -> AuditAttribution {
    if !has_base {
        return AuditAttribution {
            gate,
            ..AuditAttribution::default()
        };
    }
    AuditAttribution {
        gate,
        dead_code_introduced: comparison.dead_code.introduced_count(),
        dead_code_inherited: comparison.dead_code.inherited_count(),
        complexity_introduced: comparison.health.introduced_count(),
        complexity_inherited: comparison.health.inherited_count(),
        duplication_introduced: comparison.dupes.introduced_count(),
        duplication_inherited: comparison.dupes.inherited_count(),
    }
}

fn summary(view: &AuditAnalysesView<'_>, comparison: &AuditComparison) -> AuditSummary {
    AuditSummary {
        dead_code_issues: comparison.dead_code.visible_count(),
        dead_code_has_errors: comparison.dead_code.has_errors(),
        complexity_findings: view
            .health
            .as_ref()
            .map_or(0, |health| health.report.findings.len()),
        max_cyclomatic: view.health.as_ref().and_then(|health| {
            health
                .report
                .findings
                .iter()
                .map(|finding| finding.cyclomatic)
                .max()
        }),
        duplication_clone_groups: view
            .duplication
            .as_ref()
            .map_or(0, |duplication| duplication.clone_groups.len()),
    }
}
