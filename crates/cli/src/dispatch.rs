//! Subcommand dispatch for the `fallow` command line.

#[path = "dispatch_coverage_upload.rs"]
mod coverage_upload;

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::check::{CheckOptions, IssueFilters, TraceOptions};
use crate::cli_agent::{AgentCli, run_agent_command};
use crate::cli_args::{
    AuditCacheCli, AuditGateArg, AuditWorsenedArg, BaselinesCli, CiCli, CiProviderArg,
    CiTemplateCli, Cli, Command, CoverageCli, EffortFilter, EmailModeArg, HealthSeverityCli,
    LicenseCli, RulePackCli, SecuritySubcommand, TelemetryCli, TypeAwareCli, TypeAwareRequireArg,
};
use crate::cli_hooks::run_hooks_command;
use crate::cli_impact::{ImpactCrossRepoOpts, dispatch_impact};
use crate::cli_production::{ProductionModes, resolve_production_modes};
use crate::cli_startup::cli_global_baseline_flag;
use crate::dupes::{DupesOptions, DupesOverrides};
use crate::error::emit_error;
use crate::health::HealthOptions;
use crate::list::ListOptions;
use crate::runtime_support::{AnalysisKind, LoadConfigArgs, load_config};
use crate::{
    DEFAULT_MIN_INVOCATIONS_HOT, DispatchContext, audit, audit_cache_prune, base_worktree,
    baseline_gate, baseline_growth, baselines, check, ci, ci_template, cli_report, combined,
    config, coverage, dupes, error, explain, fix, flags, guard, health, init, json_style, license,
    list, migrate, mutation_cli, onboarding, plugin_check, regression, regression_save_targets,
    report, resolve_audit_baseline_path, rule_pack, security, setup_hooks, similar_code_cli,
    suppressions, telemetry, trace_chain, trace_dependency, trace_error, trace_path, viz, watch,
};

#[path = "dispatch_inspect.rs"]
mod inspect_dispatch;

fn dispatch_mutation_command(
    dispatch: &DispatchContext<'_>,
    subcommand: Option<mutation_cli::MutationSubcommand>,
    args: mutation_cli::MutationArgs,
) -> ExitCode {
    let cli = dispatch.cli;
    let workspace_flag = if cli.workspace.is_some() {
        Some("--workspace")
    } else if cli.changed_workspaces.is_some() {
        Some("--changed-workspaces")
    } else {
        None
    };
    if let Some(flag) = workspace_flag {
        return error::emit_error_with_style(
            &format!(
                "{flag} does not narrow `fallow mutation` yet; pass the workspace directory as a path, for example `fallow mutation packages/app`"
            ),
            2,
            dispatch.output,
            dispatch.json_style,
        );
    }
    let config = if matches!(
        subcommand,
        Some(mutation_cli::MutationSubcommand::Inspect { .. })
    ) {
        fallow_config::MutationConfig::default()
    } else {
        match fallow_engine::project_config::config_for_project_with_load_options(
            dispatch.root,
            cli.config.as_deref(),
            fallow_config::ConfigLoadOptions {
                allow_remote_extends: cli.allow_remote_extends,
            },
        ) {
            Ok(project) => project.config.mutation,
            Err(error) => {
                return error::emit_error_with_style(
                    &format!("failed to load config: {error}"),
                    2,
                    dispatch.output,
                    dispatch.json_style,
                );
            }
        }
    };
    mutation_cli::run(&mutation_cli::MutationCliInput {
        root: dispatch.root,
        config_path: cli.config.as_deref(),
        root_flag: cli.root.as_deref(),
        output_file: cli.output_file.as_deref(),
        allow_remote_extends: cli.allow_remote_extends,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        changed_since: cli.changed_since.as_deref(),
        diff_file: cli.diff_file.as_deref(),
        quiet: dispatch.quiet,
        output: dispatch.output,
        json_style: dispatch.json_style,
        config,
        args,
        subcommand,
    })
}

#[allow(
    clippy::too_many_lines,
    reason = "the command router is intentionally an exhaustive top-level dispatch table"
)]
pub fn dispatch_subcommand(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let quiet = dispatch.quiet;
    match command {
        check @ Command::Check { .. } => dispatch_check_command(check, dispatch),
        architecture @ Command::Architecture { .. } => {
            dispatch_architecture_command(architecture, dispatch)
        }
        Command::Watch { no_clear } => dispatch_watch(dispatch, no_clear),
        Command::TypeAware { subcommand } => dispatch_type_aware_command(dispatch, subcommand),
        Command::Doctor => unreachable!("doctor bypasses the normal dispatch epilogue"),
        Command::SimilarCode {
            subcommand,
            threshold,
            min_lines,
            top,
            file,
            path,
        } => {
            let scope = match crate::scope_path::resolve_command_scope(
                dispatch.root,
                dispatch.output,
                path,
            ) {
                Ok(scope) => scope,
                Err(code) => return code,
            };
            similar_code_cli::run(similar_code_cli::SimilarCodeCliInput {
                root,
                config_path: cli.config.as_deref(),
                allow_remote_extends: cli.allow_remote_extends,
                no_cache: cli.no_cache,
                threads: dispatch.threads,
                changed_since: cli.changed_since.as_deref(),
                diff_file: cli.diff_file.as_deref(),
                workspace: cli.workspace.as_deref(),
                changed_workspaces: cli.changed_workspaces.as_deref(),
                explain: cli.explain,
                quiet,
                output,
                json_style: dispatch.json_style,
                threshold,
                min_lines,
                top,
                files: file,
                scope,
                subcommand,
            })
        }
        Command::Mutation { subcommand, args } => {
            dispatch_mutation_command(dispatch, subcommand, args)
        }
        inspect @ Command::Inspect { .. } => inspect_dispatch::dispatch_inspect(dispatch, inspect),
        Command::Trace {
            symbol,
            dependency,
            specifier,
            sites,
            limit,
            cursor,
            path,
            eager_only,
            callers,
            callees,
            depth,
        } => {
            if let Some(package_name) = dependency {
                return dispatch_trace_dependency(
                    dispatch,
                    trace_dependency::TraceDependencyTarget {
                        package_name,
                        specifiers: specifier,
                        sites,
                        limit,
                        cursor,
                        callers,
                        depth,
                    },
                );
            }
            dispatch_trace_command(
                dispatch,
                symbol,
                &path,
                eager_only,
                TraceChainFlags {
                    callers,
                    callees,
                    depth,
                },
            )
        }
        Command::TraceError { trace_file } => {
            trace_error::run_trace_error(&trace_error::TraceErrorOptions {
                root: dispatch.root,
                config_path: &dispatch.cli.config,
                output: dispatch.output,
                json_style: dispatch.json_style,
                no_cache: dispatch.cli.no_cache,
                threads: dispatch.threads,
                quiet: dispatch.quiet,
                allow_remote_extends: dispatch.cli.allow_remote_extends,
                trace_file: trace_file.as_deref(),
            })
        }
        fix @ Command::Fix { .. } => dispatch_fix_command(&fix, dispatch),
        init @ Command::Init { .. } => dispatch_init_command(init, root, quiet),
        Command::Hooks { subcommand } => {
            run_hooks_command(root, subcommand, output, dispatch.json_style)
        }
        Command::Agent { subcommand } => dispatch_agent_command(dispatch, subcommand),
        Command::Ci { subcommand } => {
            ci::run(map_ci_subcommand(subcommand), output, dispatch.json_style)
        }
        Command::ConfigSchema => init::run_config_schema(dispatch.json_style),
        Command::PluginSchema => init::run_plugin_schema(dispatch.json_style),
        Command::PluginCheck => plugin_check::run_plugin_check(root, output, dispatch.json_style),
        Command::RulePackSchema => init::run_rule_pack_schema(dispatch.json_style),
        Command::RulePack { subcommand } => dispatch_rule_pack_command(dispatch, subcommand),
        Command::Guard { files } => dispatch_guard_command(dispatch, &files),
        Command::CiTemplate { subcommand } => dispatch_ci_template_command(subcommand),
        Command::Config { path } => config::run_config_with_options(config::RunConfigInput {
            root,
            explicit_config: cli.config.as_deref(),
            path_only: path,
            output,
            quiet,
            json_style: dispatch.json_style,
            load_options: fallow_config::ConfigLoadOptions {
                allow_remote_extends: cli.allow_remote_extends,
            },
        }),
        Command::Recommend => onboarding::run_recommend(root, output, dispatch.json_style),
        list @ (Command::Workspaces | Command::List { .. }) => {
            dispatch_list_command(&list, dispatch)
        }
        dupes @ Command::Dupes { .. } => dispatch_dupes_command(dupes, dispatch),
        health @ Command::Health { .. } => dispatch_health_command(health, dispatch),
        Command::Flags {
            top,
            retirement,
            reasons,
            sort,
            flag_age,
            min_age,
            flag_state,
            max_flag_age,
        } => dispatch_flags_command(
            dispatch,
            top,
            retirement.then_some(flags::RetirementArgs {
                reasons,
                sort,
                flag_age,
                min_age,
                flag_state,
                max_flag_age,
            }),
        ),
        Command::Suppressions { file } => dispatch_suppressions_command(dispatch, &file),
        Command::Explain { issue_type } => {
            explain::run_explain(&issue_type.join(" "), output, dispatch.json_style)
        }
        audit @ Command::Audit { .. } => dispatch_audit_command(audit, dispatch),
        Command::AuditCache { subcommand } => dispatch_audit_cache_command(dispatch, &subcommand),
        Command::Baselines { subcommand } => dispatch_baselines_command(dispatch, &subcommand),
        Command::DecisionSurface { max_decisions } => {
            dispatch_decision_surface(dispatch, max_decisions)
        }
        Command::Impact {
            subcommand,
            all,
            sort,
            limit,
        } => dispatch_impact(
            root,
            quiet,
            output,
            dispatch.json_style,
            subcommand,
            ImpactCrossRepoOpts { all, sort, limit },
        ),
        security @ Command::Security { .. } => dispatch_security_command(security, dispatch),
        Command::Viz {
            output: viz_output,
            no_open,
            viz_format,
        } => dispatch_viz(dispatch, viz_output.as_deref(), no_open, viz_format),
        Command::Report { from } => {
            cli_report::run_report(&from, output, root, cli.config.as_deref(), cli.quiet)
        }
        Command::Schema => unreachable!("handled above"),
        migrate @ Command::Migrate { .. } => dispatch_migrate_command(migrate, root),
        Command::License { subcommand } => {
            dispatch_license_command(subcommand, output, dispatch.json_style)
        }
        Command::Cloud { subcommand } => {
            inspect_dispatch::dispatch_cloud(subcommand, output, dispatch.json_style)
        }
        Command::Telemetry { .. } => unreachable!("handled before root validation"),
        Command::Coverage { subcommand } => dispatch_coverage_command(dispatch, &subcommand),
        setup_hooks @ Command::SetupHooks { .. } => {
            dispatch_setup_hooks_command(&setup_hooks, dispatch)
        }
    }
}

