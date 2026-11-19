//! The run inputs of a parsed command line.
//!
//! [`command_run_inputs`] is the one place that maps a subcommand to a
//! [`CommandKind`] and a [`SelectionInput`], and the global flags to the
//! fields of [`RunInputs`]. The resolver of `fallow-engine` then decides the
//! production mode of each analysis from the flags, the `FALLOW_PRODUCTION*`
//! variables that the CLI reads for the command, and the config.

use fallow_config::{
    CommandKind, EnvSnapshot, HostInputs, OutputFormat, PlannedAnalysis, RunInputs, SelectionInput,
    Surface, TypeAwareInputs,
};
use fallow_types::sourced::{Given, Origin};

use crate::cli_args::{BaselinesCli, Cli, Command, CoverageCli, RulePackCli};

/// The production inputs that a command reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProductionRule {
    /// `--production`, `--no-production`, the per-analysis flags, the
    /// variables and the config.
    Full,
    /// The config only: the command ignores the production flags.
    ConfigOnly,
}

/// The command, the selection and the production rule of one subcommand.
struct CommandRoute {
    command: CommandKind,
    selection: SelectionInput,
    production: ProductionRule,
}

impl CommandRoute {
    const fn new(
        command: CommandKind,
        selection: SelectionInput,
        production: ProductionRule,
    ) -> Self {
        Self {
            command,
            selection,
            production,
        }
    }

    const fn single(
        command: CommandKind,
        analysis: PlannedAnalysis,
        production: ProductionRule,
    ) -> Self {
        Self::new(command, SelectionInput::Single(analysis), production)
    }
}

/// `--only` and `--skip` as a selection input. `architecture` is a part of
/// the dead-code analysis.
fn lists(only: &[crate::AnalysisKind], skip: &[crate::AnalysisKind]) -> SelectionInput {
    let (only_planned, skip_planned) = crate::combined::planned_lists(only, skip);
    SelectionInput::Lists {
        only: only_planned,
        skip: skip_planned,
        origin: Origin::Flag(if only.is_empty() { "--skip" } else { "--only" }),
    }
}

/// The selection of every analysis with the default rule of the command.
const fn all_by_default() -> SelectionInput {
    SelectionInput::Lists {
        only: Vec::new(),
        skip: Vec::new(),
        origin: Origin::Flag("--only"),
    }
}

/// The selection that names every analysis.
fn every_analysis_named(origin: &'static str) -> SelectionInput {
    SelectionInput::Lists {
        only: vec![
            PlannedAnalysis::DeadCode,
            PlannedAnalysis::Duplication,
            PlannedAnalysis::Health,
        ],
        skip: Vec::new(),
        origin: Origin::Flag(origin),
    }
}

/// The route of the parsed command line, or `None` for a command that loads
/// no config.
fn command_route(cli: &Cli) -> Option<CommandRoute> {
    let Some(command) = &cli.command else {
        return Some(CommandRoute::new(
            CommandKind::Combined,
            lists(&cli.only, &cli.skip),
            ProductionRule::Full,
        ));
    };
    analysis_route(command)
        .or_else(|| {
            dead_code_kind(command).map(|kind| {
                CommandRoute::single(kind, PlannedAnalysis::DeadCode, ProductionRule::Full)
            })
        })
        .or_else(|| {
            config_only_kind(command).map(|kind| {
                CommandRoute::single(kind, PlannedAnalysis::DeadCode, ProductionRule::ConfigOnly)
            })
        })
}

/// The route of an analysis command, of `trace`, and of a command that
/// selects every analysis.
fn analysis_route(command: &Command) -> Option<CommandRoute> {
    use CommandKind as Kind;
    use PlannedAnalysis::{DeadCode, Duplication, Health};
    use ProductionRule::{ConfigOnly, Full};
    let route = match command {
        Command::Check { .. } | Command::Architecture { .. } => {
            CommandRoute::single(Kind::DeadCode, DeadCode, Full)
        }
        Command::Dupes { .. } => CommandRoute::single(Kind::Dupes, Duplication, Full),
        Command::Health { .. } => CommandRoute::single(Kind::Health, Health, Full),
        Command::Audit {
            only,
            skip,
            brief,
            walkthrough_guide,
            walkthrough,
            walkthrough_file,
            ..
        } => {
            // `--brief` and the walkthrough flags name every sub-analysis.
            let selection =
                if *brief || *walkthrough_guide || *walkthrough || walkthrough_file.is_some() {
                    every_analysis_named("--brief")
                } else {
                    lists(only, skip)
                };
            CommandRoute::new(Kind::Audit, selection, Full)
        }
        // The decision surface runs `audit --brief`.
        Command::DecisionSurface { .. } => {
            CommandRoute::new(Kind::Audit, every_analysis_named("decision-surface"), Full)
        }
        Command::Inspect { .. } => CommandRoute::new(Kind::Inspect, all_by_default(), Full),
        Command::Baselines {
            subcommand: BaselinesCli::Prune { .. },
        } => CommandRoute::new(Kind::BaselinesPrune, all_by_default(), Full),
        Command::Trace {
            dependency, path, ..
        } => {
            let kind = if dependency.is_some() {
                Kind::TraceDependency
            } else if path.len() == 2 {
                Kind::TracePath
            } else {
                Kind::TraceChain
            };
            let production = if kind == Kind::TraceDependency {
                Full
            } else {
                ConfigOnly
            };
            CommandRoute::single(kind, DeadCode, production)
        }
        Command::Config(args) if args.resolved => config_view_route(args),
        _ => return None,
    };
    Some(route)
}

