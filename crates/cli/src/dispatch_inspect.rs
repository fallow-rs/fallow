//! Dispatch of `fallow inspect` and `fallow cloud`.

use std::process::ExitCode;

use crate::cli_args::{CloudCli, Command};
use crate::coverage::login as cloud_login;
use crate::error::emit_error;
use crate::runtime_support::{ConfigLoadOptions, load_config_for_analysis};
use crate::{DispatchContext, inspect, json_style, selector};

/// Run `fallow cloud login` or `fallow cloud logout`.
pub(super) fn dispatch_cloud(
    subcommand: CloudCli,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
) -> ExitCode {
    cloud_login::run(&map_cloud_subcommand(subcommand), output, json_style)
}

/// Run `fallow inspect`.
pub(super) fn dispatch_inspect(dispatch: &DispatchContext<'_>, command: Command) -> ExitCode {
    let Command::Inspect {
        file,
        symbol,
        symbol_chain,
        churn,
        cloud,
        repo,
    } = command
    else {
        unreachable!("dispatch_inspect gets only Command::Inspect");
    };
    dispatch_inspect_command(
        dispatch,
        file,
        symbol,
        InspectFlags {
            symbol_chain,
            churn,
            cloud,
            repo,
        },
    )
}

/// The opt-in flags of `fallow inspect`.
struct InspectFlags {
    symbol_chain: bool,
    churn: bool,
    cloud: bool,
    repo: Option<String>,
}

fn map_cloud_subcommand(sub: CloudCli) -> cloud_login::CloudCommand {
    match sub {
        CloudCli::Login {
            machine_name,
            api_url,
        } => cloud_login::CloudCommand::Login(cloud_login::LoginArgs {
            machine_name,
            api_url,
        }),
        CloudCli::Logout { api_url } => cloud_login::CloudCommand::Logout { api_url },
    }
}

fn dispatch_inspect_command(
    dispatch: &DispatchContext<'_>,
    file: Option<String>,
    symbol: Option<String>,
    flags: InspectFlags,
) -> ExitCode {
    let InspectFlags {
        symbol_chain,
        churn,
        cloud,
        repo,
    } = flags;
    let target = match (file, symbol) {
        (Some(file), None) => inspect::InspectTarget::File { file },
        (None, Some(symbol)) => match selector::parse_file_symbol_selector(&symbol) {
            Some((file, export_name)) => inspect::InspectTarget::Symbol {
                file: file.to_string(),
                export_name: export_name.to_string(),
            },
            None => {
                return emit_error(
                    "--symbol must be formatted as FILE:EXPORT",
                    2,
                    dispatch.output,
                );
            }
        },
        _ => {
            return emit_error(
                "inspect requires exactly one of --file or --symbol",
                2,
                dispatch.output,
            );
        }
    };

    let churn_config = if churn {
        match load_config_for_analysis(
            dispatch.root,
            &dispatch.cli.config,
            ConfigLoadOptions {
                output: dispatch.output,
                no_cache: dispatch.cli.no_cache,
                threads: dispatch.threads,
                production_override: None,
                quiet: dispatch.quiet,
                allow_remote_extends: dispatch.cli.allow_remote_extends,
            },
            fallow_config::ProductionAnalysis::Health,
        ) {
            Ok(config) => Some(config),
            Err(code) => return code,
        }
    } else {
        None
    };

    inspect::run_inspect(&inspect::InspectOptions {
        root: dispatch.root,
        config_path: dispatch.cli.config.as_ref(),
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: dispatch.cli.no_cache,
        no_production: dispatch.cli.no_production,
        max_file_size: dispatch.cli.max_file_size,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        production: dispatch.cli.production,
        workspace: dispatch.cli.workspace.as_ref(),
        target,
        churn_cache_dir: churn_config
            .as_ref()
            .map(|config| config.cache_dir.as_path()),
        symbol_chain,
        cloud: cloud.then_some(inspect::InspectCloudOptions { repo }),
        type_aware: dispatch.cli.type_aware_override(),
        type_aware_projects: &dispatch.cli.type_aware_project,
        type_aware_require: dispatch.cli.type_aware_require.map(Into::into),
    })
}
