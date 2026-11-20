//! `fallow config` subcommand: show the resolved config and which file was loaded.
//!
//! Mirrors `eslint --print-config`, `dprint output-resolved-config`, and similar
//! ecosystem patterns. Closes the "is my config even loaded?" silent-failure gap.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat};

use crate::error::emit_input_error;
use crate::exit_codes::RESOURCE_UNAVAILABLE_EXIT_CODE as EXIT_NO_CONFIG;

/// Run the `fallow config` subcommand.
///
/// - `path_only = false` (default): print the JSON-serialized config (with
///   `extends` resolved) to stdout, and the `loaded config: <path>` provenance
///   line to stderr (unless `quiet`), so stdout stays clean, parseable JSON that
///   pipes straight into `jq` regardless of `--format`.
/// - `path_only = true`: print only the path to stdout, one line, no JSON.
///   Easier to consume from shell scripts.
///
/// When `explicit_config` is `Some`, that path is loaded directly (matching
/// the global `--config` flag's semantics elsewhere in the CLI). Otherwise
/// `find_and_load` walks up from `root` looking for a config file.
///
/// `output` selects the error envelope: `OutputFormat::Json` emits structured
/// `{"error": true, "message": ..., "exit_code": 2}` on stdout for failed
/// loads (matching the rest of the CLI's error contract); other formats
/// render to stderr.
#[cfg(test)]
pub fn run_config(
    root: &Path,
    explicit_config: Option<&Path>,
    path_only: bool,
    output: OutputFormat,
    quiet: bool,
) -> ExitCode {
    run_config_with_options(RunConfigInput {
        root,
        explicit_config,
        path_only,
        output,
        quiet,
        json_style: crate::json_style::JsonStyle::Compact,
        load_options: fallow_config::ConfigLoadOptions::default(),
    })
}

#[derive(Clone, Copy)]
pub struct RunConfigInput<'a> {
    pub(crate) root: &'a Path,
    pub(crate) explicit_config: Option<&'a Path>,
    pub(crate) path_only: bool,
    pub(crate) output: OutputFormat,
    pub(crate) quiet: bool,
    pub(crate) json_style: crate::json_style::JsonStyle,
    pub(crate) load_options: fallow_config::ConfigLoadOptions,
}