/// The route of `fallow config --resolved`: the route of the command that
/// `--for` names, with the selection and production flags of `config`.
fn config_view_route(args: &crate::cli_args::ConfigArgs) -> CommandRoute {
    use crate::cli_args::ForCommand;
    use PlannedAnalysis::{DeadCode, Duplication, Health};
    use ProductionRule::Full;
    match args.for_command.unwrap_or(ForCommand::Combined) {
        ForCommand::Combined => {
            CommandRoute::new(CommandKind::Combined, lists(&args.only, &args.skip), Full)
        }
        ForCommand::Audit => {
            CommandRoute::new(CommandKind::Audit, lists(&args.only, &args.skip), Full)
        }
        ForCommand::DeadCode => CommandRoute::single(CommandKind::DeadCode, DeadCode, Full),
        ForCommand::Dupes => CommandRoute::single(CommandKind::Dupes, Duplication, Full),
        ForCommand::Health => CommandRoute::single(CommandKind::Health, Health, Full),
    }
}

/// The command that runs on the files of the dead-code production mode and
/// reads that mode from the flags, the variables and the config.
const fn dead_code_kind(command: &Command) -> Option<CommandKind> {
    let kind = match command {
        Command::List { .. } | Command::Workspaces => CommandKind::List,
        Command::Fix { .. } => CommandKind::Fix,
        Command::Watch { .. } => CommandKind::Watch,
        Command::Viz { .. } => CommandKind::Viz,
        Command::Flags { .. } => CommandKind::Flags,
        Command::Suppressions { .. } => CommandKind::Suppressions,
        _ => return None,
    };
    Some(kind)
}

/// The dead-code command that reads production mode from the config only.
const fn config_only_kind(command: &Command) -> Option<CommandKind> {
    let kind = match command {
        Command::TraceError { .. } => CommandKind::TraceError,
        Command::Security { .. } => CommandKind::Security,
        Command::Coverage {
            subcommand: CoverageCli::Analyze { .. },
        } => CommandKind::CoverageAnalyze,
        Command::Coverage {
            subcommand:
                CoverageCli::UploadInventory { .. }
                | CoverageCli::UploadSourceMaps { .. }
                | CoverageCli::UploadStaticFindings { .. },
        } => CommandKind::CoverageUpload,
        Command::Guard { .. } => CommandKind::Guard,
        Command::RulePack {
            subcommand: RulePackCli::List | RulePackCli::Test { .. },
        } => CommandKind::RulePack,
        Command::Mutation { .. } => CommandKind::Mutation,
        Command::SimilarCode { .. } => CommandKind::SimilarCode,
        Command::PluginCheck => CommandKind::PluginCheck,
        Command::Doctor => CommandKind::Doctor,
        _ => return None,
    };
    Some(kind)
}

/// The production flag of `coverage analyze`, which the subcommand owns.
const fn coverage_analyze_production(command: Option<&Command>) -> bool {
    matches!(
        command,
        Some(Command::Coverage {
            subcommand: CoverageCli::Analyze {
                production: true,
                ..
            },
        })
    )
}

/// The per-analysis production flags of the bare run or of `audit`.
const fn per_analysis_flags(cli: &Cli) -> [bool; 3] {
    match &cli.command {
        None => [
            cli.production_dead_code,
            cli.production_health,
            cli.production_dupes,
        ],
        Some(Command::Audit {
            production_dead_code,
            production_health,
            production_dupes,
            ..
        }) => [*production_dead_code, *production_health, *production_dupes],
        Some(Command::Config(args)) => [
            args.production_dead_code,
            args.production_health,
            args.production_dupes,
        ],
        Some(_) => [false; 3],
    }
}

/// The value of a flag.
const fn given<T>(value: T, flag: &'static str) -> Given<T> {
    Given {
        value,
        origin: Origin::Flag(flag),
    }
}

