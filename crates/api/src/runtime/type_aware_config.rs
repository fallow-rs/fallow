//! Refusal of config-sourced type-aware analysis on the typed routes.
//!
//! The CLI turns type-aware analysis on from `FALLOW_TYPE_AWARE`,
//! `audit.typeAware` (audit only) and `typeAware.enabled`. The typed routes
//! read only [`crate::TypeAwareOptions::enabled`], and they cannot reproduce
//! the CLI result for these layers: the CLI degrades a failed companion run to
//! a warning, and health adds type coupling. A typed run that ignored the
//! layers would report syntactic results where `fallow` reports type-aware
//! ones, so the typed route fails with a stable code instead.

use std::path::Path;

use fallow_config::FallowConfig;

use super::{ProgrammaticResult, load_config_file};
use crate::{ProgrammaticError, TypeAwareOptions, analysis_context::ProgrammaticAnalysisContext};

/// Error code of the typed runners when the environment or the project config
/// turns on type-aware analysis that the caller did not request through
/// [`crate::TypeAwareOptions`]. Callers that can run the `fallow` CLI instead,
/// such as the MCP tools, fall back on it.
pub const TYPE_AWARE_CONFIG_UNSUPPORTED: &str = "FALLOW_TYPE_AWARE_CONFIG_UNSUPPORTED";

const TYPE_AWARE_ENV: &str = "FALLOW_TYPE_AWARE";

/// The command whose type-aware precedence applies.
#[derive(Clone, Copy)]
pub(super) enum TypeAwareCommand {
    /// `dead-code` and `health`: `FALLOW_TYPE_AWARE`, then `typeAware.enabled`.
    Analysis,
    /// `audit`: `FALLOW_TYPE_AWARE`, then `audit.typeAware`, then
    /// `typeAware.enabled`.
    Audit,
}

/// Fail with [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when the environment or the
/// config turns on type-aware analysis that `type_aware` leaves off.
///
/// # Errors
///
/// Returns the config load errors, and [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when
/// a layer turns type-aware analysis on.
pub(super) fn reject_config_type_aware(
    type_aware: &TypeAwareOptions,
    resolved: &ProgrammaticAnalysisContext,
    command: TypeAwareCommand,
) -> ProgrammaticResult<()> {
    reject_config_type_aware_at(
        type_aware,
        ConfigLocation {
            root: resolved.root(),
            config_path: resolved.config_path().as_deref(),
            allow_remote_extends: resolved.allow_remote_extends(),
        },
        command,
    )
}

/// Where the project config of a run lives.
#[derive(Clone, Copy)]
pub(super) struct ConfigLocation<'a> {
    pub root: &'a Path,
    pub config_path: Option<&'a Path>,
    pub allow_remote_extends: bool,
}

/// [`reject_config_type_aware`] for a caller without a resolved analysis
/// context.
pub(super) fn reject_config_type_aware_at(
    type_aware: &TypeAwareOptions,
    location: ConfigLocation<'_>,
    command: TypeAwareCommand,
) -> ProgrammaticResult<()> {
    if type_aware.enabled {
        return Ok(());
    }
    let source = match env_layer(std::env::var(TYPE_AWARE_ENV)) {
        EnvLayer::Off => return Ok(()),
        EnvLayer::On => Some(TYPE_AWARE_ENV),
        EnvLayer::Unset => {
            let config = load_config_file(
                location.root,
                location.config_path,
                location.allow_remote_extends,
            )?;
            config
                .as_ref()
                .and_then(|config| config_source(config, command))
        }
    };
    let Some(source) = source else {
        return Ok(());
    };
    Err(ProgrammaticError::new(
        format!(
            "`{source}` turns on type-aware analysis, which the programmatic API runs only when the `type_aware` option requests it; set that option or run the `fallow` CLI"
        ),
        2,
    )
    .with_code(TYPE_AWARE_CONFIG_UNSUPPORTED)
    .with_context(source))
}

#[derive(Debug, PartialEq, Eq)]
enum EnvLayer {
    Off,
    /// On, or a value the CLI rejects. A caller that falls back to the CLI
    /// then reports the CLI's validation error.
    On,
    Unset,
}

fn env_layer(value: Result<String, std::env::VarError>) -> EnvLayer {
    match value {
        Ok(value) if parses_as_off(&value) => EnvLayer::Off,
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => EnvLayer::On,
        Err(std::env::VarError::NotPresent) => EnvLayer::Unset,
    }
}

fn config_source(config: &FallowConfig, command: TypeAwareCommand) -> Option<&'static str> {
    if matches!(command, TypeAwareCommand::Audit)
        && let Some(enabled) = config.audit.type_aware
    {
        return enabled.then_some("audit.typeAware");
    }
    config.type_aware.enabled.then_some("typeAware.enabled")
}

/// The values that the CLI reads as off for `FALLOW_TYPE_AWARE`.
fn parses_as_off(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_layer_follows_the_cli_values() {
        for off in ["0", "false", " OFF ", "no"] {
            assert_eq!(env_layer(Ok(off.to_owned())), EnvLayer::Off, "{off}");
        }
        for on in ["1", "true", "yes", "on", "maybe"] {
            assert_eq!(env_layer(Ok(on.to_owned())), EnvLayer::On, "{on}");
        }
        assert_eq!(
            env_layer(Err(std::env::VarError::NotPresent)),
            EnvLayer::Unset
        );
    }

    #[test]
    fn audit_key_wins_over_the_top_level_key_for_audit_only() {
        let mut config = FallowConfig::default();
        config.type_aware.enabled = true;
        config.audit.type_aware = Some(false);
        assert_eq!(config_source(&config, TypeAwareCommand::Audit), None);
        assert_eq!(
            config_source(&config, TypeAwareCommand::Analysis),
            Some("typeAware.enabled")
        );

        config.type_aware.enabled = false;
        config.audit.type_aware = Some(true);
        assert_eq!(
            config_source(&config, TypeAwareCommand::Audit),
            Some("audit.typeAware")
        );
        assert_eq!(config_source(&config, TypeAwareCommand::Analysis), None);
    }
}
