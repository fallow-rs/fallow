//! Records a `fallow audit` run in the Fallow Impact store.

use std::path::Path;

use super::{AuditAnalysisSelection, AuditOptions, AuditResult};

/// Record the run in the Impact store of `host_root`. Finding paths stay
/// relative to `opts.root`, the analyzed tree.
pub(super) fn record_audit_impact(
    opts: &AuditOptions<'_>,
    host_root: &Path,
    gate_marker: Option<&str>,
    result: &AuditResult,
) -> Result<(), String> {
    // A skipped sub-analysis reports no findings, which Impact would credit as
    // resolved. Like the bare run, only a full selection writes Impact.
    // Duplication that `duplicates.enabled: false` turned off is no skip of
    // one run: the project has no duplication analysis, and its other
    // findings still need a history.
    let full_without_dupes = AuditAnalysisSelection {
        dupes: true,
        ..opts.analyses
    } == AuditAnalysisSelection::ALL;
    if !full_without_dupes || !(opts.analyses.dupes || duplication_is_off_in_config(opts)) {
        return Ok(());
    }
    let mut findings = result
        .check
        .as_ref()
        .map(|c| crate::impact::collect_dead_code_findings(&c.results))
        .unwrap_or_default();
    if let Some(health) = result.health.as_ref() {
        findings.extend(crate::impact::collect_complexity_findings(&health.report));
    }
    let clones = result
        .dupes
        .as_ref()
        .map(|d| crate::impact::collect_clone_findings(&d.report))
        .unwrap_or_default();
    let empty_supps: Vec<fallow_types::results::ActiveSuppression> = Vec::new();
    let suppressions = result.check.as_ref().map_or(empty_supps.as_slice(), |c| {
        c.results.active_suppressions.as_slice()
    });
    let attribution = crate::impact::AttributionInput {
        root: opts.root,
        scope: crate::impact::Scope::ChangedFiles(&result.changed_files),
        findings,
        clones,
        suppressions,
    };
    let analysis_identity = result
        .check
        .as_ref()
        .and_then(|check| check.type_aware_meta.as_ref())
        .and_then(|meta| meta.identity.clone())
        .unwrap_or_default();
    crate::impact::record_audit_run(
        host_root,
        &result.summary,
        &crate::impact::AuditRunRecord {
            verdict: result.verdict,
            gate_source: gate_marker.map(crate::impact::GateSource::from_marker),
            git_sha: result.head_sha.as_deref(),
            version: env!("CARGO_PKG_VERSION"),
            timestamp: &crate::vital_signs::chrono_timestamp(),
            attribution: Some(&attribution),
            analysis_identity: &analysis_identity,
        },
    )
}

/// Whether the config of the audited project has `duplicates.enabled: false`.
/// A config that does not load counts as on, so the run records nothing.
fn duplication_is_off_in_config(opts: &AuditOptions<'_>) -> bool {
    crate::load_config_for_analysis(
        opts.root,
        opts.config_path,
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: None,
            quiet: true,
            allow_remote_extends: opts.allow_remote_extends,
        },
        fallow_config::ProductionAnalysis::Dupes,
    )
    .is_ok_and(|config| !config.duplicates.runs_by_default())
}
