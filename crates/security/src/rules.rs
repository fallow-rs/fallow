//! Rule-severity policy for security-aware surfaces.

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use fallow_types::results::{SecurityFinding, SecurityFindingKind};

/// Enable the advisory security rules for a dedicated security-aware surface.
///
/// Explicit user severities are preserved. Only the default `off` state is
/// promoted to `warn`, so `fallow security` and the viz Security lens surface
/// candidates without overriding a deliberate configuration.
pub fn enable_security_rules(config: &mut ResolvedConfig) {
    if config.rules.security_client_server_leak == Severity::Off {
        config.rules.security_client_server_leak = Severity::Warn;
    }
    if config.rules.security_sink == Severity::Off {
        config.rules.security_sink = Severity::Warn;
    }
}

/// The severity of the rule that produces findings of `kind`.
#[must_use]
const fn security_rule_severity(rules: &RulesConfig, kind: SecurityFindingKind) -> Severity {
    match kind {
        SecurityFindingKind::ClientServerLeak => rules.security_client_server_leak,
        SecurityFindingKind::TaintedSink => rules.security_sink,
    }
}

/// The severity of `finding` for its own path.
///
/// The `overrides` that match the path of the finding apply in the same way as
/// for dead-code findings.
#[must_use]
pub fn resolve_security_finding_severity(
    config: &ResolvedConfig,
    finding: &SecurityFinding,
) -> Severity {
    let path = config.root.join(&finding.path);
    security_rule_severity(&config.resolve_rules_for_path(&path), finding.kind)
}

/// Remove each finding whose rule resolves to `off` for its path.
///
/// A top-level `off` stops the detector before this point. This pass applies
/// an `off` from `overrides`, so a per-path `off` drops the finding also when a
/// security-aware surface raised the top-level rule to `warn`.
pub fn retain_enabled_security_findings(
    findings: &mut Vec<SecurityFinding>,
    config: &ResolvedConfig,
) {
    if config.overrides.is_empty() {
        return;
    }
    findings.retain(|finding| resolve_security_finding_severity(config, finding) != Severity::Off);
}

/// Whether a security rule is `error` at the top level or in an override.
#[must_use]
pub fn security_rules_can_error(config: &ResolvedConfig) -> bool {
    config.rules.security_client_server_leak == Severity::Error
        || config.rules.security_sink == Severity::Error
        || config.overrides.iter().any(|entry| {
            entry.rules.security_client_server_leak == Some(Severity::Error)
                || entry.rules.security_sink == Some(Severity::Error)
        })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "fixture setup asserts its own invariants directly"
)]
mod tests {
    use super::{enable_security_rules, security_rules_can_error};

    fn resolve(config: fallow_config::FallowConfig) -> fallow_config::ResolvedConfig {
        let project = tempfile::tempdir().expect("temp dir");
        config.resolve(
            project.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    fn sink_override(severity: fallow_config::Severity) -> fallow_config::FallowConfig {
        fallow_config::FallowConfig {
            overrides: vec![fallow_config::ConfigOverride {
                files: vec!["src/generated/**".to_owned()],
                rules: fallow_config::PartialRulesConfig {
                    security_sink: Some(severity),
                    ..Default::default()
                },
            }],
            ..Default::default()
        }
    }

    #[test]
    fn override_error_makes_security_rules_able_to_error() {
        let mut config = resolve(sink_override(fallow_config::Severity::Error));
        enable_security_rules(&mut config);

        assert!(security_rules_can_error(&config));
    }

    #[test]
    fn override_off_or_warn_keeps_security_rules_advisory() {
        for severity in [fallow_config::Severity::Off, fallow_config::Severity::Warn] {
            let mut config = resolve(sink_override(severity));
            enable_security_rules(&mut config);

            assert!(!security_rules_can_error(&config));
        }
    }

    #[test]
    fn enables_only_default_off_security_rules() {
        let project = tempfile::tempdir().expect("temp dir");
        let mut config = fallow_config::FallowConfig::default().resolve(
            project.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        config.rules.security_client_server_leak = fallow_config::Severity::Off;
        config.rules.security_sink = fallow_config::Severity::Error;

        enable_security_rules(&mut config);

        assert_eq!(
            config.rules.security_client_server_leak,
            fallow_config::Severity::Warn
        );
        assert_eq!(config.rules.security_sink, fallow_config::Severity::Error);
    }
}
