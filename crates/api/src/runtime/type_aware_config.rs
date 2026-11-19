//! Refusal of config-sourced type-aware analysis on the typed routes.
//!
//! The CLI turns type-aware analysis on from `FALLOW_TYPE_AWARE`,
//! `audit.typeAware` (audit only) and `typeAware.enabled`. The typed routes
//! read only [`crate::TypeAwareOptions::enabled`], and they cannot reproduce
//! the CLI result for these layers: the CLI degrades a failed companion run to
//! a warning, and health adds type coupling. A typed run that ignored the
//! layers would report syntactic results where `fallow` reports type-aware
//! ones, so the typed route fails with a stable code instead.

use std::borrow::Cow;
use std::path::Path;

use fallow_config::FallowConfig;

use super::{ProgrammaticResult, load_config_file};
use crate::{
    ProgrammaticError, TypeAwareOptions, analysis_context::ProgrammaticAnalysisContext,
    run::ResolvedRun,
};

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
/// config of `run` turns on type-aware analysis and `type_aware` leaves the
/// choice open.
///
/// # Errors
///
/// Returns [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when a layer turns type-aware
/// analysis on.
pub(super) fn reject_config_type_aware(
    type_aware: &TypeAwareOptions,
    run: &ResolvedRun,
    command: TypeAwareCommand,
) -> ProgrammaticResult<()> {
    reject_type_aware_layers(type_aware, command, || {
        Ok(Some(Cow::Borrowed(run.config())))
    })
}

/// Resolve the run of a route, and fail with
/// [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when a layer turns type-aware analysis
/// on and `type_aware` leaves the choice open. The option and
/// `FALLOW_TYPE_AWARE` decide before the run resolves. The config layer reads
/// the config of the run. When the run does not resolve, the check reads the
/// config file at `location`, so a config that parses but does not validate
/// gives the refusal and not the validation error.
///
/// # Errors
///
/// Returns [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when a layer turns type-aware
/// analysis on, and the errors of `resolve` otherwise.
pub(super) fn resolve_run_refusing_type_aware(
    type_aware: &TypeAwareOptions,
    command: TypeAwareCommand,
    location: ConfigLocation<'_>,
    resolve: impl FnOnce() -> ProgrammaticResult<ResolvedRun>,
) -> ProgrammaticResult<ResolvedRun> {
    if !config_layer_decides(type_aware)? {
        return resolve();
    }
    match resolve() {
        Ok(run) => {
            refuse_config_source(run.config(), command)?;
            Ok(run)
        }
        Err(err) => {
            reject_config_type_aware_at(type_aware, location, command)?;
            Err(err)
        }
    }
}

/// `fallow dead-code --trace` adds semantic evidence and its completeness
/// exit when type-aware analysis is on. The typed trace has neither, so it
/// refuses a type-aware run from every layer, the `type_aware` option
/// included. `Some(false)` keeps the trace syntactic, as `--no-type-aware`
/// does. The option check comes before the config load.
///
/// # Errors
///
/// Returns [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when the option or a layer turns
/// type-aware analysis on, and the errors of `resolve` otherwise.
pub(super) fn resolve_trace_run(
    type_aware: &TypeAwareOptions,
    location: ConfigLocation<'_>,
    resolve: impl FnOnce() -> ProgrammaticResult<ResolvedRun>,
) -> ProgrammaticResult<ResolvedRun> {
    if type_aware.requested() {
        return Err(type_aware_refusal(
            "analysis.typeAware",
            "the programmatic trace has no semantic evidence; run the `fallow` CLI",
        ));
    }
    resolve_run_refusing_type_aware(type_aware, TypeAwareCommand::Analysis, location, resolve)
}

/// Where the project config of a run lives.
#[derive(Clone, Copy)]
pub(super) struct ConfigLocation<'a> {
    pub root: &'a Path,
    pub config_path: Option<&'a Path>,
    pub allow_remote_extends: bool,
}