fn dispatch_type_aware_command(
    dispatch: &DispatchContext<'_>,
    subcommand: TypeAwareCli,
) -> ExitCode {
    match subcommand {
        TypeAwareCli::Status => {
            let status = fallow_api::type_aware_status(dispatch.root);
            match dispatch.output {
                fallow_config::OutputFormat::Json => {
                    let output = type_aware_status_output(dispatch.root, status);
                    match fallow_output::serialize_type_aware_status_json_output(output) {
                        Ok(value) => match dispatch.json_style.serialize(&value) {
                            Ok(json) => {
                                crate::report::sink::outln!("{json}");
                                ExitCode::SUCCESS
                            }
                            Err(error) => emit_error(
                                &format!("failed to serialize type-aware status: {error}"),
                                2,
                                dispatch.output,
                            ),
                        },
                        Err(error) => emit_error(
                            &format!("failed to build type-aware status: {error}"),
                            2,
                            dispatch.output,
                        ),
                    }
                }
                fallow_config::OutputFormat::Human => {
                    if status.available {
                        crate::report::sink::outln!(
                            "{}",
                            report::human_status_line(
                                report::HumanStatus::Ok,
                                format_args!(
                                    "Type-aware companion: available ({}, protocol {}, TypeScript {})",
                                    status.package_version.as_deref().unwrap_or("unknown"),
                                    status.protocol_version,
                                    status.backend_version.as_deref().unwrap_or("unknown"),
                                )
                            )
                        );
                    } else {
                        crate::report::sink::outln!(
                            "{}",
                            report::human_status_line(
                                report::HumanStatus::Inactive,
                                "Type-aware companion: unavailable"
                            )
                        );
                        if let Some(remediation) = status.remediation {
                            crate::report::sink::outln!(
                                "{}",
                                report::human_status_line(
                                    report::HumanStatus::Warning,
                                    format_args!("Action: {remediation}")
                                )
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                _ => emit_error(
                    "type-aware status supports human and json output",
                    2,
                    dispatch.output,
                ),
            }
        }
    }
}

pub fn type_aware_status_output(
    root: &Path,
    status: fallow_api::TypeAwareStatus,
) -> fallow_output::TypeAwareStatusOutput {
    let companion_path = status.companion_path.as_deref().map(|path| {
        if let Ok(relative) = path.strip_prefix(root)
            && !relative.as_os_str().is_empty()
        {
            relative.to_string_lossy().replace('\\', "/")
        } else {
            path.file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned()
        }
    });
    let remediation = status.remediation.map(|message| {
        let without_root = message.replace(root.to_string_lossy().as_ref(), ".");
        status.companion_path.as_deref().map_or_else(
            || without_root.clone(),
            |path| {
                without_root.replace(
                    path.to_string_lossy().as_ref(),
                    companion_path.as_deref().unwrap_or("fallow-type-aware"),
                )
            },
        )
    });
    fallow_output::TypeAwareStatusOutput {
        schema_version: fallow_types::envelope::SchemaVersion(
            fallow_output::TYPE_AWARE_STATUS_SCHEMA_VERSION,
        ),
        version: fallow_types::envelope::ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        available: status.available,
        discovery_source: status.discovery_source.map(str::to_string),
        companion_path,
        package_version: status.package_version,
        protocol_version: status.protocol_version,
        backend_family: status.backend_family,
        backend_version: status.backend_version,
        remediation,
    }
}

/// Destructure the `Command::Check` arm and forward to `dispatch_check`.
fn dispatch_check_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let filters = check_issue_filters(&command);
    let Command::Check {
        include_dupes,
        trace,
        trace_file,
        trace_dependency,
        impact_closure,
        symbol_impact,
        top,
        file,
        finding_id,
        path,
        ..
    } = command
    else {
        unreachable!("check dispatcher only handles check commands");
    };

    let finding_ids = match fallow_engine::dead_code::FindingIdFilter::parse(&finding_id) {
        Ok(filter) => filter,
        Err(message) => return emit_error(&format!("--finding-id: {message}"), 2, dispatch.output),
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_check(
        dispatch,
        &CheckDispatchArgs {
            filters,
            trace_opts: TraceOptions {
                trace_export: trace,
                trace_file,
                trace_dependency,
                impact_closure,
                symbol_impact,
                performance: dispatch.cli.performance,
            },
            include_dupes,
            type_aware: dispatch.cli.type_aware_override(),
            type_aware_project: dispatch.cli.type_aware_project.clone(),
            type_aware_require: dispatch.cli.type_aware_require,
            top,
            file,
            scope,
            finding_ids,
            surface: check::CheckSurface::DeadCode,
        },
    )
}

/// Destructure the `Command::Architecture` arm and forward to `dispatch_check`
/// with the architecture issue types selected. The analysis, the JSON envelope
/// and the finding ids are the ones `dead-code` produces for the same filters.
fn dispatch_architecture_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Architecture {
        cycles,
        boundaries,
        policy,
        file,
        finding_id,
        path,
    } = command
    else {
        unreachable!("architecture dispatcher only handles architecture commands");
    };

    let finding_ids = match fallow_engine::dead_code::FindingIdFilter::parse(&finding_id) {
        Ok(filter) => filter,
        Err(message) => return emit_error(&format!("--finding-id: {message}"), 2, dispatch.output),
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_check(
        dispatch,
        &CheckDispatchArgs {
            filters: architecture_issue_filters(cycles, boundaries, policy),
            trace_opts: TraceOptions {
                trace_export: None,
                trace_file: None,
                trace_dependency: None,
                impact_closure: None,
                symbol_impact: None,
                performance: dispatch.cli.performance,
            },
            include_dupes: false,
            type_aware: dispatch.cli.type_aware_override(),
            type_aware_project: dispatch.cli.type_aware_project.clone(),
            type_aware_require: dispatch.cli.type_aware_require,
            top: None,
            file,
            scope,
            finding_ids,
            surface: check::CheckSurface::Architecture,
        },
    )
}

/// Map the `fallow architecture` filter flags onto the `dead-code` filter flags
/// they stand for. No flag selects every architecture issue type.
pub fn architecture_issue_filters(cycles: bool, boundaries: bool, policy: bool) -> IssueFilters {
    let all = !(cycles || boundaries || policy);
    let mut filters = IssueFilters::default();
    for (flag, active) in [
        ("--circular-deps", cycles || all),
        ("--re-export-cycles", cycles || all),
        ("--package-cycles", cycles || all),
        ("--boundary-violations", boundaries || all),
        ("--policy-violations", policy || all),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

/// Map the `Command::Check` filter flags onto `IssueFilters`. Reads the flags by
/// reference (all `Copy` bools) so the caller can still move the non-filter
/// fields out of the same `Command` value afterwards. Split into two halves to
/// keep each builder within the unit-size limit.
pub fn check_issue_filters(command: &Command) -> IssueFilters {
    check_issue_filters_framework(command, &check_issue_filters_core(command))
}

/// First half of the `IssueFilters` mapping: core/general filter flags over a
/// `Default` base. The framework/catalog half layers on top via struct update.
fn check_issue_filters_core(command: &Command) -> IssueFilters {
    let Command::Check {
        unused_files,
        unused_exports,
        unused_deps,
        unused_types,
        private_type_leaks,
        deprecated_exports_in_use,
        unused_enum_members,
        unused_class_members,
        unresolved_imports,
        unlisted_deps,
        duplicate_exports,
        circular_deps,
        re_export_cycles,
        package_cycles,
        boundary_violations,
        policy_violations,
        stale_suppressions,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = IssueFilters::default();
    for (flag, active) in [
        ("--unused-files", *unused_files),
        ("--unused-exports", *unused_exports),
        ("--unused-deps", *unused_deps),
        ("--unused-types", *unused_types),
        ("--private-type-leaks", *private_type_leaks),
        ("--deprecated-exports-in-use", *deprecated_exports_in_use),
        ("--unused-enum-members", *unused_enum_members),
        ("--unused-class-members", *unused_class_members),
        ("--unresolved-imports", *unresolved_imports),
        ("--unlisted-deps", *unlisted_deps),
        ("--duplicate-exports", *duplicate_exports),
        ("--circular-deps", *circular_deps),
        ("--re-export-cycles", *re_export_cycles),
        ("--package-cycles", *package_cycles),
        ("--boundary-violations", *boundary_violations),
        ("--policy-violations", *policy_violations),
        ("--stale-suppressions", *stale_suppressions),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

/// Second half of the `IssueFilters` mapping: framework/component, store, svelte,
/// catalog, and dependency-override flags, layered onto the core `base`.
fn check_issue_filters_framework(command: &Command, base: &IssueFilters) -> IssueFilters {
    let Command::Check {
        unused_store_members,
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        absent_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_effect_layers,
        unused_tagged_errors,
        unproduced_catch_tags,
        unused_server_actions,
        unused_load_data_keys,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = base.clone();
    for (flag, active) in [
        ("--unused-store-members", *unused_store_members),
        ("--unprovided-injects", *unprovided_injects),
        ("--unrendered-components", *unrendered_components),
        ("--unused-component-props", *unused_component_props),
        ("--absent-component-props", *absent_component_props),
        ("--unused-component-emits", *unused_component_emits),
        ("--unused-component-inputs", *unused_component_inputs),
        ("--unused-component-outputs", *unused_component_outputs),
        ("--unused-svelte-events", *unused_svelte_events),
        ("--unused-effect-layers", *unused_effect_layers),
        ("--unused-tagged-errors", *unused_tagged_errors),
        ("--unproduced-catch-tags", *unproduced_catch_tags),
        ("--unused-server-actions", *unused_server_actions),
        ("--unused-load-data-keys", *unused_load_data_keys),
        ("--unused-catalog-entries", *unused_catalog_entries),
        ("--empty-catalog-groups", *empty_catalog_groups),
        (
            "--unresolved-catalog-references",
            *unresolved_catalog_references,
        ),
        (
            "--unused-dependency-overrides",
            *unused_dependency_overrides,
        ),
        (
            "--misconfigured-dependency-overrides",
            *misconfigured_dependency_overrides,
        ),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

fn enable_check_filter(filters: &mut IssueFilters, flag: &str, active: bool) {
    if active {
        assert!(
            filters.enable_cli_filter_flag(flag),
            "check command uses unregistered dead-code filter flag {flag}"
        );
    }
}

/// The call-chain flags of `fallow trace FILE:SYMBOL`.
#[derive(Clone, Copy)]
struct TraceChainFlags {
    callers: bool,
    callees: bool,
    depth: Option<u32>,
}

fn dispatch_trace_dependency(
    dispatch: &DispatchContext<'_>,
    target: trace_dependency::TraceDependencyTarget,
) -> ExitCode {
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    trace_dependency::run_trace_dependency(&trace_dependency::TraceDependencyOptions {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: dispatch.cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: dispatch.cli.allow_remote_extends,
        production,
        workspace: dispatch.cli.workspace.as_deref(),
        changed_workspaces: dispatch.cli.changed_workspaces.as_deref(),
        target,
    })
}

fn dispatch_trace_command(
    dispatch: &DispatchContext<'_>,
    symbol: Option<String>,
    path: &[String],
    eager_only: bool,
    chain: TraceChainFlags,
) -> ExitCode {
    let TraceChainFlags {
        callers,
        callees,
        depth,
    } = chain;
    if let [from, to] = path {
        return trace_path::run_trace_path(&trace_path::TracePathOptions {
            root: dispatch.root,
            config_path: &dispatch.cli.config,
            output: dispatch.output,
            json_style: dispatch.json_style,
            no_cache: dispatch.cli.no_cache,
            threads: dispatch.threads,
            quiet: dispatch.quiet,
            allow_remote_extends: dispatch.cli.allow_remote_extends,
            from,
            to,
            eager_only,
        });
    }
    let Some(symbol) = symbol else {
        return emit_error(
            "trace requires a FILE:SYMBOL target or --path <FROM> <TO>",
            2,
            dispatch.output,
        );
    };
    trace_chain::run_trace(&trace_chain::TraceChainOptions {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: dispatch.cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: dispatch.cli.allow_remote_extends,
        target: symbol,
        callers,
        callees,
        depth: depth.unwrap_or(fallow_types::trace_chain::DEFAULT_TRACE_DEPTH),
    })
}

fn dispatch_security_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Security {
        subcommand,
        runtime_coverage,
        min_invocations_hot,
        file,
        gate,
        surface,
        path,
    } = command
    else {
        unreachable!("security dispatcher only handles security commands");
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    let gate = gate.map(security::SecurityGateArg::into_mode);
    let cli = dispatch.cli;
    let (output, _quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let derived_flags = SecurityDerivedFlagState {
        output,
        json_style: dispatch.json_style,
        ci: cli.ci,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        summary: cli.summary,
        explain: cli.explain,
        runtime_coverage: runtime_coverage.as_deref(),
        min_invocations_hot,
        file: file.as_slice(),
        gate,
        surface,
    };
    if let Some(code) = try_run_security_survivors(subcommand.as_ref(), &derived_flags) {
        return code;
    }

    let scoped_files = scoped_security_files(&file, subcommand.as_ref());
    run_security_blind_spots_or_default(
        dispatch,
        &SecurityRunInputs {
            scoped_files: &scoped_files,
            subcommand: &subcommand,
            runtime_coverage: runtime_coverage.as_deref(),
            min_invocations_hot,
            gate,
            surface,
            scope,
        },
        &derived_flags,
    )
}

/// Inputs threaded from the security dispatcher into the run step. Borrows the
/// scoped file list and subcommand so they outlive the `SecurityOptions`.
struct SecurityRunInputs<'a> {
    scoped_files: &'a [PathBuf],
    subcommand: &'a Option<SecuritySubcommand>,
    runtime_coverage: Option<&'a Path>,
    min_invocations_hot: u64,
    gate: Option<security::SecurityGateMode>,
    surface: bool,
    scope: Option<PathBuf>,
}

/// Build `SecurityOptions` and run either the blind-spots or default analysis.
fn run_security_blind_spots_or_default(
    dispatch: &DispatchContext<'_>,
    inputs: &SecurityRunInputs<'_>,
    derived_flags: &SecurityDerivedFlagState<'_>,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let opts = security::SecurityOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        summary: cli.summary,
        changed_since: cli.changed_since.as_deref(),
        use_shared_diff_index: true,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        file: inputs.scoped_files,
        surface: inputs.surface,
        scope: inputs.scope.clone(),
        gate: inputs.gate,
        runtime_coverage: inputs.runtime_coverage,
        min_invocations_hot: inputs.min_invocations_hot,
        explain: cli.explain,
    };
    if matches!(
        inputs.subcommand,
        Some(SecuritySubcommand::BlindSpots { .. })
    ) {
        if let Some(code) = validate_security_blind_spots_flags(derived_flags) {
            return code;
        }
        security::run_blind_spots(&opts)
    } else {
        security::run(&opts)
    }
}

/// Handle `fallow security survivors` as an early return. Returns `Some(code)`
/// when the subcommand is `survivors` (validated then run); `None` otherwise.
fn try_run_security_survivors(
    subcommand: Option<&SecuritySubcommand>,
    flags: &SecurityDerivedFlagState<'_>,
) -> Option<ExitCode> {
    let Some(SecuritySubcommand::Survivors {
        candidates,
        verdicts,
        require_verdict_for_each_candidate,
    }) = subcommand
    else {
        return None;
    };
    if let Some(code) = validate_security_survivors_flags(flags) {
        return Some(code);
    }
    Some(security::run_survivors(
        &security::SecuritySurvivorsOptions {
            output: flags.output,
            json_style: flags.json_style,
            candidates,
            verdicts,
            require_verdict_for_each_candidate: *require_verdict_for_each_candidate,
        },
    ))
}

/// Build the scoped file list, folding in `blind-spots` extra `--file` values.
fn scoped_security_files(
    file: &[PathBuf],
    subcommand: Option<&SecuritySubcommand>,
) -> Vec<PathBuf> {
    let mut scoped_files = file.to_vec();
    if let Some(SecuritySubcommand::BlindSpots {
        file: blind_spot_files,
    }) = subcommand
    {
        scoped_files.extend(blind_spot_files.iter().cloned());
    }
    scoped_files
}

struct SecurityDerivedFlagState<'a> {
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
    ci: bool,
    fail_on_issues: bool,
    sarif_file: Option<&'a Path>,
    summary: bool,
    explain: bool,
    runtime_coverage: Option<&'a Path>,
    min_invocations_hot: u64,
    file: &'a [PathBuf],
    gate: Option<security::SecurityGateMode>,
    surface: bool,
}

fn validate_security_survivors_flags(flags: &SecurityDerivedFlagState<'_>) -> Option<ExitCode> {
    let flag = if flags.ci {
        Some("--ci")
    } else if flags.fail_on_issues {
        Some("--fail-on-issues")
    } else if flags.sarif_file.is_some() {
        Some("--sarif-file")
    } else if flags.summary {
        Some("--summary")
    } else if flags.explain {
        Some("--explain")
    } else if flags.runtime_coverage.is_some() {
        Some("--runtime-coverage")
    } else if flags.min_invocations_hot != DEFAULT_MIN_INVOCATIONS_HOT {
        Some("--min-invocations-hot")
    } else if !flags.file.is_empty() {
        Some("--file")
    } else if flags.gate.is_some() {
        Some("--gate")
    } else if flags.surface {
        Some("--surface")
    } else {
        None
    }?;
    Some(emit_error(
        &format!("{flag} is not valid with `fallow security survivors`."),
        2,
        flags.output,
    ))
}

fn validate_security_blind_spots_flags(flags: &SecurityDerivedFlagState<'_>) -> Option<ExitCode> {
    let flag = if flags.ci {
        Some("--ci")
    } else if flags.fail_on_issues {
        Some("--fail-on-issues")
    } else if flags.sarif_file.is_some() {
        Some("--sarif-file")
    } else if flags.summary {
        Some("--summary")
    } else if flags.explain {
        Some("--explain")
    } else if flags.runtime_coverage.is_some() {
        Some("--runtime-coverage")
    } else if flags.min_invocations_hot != DEFAULT_MIN_INVOCATIONS_HOT {
        Some("--min-invocations-hot")
    } else if flags.gate.is_some() {
        Some("--gate")
    } else if flags.surface {
        Some("--surface")
    } else {
        None
    }?;
    Some(emit_error(
        &format!("{flag} is not valid with `fallow security blind-spots`."),
        2,
        flags.output,
    ))
}

fn dispatch_dupes_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Dupes {
        mode,
        near,
        min_tokens,
        min_lines,
        min_occurrences,
        threshold,
        skip_local,
        ignore_symlinks,
        no_ignore_symlinks,
        cross_language,
        ignore_imports,
        no_ignore_imports,
        top,
        no_fragments,
        trace,
        path,
    } = command
    else {
        unreachable!("dupes dispatcher only handles dupes commands");
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_dupes(
        dispatch,
        &DupesDispatchArgs {
            overrides: DupesOverrides {
                mode,
                near,
                threshold,
                min_tokens,
                min_lines,
                min_occurrences,
                skip_local,
                ignore_symlinks: resolve_negatable_flag(ignore_symlinks, no_ignore_symlinks),
                cross_language,
                ignore_imports: resolve_negatable_flag(ignore_imports, no_ignore_imports),
            },
            top,
            no_fragments,
            trace,
            scope,
        },
    )
}

fn dispatch_agent_command(dispatch: &DispatchContext<'_>, subcommand: AgentCli) -> ExitCode {
    run_agent_command(
        dispatch.root,
        dispatch.cli.root.is_some(),
        subcommand,
        dispatch.output,
        dispatch.json_style,
    )
}

fn dispatch_init_command(command: Command, root: &Path, quiet: bool) -> ExitCode {
    let Command::Init {
        toml,
        agents,
        hooks,
        branch,
        decline,
    } = command
    else {
        unreachable!("init dispatcher only handles init commands");
    };

    init::run_init(&init::InitOptions {
        root,
        use_toml: toml,
        agents,
        hooks,
        branch: branch.as_deref(),
        decline,
        quiet,
    })
}

fn dispatch_fix_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Fix {
        dry_run,
        yes,
        no_create_config,
        path,
    } = command
    else {
        unreachable!("fix dispatcher only handles fix commands");
    };

    let scope = match crate::scope_path::resolve_command_scope(
        dispatch.root,
        dispatch.output,
        path.clone(),
    ) {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_fix(
        dispatch,
        &FixDispatchArgs {
            dry_run: *dry_run,
            yes: *yes,
            no_create_config: *no_create_config,
            scope,
        },
    )
}

fn dispatch_list_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    match command {
        Command::Workspaces => dispatch_list(dispatch, &ListDispatchArgs::workspaces()),
        Command::List {
            entry_points,
            files,
            plugins,
            boundaries,
            workspaces,
            entry_weight,
            path,
        } => {
            let scope = match crate::scope_path::resolve_command_scope(
                dispatch.root,
                dispatch.output,
                path.clone(),
            ) {
                Ok(scope) => scope.map(|resolved| resolved.absolute),
                Err(code) => return code,
            };
            dispatch_list(
                dispatch,
                &ListDispatchArgs {
                    entry_points: *entry_points,
                    files: *files,
                    plugins: *plugins,
                    boundaries: *boundaries,
                    workspaces: *workspaces,
                    entry_weight: *entry_weight,
                    scope,
                },
            )
        }
        _ => unreachable!("list dispatcher only handles list commands"),
    }
}

fn dispatch_migrate_command(command: Command, root: &Path) -> ExitCode {
    let Command::Migrate {
        toml,
        jsonc,
        dry_run,
        from,
    } = command
    else {
        unreachable!("migrate dispatcher only handles migrate commands");
    };

    migrate::run_migrate(root, toml, jsonc, dry_run, from.as_deref())
}

fn dispatch_license_command(
    subcommand: LicenseCli,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
) -> ExitCode {
    license::run(&map_license_subcommand(subcommand), output, json_style)
}

fn dispatch_ci_template_command(subcommand: CiTemplateCli) -> ExitCode {
    match subcommand {
        CiTemplateCli::Gitlab { vendor, force } => {
            ci_template::run_gitlab_template(&ci_template::GitlabTemplateOptions {
                vendor_dir: vendor,
                force,
            })
        }
    }
}

fn dispatch_coverage_command(dispatch: &DispatchContext<'_>, subcommand: &CoverageCli) -> ExitCode {
    let cli = dispatch.cli;
    coverage::run(
        map_coverage_subcommand(subcommand, cli.explain, cli.changed_since.as_deref()),
        &coverage::RunContext {
            root: dispatch.root,
            config_path: &cli.config,
            output: dispatch.output,
            json_style: dispatch.json_style,
            quiet: dispatch.quiet,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            explain: cli.explain,
            allow_remote_extends: cli.allow_remote_extends,
        },
    )
}

fn dispatch_health_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Health {
        max_cyclomatic,
        max_cognitive,
        max_crap,
        max_file_lines,
        top,
        sort,
        complexity,
        complexity_breakdown,
        file_scores,
        coverage_gaps,
        hotspots,
        ownership,
        ownership_emails,
        targets,
        type_coupling,
        css,
        effort,
        score,
        min_score,
        min_severity,
        report_only,
        since,
        min_commits,
        save_snapshot,
        trend,
        trend_from,
        coverage,
        coverage_root,
        runtime_coverage,
        mutation_report,
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        path,
    } = command
    else {
        unreachable!("health dispatcher only handles health commands");
    };

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    let ownership = ownership || ownership_emails.is_some();
    let hotspots = hotspots || ownership;
    let args = HealthDispatchArgs {
        max_cyclomatic,
        max_cognitive,
        max_crap,
        max_file_lines,
        top,
        sort,
        complexity,
        complexity_breakdown,
        file_scores,
        coverage_gaps,
        hotspots,
        ownership,
        ownership_emails: ownership_emails.map(EmailModeArg::to_config),
        targets,
        type_coupling,
        css,
        effort,
        score,
        min_score,
        min_severity: min_severity.map(HealthSeverityCli::to_health_severity),
        report_only,
        since: since.as_deref(),
        min_commits,
        save_snapshot: save_snapshot.as_ref(),
        trend: trend || trend_from.is_some(),
        trend_from: trend_from.as_deref(),
        fail_on_stale_baseline: dispatch.cli.fail_on_stale_baseline,
        fail_on_parse_error: dispatch.cli.fail_on_parse_error,
        coverage: coverage.as_deref(),
        coverage_root: coverage_root.as_deref(),
        runtime_coverage: runtime_coverage.as_deref(),
        mutation_report: mutation_report.as_deref(),
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        scope,
    };
    dispatch_health(dispatch, &args)
}

fn dispatch_setup_hooks_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::SetupHooks {
        agent,
        dry_run,
        force,
        user,
        gitignore_claude,
        uninstall,
    } = command
    else {
        unreachable!("setup-hooks dispatcher only handles setup-hooks commands");
    };

    eprintln!(
        "warning: `fallow setup-hooks` is deprecated and will be removed in the next major; use `fallow agent install` or `fallow hooks install --target agent`."
    );
    setup_hooks::run_setup_hooks(&setup_hooks::SetupHooksOptions {
        root: dispatch.root,
        agent: *agent,
        dry_run: *dry_run,
        force: *force,
        user: *user,
        home: None,
        gitignore_claude: *gitignore_claude,
        uninstall: *uninstall,
    })
}

fn dispatch_audit_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Audit {
        production_dead_code,
        production_health,
        production_dupes,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        max_crap,
        coverage,
        coverage_root,
        only,
        skip,
        no_css,
        css_deep,
        no_css_deep,
        gate,
        worsened,
        runtime_coverage,
        min_invocations_hot,
        gate_marker,
        brief,
        max_decisions,
        walkthrough_guide,
        walkthrough_file,
        walkthrough,
        mark_viewed,
        show_cleared,
        show_deprioritized,
        path,
    } = command
    else {
        unreachable!("audit dispatcher only handles audit commands");
    };

    // The walkthrough flags imply the brief path (the guide digest + the
    // graph-snapshot pin are brief-path data).
    let brief = brief || walkthrough_guide || walkthrough || walkthrough_file.is_some();

    let scope = match crate::scope_path::resolve_command_scope(dispatch.root, dispatch.output, path)
    {
        Ok(scope) => scope.map(|resolved| resolved.absolute),
        Err(code) => return code,
    };

    dispatch_audit(
        dispatch,
        &AuditDispatchArgs {
            production_dead_code,
            production_health,
            production_dupes,
            dead_code_baseline,
            health_baseline,
            dupes_baseline,
            max_crap,
            coverage,
            coverage_root,
            only,
            skip,
            no_css,
            css_deep,
            no_css_deep,
            gate,
            worsened,
            runtime_coverage,
            min_invocations_hot,
            gate_marker,
            brief,
            max_decisions,
            walkthrough_guide,
            walkthrough_file,
            walkthrough,
            mark_viewed,
            show_cleared,
            show_deprioritized,
            scope,
        },
    )
}

fn dispatch_audit_cache_command(
    dispatch: &DispatchContext<'_>,
    subcommand: &AuditCacheCli,
) -> ExitCode {
    match subcommand {
        AuditCacheCli::Remove { dry_run, yes } => {
            if !*dry_run && !*yes && !std::io::stdin().is_terminal() {
                return emit_error(
                    "audit-cache remove requires --yes (or --force) in non-interactive environments. Use --dry-run to preview removal first, then pass --yes to confirm.",
                    2,
                    dispatch.output,
                );
            }
            match base_worktree::remove_reusable_audit_caches(dispatch.root, *dry_run) {
                Ok(report) => {
                    let action = if *dry_run { "would remove" } else { "removed" };
                    if matches!(dispatch.output, fallow_config::OutputFormat::Json) {
                        let value = serde_json::json!({
                            "kind": "audit-cache-remove",
                            "schema_version": 1,
                            "command": "audit-cache remove",
                            "root": dispatch.root,
                            "dry_run": report.dry_run,
                            "found": report.found,
                            "would_remove": report.found.saturating_sub(report.skipped),
                            "removed": report.removed,
                            "skipped": report.skipped,
                            "complete": report.skipped == 0,
                        });
                        let output_code = report::emit_report_json(
                            &value,
                            "audit cache removal",
                            dispatch.json_style,
                        );
                        if output_code != ExitCode::SUCCESS {
                            return output_code;
                        }
                    } else if !dispatch.quiet {
                        println!(
                            "audit cache: {action} {}, skipped {} for {}",
                            if *dry_run {
                                report.found.saturating_sub(report.skipped)
                            } else {
                                report.removed
                            },
                            report.skipped,
                            dispatch.root.display(),
                        );
                    }
                    if report.skipped == 0 {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(2)
                    }
                }
                Err(error) => emit_error(
                    &format!(
                        "failed to remove audit caches for {}: {error}",
                        dispatch.root.display()
                    ),
                    2,
                    dispatch.output,
                ),
            }
        }
        AuditCacheCli::Prune {
            dry_run,
            max_age_days,
        } => audit_cache_prune::run_audit_cache_prune(&audit_cache_prune::AuditCachePruneOptions {
            root: dispatch.root,
            config_path: dispatch.cli.config.as_ref(),
            allow_remote_extends: dispatch.cli.allow_remote_extends,
            dry_run: *dry_run,
            max_age_days: *max_age_days,
            output: dispatch.output,
            json_style: dispatch.json_style,
            quiet: dispatch.quiet,
        }),
    }
}

fn dispatch_baselines_command(
    dispatch: &DispatchContext<'_>,
    subcommand: &BaselinesCli,
) -> ExitCode {
    let BaselinesCli::Prune {
        check,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        coverage,
        coverage_root,
    } = subcommand;
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    if let Some(flag) = baselines_prune_narrowing_flag(cli) {
        return emit_error(
            &format!(
                "`fallow baselines prune` always analyzes the whole project, so it does not accept `{flag}`. A narrowed run would remove valid entries."
            ),
            2,
            output,
        );
    }
    if let Some(flag) = cli_global_baseline_flag(cli) {
        return emit_error(
            &format!(
                "`fallow baselines prune` does not accept `{flag}`. Name the files with --dead-code-baseline, --health-baseline and --dupes-baseline, or with `audit.*Baseline` in the config."
            ),
            2,
            output,
        );
    }
    let config = match load_config(
        root,
        &cli.config,
        LoadConfigArgs {
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            production: false,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
        },
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };
    let targets = baselines::BaselineTargets {
        dead_code: resolve_audit_baseline_path(
            root,
            dead_code_baseline.as_deref(),
            config.audit.dead_code_baseline.as_deref(),
        ),
        health: resolve_audit_baseline_path(
            root,
            health_baseline.as_deref(),
            config.audit.health_baseline.as_deref(),
        ),
        dupes: resolve_audit_baseline_path(
            root,
            dupes_baseline.as_deref(),
            config.audit.dupes_baseline.as_deref(),
        ),
    };
    if targets.dead_code.is_none() && targets.health.is_none() && targets.dupes.is_none() {
        return emit_error(
            "no baseline to prune. Set `audit.deadCodeBaseline`, `audit.healthBaseline` or `audit.dupesBaseline` in the config, or pass --dead-code-baseline, --health-baseline or --dupes-baseline",
            2,
            output,
        );
    }
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(production) => production,
        Err(code) => return code,
    };
    let coverage_inputs = match resolve_coverage_inputs(
        coverage.as_deref(),
        coverage_root.as_deref(),
        output,
        || Ok(config.health),
    ) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    baselines::run_baselines_prune(&baselines::BaselinesPruneOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        check: *check,
        targets,
        production,
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        regression_opts: dispatch.regression_opts(true),
    })
}

/// The first flag or environment variable that would narrow the prune run.
fn baselines_prune_narrowing_flag(cli: &Cli) -> Option<&'static str> {
    [
        (cli.changed_since.is_some(), "--changed-since"),
        (cli.workspace.is_some(), "--workspace"),
        (cli.changed_workspaces.is_some(), "--changed-workspaces"),
        (cli.diff_file.is_some(), "--diff-file"),
        (cli.diff_stdin, "--diff-stdin"),
        (
            std::env::var_os("FALLOW_DIFF_FILE").is_some_and(|value| !value.is_empty()),
            "FALLOW_DIFF_FILE",
        ),
        (cli.production, "--production"),
    ]
    .into_iter()
    .find_map(|(present, flag)| present.then_some(flag))
}

fn dispatch_flags_command(
    dispatch: &DispatchContext<'_>,
    top: Option<usize>,
    retirement: Option<flags::RetirementArgs>,
) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let quiet = dispatch.quiet;
    let threads = dispatch.threads;
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
        Err(code) => return code,
    };
    flags::run_flags(&flags::FlagsOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        production,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        explain: cli.explain,
        top,
        retirement,
        regression: dispatch.regression_opts(false),
        regression_flag: first_regression_flag(cli),
    })
}

