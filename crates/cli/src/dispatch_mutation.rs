//! Dispatch of `fallow mutation` and `fallow similar-code`. Both commands
//! run through the programmatic API, and the host of the command line makes
//! that run resolve as the CLI.

use std::process::ExitCode;

use crate::cli_args::Command;
use crate::{DispatchContext, error, mutation_cli, similar_code_cli};

/// Run `fallow mutation`.
pub(super) fn dispatch_mutation_command(
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
        return error::emit_input_error_with_style(
            &format!(
                "{flag} does not narrow `fallow mutation` yet; pass the workspace directory as a path, for example `fallow mutation packages/app`"
            ),
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
        // The command printed no `loaded config` line before it resolved a run.
        match (DispatchContext {
            quiet: true,
            ..*dispatch
        })
        .resolve_run()
        {
            Ok(run) => run.config().mutation.clone(),
            Err(code) => return code,
        }
    };
    mutation_cli::run(&mutation_input(dispatch, config, args, subcommand))
}

/// The input of `fallow mutation` for the command line of `dispatch`.
fn mutation_input<'a>(
    dispatch: &DispatchContext<'a>,
    config: fallow_config::MutationConfig,
    args: mutation_cli::MutationArgs,
    subcommand: Option<mutation_cli::MutationSubcommand>,
) -> mutation_cli::MutationCliInput<'a> {
    let cli = dispatch.cli;
    mutation_cli::MutationCliInput {
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
        host: Some(crate::run_inputs::cli_host(dispatch.run_inputs)),
    }
}

/// Run `fallow similar-code`.
pub(super) fn dispatch_similar_code(dispatch: &DispatchContext<'_>, command: Command) -> ExitCode {
    let Command::SimilarCode { path, .. } = &command else {
        unreachable!("dispatch_similar_code gets only Command::SimilarCode");
    };
    let scope = match crate::scope_path::resolve_command_scope(
        dispatch.root,
        dispatch.output,
        path.clone(),
    ) {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    similar_code_cli::run(similar_code_input(dispatch, command, scope))
}

/// The input of `fallow similar-code` for the command line of `dispatch`.
fn similar_code_input<'a>(
    dispatch: &DispatchContext<'a>,
    command: Command,
    scope: Option<crate::scope_path::ScopePath>,
) -> similar_code_cli::SimilarCodeCliInput<'a> {
    let Command::SimilarCode {
        subcommand,
        threshold,
        min_lines,
        top,
        file,
        path: _,
    } = command
    else {
        unreachable!("similar_code_input gets only Command::SimilarCode");
    };
    let cli = dispatch.cli;
    similar_code_cli::SimilarCodeCliInput {
        root: dispatch.root,
        config_path: cli.config.as_deref(),
        allow_remote_extends: cli.allow_remote_extends,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        changed_since: cli.changed_since.as_deref(),
        diff_file: cli.diff_file.as_deref(),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        explain: cli.explain,
        quiet: dispatch.quiet,
        output: dispatch.output,
        json_style: dispatch.json_style,
        threshold,
        min_lines,
        top,
        files: file,
        scope,
        subcommand,
        host: Some(crate::run_inputs::cli_host(dispatch.run_inputs)),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use clap::Parser as _;
    use fallow_config::{EnvSnapshot, HostInputs, RunInputs, Surface};

    use super::*;
    use crate::cli_args::Cli;

    /// The current directory of the run inputs. A host that captures the
    /// process environment has another current directory.
    const MARKER_CWD: &str = "/fallow-dispatch-host-marker";

    fn inputs(cli: &Cli) -> RunInputs {
        let env = EnvSnapshot::none().with_cwd(PathBuf::from(MARKER_CWD));
        crate::run_inputs::command_run_inputs(cli, env).expect("the command loads a config")
    }

    fn context<'a>(cli: &'a Cli, inputs: &'a RunInputs) -> DispatchContext<'a> {
        DispatchContext {
            cli,
            root: Path::new("/repo"),
            output: fallow_config::OutputFormat::Json,
            quiet: true,
            fail_on_issues: false,
            json_style: crate::json_style::JsonStyle::default(),
            threads: 1,
            tolerance: crate::regression::Tolerance::Absolute(0),
            save_regression_file: None,
            save_to_config: false,
            run_inputs: Some(inputs),
        }
    }

    fn assert_cli_host(host: Option<HostInputs>) {
        let host = host.expect("the dispatch passes a host");
        assert_eq!(host.surface, Surface::Cli);
        assert_eq!(host.env.cwd(), Some(Path::new(MARKER_CWD)));
    }

    #[test]
    fn mutation_dispatch_passes_the_cli_host_of_the_command_line() {
        let mut cli = Cli::try_parse_from(["fallow", "mutation"]).expect("argv parses");
        let run_inputs = inputs(&cli);
        let Some(Command::Mutation { subcommand, args }) = cli.command.take() else {
            unreachable!("the argv names the mutation command");
        };
        let dispatch = context(&cli, &run_inputs);
        let input = mutation_input(
            &dispatch,
            fallow_config::MutationConfig::default(),
            args,
            subcommand,
        );
        assert_cli_host(input.host);
    }

    #[test]
    fn similar_code_dispatch_passes_the_cli_host_of_the_command_line() {
        let mut cli = Cli::try_parse_from(["fallow", "similar-code"]).expect("argv parses");
        let run_inputs = inputs(&cli);
        let command = cli.command.take().expect("the argv names a command");
        let dispatch = context(&cli, &run_inputs);
        assert_cli_host(similar_code_input(&dispatch, command, None).host);
    }
}
