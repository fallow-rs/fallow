//! Configuration for dead-code and architecture requests.
use super::*;

pub(super) fn prepare_check_config(opts: &CheckOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    let mut config = load_config_for_analysis(
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
    )?;
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

pub(super) fn parse_type_aware_bool(value: &str, output: OutputFormat) -> Result<bool, ExitCode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(emit_input_error(
            "FALLOW_TYPE_AWARE must be one of true, false, 1, 0, yes, no, on, or off",
            output,
        )),
    }
}

pub(super) fn parse_type_aware_require(
    value: &str,
    output: OutputFormat,
) -> Result<fallow_config::TypeAwareRequire, ExitCode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "best-effort" => Ok(fallow_config::TypeAwareRequire::BestEffort),
        "complete" => Ok(fallow_config::TypeAwareRequire::Complete),
        _ => Err(emit_input_error(
            "FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete",
            output,
        )),
    }
}