/// The first regression-gate option on the command line, if any.
fn first_regression_flag(cli: &Cli) -> Option<&'static str> {
    [
        (cli.fail_on_regression, "--fail-on-regression"),
        (cli.regression_baseline.is_some(), "--regression-baseline"),
        (
            cli.save_regression_baseline.is_some(),
            "--save-regression-baseline",
        ),
        (cli.tolerance != "0", "--tolerance"),
    ]
    .into_iter()
    .find_map(|(used, flag)| used.then_some(flag))
}

fn dispatch_suppressions_command(
    dispatch: &DispatchContext<'_>,
    file: &[std::path::PathBuf],
) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
        Err(code) => return code,
    };
    suppressions::run_suppressions(&suppressions::SuppressionsOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        production,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        file,
    })
}

fn dispatch_guard_command(dispatch: &DispatchContext<'_>, files: &[String]) -> ExitCode {
    guard::run_guard(&guard::GuardOptions {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        quiet: dispatch.quiet,
        allow_remote_extends: dispatch.cli.allow_remote_extends,
        files,
    })
}

fn dispatch_rule_pack_command(dispatch: &DispatchContext<'_>, subcommand: RulePackCli) -> ExitCode {
    let ctx = rule_pack::RulePackContext {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        quiet: dispatch.quiet,
        no_cache: dispatch.cli.no_cache,
        threads: Some(dispatch.threads),
        allow_remote_extends: dispatch.cli.allow_remote_extends,
    };
    rule_pack::run(&map_rule_pack_subcommand(subcommand), &ctx)
}

