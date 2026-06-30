use crate::check::CheckResult;
use crate::dupes::DupesResult;
use crate::health::HealthResult;

use fallow_engine::clear_ambient_git_env;

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
/// - not all three analyses ran (`--only` / `--skip`): a partial count is not a
///   whole-project total;
/// - any scope narrowing (`--changed-since` / `--workspace` /
///   `--changed-workspaces`): a subset count;
/// - a process-wide diff filter is active (`--diff-file` / `--diff-stdin`,
///   read from the shared cache, NOT a `CombinedOptions` field): diff-scoped;
/// - production mode (global or per-analysis): a different finding denominator
///   that must not interleave with default-mode records.
fn is_whole_project_run(opts: &CombinedOptions<'_>) -> bool {
    let all_analyses = opts.run_check && opts.run_dupes && opts.run_health;
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

/// Get the current short git SHA, or None outside a git repo. Mirrors
/// `vital_signs`'s private helper; kept local to avoid widening its visibility.
fn combined_git_sha(root: &std::path::Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// Best-effort whole-project Impact recording. No-op unless impact is enabled
/// AND this is a genuine whole-project run. Builds whole-project counts plus a
/// `Scope::WholeProject` attribution input (sharing the same `active_suppressions`
/// snapshot audit uses, so a suppressed-but-unchanged finding is credited
/// suppressed, never resolved). Swallows everything: never touches exit/output.
pub(super) fn record_combined_impact(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) {
    if !is_whole_project_run(opts) {
        return;
    }
    let (Some(check), Some(dupes), Some(health)) = (check_result, dupes_result, health_result)
    else {
        return;
    };

    let dead_code = check.results.total_issues();
    let complexity = health.report.findings.len();
    let duplication = dupes.report.clone_groups.len();
    let counts = crate::impact::ImpactCounts::from_combined(dead_code, complexity, duplication);

    let mut findings = crate::impact::collect_dead_code_findings(&check.results);
    findings.extend(crate::impact::collect_complexity_findings(&health.report));
    let clones = crate::impact::collect_clone_findings(&dupes.report);
    let suppressions = check.results.active_suppressions.as_slice();

    let attribution = crate::impact::AttributionInput {
        root: opts.root,
        scope: crate::impact::Scope::WholeProject,
        findings,
        clones,
        suppressions,
    };

    crate::impact::record_combined_run(
        opts.root,
        counts,
        combined_git_sha(opts.root).as_deref(),
        env!("CARGO_PKG_VERSION"),
        &crate::vital_signs::chrono_timestamp(),
        Some(&attribution),
    );
}
