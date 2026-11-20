//! A minimal [`AuditResult`] for unit tests of the audit renderers.

use std::time::Duration;

use fallow_config::{AuditGate, OutputFormat};

use super::{AuditAttribution, AuditResult, AuditSummary, AuditVerdict};

/// An audit result with no finding and no changed file, compared against
/// `origin/main`.
pub fn audit_result(verdict: AuditVerdict, output: OutputFormat) -> AuditResult {
    AuditResult {
        verdict,
        summary: AuditSummary {
            dead_code_issues: 0,
            dead_code_has_errors: false,
            dead_code_skipped: false,
            complexity_skipped: false,
            duplication_skipped: false,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        },
        attribution: AuditAttribution {
            gate: AuditGate::NewOnly,
            ..AuditAttribution::default()
        },
        dupe_demotion_diff_source: None,
        base_snapshot: None,
        comparison: None,
        base_snapshot_skipped: false,
        changed_files_count: 0,
        changed_files: Vec::new(),
        base_ref: "origin/main".to_string(),
        base_description: None,
        base_sha: None,
        shallow_clone_without_base: false,
        head_sha: None,
        output,
        performance: false,
        check: None,
        dupes: None,
        health: None,
        elapsed: Duration::ZERO,
        review_deltas: None,
        weakening_signals: Vec::new(),
        diff_risk: fallow_output::DiffRiskReport::default(),
        routing: None,
        ownership: None,
        decision_surface: None,
        graph_snapshot_hash: None,
        change_anchors: Vec::new(),
        diff_index: None,
    }
}