fn map_rule_pack_subcommand(subcommand: RulePackCli) -> rule_pack::RulePackSubcommand {
    match subcommand {
        RulePackCli::Init {
            name,
            template,
            dir,
            no_config,
        } => rule_pack::RulePackSubcommand::Init(rule_pack::InitArgs {
            name,
            template,
            dir,
            no_config,
        }),
        RulePackCli::List => rule_pack::RulePackSubcommand::List,
        RulePackCli::Test { pack } => {
            rule_pack::RulePackSubcommand::Test(rule_pack::TestArgs { pack })
        }
        RulePackCli::Schema => rule_pack::RulePackSubcommand::Schema,
    }
}

fn map_license_subcommand(sub: LicenseCli) -> license::LicenseSubcommand {
    match sub {
        LicenseCli::Activate {
            jwt,
            from_file,
            stdin,
            trial,
            email,
        } => license::LicenseSubcommand::Activate(license::ActivateArgs {
            raw_jwt: jwt,
            from_file,
            from_stdin: stdin,
            trial,
            email,
        }),
        LicenseCli::Status => license::LicenseSubcommand::Status,
        LicenseCli::Refresh { api_key } => {
            license::LicenseSubcommand::Refresh(license::RefreshArgs { api_key })
        }
        LicenseCli::Deactivate => license::LicenseSubcommand::Deactivate,
    }
}

