use crate::audit::AuditAnalysisSelection;
use crate::check::CheckResult;
use crate::dupes::DupesResult;
use crate::health::HealthResult;

use fallow_engine::repo_refs::short_head_sha;

use super::CombinedOptions;

pub(super) fn record_combined_cache_state(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
) {
    if opts.no_cache {
        crate::telemetry::note_cache_state_unknown();
        return;
    }

    let Some(timings) = check_result.and_then(|check| check.timings.as_ref()) else {
        crate::telemetry::note_cache_state_unknown();
        return;
    };

    crate::telemetry::note_cache_state(timings.cache_hits, timings.cache_misses);
}

/// Whether this combined invocation is a true WHOLE-PROJECT run, the only shape
/// allowed to write the Impact project track. Every narrowing that would make
/// the counts not a stable whole-project denominator disqualifies it:
/// - the selection of analyses is not full by
///   [`AuditAnalysisSelection::records_impact`], the rule that `fallow audit`
///   also uses. `duplicates_off_in_config` is true when
///   `duplicates.enabled: false` is in the config, whatever `--only` and
///   `--skip` select;
/// - any scope narrowing (`--changed-since` / `--workspace` /
///   `--changed-workspaces`): a subset count;
/// - a process-wide diff filter is active (`--diff-file` / `--diff-stdin`,
///   read from the shared cache, NOT a `CombinedOptions` field): diff-scoped;
/// - production mode (global or per-analysis): a different finding denominator
///   that must not interleave with default-mode records.
fn is_whole_project_run(opts: &CombinedOptions<'_>, duplicates_off_in_config: bool) -> bool {
    let all_analyses = AuditAnalysisSelection {
        dead_code: opts.run_check,
        health: opts.run_health,
        dupes: opts.run_dupes,
        architecture: opts.architecture,
        duplicates_off_in_config,
    }
    .records_impact();
    let no_scope_narrowing = opts.changed_since.is_none()
        && opts.workspace.is_none()
        && opts.changed_workspaces.is_none();
    let no_diff_filter = crate::report::ci::diff_filter::shared_diff_index().is_none();
    let no_production = !opts.production
        && opts.production_dead_code != Some(true)
        && opts.production_health != Some(true)
        && opts.production_dupes != Some(true);
    all_analyses && no_scope_narrowing && no_diff_filter && no_production
}

/// Best-effort whole-project Impact recording. No-op unless impact is enabled
/// AND this is a genuine whole-project run. Builds whole-project counts plus a
/// `Scope::WholeProject` attribution input (sharing the same `active_suppressions`
/// snapshot audit uses, so a suppressed-but-unchanged finding is credited
/// suppressed, never resolved). Incompatible identities skip the write so local
/// value tracking can never change analysis output or exit behavior.
pub(super) fn record_combined_impact(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) -> Result<(), String> {
    let (Some(check), Some(health)) = (check_result, health_result) else {
        return Ok(());
    };
    let duplicates_off_in_config = !check.config.duplicates.runs_by_default();
    if !is_whole_project_run(opts, duplicates_off_in_config) {
        return Ok(());
    }
    if dupes_result.is_none() && !duplicates_off_in_config {
        return Ok(());
    }
    let dupes_report = dupes_result.map(|dupes| &dupes.report);

    let dead_code = check.results.total_issues();
    let complexity = health.report.findings.len();
    let duplication = dupes_report.map_or(0, |report| report.clone_groups.len());
    let counts = crate::impact::ImpactCounts::from_combined(dead_code, complexity, duplication);

    let mut findings = crate::impact::collect_dead_code_findings(&check.results);
    findings.extend(crate::impact::collect_complexity_findings(&health.report));
    let clones = dupes_report.map(crate::impact::collect_clone_findings);
    let suppressions = check.results.active_suppressions.as_slice();

    let attribution = crate::impact::AttributionInput {
        root: opts.root,
        scope: crate::impact::Scope::WholeProject,
        findings,
        clones,
        suppressions,
    };

    let analysis_identity = check
        .type_aware_meta
        .as_ref()
        .and_then(|meta| meta.identity.clone())
        .unwrap_or_default();
    crate::impact::record_combined_run(
        opts.root,
        counts,
        short_head_sha(opts.root).as_deref(),
        env!("CARGO_PKG_VERSION"),
        &crate::vital_signs::chrono_timestamp(),
        Some(&attribution),
        &analysis_identity,
    )
}
