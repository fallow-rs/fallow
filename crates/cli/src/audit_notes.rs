//! Notes that an audit prints on stderr after its report, also under
//! `--quiet`. Each note names a flag or a scope that the run could not judge,
//! so a CI job that reads only the verdict does not take a pass as evidence.

use super::{AuditOptions, AuditResult, AuditTypeAwareOptions};

/// Print every note that applies to this run.
pub(super) fn print_run_notes(
    opts: &AuditOptions<'_>,
    type_aware: AuditTypeAwareOptions<'_>,
    result: &AuditResult,
) {
    note_stale_baseline_gate_inert(opts);
    note_type_aware_require_inert(opts, type_aware);
    note_empty_audit_scope(result);
}

/// Say once that `--fail-on-stale-baseline` cannot fire on `fallow audit`.
///
/// Audit always narrows every sub-pass to the files that changed against its
/// base ref, so a whole-project baseline matches less of the run for reasons
/// that are not rot; gating on that would fail every review job that loads a
/// baseline. Accepting the flag in silence is the worse option, because a job
/// that believes it gates would stay green forever, so the run names the
/// reason. It is printed here rather than forwarded into the three sub-passes
/// so that an audit with three baselines says it once, not three times.
fn note_stale_baseline_gate_inert(opts: &AuditOptions<'_>) {
    if !opts.fail_on_stale_baseline {
        return;
    }
    if opts.dead_code_baseline.is_none()
        && opts.health_baseline.is_none()
        && opts.dupes_baseline.is_none()
    {
        return;
    }
    eprintln!(
        "Note: --fail-on-stale-baseline did not run: `fallow audit` analyzes only the files that \
         changed against its base, which cannot judge a whole-project baseline. Run the gate on \
         `fallow dead-code`, `fallow dupes` or `fallow health` instead."
    );
}

/// Type-aware analysis feeds only the dead-code pass, so a skipped pass leaves
/// `--type-aware-require` nothing to judge. Say so instead of passing silently.
fn note_type_aware_require_inert(opts: &AuditOptions<'_>, type_aware: AuditTypeAwareOptions<'_>) {
    if opts.analyses.dead_code || type_aware.require.is_none() {
        return;
    }
    eprintln!(
        "Note: --type-aware-require did not run: this audit skips the dead-code sub-analysis \
         (--only, --skip or the audit config), which is the only one that uses type-aware analysis."
    );
}

/// An audit with no changed file judges nothing, so its verdict is no
/// evidence. A CI job on a push to the default branch causes this: the
/// auto-detected base is the branch itself, which is HEAD. The note goes to
/// stderr also under `--quiet`, because a job that reads only the JSON verdict
/// otherwise sees a pass.
fn note_empty_audit_scope(result: &AuditResult) {
    if result.changed_files_count != 0 {
        return;
    }
    eprintln!(
        "Note: the audit compared no changed files against {}, so its verdict checked nothing. \
         In CI, run it on pull or merge requests, or pass --base <ref> with the ref to compare \
         with.",
        result.base_ref
    );
}