pub fn map_telemetry_subcommand(sub: TelemetryCli) -> telemetry::TelemetryCommand {
    match sub {
        TelemetryCli::Status => telemetry::TelemetryCommand::Status,
        TelemetryCli::Enable => telemetry::TelemetryCommand::Enable,
        TelemetryCli::Disable => telemetry::TelemetryCommand::Disable,
        TelemetryCli::Inspect { example } => telemetry::TelemetryCommand::Inspect { example },
    }
}

fn map_ci_subcommand(sub: CiCli) -> ci::CiCommand {
    match sub {
        command @ CiCli::PlanPrComment { .. } => map_ci_plan_pr_comment(command),
        command @ CiCli::PostPrComment { .. } => map_ci_post_pr_comment(command),
        command @ CiCli::PostReview { .. } => map_ci_post_review(command),
        command @ CiCli::PostCheckRun { .. } => map_ci_post_check_run(command),
        command @ CiCli::ReconcileReview { .. } => map_ci_reconcile_review(command),
    }
}

fn map_ci_plan_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    } = command
    else {
        unreachable!("ci plan-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    }
}

fn map_ci_post_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PostPrComment {
        provider,
        pr,
        mr,
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PostPrComment {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_review(command: CiCli) -> ci::CiCommand {
    let CiCli::PostReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-review mapper called with different variant");
    };

    ci::CiCommand::PostReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_check_run(command: CiCli) -> ci::CiCommand {
    let CiCli::PostCheckRun {
        provider,
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    } = command
    else {
        unreachable!("ci post-check-run mapper called with different variant");
    };

    ci::CiCommand::PostCheckRun {
        provider: map_ci_provider(provider),
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    }
}

fn map_ci_reconcile_review(command: CiCli) -> ci::CiCommand {
    let CiCli::ReconcileReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci reconcile-review mapper called with different variant");
    };

    ci::CiCommand::ReconcileReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_provider(provider: CiProviderArg) -> ci::CiProvider {
    match provider {
        CiProviderArg::Github => ci::CiProvider::Github,
        CiProviderArg::Gitlab => ci::CiProvider::Gitlab,
    }
}

fn map_coverage_subcommand(
    sub: &CoverageCli,
    explain: bool,
    base: Option<&str>,
) -> coverage::CoverageSubcommand {
    match sub {
        CoverageCli::Setup { args } => args.to_subcommand(explain),
        CoverageCli::Analyze { .. } => map_coverage_analyze(sub),
        CoverageCli::UploadInventory { .. } => coverage_upload::map_coverage_upload_inventory(sub),
        CoverageCli::UploadSourceMaps { .. } => {
            coverage_upload::map_coverage_upload_source_maps(sub)
        }
        CoverageCli::UploadStaticFindings { .. } => map_coverage_upload_static_findings(sub),
        CoverageCli::ReviewPacket {
            api_key,
            api_endpoint,
            repo,
            project_id,
            coverage_period,
            commit_sha,
            files,
            functions,
        } => coverage::CoverageSubcommand::ReviewPacket(coverage::ReviewPacketArgs {
            api_key: api_key.clone(),
            api_endpoint: api_endpoint.clone(),
            repo: repo.clone(),
            project_id: project_id.clone(),
            coverage_period: *coverage_period,
            commit_sha: commit_sha.clone(),
            files: files.clone(),
            functions: functions.clone(),
            base: base.map(str::to_owned),
        }),
        CoverageCli::DeploymentChanges {
            api_key,
            api_endpoint,
            repo,
            sha,
            change,
            limit,
            cursor,
        } => coverage::CoverageSubcommand::DeploymentChanges(coverage::DeploymentChangesArgs {
            api_key: api_key.clone(),
            api_endpoint: api_endpoint.clone(),
            repo: repo.clone(),
            sha: sha.clone(),
            base: base.map(str::to_owned),
            change: change.clone(),
            limit: *limit,
            cursor: cursor.clone(),
        }),
    }
}

fn map_coverage_analyze(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::Analyze {
        runtime_coverage,
        cloud,
        api_key,
        api_endpoint,
        repo,
        project_id,
        coverage_period,
        environment,
        commit_sha,
        production,
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        top,
        blast_radius,
        importance,
        debug_unmatched,
    } = sub
    else {
        unreachable!("coverage analyze mapper called with non-analyze variant");
    };
    coverage::CoverageSubcommand::Analyze(coverage::AnalyzeArgs {
        runtime_coverage: runtime_coverage.clone(),
        cloud: *cloud,
        api_key: api_key.clone(),
        api_endpoint: api_endpoint.clone(),
        repo: repo.clone(),
        project_id: project_id.clone(),
        coverage_period: *coverage_period,
        environment: environment.clone(),
        commit_sha: commit_sha.clone(),
        production: *production,
        min_invocations_hot: *min_invocations_hot,
        min_observation_volume: *min_observation_volume,
        low_traffic_threshold: *low_traffic_threshold,
        top: *top,
        blast_radius: *blast_radius,
        importance: *importance,
        debug_unmatched: *debug_unmatched,
    })
}

fn map_coverage_upload_static_findings(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadStaticFindings {
        api_key,
        api_endpoint,
        project_id,
        git_sha,
        allow_dirty,
        dry_run,
        ignore_upload_errors,
    } = sub
    else {
        unreachable!("coverage static-findings mapper called with non-static variant");
    };
    coverage::CoverageSubcommand::UploadStaticFindings(coverage::UploadStaticFindingsArgs {
        api_key: api_key.clone(),
        api_endpoint: api_endpoint.clone(),
        project_id: project_id.clone(),
        git_sha: git_sha.clone(),
        allow_dirty: *allow_dirty,
        dry_run: *dry_run,
        ignore_upload_errors: *ignore_upload_errors,
    })
}

struct CheckDispatchArgs {
    filters: IssueFilters,
    trace_opts: TraceOptions,
    include_dupes: bool,
    type_aware: Option<bool>,
    type_aware_project: Vec<std::path::PathBuf>,
    type_aware_require: Option<TypeAwareRequireArg>,
    top: Option<usize>,
    file: Vec<std::path::PathBuf>,
    scope: Option<std::path::PathBuf>,
    finding_ids: Option<fallow_engine::dead_code::FindingIdFilter>,
    surface: check::CheckSurface,
}

#[derive(Clone)]
struct ListDispatchArgs {
    entry_points: bool,
    files: bool,
    plugins: bool,
    boundaries: bool,
    workspaces: bool,
    entry_weight: bool,
    scope: Option<std::path::PathBuf>,
}

impl ListDispatchArgs {
    fn workspaces() -> Self {
        Self {
            entry_points: false,
            files: false,
            plugins: false,
            boundaries: false,
            workspaces: true,
            entry_weight: false,
            scope: None,
        }
    }
}

fn dispatch_viz(
    dispatch: &DispatchContext<'_>,
    output_path: Option<&std::path::Path>,
    no_open: bool,
    format: viz::VizFormat,
) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    viz::run_viz(&viz::VizOptions {
        root: dispatch.root,
        config_path: &cli.config,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        production,
        allow_remote_extends: cli.allow_remote_extends,
        output_path,
        no_open,
        format,
    })
}

fn dispatch_watch(dispatch: &DispatchContext<'_>, no_clear: bool) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    watch::run_watch(&watch::WatchOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        production,
        clear_screen: !no_clear,
        explain: cli.explain,
        include_entry_exports: cli.include_entry_exports,
        show_cascade: cli.show_cascade,
        type_aware: cli.type_aware_override(),
        type_aware_projects: &cli.type_aware_project,
        type_aware_require: cli.type_aware_require.map(Into::into),
    })
}

struct FixDispatchArgs {
    dry_run: bool,
    yes: bool,
    no_create_config: bool,
    scope: Option<std::path::PathBuf>,
}

fn dispatch_fix(dispatch: &DispatchContext<'_>, args: &FixDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    fix::run_fix(&fix::FixOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        emit_output: true,
        allow_remote_extends: cli.allow_remote_extends,
        dry_run: args.dry_run,
        yes: args.yes,
        production,
        no_create_config: args.no_create_config,
        type_aware: cli.type_aware_override(),
        type_aware_projects: &cli.type_aware_project,
        type_aware_require: cli.type_aware_require.map(Into::into),
        scope: args.scope.clone(),
        show_cascade: cli.show_cascade,
    })
}

fn dispatch_list(dispatch: &DispatchContext<'_>, args: &ListDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let (save_regression_file, save_to_config) = regression_save_targets(cli);
    // Only the entry weight gate reads the global `--tolerance`, so a plain
    // listing does not fail on a value it never uses.
    let entry_weight_gate = if args.entry_weight {
        let tolerance = match regression::Tolerance::parse(&cli.tolerance) {
            Ok(tolerance) => tolerance,
            Err(message) => {
                return emit_error(
                    &format!("invalid --tolerance: {message}"),
                    2,
                    dispatch.output,
                );
            }
        };
        Some(regression::EntryWeightGate {
            fail_on_regression: cli.fail_on_regression,
            tolerance,
            baseline_file: cli.regression_baseline.as_deref(),
            save_file: save_regression_file.as_deref(),
            save_to_config,
        })
    } else {
        None
    };
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    list::run_list(&ListOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        threads: dispatch.threads,
        no_cache: cli.no_cache,
        entry_points: args.entry_points,
        files: args.files,
        plugins: args.plugins,
        boundaries: args.boundaries,
        workspaces: args.workspaces,
        entry_weight: args.entry_weight,
        entry_weight_gate,
        production,
        allow_remote_extends: cli.allow_remote_extends,
        scope: args.scope.clone(),
    })
}