impl<'a> ConfigLocation<'a> {
    /// The config location of a resolved analysis context.
    pub(super) fn of(resolved: &'a ProgrammaticAnalysisContext) -> Self {
        Self {
            root: resolved.root(),
            config_path: resolved.config_path().as_deref(),
            allow_remote_extends: resolved.allow_remote_extends(),
        }
    }
}

/// [`reject_config_type_aware`] for a caller without a resolved run. It
/// reads the config file only when the environment leaves the choice open.
pub(super) fn reject_config_type_aware_at(
    type_aware: &TypeAwareOptions,
    location: ConfigLocation<'_>,
    command: TypeAwareCommand,
) -> ProgrammaticResult<()> {
    reject_type_aware_layers(type_aware, command, || {
        Ok(load_config_file(
            location.root,
            location.config_path,
            location.allow_remote_extends,
        )?
        .map(Cow::Owned))
    })
}

/// The check of [`reject_config_type_aware`]. `config` gives the config file,
/// or `None` when the project has none.
fn reject_type_aware_layers<'config>(
    type_aware: &TypeAwareOptions,
    command: TypeAwareCommand,
    config: impl FnOnce() -> ProgrammaticResult<Option<Cow<'config, FallowConfig>>>,
) -> ProgrammaticResult<()> {
    if !config_layer_decides(type_aware)? {
        return Ok(());
    }
    match config()? {
        Some(config) => refuse_config_source(&config, command),
        None => Ok(()),
    }
}

/// Whether the config decides type-aware analysis: `false` when the
/// `type_aware` option or `FALLOW_TYPE_AWARE` decides it.
///
/// # Errors
///
/// Returns [`TYPE_AWARE_CONFIG_UNSUPPORTED`] when `FALLOW_TYPE_AWARE` turns
/// type-aware analysis on.
fn config_layer_decides(type_aware: &TypeAwareOptions) -> ProgrammaticResult<bool> {
    // An explicit choice wins over the environment and the config, as the
    // `--type-aware` and `--no-type-aware` flags do.
    if type_aware.enabled.is_some() {
        return Ok(false);
    }
    match env_layer(std::env::var(TYPE_AWARE_ENV)) {
        EnvLayer::Off => Ok(false),
        EnvLayer::On => Err(layer_refusal(TYPE_AWARE_ENV)),
        EnvLayer::Unset => Ok(true),
    }
}

fn refuse_config_source(
    config: &FallowConfig,
    command: TypeAwareCommand,
) -> ProgrammaticResult<()> {
    config_source(config, command).map_or(Ok(()), |source| Err(layer_refusal(source)))
}

fn layer_refusal(source: &str) -> ProgrammaticError {
    type_aware_refusal(
        source,
        "the programmatic API runs it only when the `type_aware` option requests it; set that option or run the `fallow` CLI",
    )
}

fn type_aware_refusal(source: &str, reason: &str) -> ProgrammaticError {
    ProgrammaticError::new(
        format!("`{source}` turns on type-aware analysis, but {reason}"),
        2,
    )
    .with_code(TYPE_AWARE_CONFIG_UNSUPPORTED)
    .with_context(source)
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

    /// An explicit choice returns before the environment or the config is
    /// read, as `--type-aware` and `--no-type-aware` win over both. The root
    /// does not exist, so a config read would fail.
    #[test]
    fn explicit_choice_wins_before_the_environment_and_the_config() {
        for enabled in [Some(true), Some(false)] {
            let options = TypeAwareOptions {
                enabled,
                ..TypeAwareOptions::default()
            };
            let location = ConfigLocation {
                root: Path::new("/fallow-no-such-root"),
                config_path: Some(Path::new("/fallow-no-such-root/.fallowrc.json")),
                allow_remote_extends: false,
            };
            assert!(
                reject_config_type_aware_at(&options, location, TypeAwareCommand::Audit).is_ok(),
                "{enabled:?}"
            );
        }
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
