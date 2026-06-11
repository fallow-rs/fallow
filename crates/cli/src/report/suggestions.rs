//! Command-level `next_steps[]` builder.
//!
//! Computes a small list of read-only, runnable follow-up commands from a run's
//! findings, surfaced at the JSON root (and as a one-line human `Next:` hint).
//! The purpose is to point agents and humans sideways to fallow's adjacent
//! verification capabilities (trace, complexity breakdown, audit, workspace
//! scoping) that telemetry shows agents rarely discover, because they act on the
//! output in front of them rather than on reference docs.
//!
//! Two hard contracts, both enforced by the tests in this module and by the
//! `next_step` constructor's debug assertions:
//!
//! 1. **Read-only.** A step NEVER suggests `fallow fix` or any mutating command.
//! 2. **Runnable, placeholder-free.** Every `command` runs as-is; it never
//!    contains an angle-bracket placeholder. Finding-derived values come from a
//!    real, deterministically-selected finding; values that cannot be made
//!    concrete (a coverage path) are dropped from v1 rather than shipped as a
//!    placeholder, and an unresolvable git ref omits its step entirely.

use std::path::Path;
use std::process::Command;

use fallow_core::results::AnalysisResults;
use fallow_types::output::NextStep;

use crate::health_types::HealthReport;
use crate::output_dupes::DupesReportPayload;

/// Maximum number of next-steps emitted per envelope. Keeps the array a glance,
/// not a wall; the priority order decides which survive the cap.
const MAX_NEXT_STEPS: usize = 3;

/// Mutating verbs a next-step must never suggest (the read-only contract).
const MUTATING_VERBS: [&str; 5] = ["fix", "init", "hooks", "migrate", "setup-hooks"];

/// `FALLOW_SUGGESTIONS=off` (or `0`/`false`/`no`/`disabled`) disables next-steps
/// entirely. Default on. This is the documented escape hatch for CI consumers
/// that snapshot-diff raw `--format json` output; it reaches the spawned-CLI and
/// MCP surfaces without a CLI flag.
#[must_use]
pub fn suggestions_enabled() -> bool {
    suggestions_enabled_from(std::env::var("FALLOW_SUGGESTIONS").ok().as_deref())
}

/// Pure parse of the `FALLOW_SUGGESTIONS` value (kept separate so it is testable
/// without mutating process env, which is `unsafe` under edition 2024).
#[must_use]
fn suggestions_enabled_from(value: Option<&str>) -> bool {
    match value {
        Some(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no" | "disabled"
        ),
        None => true,
    }
}

/// Construct a next-step, asserting the two contracts in debug builds so a new
/// trigger that violates them trips the test suite rather than shipping.
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

/// Project-root-relative, forward-slash path for embedding in a command string,
/// matching the wire form of finding paths.
fn relative_command_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// ---------------------------------------------------------------------------
// Individual triggers. Each returns `Some(step)` only when its evidence exists.
// ---------------------------------------------------------------------------

/// `trace-unused-export`: verify an export is truly unused before deleting.
/// Uses the lexicographically smallest `(path, name)` finding so the embedded
/// command is deterministic across runs (independent of internal vec order).
fn trace_unused_export(results: &AnalysisResults, root: &Path) -> Option<NextStep> {
    let target = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                relative_command_path(&finding.export.path, root),
                finding.export.export_name.clone(),
            )
        })
        .min()?;
    Some(next_step(
        "trace-unused-export",
        format!("fallow dead-code --trace {}:{}", target.0, target.1),
        "verify an export is truly unused before deleting",
    ))
}

/// `trace-clone`: see sibling locations and an extract-function suggestion for a
/// duplicated block. Uses the smallest fingerprint for run-to-run determinism.
fn trace_clone(payload: &DupesReportPayload) -> Option<NextStep> {
    let fingerprint = payload
        .clone_groups
        .iter()
        .map(|group| group.fingerprint.as_str())
        .min()?;
    Some(next_step(
        "trace-clone",
        format!("fallow dupes --trace {fingerprint}"),
        "see sibling locations and an extract-function suggestion",
    ))
}

/// `complexity-breakdown`: see the per-decision-point contributions behind a
/// high-complexity finding.
fn complexity_breakdown(report: &HealthReport) -> Option<NextStep> {
    if report.findings.is_empty() {
        return None;
    }
    Some(next_step(
        "complexity-breakdown",
        "fallow health --complexity-breakdown".to_string(),
        "see per-decision-point contributions for a hotspot",
    ))
}