fn dispatch_check(dispatch: &DispatchContext<'_>, args: &CheckDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    if let Some(code) = validate_type_aware_check_options(dispatch, args) {
        return code;
    }
    let growth_targets = baseline_growth::targets(&[(
        cli.baseline.as_deref(),
        fallow_engine::baseline::BaselineKind::DeadCode,
    )]);
    baseline_growth::run_gated(
        &dispatch.growth_ctx(baseline_growth::GrowthOwner::DeadCode),
        &growth_targets,
        || dispatch_check_run(dispatch, args, production),
    )
}

fn dispatch_check_run(
    dispatch: &DispatchContext<'_>,
    args: &CheckDispatchArgs,
    production: bool,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    check::run_check(&CheckOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        fail_on_issues,
        filters: &args.filters,
        changed_since: cli.changed_since.as_deref(),
        change_scope_owner: fallow_engine::change_scope::ChangeScopeOwner::Run,
        no_package_baselines: cli.no_package_baselines,
        diff_index: None,
        use_shared_diff_index: true,
        baseline: cli.baseline.as_deref(),
        baseline_flag: "--baseline",
        bare_run: false,
        save_baseline: cli.save_baseline.as_deref(),
        fail_on_stale_baseline: cli.fail_on_stale_baseline,
        sarif_file: cli.sarif_file.as_deref(),
        production,
        production_override: Some(production),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        group_by: cli.group_by,
        include_dupes: args.include_dupes,
        type_aware: args.type_aware,
        type_aware_config_override: None,
        type_aware_projects: &args.type_aware_project,
        type_aware_require: args.type_aware_require.map(Into::into),
        trace_opts: &args.trace_opts,
        explain: cli.explain,
        top: args.top,
        file: &args.file,
        scope: args.scope.clone(),
        finding_ids: args.finding_ids.as_ref(),
        include_entry_exports: cli.include_entry_exports,
        show_cascade: cli.show_cascade,
        fail_on_parse_error: cli.fail_on_parse_error,
        summary: cli.summary,
        regression_opts: dispatch.regression_opts(
            cli.changed_since.is_some()
                || cli.workspace.is_some()
                || cli.changed_workspaces.is_some()
                || !args.file.is_empty()
                || args.scope.is_some(),
        ),
        retain_modules_for_health: false,
        defer_performance: true,
        analysis_snapshot: fallow_config::AnalysisSnapshot::Current,
        explain_skipped: cli.explain_skipped,
        surface: args.surface,
    })
}

fn validate_type_aware_check_options(
    dispatch: &DispatchContext<'_>,
    args: &CheckDispatchArgs,
) -> Option<ExitCode> {
    let output = dispatch.output;
    if !args.type_aware_project.is_empty() && args.type_aware != Some(true) {
        return Some(emit_error(
            "--type-aware-project requires --type-aware",
            2,
            output,
        ));
    }
    if args.type_aware_require.is_some() && args.type_aware != Some(true) {
        return Some(emit_error(
            "--type-aware-require requires --type-aware",
            2,
            output,
        ));
    }
    if args.trace_opts.symbol_impact.is_some() && args.type_aware != Some(true) {
        return Some(emit_error(
            "--symbol-impact requires --type-aware",
            2,
            output,
        ));
    }
    let focused_output = args.trace_opts.trace_export.is_some()
        || args.trace_opts.trace_file.is_some()
        || args.trace_opts.trace_dependency.is_some()
        || args.trace_opts.impact_closure.is_some()
        || args.trace_opts.symbol_impact.is_some();
    if focused_output
        && !matches!(
            output,
            fallow_config::OutputFormat::Human | fallow_config::OutputFormat::Json
        )
    {
        return Some(emit_error(
            "focused trace and impact queries support human and JSON output",
            2,
            output,
        ));
    }
    if args.type_aware == Some(true)
        && !matches!(
            output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Json
                | fallow_config::OutputFormat::Sarif
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
                | fallow_config::OutputFormat::CodeClimate
                | fallow_config::OutputFormat::PrCommentGithub
                | fallow_config::OutputFormat::PrCommentGitlab
                | fallow_config::OutputFormat::ReviewGithub
                | fallow_config::OutputFormat::ReviewGitlab
        )
    {
        return Some(emit_error(
            "--type-aware supports human, JSON, SARIF, compact, markdown, CodeClimate, PR-comment, and review output; pair presentation formats with the JSON artifact to preserve semantic provenance",
            2,
            output,
        ));
    }
    None
}

/// Resolve the three-state `ignoreImports` CLI override from the opt-in /
/// opt-out flag pair. clap's `conflicts_with` guarantees the two are never both
/// set, so this maps `--no-ignore-imports` -> `Some(false)`, `--ignore-imports`
/// -> `Some(true)`, and neither -> `None` (defer to config, which defaults to
/// `true`).
/// Resolve a `--flag` / `--no-flag` pair: `Some(false)` for the opt-out,
/// `Some(true)` for the opt-in, `None` to defer to config.
pub const fn resolve_negatable_flag(opt_in: bool, opt_out: bool) -> Option<bool> {
    if opt_out {
        Some(false)
    } else if opt_in {
        Some(true)
    } else {
        None
    }
}

struct DupesDispatchArgs {
    overrides: DupesOverrides,
    top: Option<usize>,
    no_fragments: bool,
    trace: Option<String>,
    scope: Option<std::path::PathBuf>,
}

fn dispatch_dupes(dispatch: &DispatchContext<'_>, args: &DupesDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::Dupes) {
        Ok(production) => production,
        Err(code) => return code,
    };
    let growth_targets = baseline_growth::targets(&[(
        cli.baseline.as_deref(),
        fallow_engine::baseline::BaselineKind::Dupes,
    )]);
    baseline_growth::run_gated(
        &dispatch.growth_ctx(baseline_growth::GrowthOwner::Dupes),
        &growth_targets,
        || dispatch_dupes_run(dispatch, args, production),
    )
}

fn dispatch_dupes_run(
    dispatch: &DispatchContext<'_>,
    args: &DupesDispatchArgs,
    production: bool,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet) = (dispatch.output, dispatch.quiet);
    dupes::run_dupes(&DupesOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        overrides: args.overrides,
        top: args.top,
        baseline_path: cli.baseline.as_deref(),
        baseline_flag: "--baseline",
        save_baseline_path: cli.save_baseline.as_deref(),
        fail_on_stale_baseline: cli.fail_on_stale_baseline,
        fail_on_issues: dispatch.fail_on_issues,
        production,
        production_override: Some(production),
        trace: args.trace.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        diff_index: None,
        use_shared_diff_index: true,
        changed_files: None,
        change_scope_owner: fallow_engine::change_scope::ChangeScopeOwner::Run,
        no_package_baselines: cli.no_package_baselines,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        explain: cli.explain,
        explain_skipped: cli.explain_skipped,
        summary: cli.summary,
        group_by: cli.group_by,
        performance: cli.performance,
        include_fragments: !args.no_fragments,
        retain_unfiltered_report: false,
        scope: args.scope.clone(),
    })
}

struct AuditDispatchArgs {
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
    dead_code_baseline: Option<PathBuf>,
    health_baseline: Option<PathBuf>,
    dupes_baseline: Option<PathBuf>,
    max_crap: Option<f64>,
    coverage: Option<PathBuf>,
    coverage_root: Option<PathBuf>,
    only: Vec<AnalysisKind>,
    skip: Vec<AnalysisKind>,
    no_css: bool,
    css_deep: bool,
    no_css_deep: bool,
    gate: Option<AuditGateArg>,
    worsened: Option<AuditWorsenedArg>,
    runtime_coverage: Option<PathBuf>,
    min_invocations_hot: u64,
    gate_marker: Option<String>,
    brief: bool,
    max_decisions: usize,
    /// Emit the agent-contract walkthrough guide instead of the brief body.
    walkthrough_guide: bool,
    /// Post-validate an agent's judgment JSON from this path against the
    /// live graph.
    walkthrough_file: Option<PathBuf>,
    /// Render the existing walkthrough guide as a staged human/markdown tour.
    walkthrough: bool,
    /// Changed files to record as VIEWED before rendering the tour.
    mark_viewed: Vec<PathBuf>,
    /// Expand the Cleared panel (de-prioritized + viewed) in the tour.
    show_cleared: bool,
    /// Expand the de-prioritized units in the human focus map.
    show_deprioritized: bool,
    scope: Option<PathBuf>,
}

struct ResolvedAuditInputs {
    audit_cfg: fallow_config::AuditConfig,
    analyses: audit::AuditAnalysisSelection,
    cache_dir: PathBuf,
    production: ProductionModes,
    dead_code_baseline: Option<PathBuf>,
    health_baseline: Option<PathBuf>,
    dupes_baseline: Option<PathBuf>,
    /// Istanbul coverage inputs resolved with the health precedence (flag,
    /// env, then `health.coverage` / `health.coverageRoot`), so the head and
    /// base passes score from the same map the standalone health run uses.
    coverage: Option<PathBuf>,
    coverage_root: Option<PathBuf>,
    file_lines_check: bool,
}

fn dispatch_audit(dispatch: &DispatchContext<'_>, args: &AuditDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let output = dispatch.output;

    if cli.baseline.is_some() || cli.save_baseline.is_some() {
        return emit_error(
            "audit uses per-analysis baselines. Use --dead-code-baseline, --health-baseline, or --dupes-baseline (or save them with `fallow dead-code|health|dupes --save-baseline <file>`)",
            2,
            output,
        );
    }

    let inputs = match resolve_audit_inputs(dispatch, args) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    if (!args.only.is_empty() || !args.skip.is_empty()) && audit_renders_brief(args) {
        eprintln!(
            "Note: --only and --skip have no effect on the review brief, which keeps every \
             sub-analysis."
        );
    }
    note_inert_health_flags(args, inputs.analyses);

    // The brief and the walkthrough views always exit 0, so the gate stands
    // down there and says so.
    if cli.fail_on_baseline_growth && audit_renders_brief(args) {
        baseline_growth::note_stood_down(
            dispatch.growth_flags(),
            "the review brief never fails a run",
        );
        return run_resolved_audit(dispatch, args, &inputs);
    }
    let growth_targets = baseline_growth::targets(&[
        (
            inputs.dead_code_baseline.as_deref(),
            fallow_engine::baseline::BaselineKind::DeadCode,
        ),
        (
            inputs.health_baseline.as_deref(),
            fallow_engine::baseline::BaselineKind::Health,
        ),
        (
            inputs.dupes_baseline.as_deref(),
            fallow_engine::baseline::BaselineKind::Dupes,
        ),
    ]);
    baseline_growth::run_gated(
        &dispatch.growth_ctx(baseline_growth::GrowthOwner::Audit),
        &growth_targets,
        || run_resolved_audit(dispatch, args, &inputs),
    )
}

const fn audit_renders_brief(args: &AuditDispatchArgs) -> bool {
    args.brief || args.walkthrough_guide || args.walkthrough || args.walkthrough_file.is_some()
}

