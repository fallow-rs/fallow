use fallow_config::ResolvedConfig;

/// Remove issues whose effective severity is `Off` from the results.
///
/// The policy itself lives in `fallow_engine::dead_code` so every surface that
/// reports findings resolves rule severity through the same code.
pub fn apply_rules(results: &mut fallow_types::results::AnalysisResults, config: &ResolvedConfig) {
    fallow_engine::dead_code::apply_rule_severities(results, config);
}

pub use fallow_engine::error_severity::{
    count_error_severity_issues, promote_policy_finding_warns, promote_warns_to_errors,
};

/// Promote every per-finding `warn` severity to `error` for a strict
/// (fail-on-issues) run: the policy severities and the gate severity that the
/// CI formats read. Run it again after each rule pass, because a rule pass
/// writes the gate severities from the config again.
pub fn promote_finding_warns(results: &mut fallow_types::results::AnalysisResults) {
    promote_policy_finding_warns(results);
    fallow_engine::dead_code::promote_effective_warns(results);
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
