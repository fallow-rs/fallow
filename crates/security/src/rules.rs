//! Rule-severity policy for security-aware surfaces.

use fallow_config::{ResolvedConfig, Severity};

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

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "fixture setup asserts its own invariants directly"
)]
mod tests {
    use super::enable_security_rules;

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