fn resolve_audit_inputs(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
) -> Result<ResolvedAuditInputs, ExitCode> {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let config = load_config(
        root,
        &cli.config,
        LoadConfigArgs {
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            production: cli.production,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
        },
    )?;
    let cache_dir = config.cache_dir.clone();
    let audit_cfg = config.audit;
    let file_lines_check = config.health.file_lines_check_enabled();
    let production = resolve_production_modes(
        cli,
        root,
        output,
        args.production_dead_code,
        args.production_health,
        args.production_dupes,
    )?;
    let analyses = resolve_audit_analyses(&audit_cfg, args);
    if !(analyses.dead_code || analyses.health || analyses.dupes) {
        return Err(emit_error(
            "this audit selects no sub-analysis: --skip and the audit.deadCode, audit.health and audit.dupes config keys leave nothing to run",
            2,
            output,
        ));
    }
    // A skipped sub-analysis reads no baseline. An explicit flag for one is a
    // mistake; a configured one is dropped so the baseline gates do not see it.
    let baseline = |enabled: bool, flag: &str, cli_path: Option<&Path>, cfg_path: Option<&str>| {
        if enabled {
            return Ok(resolve_audit_baseline_path(root, cli_path, cfg_path));
        }
        if cli_path.is_some() {
            return Err(emit_error(
                &format!(
                    "{flag} has no effect: this audit skips that sub-analysis (--only, --skip or the audit config)"
                ),
                2,
                output,
            ));
        }
        Ok(None)
    };
    let resolved_dead_code_baseline = baseline(
        analyses.dead_code,
        "--dead-code-baseline",
        args.dead_code_baseline.as_deref(),
        audit_cfg.dead_code_baseline.as_deref(),
    )?;
    let resolved_health_baseline = baseline(
        analyses.health,
        "--health-baseline",
        args.health_baseline.as_deref(),
        audit_cfg.health_baseline.as_deref(),
    )?;
    let resolved_dupes_baseline = baseline(
        analyses.dupes,
        "--dupes-baseline",
        args.dupes_baseline.as_deref(),
        audit_cfg.dupes_baseline.as_deref(),
    )?;
    let coverage_inputs = resolve_coverage_inputs(
        args.coverage.as_deref(),
        args.coverage_root.as_deref(),
        output,
        || Ok(config.health),
    )?;

    Ok(ResolvedAuditInputs {
        audit_cfg,
        analyses,
        cache_dir,
        production,
        dead_code_baseline: resolved_dead_code_baseline,
        health_baseline: resolved_health_baseline,
        dupes_baseline: resolved_dupes_baseline,
        coverage: coverage_inputs.coverage,
        coverage_root: coverage_inputs.coverage_root,
        file_lines_check,
    })
}

/// Name the health-only flags that a skipped health sub-analysis leaves without
/// effect, so a CI job that skips health does not lose a gate in silence.
fn note_inert_health_flags(args: &AuditDispatchArgs, analyses: audit::AuditAnalysisSelection) {
    if analyses.health {
        return;
    }
    let inert: Vec<&str> = [
        (args.max_crap.is_some(), "--max-crap"),
        (args.coverage.is_some(), "--coverage"),
        (args.coverage_root.is_some(), "--coverage-root"),
        (args.runtime_coverage.is_some(), "--runtime-coverage"),
    ]
    .into_iter()
    .filter_map(|(set, flag)| set.then_some(flag))
    .collect();
    if !inert.is_empty() {
        eprintln!(
            "Note: {} did not run: this audit skips the health sub-analysis.",
            inert.join(", ")
        );
    }
}

/// Resolve the sub-analyses of one audit with the bare `--only` / `--skip`
/// semantics. `--only` replaces the `audit.*` selection and `--skip` removes
/// from it. The review brief keeps every sub-analysis, because it reads the
/// module graph and all three sections.
fn resolve_audit_analyses(
    config: &fallow_config::AuditConfig,
    args: &AuditDispatchArgs,
) -> audit::AuditAnalysisSelection {
    if audit_renders_brief(args) {
        return audit::AuditAnalysisSelection::ALL;
    }
    let architecture = combined::resolve_architecture_selection(&args.only, &args.skip);
    if !args.only.is_empty() {
        let (dead_code, dupes, health) = combined::resolve_analyses(&args.only, &[]);
        return audit::AuditAnalysisSelection {
            dead_code,
            health,
            dupes,
            architecture,
        };
    }
    let (dead_code, dupes, health) = combined::resolve_analyses(&[], &args.skip);
    audit::AuditAnalysisSelection {
        dead_code: dead_code && config.dead_code.unwrap_or(true),
        health: health && config.health.unwrap_or(true),
        dupes: dupes && config.dupes.unwrap_or(true),
        architecture,
    }
}

fn audit_css_enabled(config: &fallow_config::AuditConfig, args: &AuditDispatchArgs) -> bool {
    !args.no_css && config.css.unwrap_or(true)
}

fn audit_css_deep_enabled(config: &fallow_config::AuditConfig, args: &AuditDispatchArgs) -> bool {
    audit_css_enabled(config, args)
        && !args.no_css_deep
        && (args.css_deep || config.css_deep.unwrap_or(true))
}

fn run_resolved_audit(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
    inputs: &ResolvedAuditInputs,
) -> ExitCode {
    let cli = dispatch.cli;
    audit::run_audit_with_type_aware(
        &audit::AuditOptions {
            root: dispatch.root,
            config_path: &cli.config,
            cache_dir: &inputs.cache_dir,
            output: dispatch.output,
            json_style: dispatch.json_style,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
            changed_since: cli.changed_since.as_deref(),
            production: cli.production,
            production_dead_code: Some(inputs.production.dead_code),
            production_health: Some(inputs.production.health),
            production_dupes: Some(inputs.production.dupes),
            workspace: cli.workspace.as_deref(),
            changed_workspaces: cli.changed_workspaces.as_deref(),
            explain: cli.explain,
            explain_skipped: cli.explain_skipped,
            performance: cli.performance,
            group_by: cli.group_by,
            dead_code_baseline: inputs.dead_code_baseline.as_deref(),
            health_baseline: inputs.health_baseline.as_deref(),
            dupes_baseline: inputs.dupes_baseline.as_deref(),
            health_baseline_mode: cli.baseline_mode.unwrap_or_default().into(),
            fail_on_stale_baseline: cli.fail_on_stale_baseline,
            max_crap: args.max_crap,
            file_lines_check: inputs.file_lines_check,
            coverage: inputs.coverage.as_deref(),
            coverage_root: inputs.coverage_root.as_deref(),
            gate: args.gate.map_or(inputs.audit_cfg.gate, Into::into),
            worsened: args.worsened.map_or(inputs.audit_cfg.worsened, Into::into),
            include_entry_exports: cli.include_entry_exports,
            show_cascade: cli.show_cascade,
            fail_on_parse_error: cli.fail_on_parse_error,
            analyses: inputs.analyses,
            // Styling analytics, including deep cross-file reachability, is on
            // by default in `fallow audit`; both layers remain verdict-neutral
            // unless a user escalates a styling rule to error.
            css: audit_css_enabled(&inputs.audit_cfg, args),
            css_deep: audit_css_deep_enabled(&inputs.audit_cfg, args),
            runtime_coverage: args.runtime_coverage.as_deref(),
            min_invocations_hot: args.min_invocations_hot,
            brief: args.brief,
            max_decisions: args.max_decisions,
            walkthrough_guide: args.walkthrough_guide,
            walkthrough: args.walkthrough,
            mark_viewed: &args.mark_viewed,
            show_cleared: args.show_cleared,
            walkthrough_file: args.walkthrough_file.as_deref(),
            show_deprioritized: args.show_deprioritized,
            scope: args.scope.clone(),
        },
        args.gate_marker.as_deref(),
        audit::AuditTypeAwareOptions {
            enabled: cli.type_aware_override(),
            config_default: inputs.audit_cfg.type_aware,
            projects: &cli.type_aware_project,
            require: cli.type_aware_require.map(Into::into),
        },
    )
}

/// Dispatch `fallow decision-surface`: the separable apex. Reuses the audit
/// input resolution in brief mode (changed-code scope) with all gating /
/// coverage / baseline knobs defaulted, then renders ONLY the decision surface.
fn dispatch_decision_surface(dispatch: &DispatchContext<'_>, max_decisions: usize) -> ExitCode {
    let args = decision_surface_audit_args(max_decisions);
    let inputs = match resolve_audit_inputs(dispatch, &args) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    // The brief owns no exit gates (see `decision_surface_audit_options`), so
    // an opted-in stale-baseline gate stands down here and says so for every
    // baseline the config supplied, like every other path that skips it.
    if dispatch.cli.fail_on_stale_baseline {
        for path in [
            &inputs.dead_code_baseline,
            &inputs.health_baseline,
            &inputs.dupes_baseline,
        ] {
            baseline_gate::note_stood_down(
                path.as_deref(),
                true,
                "decision-surface renders a brief without exit gates",
            );
        }
    }
    audit::run_decision_surface(&decision_surface_audit_options(
        dispatch,
        &inputs,
        max_decisions,
    ))
}

fn decision_surface_audit_args(max_decisions: usize) -> AuditDispatchArgs {
    AuditDispatchArgs {
        production_dead_code: false,
        production_health: false,
        production_dupes: false,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        only: Vec::new(),
        skip: Vec::new(),
        no_css: true,
        css_deep: false,
        no_css_deep: false,
        gate: None,
        worsened: None,
        runtime_coverage: None,
        min_invocations_hot: 0,
        gate_marker: None,
        brief: true,
        max_decisions,
        walkthrough_guide: false,
        walkthrough_file: None,
        walkthrough: false,
        mark_viewed: Vec::new(),
        show_cleared: false,
        show_deprioritized: false,
        scope: None,
    }
}

#[path = "dispatch_decision_surface.rs"]
mod decision_surface_options;
use decision_surface_options::*;

struct HealthDispatchArgs<'a> {
    max_cyclomatic: Option<u16>,
    max_cognitive: Option<u16>,
    max_crap: Option<f64>,
    max_file_lines: Option<u32>,
    top: Option<usize>,
    sort: health::SortBy,
    complexity: bool,
    complexity_breakdown: bool,
    file_scores: bool,
    coverage_gaps: bool,
    hotspots: bool,
    ownership: bool,
    ownership_emails: Option<fallow_config::EmailMode>,
    targets: bool,
    type_coupling: bool,
    css: bool,
    effort: Option<EffortFilter>,
    score: bool,
    min_score: Option<f64>,
    min_severity: Option<fallow_output::FindingSeverity>,
    report_only: bool,
    fail_on_stale_baseline: bool,
    fail_on_parse_error: bool,
    since: Option<&'a str>,
    min_commits: Option<u32>,
    save_snapshot: Option<&'a Option<String>>,
    trend: bool,
    trend_from: Option<&'a std::path::Path>,
    coverage: Option<&'a std::path::Path>,
    coverage_root: Option<&'a std::path::Path>,
    runtime_coverage: Option<&'a std::path::Path>,
    mutation_report: Option<&'a std::path::Path>,
    min_invocations_hot: u64,
    min_observation_volume: Option<u32>,
    low_traffic_threshold: Option<f64>,
    scope: Option<std::path::PathBuf>,
}

pub type ResolvedHealthCoverageInputs = fallow_api::CoverageInputs;

