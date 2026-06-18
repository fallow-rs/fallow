use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};
use fallow_types::results::{PolicyRuleKind, PolicyViolationSeverity};

use crate::common::fixture_path;

/// Resolve the rule-packs fixture with the team-policy pack loaded from disk
/// (the same path `resolve()` takes for a real `rulePacks` config entry).
fn fixture_config(rule_packs: Vec<String>) -> fallow_config::ResolvedConfig {
    FallowConfig {
        entry: vec!["src/index.ts".to_string()],
        rules: RulesConfig {
            policy_violation: Severity::Warn,
            ..RulesConfig::default()
        },
        rule_packs,
        ..Default::default()
    }
    .resolve(
        fixture_path("rule-packs"),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    )
}

#[test]
fn rule_pack_reports_banned_calls_imports_and_effects_end_to_end() {
    let config = fixture_config(vec!["packs/team-policy.jsonc".to_string()]);
    assert_eq!(config.rule_packs.len(), 1, "pack should load from disk");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let by_rule: Vec<(&str, &str, PolicyRuleKind, PolicyViolationSeverity, String)> = results
        .policy_violations
        .iter()
        .map(|f| {
            (
                f.violation.rule_id.as_str(),
                f.violation.matched.as_str(),
                f.violation.kind,
                f.violation.severity,
                f.violation.path.to_string_lossy().replace('\\', "/"),
            )
        })
        .collect();

    // The literal-arg call in index.ts fires with the rule-level error
    // severity overriding the warn master.
    let banned_call: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-child-process")
        .collect();
    assert_eq!(
        banned_call.len(),
        1,
        "exactly one banned-call finding expected: {by_rule:?}"
    );
    assert_eq!(banned_call[0].1, "execSync");
    assert_eq!(banned_call[0].2, PolicyRuleKind::BannedCall);
    assert_eq!(banned_call[0].3, PolicyViolationSeverity::Error);
    assert!(banned_call[0].4.ends_with("src/index.ts"));

    // Banned imports: the value import and the subpath import fire with the
    // warn master; the type-only import (ignoreTypeOnly) and moment-timezone
    // (segment-aware) stay quiet.
    let banned_imports: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-moment")
        .collect();
    let matched: Vec<&str> = banned_imports.iter().map(|entry| entry.1).collect();
    assert_eq!(
        matched,
        vec!["moment", "moment/locale/nl"],
        "segment-aware import matching: {by_rule:?}"
    );
    assert!(
        banned_imports
            .iter()
            .all(|entry| entry.3 == PolicyViolationSeverity::Warn)
    );

    let banned_effects: Vec<_> = by_rule
        .iter()
        .filter(|(rule, ..)| *rule == "no-network")
        .collect();
    assert_eq!(
        banned_effects.len(),
        1,
        "exactly one banned-effect finding expected: {by_rule:?}"
    );
    assert_eq!(banned_effects[0].1, "network: fetch");
    assert_eq!(banned_effects[0].2, PolicyRuleKind::BannedEffect);
    assert_eq!(banned_effects[0].3, PolicyViolationSeverity::Warn);

    // Nothing else fires: the suppressed call is consumed (not stale) and the
    // tooling file is excluded by the rule's glob.
    assert_eq!(results.policy_violations.len(), 4, "{by_rule:?}");
    assert!(
        results
            .stale_suppressions
            .iter()
            .all(|s| !s.path.ends_with("suppressed.ts")),
        "consumed policy suppression must not be stale: {:?}",
        results.stale_suppressions
    );

    // Counted toward the run total.
    assert!(results.total_issues() >= 4);
}

#[test]
fn no_rule_packs_configured_means_zero_policy_findings() {
    let config = fixture_config(Vec::new());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(results.policy_violations.is_empty());
}

#[test]
fn master_off_disables_the_evaluator_entirely() {
    let mut config = fixture_config(vec!["packs/team-policy.jsonc".to_string()]);
    config.rules.policy_violation = Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.policy_violations.is_empty(),
        "master off is a kill switch even for severity: error rules"
    );
}
