//! `fallow config` subcommand: show the resolved config and which file was loaded.
//!
//! Mirrors `eslint --print-config`, `dprint output-resolved-config`, and similar
//! ecosystem patterns. Closes the "is my config even loaded?" silent-failure gap.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat};

use crate::error::emit_error;

/// Exit code for `--path` when no config file was found (there is no path to
/// print). The default resolved-config view instead succeeds and prints the
/// effective defaults, because a zero-config project is fully supported.
const EXIT_NO_CONFIG: u8 = 3;

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
    let result = match input.explicit_config {
        Some(path) => FallowConfig::load_with_options(path, input.load_options)
            .map(|c| Some((c, path.to_path_buf())))
            .map_err(|e| format!("failed to load config '{}': {e}", path.display())),
        None => FallowConfig::find_and_load_with_options(input.root, input.load_options),
    };

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
                return emit_error(&msg, 2, output);
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
                    Ok(json) => println!("{json}"),
                    Err(e) => {
                        return emit_error(&format!("failed to serialize config: {e}"), 2, output);
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
                    println!("{json}");
                    ExitCode::SUCCESS
                }
                Err(e) => emit_error(&format!("failed to serialize config: {e}"), 2, output),
            }
        }
        Err(e) => emit_error(&e, 2, output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::from(2)));
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
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::from(2)));
    }
}
