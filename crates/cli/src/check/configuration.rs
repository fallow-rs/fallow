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
