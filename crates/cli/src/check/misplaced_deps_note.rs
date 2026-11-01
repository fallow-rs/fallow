//! The note for a run whose exit code changed when `--unused-deps` stopped
//! selecting the placement kinds. Remove this module at the next major version.
//!
//! The upgrade can turn a failed gate into a passed gate with no other signal,
//! so the run counts the findings that the narrower selection hid and that
//! would fail the gate. A plain `--unused-deps` run counts none and stays quiet.

use fallow_config::ResolvedConfig;
use fallow_types::results::AnalysisResults;
use fallow_types::semantic::SemanticAnalysisIdentity;

use super::{CheckOptions, IssueFilters, rules};

/// Copy the placement findings when `--unused-deps` hides them from this run.
/// Call this before the issue-type filter clears them.
pub(super) fn capture_hidden(
    filters: &IssueFilters,
    results: &AnalysisResults,
) -> Option<AnalysisResults> {
    if !filters.unused_deps || filters.misplaced_deps {
        return None;
    }
    Some(AnalysisResults {
        type_only_dependencies: results.type_only_dependencies.clone(),
        test_only_dependencies: results.test_only_dependencies.clone(),
        dev_dependencies_in_production: results.dev_dependencies_in_production.clone(),
        ..AnalysisResults::default()
    })
}

/// Count the hidden findings that would fail the gate of this run, with the
/// rules and the baseline that the reported findings pass through.
pub(super) fn count_gate_findings(
    mut hidden: AnalysisResults,
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    analysis_identity: &SemanticAnalysisIdentity,
) -> usize {
    if opts.finding_ids.is_some() {
        return 0;
    }
    rules::apply_rules(&mut hidden, config);
    if let Some(content) = opts
        .baseline
        .and_then(|path| std::fs::read_to_string(path).ok())
    {
        // The reported findings already passed this baseline, so a parse error
        // or an identity error stopped the run before this point.
        let _ = fallow_engine::baseline::apply_dead_code_baseline(
            &mut hidden,
            &content,
            &config.root,
            analysis_identity,
            false,
        );
    }
    let mut severities = config.rules.clone();
    if opts.fail_on_issues {
        rules::promote_warns_to_errors(&mut severities);
    }
    rules::count_error_severity_issues(&hidden, &severities, Some(config), opts.fail_on_issues)
}

/// Print regardless of `--quiet`, like the gate lines: `--ci` implies `--quiet`,
/// and no report states this fact.
pub(super) fn print(gate_findings: usize) {
    if gate_findings == 0 {
        return;
    }
    eprintln!(
        "note: --unused-deps no longer selects misplaced dependencies. {gate_findings} hidden finding(s) would fail this gate. Add --misplaced-deps to keep the gate, or set the rule to \"off\" to remove this note."
    );
}
