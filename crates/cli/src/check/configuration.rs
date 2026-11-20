//! Configuration for dead-code and architecture requests.
use std::path::PathBuf;

use fallow_engine::resolved_run::{ResolveError, ResolvedRun};

use super::*;

pub(super) fn prepare_check_config(opts: &CheckOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    let mut config = match opts.run {
        Some(runnable) => runnable.config().clone(),
        None => load_config_for_analysis(
            opts.root,
            opts.config_path,
            crate::ConfigLoadOptions {
                output: opts.output,
                no_cache: opts.no_cache,
                threads: opts.threads,
                production_override:
                    fallow_engine::project_config::ProductionFlags::single_analysis_override(
                        opts.production,
                        opts.production_override,
                    ),
                quiet: opts.quiet,
                allow_remote_extends: opts.allow_remote_extends,
            },
            fallow_config::ProductionAnalysis::DeadCode,
        )?,
    };
    if opts.filters.shared_state {
        config.request_shared_state();
    }
    config.analysis_snapshot = opts.analysis_snapshot;
    if opts.include_entry_exports {
        config.force_include_entry_exports();
    }
    if opts.show_cascade {
        config.show_cascade = true;
    }
    if opts.fail_on_parse_error {
        config.fail_on_parse_error = true;
    }
    apply_type_aware_overrides(opts, &mut config)?;
    opts.filters.activate_explicit_opt_ins(&mut config.rules);
    apply_type_aware_private_leak_default(&mut config, opts.filters.any_active());
    Ok(config)
}

/// Default the opt-in `private-type-leaks` rule to `warn` for type-aware runs,
/// where semantic confirmation makes it trustworthy. An explicit user setting
/// always wins: `"private-type-leaks": "off"` stays off (issue #2170).
pub(super) fn apply_type_aware_private_leak_default(
    config: &mut ResolvedConfig,
    filters_active: bool,
) {
    if config.type_aware.enabled
        && !filters_active
        && config.rules.private_type_leaks == Severity::Off
        && !config.rules.private_type_leaks_configured
    {
        config.rules.private_type_leaks = Severity::Warn;
    }
}

fn apply_type_aware_overrides(
    opts: &CheckOptions<'_>,
    config: &mut ResolvedConfig,
) -> Result<(), ExitCode> {
    apply_type_aware_overrides_from(
        opts.run
            .as_ref()
            .map(fallow_engine::resolved_run::Runnable::run),
        opts.output,
        &TypeAwareRequest {
            enabled: opts.type_aware,
            config_override: opts.type_aware_config_override,
            projects: opts.type_aware_projects,
            require: opts.type_aware_require,
        },
        config,
    )
}

/// The type-aware flags of one command, and the command-scoped config key
/// (`audit.typeAware`).
#[derive(Clone, Copy, Default)]
pub struct TypeAwareRequest<'a> {
    /// `Some(true)` for `--type-aware`, `Some(false)` for `--no-type-aware`.
    pub enabled: Option<bool>,
    /// The command-scoped config key, below the flags and
    /// `FALLOW_TYPE_AWARE`, above `typeAware.enabled`.
    pub config_override: Option<bool>,
    /// `--type-aware-project`. An empty list is not set.
    pub projects: &'a [PathBuf],
    /// `--type-aware-require`.
    pub require: Option<fallow_config::TypeAwareRequire>,
}

/// The type-aware settings of one command.
pub struct TypeAwareValues {
    /// Whether the command runs the type-aware companion.
    pub enabled: bool,
    /// The TypeScript project configs.
    pub projects: Vec<PathBuf>,
    /// The completeness policy.
    pub require: fallow_config::TypeAwareRequire,
}

/// The type-aware settings of a command. The order is the flags of
/// `request`, then the environment, then the command-scoped key of
/// `request`, then `section`. The environment is the snapshot of `run`;
/// without a run (a benchmark hook or a unit test) it is the process
/// environment.
///
/// The flags and the command-scoped key come from `request`, not from the
/// run inputs: a command that reads no type-aware flag (such as
/// `decision-surface`) passes none, also when the command line has one.
///
/// # Errors
///
/// Returns the input error of a `FALLOW_TYPE_AWARE` or
/// `FALLOW_TYPE_AWARE_REQUIRE` value that the command cannot use.
pub fn type_aware_values(
    run: Option<&ResolvedRun>,
    request: &TypeAwareRequest<'_>,
    section: &fallow_config::TypeAwareConfig,
) -> Result<TypeAwareValues, ResolveError> {
    let inputs = fallow_config::TypeAwareInputs {
        enabled: request.enabled.map(|on| given(on, "--type-aware")),
        projects: (!request.projects.is_empty())
            .then(|| given(request.projects.to_vec(), "--type-aware-project")),
        require: request
            .require
            .map(|require| given(require, "--type-aware-require")),
    };
    // The resolver reads the command-scoped key for an audit only.
    let command = if request.config_override.is_some() {
        fallow_config::CommandKind::Audit
    } else {
        fallow_config::CommandKind::DeadCode
    };
    let env = run.map_or_else(fallow_config::EnvSnapshot::capture, |run| run.env().clone());
    let settings = fallow_engine::resolved_run::decide_type_aware(
        command,
        &inputs,
        &env,
        fallow_engine::resolved_run::TypeAwareConfigInput {
            section,
            audit: request.config_override,
            file: None,
        },
    )?;
    Ok(TypeAwareValues {
        enabled: *settings.enabled.value(),
        projects: settings.projects.value().clone(),
        require: *settings.require.value(),
    })
}

const fn given<T>(value: T, flag: &'static str) -> fallow_types::sourced::Given<T> {
    fallow_types::sourced::Given {
        value,
        origin: fallow_types::sourced::Origin::Flag(flag),
    }
}

/// Write the type-aware settings of a command into `config`. An input error
/// prints with the output format and gives exit code 2.
///
/// # Errors
///
/// Returns exit code 2 for a `FALLOW_TYPE_AWARE` or
/// `FALLOW_TYPE_AWARE_REQUIRE` value that the command cannot use.
pub fn apply_type_aware_overrides_from(
    run: Option<&ResolvedRun>,
    output: OutputFormat,
    request: &TypeAwareRequest<'_>,
    config: &mut ResolvedConfig,
) -> Result<(), ExitCode> {
    let values = type_aware_values(run, request, &config.type_aware)
        .map_err(|err| emit_input_error(&err.message(), output))?;
    config.type_aware.enabled = values.enabled;
    config.type_aware.projects = values
        .projects
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    config.type_aware.require = values.require;
    Ok(())
}