/// `scope-workspaces`: scope a monorepo run to the packages touched since the
/// default branch. Emitted only when the project is a monorepo AND a concrete
/// default ref resolves, so the embedded ref is real (never a placeholder).
fn scope_workspaces(root: &Path) -> Option<NextStep> {
    if fallow_config::discover_workspaces(root).is_empty() {
        return None;
    }
    let reference = resolve_default_workspace_ref(root)?;
    Some(next_step(
        "scope-workspaces",
        format!("fallow dead-code --changed-workspaces {reference}"),
        "scope a monorepo run to the packages your branch touched",
    ))
}

/// `audit-changed`: gate only the files the current branch changed. `fallow
/// audit` auto-detects its base, so no ref needs embedding.
fn audit_changed(root: &Path) -> Option<NextStep> {
    if !fallow_core::churn::is_git_repo(root) {
        return None;
    }
    Some(next_step(
        "audit-changed",
        "fallow audit".to_string(),
        "gate only the files your branch changed (auto-detects the base)",
    ))
}

// ---------------------------------------------------------------------------
// Git ref resolution (self-contained; keeps `scope-workspaces` placeholder-free)
// ---------------------------------------------------------------------------

/// Resolve a concrete, human-readable default ref for `--changed-workspaces`.
/// Tries `origin/HEAD` then verifies `origin/main` / `origin/master`. Returns
/// the first that resolves, or `None` (in which case `scope-workspaces` is
/// omitted rather than shipping an unrunnable `origin/main` guess).
fn resolve_default_workspace_ref(root: &Path) -> Option<String> {
    if let Some(reference) = run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        let reference = reference.trim();
        if !reference.is_empty() {
            return Some(reference.to_string());
        }
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_string)
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(["rev-parse", "--verify", "--quiet", reference])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

// ---------------------------------------------------------------------------
// Public per-command builders. Each no-ops when suggestions are disabled or the
// run is clean (no findings), so a clean run never emits `next_steps`.
// ---------------------------------------------------------------------------