pub fn run_config_with_options(input: RunConfigInput<'_>) -> ExitCode {
    let output = input.output;
    let result = fallow_engine::project_config::load_config_file(
        input.root,
        input.explicit_config,
        fallow_engine::project_config::ConfigFileOptions::new(
            input.load_options.allow_remote_extends,
            &fallow_config::EnvSnapshot::capture(),
        ),
    )
    .map(|file| file.path.map(|path| (file.config, path)))
    .map_err(|err| crate::runtime_support::config_load_error_message(&err));

    match result {
        Ok(Some((config, path))) => {
            crate::runtime_support::warn_unknown_security_categories(&config.security);
            if let Err(errors) = config.validate_resolved_boundaries(input.root) {
                let joined = errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n  - ");
                let msg = format!("invalid boundary configuration:\n  - {joined}");
                return emit_input_error(&msg, output);
            }
            if input.path_only {
                // Machine payload for `--path`: the config file path on stdout.
                println!("{}", path.display());
            } else {
                // The resolved config JSON is the machine payload on stdout; the
                // provenance line is chrome and goes to stderr so `fallow config
                // | jq` (any format) gets clean, parseable JSON.
                if !input.quiet {
                    eprintln!("loaded config: {}", path.display());
                }
                match input.json_style.serialize(&config) {
                    Ok(json) => crate::report::sink::stdoutln!("{json}"),
                    Err(e) => {
                        return emit_input_error(
                            &format!("failed to serialize config: {e}"),
                            output,
                        );
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            if input.path_only {
                // No config file exists, so there is no path to print. Preserve
                // the not-found exit code for scripts that probe for a config.
                return ExitCode::from(EXIT_NO_CONFIG);
            }
            // `fallow config` answers "what am I running with?" On a zero-config
            // project (fully supported) that is the resolved default config, and
            // the command succeeds: exit 0 with the defaults on stdout so
            // `fallow config | jq` works and an agent does not read a non-zero
            // code as an error. The provenance line stays stderr chrome.
            if !input.quiet {
                eprintln!("no config file found, using defaults");
            }
            match input.json_style.serialize(&FallowConfig::default()) {
                Ok(json) => {
                    crate::report::sink::stdoutln!("{json}");
                    ExitCode::SUCCESS
                }
                Err(e) => emit_input_error(&format!("failed to serialize config: {e}"), output),
            }
        }
        Err(e) => emit_input_error(&e, output),
    }
}

/// Run `fallow config`: the view of the run with `--resolved`, else the
/// loaded config file.
pub fn run_config_command(
    dispatch: &crate::DispatchContext<'_>,
    args: &crate::cli_args::ConfigArgs,
) -> ExitCode {
    if args.resolved {
        return run_resolved_view(dispatch, args);
    }
    run_config_with_options(RunConfigInput {
        root: dispatch.root,
        explicit_config: dispatch.cli.config.as_deref(),
        path_only: args.path,
        output: dispatch.output,
        quiet: dispatch.quiet,
        json_style: dispatch.json_style,
        load_options: fallow_config::ConfigLoadOptions::new(dispatch.cli.allow_remote_extends),
    })
}

/// Run `fallow config --resolved`: resolve the run of the command that
/// `--for` names, without its side effects, and print the view of the run as
/// JSON on stdout for each output format. A path under the root prints
/// relative to the root. The run notices print as notes on stderr when the
/// user selected the human format. A resolve error, and `--only` or `--skip`
/// with a `--for` command of one analysis, print as a structured error and
/// give exit code 2.
pub fn run_resolved_view(
    dispatch: &crate::DispatchContext<'_>,
    args: &crate::cli_args::ConfigArgs,
) -> ExitCode {
    let output = dispatch.output;
    if let Some(message) = single_analysis_list_error(args) {
        return emit_input_error(&message, output);
    }
    let Some(inputs) = dispatch.run_inputs else {
        return emit_input_error("this command resolves no run", output);
    };
    let run = match fallow_engine::resolved_run::resolve_run(inputs) {
        Ok(run) => run,
        Err(err) => {
            let given = dispatch.cli.config.as_deref();
            return emit_input_error(
                &crate::cli_production::resolve_error_message(&err, given),
                output,
            );
        }
    };
    let mut view = match serde_json::to_value(run.view()) {
        Ok(view) => view,
        Err(e) => return emit_input_error(&format!("failed to serialize the run: {e}"), output),
    };
    let notes = crate::cli_format::selected_output(dispatch.cli);
    crate::report::notices::print_run_notices(run.notices(), notes, dispatch.quiet);
    relativize_paths(&mut view, run.root().value());
    match dispatch.json_style.serialize(&view) {
        Ok(json) => {
            crate::report::sink::stdoutln!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => emit_input_error(&format!("failed to serialize the run: {e}"), output),
    }
}

/// The error for `--only` or `--skip` with a `--for` command that runs one
/// analysis: that command has no selection, so the flag would have no effect.
fn single_analysis_list_error(args: &crate::cli_args::ConfigArgs) -> Option<String> {
    use crate::cli_args::ForCommand;
    let command = match args.for_command? {
        ForCommand::Combined | ForCommand::Audit => return None,
        ForCommand::DeadCode => "dead-code",
        ForCommand::Dupes => "dupes",
        ForCommand::Health => "health",
    };
    let flag = if !args.only.is_empty() {
        "--only"
    } else if !args.skip.is_empty() {
        "--skip"
    } else {
        return None;
    };
    Some(format!(
        "{flag} has no effect with --for {command}, because that command runs one analysis. \
         Use --for combined or --for audit."
    ))
}

/// Rewrite every path string under `root` relative to `root`. The `root`
/// member keeps its absolute path, so a reader can join the others to it.
fn relativize_paths(view: &mut serde_json::Value, root: &Path) {
    fn walk(value: &mut serde_json::Value, prefixes: &[String]) {
        match value {
            serde_json::Value::String(text) => {
                if let Some(rest) = prefixes.iter().find_map(|prefix| text.strip_prefix(prefix)) {
                    *text = rest.replace('\\', "/");
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    walk(item, prefixes);
                }
            }
            serde_json::Value::Object(members) => {
                members
                    .values_mut()
                    .for_each(|member| walk(member, prefixes));
            }
            _ => {}
        }
    }
    let display = root.display().to_string();
    let prefixes = [
        format!("{display}/"),
        format!("{display}\\"),
        format!("{}/", display.replace('\\', "/")),
    ];
    let root_member = view.get("root").cloned();
    walk(view, &prefixes);
    if let (Some(member), Some(object)) = (root_member, view.as_object_mut()) {
        object.insert("root".to_owned(), member);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exit_codes::exit_invalid_input;

    #[test]
    fn run_config_no_file_prints_defaults_and_succeeds() {
        // A zero-config project is fully supported: `fallow config` answers
        // "what am I running with?" with the resolved defaults and exits 0, so an
        // agent does not misread a non-zero code as an error.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let exit = run_config(dir.path(), None, false, OutputFormat::Human, false);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_with_file_returns_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{"entry": ["src/index.ts"]}"#,
        )
        .unwrap();
        let exit = run_config(dir.path(), None, false, OutputFormat::Human, false);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_path_only_with_file_returns_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".fallowrc.json"), "{}").unwrap();
        let exit = run_config(dir.path(), None, true, OutputFormat::Human, false);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_path_only_no_file_returns_exit_3() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let exit = run_config(dir.path(), None, true, OutputFormat::Human, false);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_NO_CONFIG))
        );
    }

    #[test]
    fn run_config_explicit_config_path_is_used_over_discovery() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let discovered = dir.path().join(".fallowrc.json");
        std::fs::write(&discovered, r#"{"entry": ["src/discovered.ts"]}"#).unwrap();
        let explicit = dir.path().join("explicit.json");
        std::fs::write(&explicit, r#"{"entry": ["src/explicit.ts"]}"#).unwrap();

        let exit = run_config(
            dir.path(),
            Some(&explicit),
            true,
            OutputFormat::Human,
            false,
        );
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_explicit_config_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.json");
        let exit = run_config(
            dir.path(),
            Some(&missing),
            false,
            OutputFormat::Human,
            false,
        );
        assert_eq!(format!("{exit:?}"), format!("{:?}", exit_invalid_input()));
    }

    #[test]
    fn run_config_rejects_unknown_boundary_zone_reference() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(
            dir.path().join(".fallowrc.json"),
            r#"{
                "boundaries": {
                    "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
                    "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
                }
            }"#,
        )
        .unwrap();
        let exit = run_config(dir.path(), None, false, OutputFormat::Human, false);
        assert_eq!(format!("{exit:?}"), format!("{:?}", exit_invalid_input()));
    }
}