/// Resolve Istanbul coverage inputs for `health`, bare combined mode, and
/// `audit` (#2359) with the precedence owned by
/// [`fallow_api::resolve_coverage_inputs`]: the CLI flag, then
/// `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT` (read here, at the CLI
/// boundary), then `health.coverage` / `health.coverageRoot`. Auto-detection
/// of `coverage/coverage-final.json` stays in the engine and only applies when
/// every layer is empty. `config_health` is consulted only when both a flag
/// and an env var are absent for at least one input, so a command that has
/// not loaded config yet can defer that load. A relative winning root is a
/// structured exit 2 before any analysis starts.
pub fn resolve_coverage_inputs(
    cli_coverage: Option<&std::path::Path>,
    cli_coverage_root: Option<&std::path::Path>,
    output: fallow_config::OutputFormat,
    config_health: impl FnOnce() -> Result<fallow_config::HealthConfig, ExitCode>,
) -> Result<ResolvedHealthCoverageInputs, ExitCode> {
    let explicit = fallow_api::CoverageInputs {
        coverage: cli_coverage.map(std::path::Path::to_path_buf),
        coverage_root: cli_coverage_root.map(std::path::Path::to_path_buf),
    };
    let env = fallow_api::CoverageInputs {
        coverage: path_from_env("FALLOW_COVERAGE"),
        coverage_root: path_from_env("FALLOW_COVERAGE_ROOT"),
    };
    let config_health = if fallow_api::CoverageInputs::needs_config_layer(&explicit, &env) {
        Some(config_health()?)
    } else {
        None
    };

    fallow_api::resolve_coverage_inputs(explicit, env, config_health.as_ref())
        .map_err(|err| emit_error(&err.to_string(), 2, output))
}

/// [`resolve_coverage_inputs`] for commands that load config lazily: the
/// config is read only when a flag and env var are both absent.
pub fn resolve_health_coverage_inputs(
    dispatch: &DispatchContext<'_>,
    cli_coverage: Option<&std::path::Path>,
    cli_coverage_root: Option<&std::path::Path>,
) -> Result<ResolvedHealthCoverageInputs, ExitCode> {
    resolve_coverage_inputs(cli_coverage, cli_coverage_root, dispatch.output, || {
        Ok(load_config(
            dispatch.root,
            &dispatch.cli.config,
            LoadConfigArgs {
                output: dispatch.output,
                no_cache: dispatch.cli.no_cache,
                threads: dispatch.threads,
                production: dispatch.cli.production,
                quiet: dispatch.quiet,
                allow_remote_extends: dispatch.cli.allow_remote_extends,
            },
        )?
        .health)
    })
}

fn path_from_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn validate_health_report_only_gate(
    report_only: bool,
    min_score: Option<f64>,
    min_severity: Option<fallow_output::FindingSeverity>,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    if report_only && (min_score.is_some() || min_severity.is_some()) {
        return Err(emit_error(
            "--report-only cannot be combined with --min-score or --min-severity. \
             --report-only always exits 0; drop it to gate on score/severity, or \
             drop the gate flags to stay advisory.",
            2,
            output,
        ));
    }

    Ok(())
}

fn resolve_runtime_coverage_options(
    runtime_coverage: Option<&std::path::Path>,
    min_invocations_hot: u64,
    min_observation_volume: Option<u32>,
    low_traffic_threshold: Option<f64>,
    output: fallow_config::OutputFormat,
) -> Result<Option<fallow_engine::health::RuntimeCoverageOptions>, ExitCode> {
    let Some(path) = runtime_coverage else {
        return Ok(None);
    };

    health::coverage::prepare_options(
        path,
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        output,
    )
    .map(Some)
}

fn dispatch_health(dispatch: &DispatchContext<'_>, args: &HealthDispatchArgs<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let (output, _quiet, _fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    if let Err(code) = validate_health_report_only_gate(
        args.report_only,
        args.min_score,
        args.min_severity,
        output,
    ) {
        return code;
    }
    let runtime_coverage = match resolve_runtime_coverage_options(
        args.runtime_coverage,
        args.min_invocations_hot,
        args.min_observation_volume,
        args.low_traffic_threshold,
        output,
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::Health),
        Err(code) => return code,
    };
    let coverage_inputs =
        match resolve_health_coverage_inputs(dispatch, args.coverage, args.coverage_root) {
            Ok(inputs) => inputs,
            Err(code) => return code,
        };
    let run = derive_health_dispatch_run(args, output, &coverage_inputs, runtime_coverage);
    let resolved = ResolvedHealthDispatch { run, production };
    // `--report-only` is a request never to fail, so the gate stands down and
    // says so instead of judging a baseline whose verdict cannot count.
    if args.report_only && cli.fail_on_baseline_growth {
        baseline_growth::note_stood_down(
            dispatch.growth_flags(),
            "health --report-only never fails a run",
        );
        return run_health_dispatch(dispatch, args, resolved);
    }
    let growth_targets = baseline_growth::targets(&[(
        cli.baseline.as_deref(),
        fallow_engine::baseline::BaselineKind::Health,
    )]);
    baseline_growth::run_gated(
        &dispatch.growth_ctx(baseline_growth::GrowthOwner::Health),
        &growth_targets,
        || run_health_dispatch(dispatch, args, resolved),
    )
}

fn derive_health_dispatch_run<'a>(
    args: &'a HealthDispatchArgs<'a>,
    output: fallow_config::OutputFormat,
    coverage_inputs: &'a ResolvedHealthCoverageInputs,
    runtime_coverage: Option<fallow_engine::health::RuntimeCoverageOptions>,
) -> fallow_engine::health::HealthRunOptions<'a> {
    let mut run = fallow_engine::health::derive_health_run_options(
        fallow_engine::health::HealthRunOptionsInput {
            output,
            thresholds: health_threshold_overrides(args),
            top: args.top,
            sort: args.sort.clone().into(),
            complexity: args.complexity,
            file_scores: args.file_scores,
            coverage_gaps: args.coverage_gaps,
            hotspots: args.hotspots,
            ownership: args.ownership,
            ownership_emails: args.ownership_emails,
            targets: args.targets,
            css: args.css,
            effort: args.effort.map(EffortFilter::to_estimate),
            score: args.score,
            gates: health_gate_options(args),
            snapshot_requested: args.save_snapshot.is_some(),
            trend: args.trend,
            since: args.since,
            min_commits: args.min_commits,
            coverage_inputs: health_coverage_inputs(coverage_inputs),
            runtime_coverage,
        },
    );
    if args.type_coupling && !run.sections.any_section {
        run.sections = fallow_engine::health::DerivedHealthSections {
            any_section: true,
            complexity: false,
            file_scores: false,
            coverage_gaps: false,
            hotspots: false,
            targets: false,
            css: false,
            score: false,
            force_full: false,
            score_only_output: false,
        };
    }
    run
}

fn health_threshold_overrides(
    args: &HealthDispatchArgs<'_>,
) -> fallow_engine::health::HealthThresholdOverrides {
    fallow_engine::health::HealthThresholdOverrides {
        max_cyclomatic: args.max_cyclomatic,
        max_cognitive: args.max_cognitive,
        max_crap: args.max_crap,
        max_file_lines: args.max_file_lines,
    }
}

fn health_gate_options(args: &HealthDispatchArgs<'_>) -> fallow_engine::health::HealthGateOptions {
    fallow_engine::health::HealthGateOptions {
        min_score: args.min_score,
        min_severity: args.min_severity,
        report_only: args.report_only,
        fail_on_stale_baseline: args.fail_on_stale_baseline,
        fail_on_parse_error: args.fail_on_parse_error,
        fail_on_issues: false,
    }
}

fn health_coverage_inputs(
    coverage_inputs: &ResolvedHealthCoverageInputs,
) -> fallow_engine::health::HealthCoverageInputs<'_> {
    fallow_engine::health::HealthCoverageInputs {
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        coverage_relocated: false,
    }
}

/// Resolved inputs threaded from `dispatch_health` into the `HealthOptions`
/// builder. Owns the normalized engine run contract and resolved production
/// mode.
struct ResolvedHealthDispatch<'a> {
    run: fallow_engine::health::HealthRunOptions<'a>,
    production: bool,
}

/// Build `HealthOptions` from the parsed args plus the resolved dispatch inputs,
/// then run the health analysis.
fn run_health_dispatch(
    dispatch: &DispatchContext<'_>,
    args: &HealthDispatchArgs<'_>,
    resolved: ResolvedHealthDispatch<'_>,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let mut run = resolved.run;
    run.gates.fail_on_issues = fail_on_issues;
    let sections = run.sections;
    let production = resolved.production;
    health::run_health(
        &HealthOptions {
            root: dispatch.root,
            config_path: &cli.config,
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            quiet,
            thresholds: run.thresholds,
            top: run.top,
            sort: run.sort,
            production,
            production_override: Some(production),
            allow_remote_extends: cli.allow_remote_extends,
            changed_since: cli.changed_since.as_deref(),
            diff_index: None,
            use_shared_diff_index: true,
            workspace: cli.workspace.as_deref(),
            changed_workspaces: cli.changed_workspaces.as_deref(),
            baseline: cli.baseline.as_deref(),
            save_baseline: cli.save_baseline.as_deref(),
            baseline_mode: cli.baseline_mode.unwrap_or_default().into(),
            baseline_mode_explicit: cli.baseline_mode.is_some(),
            complexity: sections.complexity,
            file_scores: sections.file_scores,
            coverage_gaps: sections.coverage_gaps,
            config_activates_coverage_gaps: !sections.any_section,
            hotspots: sections.hotspots,
            ownership: run.ownership,
            ownership_emails: run.ownership_emails,
            targets: sections.targets,
            css: sections.css,
            css_deep: false,
            force_full: sections.force_full,
            score_only_output: sections.score_only_output,
            enforce_coverage_gap_gate: true,
            effort: run.effort,
            score: sections.score,
            gates: run.gates,
            since: run.since,
            min_commits: run.min_commits,
            explain: cli.explain,
            summary: cli.summary,
            save_snapshot: args
                .save_snapshot
                .map(|opt| PathBuf::from(opt.as_deref().unwrap_or_default())),
            trend: args.trend,
            coverage_inputs: run.coverage_inputs,
            performance: cli.performance,
            runtime_coverage: run.runtime_coverage,
            churn_file: cli.churn_file.as_deref(),
            analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity::default(),
            complexity_breakdown: args.complexity_breakdown,
            group_by: cli.group_by.map(Into::into),
            group_filter: cli.group.as_deref(),
            trend_from: args.trend_from,
            scope: args.scope.clone(),
        },
        dispatch.json_style,
        &health::TypeAwareHealthOptions {
            enabled: cli.type_aware_override(),
            requested: args.type_coupling,
            unfiltered: health_type_coupling_is_default_section(args),
            projects: &cli.type_aware_project,
            require: cli.type_aware_require.map(Into::into),
        },
        args.mutation_report,
    )
}

fn health_type_coupling_is_default_section(args: &HealthDispatchArgs<'_>) -> bool {
    !args.complexity
        && !args.file_scores
        && !args.coverage_gaps
        && !args.hotspots
        && !args.ownership
        && !args.targets
        && !args.css
        && !args.score
        && args.min_score.is_none()
        && args.min_severity.is_none()
        && args.runtime_coverage.is_none()
}