/// A flag that is set as a given `true` value.
const fn flag_on(set: bool, flag: &'static str) -> Option<Given<bool>> {
    if set {
        Some(Given {
            value: true,
            origin: Origin::Flag(flag),
        })
    } else {
        None
    }
}

/// The run inputs of a parsed command line. `None` for a command that loads
/// no config.
///
/// The output format and the quiet flag come from `--format` and `--quiet`.
/// A caller that resolves `FALLOW_FORMAT`, `FALLOW_QUIET` or `--ci` sets the
/// two fields after the call.
pub fn command_run_inputs(cli: &Cli, env: EnvSnapshot) -> Option<RunInputs> {
    let route = command_route(cli)?;
    let mut inputs = RunInputs::new(Surface::Cli, env, route.command, route.selection);
    inputs.root = cli.root.clone().map(|root| given(root, "--root"));
    inputs.config_path = cli.config.clone().map(|path| given(path, "--config"));
    inputs.allow_remote_extends = flag_on(cli.allow_remote_extends, "--allow-remote-extends");
    inputs.no_cache = flag_on(cli.no_cache, "--no-cache");
    // `--threads 0` gives the default thread count of the worker pool, so the
    // resolver gets no thread input for it.
    inputs.threads = cli
        .threads
        .filter(|threads| *threads > 0)
        .map(|threads| given(threads, "--threads"));
    inputs.max_file_size_mb = cli.max_file_size.map(|mb| given(mb, "--max-file-size"));
    inputs.package_baselines = cli
        .no_package_baselines
        .then(|| given(false, "--no-package-baselines"));
    inputs.include_entry_exports = flag_on(cli.include_entry_exports, "--include-entry-exports");
    match route.production {
        ProductionRule::Full => {
            inputs.production = if cli.production {
                Some(given(true, "--production"))
            } else if cli.no_production {
                Some(given(false, "--no-production"))
            } else {
                None
            };
            let [dead_code, health, dupes] = per_analysis_flags(cli);
            inputs.production_dead_code = flag_on(dead_code, "--production-dead-code");
            inputs.production_health = flag_on(health, "--production-health");
            inputs.production_dupes = flag_on(dupes, "--production-dupes");
        }
        ProductionRule::ConfigOnly => {
            inputs.production = flag_on(
                coverage_analyze_production(cli.command.as_ref()),
                "--production",
            );
        }
    }
    if cli.dupes_baseline.is_some() {
        inputs.analysis_flags.push((
            PlannedAnalysis::Duplication,
            Origin::Flag("--dupes-baseline"),
        ));
    }
    inputs.type_aware = type_aware_inputs(cli);
    // `--changed-since` and its alias `--base` name the base ref of an audit.
    if route.command == CommandKind::Audit {
        inputs.audit_base = cli.changed_since.clone().map(|base| given(base, "--base"));
    }
    inputs.output = OutputFormat::from(cli.format);
    inputs.quiet = cli.quiet;
    Some(inputs)
}

/// The type-aware flags of a command line.
fn type_aware_inputs(cli: &Cli) -> TypeAwareInputs {
    TypeAwareInputs {
        enabled: cli.type_aware_override().map(|on| {
            given(
                on,
                if on {
                    "--type-aware"
                } else {
                    "--no-type-aware"
                },
            )
        }),
        projects: (!cli.type_aware_project.is_empty())
            .then(|| given(cli.type_aware_project.clone(), "--type-aware-project")),
        require: cli
            .type_aware_require
            .map(|require| given(require.into(), "--type-aware-require")),
    }
}

/// The host of a command that runs through `fallow-api` (`mutation` and
/// `similar-code`), so that its run resolves as [`Surface::Cli`] with the
/// environment snapshot of `inputs`. `None` captures the environment.
pub fn cli_host(inputs: Option<&RunInputs>) -> HostInputs {
    HostInputs {
        surface: Surface::Cli,
        env: inputs.map_or_else(EnvSnapshot::capture, |inputs| inputs.env().clone()),
    }
}

/// The run inputs of the command that the CLI dispatches, with the output
/// format and the quiet flag that the CLI resolved from `--format`,
/// `--quiet`, `--ci`, `FALLOW_FORMAT` and `FALLOW_QUIET`.
pub fn dispatch_inputs(
    cli: &Cli,
    env: EnvSnapshot,
    output: OutputFormat,
    quiet: bool,
) -> Option<RunInputs> {
    let mut inputs = command_run_inputs(cli, env)?;
    inputs.output = output;
    inputs.quiet = quiet;
    Some(inputs)
}

#[cfg(test)]
#[path = "run_parity_tests.rs"]
mod parity_tests;

#[cfg(test)]
#[path = "run_inputs_tests.rs"]
mod tests;