/// Next-steps for standalone `fallow dead-code`.
#[must_use]
pub fn build_dead_code_next_steps(results: &AnalysisResults, root: &Path) -> Vec<NextStep> {
    if !suggestions_enabled() || results.total_issues() == 0 {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [
        trace_unused_export(results, root),
        scope_workspaces(root),
        audit_changed(root),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for standalone `fallow health`.
#[must_use]
pub fn build_health_next_steps(report: &HealthReport, root: &Path) -> Vec<NextStep> {
    if !suggestions_enabled() || report.findings.is_empty() {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [complexity_breakdown(report), audit_changed(root)]
        .into_iter()
        .flatten()
        .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for standalone `fallow dupes`.
#[must_use]
pub fn build_dupes_next_steps(payload: &DupesReportPayload, root: &Path) -> Vec<NextStep> {
    if !suggestions_enabled() || payload.clone_groups.is_empty() {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [trace_clone(payload), audit_changed(root)]
        .into_iter()
        .flatten()
        .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Aggregated next-steps for bare `fallow` (combined). Candidates are pushed in
/// priority order, then capped. `trace-unused-export` leads because it is the
/// highest-value verification path; `scope-workspaces` is boosted above the
/// trace-clone / complexity tier so big-repo runs that trigger everything still
/// surface the rare monorepo-scoping capability instead of always dropping it
/// under the cap. `audit-changed` is last (broadly applicable, least specific).
#[must_use]
pub fn build_combined_next_steps(
    results: Option<&AnalysisResults>,
    dupes: Option<&DupesReportPayload>,
    health: Option<&HealthReport>,
    root: &Path,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    let has_findings = results.is_some_and(|r| r.total_issues() > 0)
        || dupes.is_some_and(|d| !d.clone_groups.is_empty())
        || health.is_some_and(|h| !h.findings.is_empty());
    if !has_findings {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [
        results.and_then(|r| trace_unused_export(r, root)),
        scope_workspaces(root),
        dupes.and_then(trace_clone),
        health.and_then(complexity_breakdown),
        audit_changed(root),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// Next-steps for `fallow audit`. No `audit-changed` (audit IS the changed
/// scope) and no `scope-workspaces` (audit already gates the change). The
/// `check` tuple carries the changed-file analysis results plus the project root
/// so the trace anchor is made root-relative the same way every other surface
/// does it (in-memory finding paths are absolute; the wire form is relative).
#[must_use]
pub fn build_audit_next_steps(
    check: Option<(&AnalysisResults, &Path)>,
    complexity: Option<&HealthReport>,
) -> Vec<NextStep> {
    if !suggestions_enabled() {
        return Vec::new();
    }
    let mut steps: Vec<NextStep> = [
        check.and_then(|(results, root)| trace_unused_export(results, root)),
        complexity.and_then(complexity_breakdown),
    ]
    .into_iter()
    .flatten()
    .collect();
    steps.truncate(MAX_NEXT_STEPS);
    steps
}

/// The single highest-priority next-step for the human `Next:` line, computed
/// from the same candidates and ordering as the combined JSON array so a human
/// and an agent on the same run never see a contradictory top step.
#[must_use]
pub fn top_combined_next_step(
    results: Option<&AnalysisResults>,
    dupes: Option<&DupesReportPayload>,
    health: Option<&HealthReport>,
    root: &Path,
) -> Option<NextStep> {
    build_combined_next_steps(results, dupes, health, root)
        .into_iter()
        .next()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::output_dead_code::UnusedExportFinding;
    use fallow_types::results::{AnalysisResults, UnusedExport};

    use super::*;

    fn unused_export(path: &str, name: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    fn results_with_exports(exports: Vec<UnusedExportFinding>) -> AnalysisResults {
        AnalysisResults {
            unused_exports: exports,
            ..AnalysisResults::default()
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
    fn trace_unused_export_emits_runnable_relative_command() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/util.ts", "foo")]);
        let step = trace_unused_export(&results, &root).expect("step");
        assert_eq!(step.id, "trace-unused-export");
        assert_eq!(step.command, "fallow dead-code --trace src/util.ts:foo");
        assert_valid(&step);
    }

    #[test]
    fn trace_unused_export_is_deterministic_regardless_of_vec_order() {
        let root = PathBuf::from("/project");
        let forward = results_with_exports(vec![
            unused_export("/project/src/b.ts", "beta"),
            unused_export("/project/src/a.ts", "alpha"),
        ]);
        let reverse = results_with_exports(vec![
            unused_export("/project/src/a.ts", "alpha"),
            unused_export("/project/src/b.ts", "beta"),
        ]);
        let a = trace_unused_export(&forward, &root).expect("step");
        let b = trace_unused_export(&reverse, &root).expect("step");
        assert_eq!(a.command, b.command);
        assert_eq!(a.command, "fallow dead-code --trace src/a.ts:alpha");
    }

    #[test]
    fn clean_run_emits_no_next_steps() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        assert!(build_dead_code_next_steps(&results, &root).is_empty());
    }

    #[test]
    fn suggestions_enabled_parses_off_values() {
        for off in ["off", "0", "false", "no", "disabled", "OFF", " Off "] {
            assert!(!suggestions_enabled_from(Some(off)), "{off} should disable");
        }
        for on in ["on", "1", "true", "", "yes"] {
            assert!(suggestions_enabled_from(Some(on)), "{on} should enable");
        }
        assert!(suggestions_enabled_from(None), "default is enabled");
    }

    #[test]
    fn every_emitted_command_is_runnable_and_read_only() {
        // Exercise every data-driven trigger and assert both contracts.
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        let mut all = Vec::new();
        all.extend(trace_unused_export(&results, &root));
        // Static-command triggers (no findings needed to inspect the string).
        all.push(next_step("audit-changed", "fallow audit".to_string(), "x"));
        all.push(next_step(
            "scope-workspaces",
            "fallow dead-code --changed-workspaces origin/main".to_string(),
            "x",
        ));
        all.push(next_step(
            "complexity-breakdown",
            "fallow health --complexity-breakdown".to_string(),
            "x",
        ));
        all.push(next_step(
            "trace-clone",
            "fallow dupes --trace dup:abcd1234".to_string(),
            "x",
        ));
        assert!(!all.is_empty());
        for step in &all {
            assert_valid(step);
        }
    }

    #[test]
    fn dead_code_steps_capped_at_three() {
        let root = PathBuf::from("/project");
        let results = results_with_exports(vec![unused_export("/project/src/a.ts", "alpha")]);
        // Even if git/workspaces add candidates, the cap holds.
        let steps = build_dead_code_next_steps(&results, &root);
        assert!(steps.len() <= MAX_NEXT_STEPS);
    }
}
