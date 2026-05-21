//! `fallow config` subcommand: show the resolved config and which file was loaded.
//!
//! Mirrors `eslint --print-config`, `dprint output-resolved-config`, and similar
//! ecosystem patterns. Closes the "is my config even loaded?" silent-failure gap.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat};

use crate::error::emit_error;

/// Exit code when no config file was found (only defaults are in effect).
const EXIT_NO_CONFIG: u8 = 3;

/// Run the `fallow config` subcommand.
///
/// - `path_only = false` (default): print the loaded config path on the first
///   line, followed by the JSON-serialized config (with `extends` resolved).
/// - `path_only = true`: print only the path, one line, no JSON. Easier to
///   consume from shell scripts.
///
/// When `explicit_config` is `Some`, that path is loaded directly (matching
/// the global `--config` flag's semantics elsewhere in the CLI). Otherwise
/// `find_and_load` walks up from `root` looking for a config file.
///
/// `output` selects the error envelope: `OutputFormat::Json` emits structured
/// `{"error": true, "message": ..., "exit_code": 2}` on stdout for failed
/// loads (matching the rest of the CLI's error contract); other formats
/// render to stderr.
pub fn run_config(
    root: &Path,
    explicit_config: Option<&Path>,
    path_only: bool,
    output: OutputFormat,
) -> ExitCode {
    let result = if let Some(path) = explicit_config {
        FallowConfig::load(path)
            .map(|c| Some((c, path.to_path_buf())))
            .map_err(|e| format!("failed to load config '{}': {e}", path.display()))
    } else {
        FallowConfig::find_and_load(root)
    };

    match result {
        Ok(Some((config, path))) => {
            // Mirror the contract the analysis path enforces: an invalid
            // boundary configuration (unknown zone reference, redundant
            // root-prefix) exits 2 at config load. Without this, `fallow config`
            // happily prints a "loaded fine" view of a config that `fallow
            // check` immediately rejects, producing a false signal during
            // debug sessions. Surfaced by review of #468.
            if let Err(errors) = config.validate_resolved_boundaries(root) {
                let joined = errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n  - ");
                let msg = format!("invalid boundary configuration:\n  - {joined}");
                return emit_error(&msg, 2, output);
            }
            if path_only {
                println!("{}", path.display());
            } else {
                println!("loaded config: {}", path.display());
                match serde_json::to_string_pretty(&config) {
                    Ok(json) => println!("{json}"),
                    Err(e) => {
                        return emit_error(&format!("failed to serialize config: {e}"), 2, output);
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Ok(None) => {
            if !path_only {
                println!("no config file found, using defaults");
            }
            // Empty stdout when --path is set; non-zero exit so scripts can detect.
            ExitCode::from(EXIT_NO_CONFIG)
        }
        Err(e) => emit_error(&e, 2, output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_config_no_file_returns_exit_3() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        // No config file in the directory.
        let exit = run_config(dir.path(), None, false, OutputFormat::Human);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_NO_CONFIG))
        );
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
        let exit = run_config(dir.path(), None, false, OutputFormat::Human);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_path_only_with_file_returns_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".fallowrc.json"), "{}").unwrap();
        let exit = run_config(dir.path(), None, true, OutputFormat::Human);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_path_only_no_file_returns_exit_3() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let exit = run_config(dir.path(), None, true, OutputFormat::Human);
        assert_eq!(
            format!("{exit:?}"),
            format!("{:?}", ExitCode::from(EXIT_NO_CONFIG))
        );
    }

    #[test]
    fn run_config_explicit_config_path_is_used_over_discovery() {
        // Confirm `--config` overrides directory walk (the discovered config
        // would be `discovered.json`, but we pass `explicit.json`).
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let discovered = dir.path().join(".fallowrc.json");
        std::fs::write(&discovered, r#"{"entry": ["src/discovered.ts"]}"#).unwrap();
        let explicit = dir.path().join("explicit.json");
        std::fs::write(&explicit, r#"{"entry": ["src/explicit.ts"]}"#).unwrap();

        let exit = run_config(dir.path(), Some(&explicit), true, OutputFormat::Human);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn run_config_explicit_config_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.json");
        let exit = run_config(dir.path(), Some(&missing), false, OutputFormat::Human);
        // Failure to load explicit config returns exit 2 (error), not exit 3 (no config).
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::from(2)));
    }

    #[test]
    fn run_config_rejects_unknown_boundary_zone_reference() {
        // The CLI's `fallow config` subcommand must enforce the same
        // hard-error contract as the analysis paths: a typo'd zone in
        // `boundaries.rules[]` exits 2 instead of printing a "loaded fine"
        // view of a config that `fallow check` then rejects. Surfaced by
        // review of #468.
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
        let exit = run_config(dir.path(), None, false, OutputFormat::Human);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::from(2)));
    }
}
