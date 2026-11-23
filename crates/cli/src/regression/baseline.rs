use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_types::results::AnalysisResults;

use super::counts::{
    CheckCounts, DupesCounts, FlagsCounts, REGRESSION_SCHEMA_VERSION, RegressionBaseline,
};
use super::outcome::RegressionOutcome;
use super::recovery::{BaselineCommand, RegressionArgs};
use super::tolerance::Tolerance;

use crate::error::emit_input_error;
use crate::state_file::{Existing, load_existing};

/// Number of seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Where to save the regression baseline.
#[derive(Clone, Copy)]
pub enum SaveRegressionTarget<'a> {
    /// Don't save.
    None,
    /// Save into the config file (.fallowrc.json / .fallowrc.jsonc / fallow.toml / .fallow.toml).
    Config,
    /// Save to an explicit file path.
    File(&'a Path),
}

/// Options for regression detection.
#[derive(Clone, Copy)]
pub struct RegressionOpts<'a> {
    pub args: RegressionArgs<'a>,
    pub fail_on_regression: bool,
    pub tolerance: Tolerance,
    /// Explicit regression baseline file path (overrides config).
    pub regression_baseline_file: Option<&'a Path>,
    /// Where to save the regression baseline.
    pub save_target: SaveRegressionTarget<'a>,
    /// Whether --changed-since or --workspace is active (makes counts incomparable).
    pub scoped: bool,
    pub quiet: bool,
    /// Output format. Drives whether load errors are emitted as structured JSON on stdout
    /// (for `--format json` CI consumers) or human text on stderr.
    pub output: OutputFormat,
}

/// Check whether a path is likely gitignored by running `git check-ignore`.
/// Returns `false` if git is unavailable or the check fails (conservative).
fn is_likely_gitignored(path: &Path, root: &Path) -> bool {
    let mut command = fallow_engine::changed_files::git_command_in(root);
    command.args(["check-ignore", "-q"]).arg(path);
    command.output().is_ok_and(|o| o.status.success())
}

/// Get the current git SHA, if available.
pub(super) fn current_git_sha(root: &Path) -> Option<String> {
    fallow_engine::repo_refs::head_sha(root).ok().flatten()
}

#[cfg(test)]
fn save_regression_baseline(
    path: &Path,
    root: &Path,
    check_counts: Option<&CheckCounts>,
    dupes_counts: Option<&DupesCounts>,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    save_regression_baseline_with_identity(
        path,
        root,
        check_counts,
        dupes_counts,
        output,
        &fallow_types::semantic::SemanticAnalysisIdentity::syntactic(),
    )
}

/// Save counts with the compatibility identity of the producing analysis.
///
/// # Errors
///
/// Returns exit code 2 when the file exists and the read fails, and when the
/// write fails.
pub fn save_regression_baseline_with_identity(
    path: &Path,
    root: &Path,
    check_counts: Option<&CheckCounts>,
    dupes_counts: Option<&DupesCounts>,
    output: OutputFormat,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(), ExitCode> {
    // `list --entry-weight` and `flags --retirement` write their blocks into
    // the same file, so a save of the issue counts keeps them.
    let existing = existing_baseline_for_replace(path, output)?;
    let (entry_weight, flags) = existing
        .map(|existing| (existing.entry_weight, existing.flags))
        .unwrap_or_default();
    let baseline = RegressionBaseline {
        schema_version: REGRESSION_SCHEMA_VERSION,
        fallow_version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono_now(),
        git_sha: current_git_sha(root),
        analysis_identity: analysis_identity.clone(),
        check: check_counts.cloned(),
        dupes: dupes_counts.cloned(),
        entry_weight,
        flags,
    };
    write_regression_baseline(path, root, &baseline, output)
}

/// Save the flag counts of `fallow flags --retirement` into a regression
/// baseline file. The other blocks that the file already holds stay.
///
/// # Errors
///
/// Returns exit code 2 when the file exists and this build cannot read or
/// parse it, and when the write fails.
pub fn save_flags_regression_baseline(
    path: &Path,
    root: &Path,
    flags_counts: &FlagsCounts,
    output: OutputFormat,
    args: RegressionArgs<'_>,
) -> Result<(), ExitCode> {
    let existing = existing_baseline_for_merge(path, output, BaselineCommand::Flags(args))?;
    let baseline = RegressionBaseline {
        schema_version: REGRESSION_SCHEMA_VERSION,
        fallow_version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono_now(),
        git_sha: current_git_sha(root),
        analysis_identity: existing.as_ref().map_or_else(
            fallow_types::semantic::SemanticAnalysisIdentity::syntactic,
            |existing| existing.analysis_identity.clone(),
        ),
        check: existing
            .as_ref()
            .and_then(|existing| existing.check.clone()),
        dupes: existing
            .as_ref()
            .and_then(|existing| existing.dupes.clone()),
        entry_weight: existing.and_then(|existing| existing.entry_weight),
        flags: Some(flags_counts.clone()),
    };
    write_regression_baseline(path, root, &baseline, output)
}

/// Parse a baseline with the version rule of the loader.
fn parse_existing_baseline(content: &str) -> Result<RegressionBaseline, String> {
    let baseline: RegressionBaseline = serde_json::from_str(content).map_err(|e| e.to_string())?;
    if !is_supported_schema_version(baseline.schema_version) {
        return Err(format!(
            "schema_version {} is not supported by this fallow build (it reads 1 and {REGRESSION_SCHEMA_VERSION})",
            baseline.schema_version
        ));
    }
    Ok(baseline)
}

/// The baseline that a partial save merges into. `None` when no file
/// exists.
///
/// # Errors
///
/// Returns exit code 2 when the file exists and this build cannot read or
/// parse it. The file stays unchanged.
pub(super) fn existing_baseline_for_merge(
    path: &Path,
    output: OutputFormat,
    command: BaselineCommand<'_>,
) -> Result<Option<RegressionBaseline>, ExitCode> {
    match load_existing(path, parse_existing_baseline) {
        Existing::Absent => Ok(None),
        Existing::Loaded(baseline) => Ok(Some(baseline)),
        Existing::Unreadable(err) => Err(unreadable_baseline_error(path, &err, output)),
        Existing::Invalid(reason) => Err(emit_input_error(
            &format!(
                "regression baseline '{}' is not usable: {reason}.\n\
                 The file is unchanged. Upgrade fallow to read it. {}",
                path.display(),
                command.regenerate(path)
            ),
            output,
        )),
    }
}

/// The baseline that a full save of the issue counts replaces. `None`
/// when no file exists, and when the file does not parse. The second
/// case prints a warning, because the save then drops the other blocks.
///
/// # Errors
///
/// Returns exit code 2 when the file exists and the read fails. The file
/// stays unchanged.
fn existing_baseline_for_replace(
    path: &Path,
    output: OutputFormat,
) -> Result<Option<RegressionBaseline>, ExitCode> {
    match load_existing(path, parse_existing_baseline) {
        Existing::Absent => Ok(None),
        Existing::Loaded(baseline) => Ok(Some(baseline)),
        Existing::Unreadable(err) => Err(unreadable_baseline_error(path, &err, output)),
        Existing::Invalid(reason) => {
            eprintln!(
                "Warning: replaced regression baseline '{}', which was not usable ({reason}). \
                 Entry weights and flag counts of the old file were not kept.",
                path.display()
            );
            Ok(None)
        }
    }
}

fn unreadable_baseline_error(path: &Path, err: &std::io::Error, output: OutputFormat) -> ExitCode {
    emit_input_error(
        &format!(
            "failed to read regression baseline '{}': {err}.\nThe file is unchanged.",
            path.display()
        ),
        output,
    )
}

/// Serialize and write a regression baseline file.
pub(super) fn write_regression_baseline(
    path: &Path,
    root: &Path,
    baseline: &RegressionBaseline,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let json = serde_json::to_string_pretty(baseline).map_err(|e| {
        emit_input_error(
            &format!("failed to serialize regression baseline: {e}"),
            output,
        )
    })?;
    fallow_engine::write_guard::write_file(
        path,
        json.as_bytes(),
        fallow_engine::write_guard::WriteTarget::Path,
    )
    .map_err(|e| emit_input_error(&format!("failed to save regression baseline: {e}"), output))?;
    eprintln!("Regression baseline saved to {}", path.display());
    if is_likely_gitignored(path, root) {
        eprintln!(
            "Warning: '{}' may be gitignored. Commit this file so CI can compare against it.",
            path.display()
        );
    }
    Ok(())
}

/// Save config-embedded counts with their analysis compatibility identity.
#[cfg(test)]
fn save_baseline_to_config(
    config_path: &Path,
    counts: &CheckCounts,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    save_baseline_to_config_with_identity(
        config_path,
        counts,
        output,
        &fallow_types::semantic::SemanticAnalysisIdentity::syntactic(),
    )
}

pub fn save_baseline_to_config_with_identity(
    config_path: &Path,
    counts: &CheckCounts,
    output: OutputFormat,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<(), ExitCode> {
    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let is_toml = config_path.extension().is_some_and(|ext| ext == "toml");
            if is_toml {
                String::new()
            } else {
                "{}".to_string()
            }
        }
        Err(e) => {
            return Err(emit_input_error(
                &format!(
                    "failed to read config file '{}': {e}",
                    config_path.display()
                ),
                output,
            ));
        }
    };

    let mut baseline = counts.to_config_baseline();
    baseline.analysis_identity = analysis_identity.clone();
    let is_toml = config_path.extension().is_some_and(|ext| ext == "toml");

    let format = if is_toml {
        fallow_config::ConfigFormat::Toml
    } else {
        fallow_config::ConfigFormat::Json
    };
    let value = serde_json::to_value(&baseline).map_err(|e| {
        emit_input_error(
            &format!("failed to serialize regression baseline: {e}"),
            output,
        )
    })?;
    let updated = fallow_config::set_config_value_in_string(
        &content,
        format,
        &["regression", "baseline"],
        &value,
    )
    .map_err(|e| {
        emit_input_error(
            &format!(
                "failed to update config file '{}': {e}",
                config_path.display()
            ),
            output,
        )
    })?;

    fallow_engine::write_guard::write_file(
        config_path,
        updated.as_bytes(),
        fallow_engine::write_guard::WriteTarget::DiscoveredConfig,
    )
    .map_err(|e| {
        emit_input_error(
            &format!(
                "failed to write config file '{}': {e}",
                config_path.display()
            ),
            output,
        )
    })?;

    eprintln!(
        "Regression baseline saved to {} (regression.baseline section)",
        config_path.display()
    );
    Ok(())
}

/// Build the human-readable schema-version mismatch message. Factored out so
/// tests can assert on the wording without capturing stderr.
fn format_schema_mismatch_error(
    path: &Path,
    expected: u32,
    actual: u32,
    writer_version: &str,
    command: BaselineCommand<'_>,
) -> String {
    let path_display = path.display();
    let recovery = command.regenerate(path);
    if actual == 0 {
        format!(
            "regression baseline '{path_display}' appears to predate schema versioning \
             (schema_version is 0; this fallow build expects {expected}).\n\
             The baseline was written by fallow {writer_version}.\n\
             {recovery}"
        )
    } else {
        format!(
            "regression baseline '{path_display}' has schema_version {actual} but this fallow build expects {expected}.\n\
             The baseline was written by fallow {writer_version}.\n\
             {recovery}"
        )
    }
}

/// Build the message for a baseline missing `schema_version` entirely. Pre-versioning
/// baselines (hand-edited or written by a very old fallow) hit this path; the raw
/// serde error ("missing field `schema_version`") is unhelpful to a CI user.
fn format_missing_schema_version_error(path: &Path, command: BaselineCommand<'_>) -> String {
    let path_display = path.display();
    let recovery = command.regenerate(path);
    let expected = REGRESSION_SCHEMA_VERSION;
    format!(
        "regression baseline '{path_display}' is missing the schema_version field; \
         this fallow build expects schema_version {expected}.\n\
         The baseline likely predates schema versioning or was hand-edited.\n\
         {recovery}"
    )
}

/// The schema versions that this build reads.
const fn is_supported_schema_version(version: u32) -> bool {
    matches!(version, 1 | REGRESSION_SCHEMA_VERSION)
}

/// Load the counts from a file with a supported schema version.
pub(super) fn load_regression_baseline_for(
    path: &Path,
    output: OutputFormat,
    command: BaselineCommand<'_>,
) -> Result<RegressionBaseline, ExitCode> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            emit_input_error(
                &format!(
                    "no regression baseline found at '{}'.\n\
                     Create it on your main branch with: {}",
                    path.display(),
                    command.save(Some(path))
                ),
                output,
            )
        } else {
            emit_input_error(
                &format!(
                    "failed to read regression baseline '{}': {e}",
                    path.display()
                ),
                output,
            )
        }
    })?;
    let baseline: RegressionBaseline = serde_json::from_str(&content).map_err(|e| {
        let message = if e.to_string().contains("missing field `schema_version`") {
            format_missing_schema_version_error(path, command)
        } else {
            format!(
                "failed to parse regression baseline '{}': {e}.\n{}",
                path.display(),
                command.regenerate(path)
            )
        };
        emit_input_error(&message, output)
    })?;
    if !is_supported_schema_version(baseline.schema_version) {
        let message = format_schema_mismatch_error(
            path,
            REGRESSION_SCHEMA_VERSION,
            baseline.schema_version,
            &baseline.fallow_version,
            command,
        );
        return Err(emit_input_error(&message, output));
    }
    Ok(baseline)
}

/// Compare counts only when the stored and live semantic identities match.
#[cfg(test)]
fn compare_check_regression(
    results: &AnalysisResults,
    opts: &RegressionOpts<'_>,
    config_baseline: Option<&fallow_config::RegressionBaseline>,
) -> Result<Option<RegressionOutcome>, ExitCode> {
    compare_check_regression_with_identity(
        results,
        opts,
        config_baseline,
        &fallow_types::semantic::SemanticAnalysisIdentity::syntactic(),
        "dead-code",
    )
}

pub fn compare_check_regression_with_identity(
    results: &AnalysisResults,
    opts: &RegressionOpts<'_>,
    config_baseline: Option<&fallow_config::RegressionBaseline>,
    analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity,
    command: &str,
) -> Result<Option<RegressionOutcome>, ExitCode> {
    if !opts.fail_on_regression {
        return Ok(None);
    }

    if opts.scoped {
        let reason = "--changed-since or --workspace is active; regression check skipped \
                      (counts not comparable to full-project baseline)";
        if !opts.quiet {
            eprintln!("Warning: {reason}");
        }
        return Ok(Some(RegressionOutcome::Skipped { reason }));
    }

    let command = BaselineCommand::Check {
        name: command,
        mode: analysis_identity.mode,
        args: opts.args,
    };
    let baseline_counts: CheckCounts = if let Some(baseline_path) = opts.regression_baseline_file {
        let baseline = load_regression_baseline_for(baseline_path, opts.output, command)?;
        ensure_regression_identity(
            &baseline.analysis_identity,
            analysis_identity,
            Some(baseline_path),
            opts.output,
            command,
        )?;
        let Some(counts) = baseline.check else {
            return Err(emit_input_error(
                &format!(
                    "regression baseline '{}' has no check data. Save the counts with: {}",
                    baseline_path.display(),
                    command.save(Some(baseline_path))
                ),
                opts.output,
            ));
        };
        counts
    } else if let Some(config_baseline) = config_baseline {
        ensure_regression_identity(
            &config_baseline.analysis_identity,
            analysis_identity,
            None,
            opts.output,
            command,
        )?;
        CheckCounts::from_config_baseline(config_baseline)
    } else {
        return Err(emit_input_error(
            &format!(
                "no regression baseline found.\n\
             Save a `regression.baseline` section in your config with: {}\n\
             Or provide an explicit file via --regression-baseline <PATH>.",
                command.save(None)
            ),
            opts.output,
        ));
    };

    let current_total = results.total_issues();
    let baseline_total = baseline_counts.total_issues;

    if opts.tolerance.exceeded(baseline_total, current_total) {
        let mut current_counts = CheckCounts::from_results(results);
        if opts.regression_baseline_file.is_none() {
            // Config baselines store only a subset of the per-kind counts.
            current_counts =
                CheckCounts::from_config_baseline(&current_counts.to_config_baseline());
        }
        let type_deltas = baseline_counts.deltas(&current_counts);
        Ok(Some(RegressionOutcome::Exceeded {
            baseline_total,
            current_total,
            tolerance: opts.tolerance,
            type_deltas,
        }))
    } else {
        Ok(Some(RegressionOutcome::Pass {
            baseline_total,
            current_total,
        }))
    }
}

fn ensure_regression_identity(
    stored: &fallow_types::semantic::SemanticAnalysisIdentity,
    current: &fallow_types::semantic::SemanticAnalysisIdentity,
    file: Option<&Path>,
    output: OutputFormat,
    command: BaselineCommand<'_>,
) -> Result<(), ExitCode> {
    let incompatible = stored.incompatible_fields(current);
    if incompatible.is_empty() {
        return Ok(());
    }
    let source = file.map_or_else(
        || "in the project config".to_owned(),
        |path| format!("'{}'", path.display()),
    );
    Err(emit_input_error(
        &format!(
            "regression baseline {source} has an incompatible analysis identity in: {}. Regenerate it with: {}",
            incompatible.join(", "),
            command.save(file),
        ),
        output,
    ))
}

/// ISO 8601 UTC timestamp without external dependencies.
pub(super) fn chrono_now() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / SECS_PER_DAY;
    let time_secs = secs % SECS_PER_DAY;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::output_dead_code::*;
    use fallow_types::results::*;
    use std::path::PathBuf;

    fn check_command(name: &str) -> BaselineCommand<'_> {
        BaselineCommand::Check {
            name,
            mode: fallow_types::semantic::SemanticAnalysisMode::Syntactic,
            args: RegressionArgs::default(),
        }
    }

    fn load_regression_baseline(
        path: &Path,
        output: OutputFormat,
    ) -> Result<RegressionBaseline, ExitCode> {
        load_regression_baseline_for(path, output, check_command("dead-code"))
    }

    #[test]
    fn chrono_now_format() {
        let ts = chrono_now();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("regression-baseline.json");
        let counts = CheckCounts {
            total_issues: 15,
            unused_files: 3,
            unused_exports: 5,
            unused_types: 2,
            unused_dependencies: 1,
            unused_dev_dependencies: 1,
            unused_optional_dependencies: 0,
            unused_enum_members: 1,
            unused_class_members: 0,
            unused_namespace_members: 0,
            unused_protected_class_members: 0,
            namespace_escapes: 0,
            unused_store_members: 0,
            unprovided_injects: 0,
            unrendered_components: 0,
            unused_component_props: 0,
            absent_component_props: 0,
            constant_parameters: 0,
            unused_component_emits: 0,
            unused_component_inputs: 0,
            unused_component_outputs: 0,
            unused_svelte_events: 0,
            unused_effect_layers: 0,
            unused_tagged_errors: 0,
            unproduced_catch_tags: 0,
            unused_server_actions: 0,
            unused_load_data_keys: 0,
            unresolved_imports: 1,
            unlisted_dependencies: 0,
            unlisted_binaries: 0,
            duplicate_exports: 1,
            duplicate_export_aliases: 0,
            circular_dependencies: 0,
            re_export_cycles: 0,
            package_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
            dev_dependencies_in_production: 0,
            boundary_violations: 0,
            boundary_coverage_violations: 0,
            boundary_call_violations: 0,
            policy_violations: 0,
        };
        let dupes = DupesCounts {
            clone_groups: 4,
            duplication_percentage: 2.5,
        };

        save_regression_baseline(
            &path,
            dir.path(),
            Some(&counts),
            Some(&dupes),
            OutputFormat::Human,
        )
        .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert_eq!(loaded.schema_version, REGRESSION_SCHEMA_VERSION);
        let check = loaded.check.unwrap();
        assert_eq!(check.total_issues, 15);
        assert_eq!(check.unused_files, 3);
        assert_eq!(check.unused_exports, 5);
        assert_eq!(check.unused_types, 2);
        assert_eq!(check.unused_dependencies, 1);
        assert_eq!(check.unresolved_imports, 1);
        assert_eq!(check.duplicate_exports, 1);
        let dupes = loaded.dupes.unwrap();
        assert_eq!(dupes.clone_groups, 4);
        assert!((dupes.duplication_percentage - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn save_load_roundtrip_check_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("regression-baseline.json");
        let counts = CheckCounts {
            total_issues: 5,
            unused_files: 5,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };

        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert!(loaded.check.is_some());
        assert!(loaded.dupes.is_none());
        assert_eq!(loaded.check.unwrap().unused_files, 5);
    }

    #[test]
    fn saving_issue_counts_keeps_the_saved_entry_weights() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("regression-baseline.json");
        std::fs::write(
            &path,
            r#"{"schema_version":2,"fallow_version":"0.0.0","timestamp":"t",
               "entry_weight":{"entries":[{"path":"src/main.ts","eager_bytes":10,"eager_modules":1}]}}"#,
        )
        .unwrap();
        let counts = CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };

        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert_eq!(loaded.check.unwrap().total_issues, 1);
        let entries = loaded.entry_weight.expect("entry weights kept").entries;
        assert_eq!(entries[0].path, "src/main.ts");
    }

    #[test]
    fn saving_flag_counts_and_issue_counts_keeps_both_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("regression-baseline.json");
        std::fs::write(
            &path,
            r#"{"schema_version":2,"fallow_version":"0.0.0","timestamp":"t",
               "entry_weight":{"entries":[{"path":"src/main.ts","eager_bytes":10,"eager_modules":1}]}}"#,
        )
        .unwrap();
        let flags = FlagsCounts {
            total_flags: 3,
            distinct_flags: 2,
            by_reason: std::collections::BTreeMap::new(),
        };
        let counts = CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };

        save_flags_regression_baseline(
            &path,
            dir.path(),
            &flags,
            OutputFormat::Human,
            RegressionArgs::default(),
        )
        .unwrap();
        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert_eq!(loaded.check.unwrap().total_issues, 1);
        assert_eq!(loaded.flags.expect("flag counts kept").distinct_flags, 2);
        assert!(loaded.entry_weight.is_some(), "entry weights kept");

        save_flags_regression_baseline(
            &path,
            dir.path(),
            &flags,
            OutputFormat::Human,
            RegressionArgs::default(),
        )
        .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();
        assert_eq!(loaded.check.expect("issue counts kept").total_issues, 1);
        assert!(loaded.entry_weight.is_some(), "entry weights kept");
    }

    const CONFLICT_SEED: &str = "<<<<<<< HEAD\n{\"schema_version\":2,\"fallow_version\":\"0.0.0\",\"timestamp\":\"t\",\"check\":{\"total_issues\":1}}\n=======\n{\"schema_version\":2,\"fallow_version\":\"0.0.0\",\"timestamp\":\"t\",\"check\":{\"total_issues\":2}}\n>>>>>>> feature\n";
    const NEWER_SCHEMA_SEED: &str = r#"{"schema_version":3,"fallow_version":"9.0.0","timestamp":"t","check":{"total_issues":4},"future_block":{"x":1}}"#;

    fn sample_flags() -> FlagsCounts {
        FlagsCounts {
            total_flags: 3,
            distinct_flags: 2,
            by_reason: std::collections::BTreeMap::new(),
        }
    }

    fn sample_check_counts() -> CheckCounts {
        CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        }
    }

    fn seed_baseline(dir: &Path, bytes: &[u8]) -> PathBuf {
        let path = dir.join("regression-baseline.json");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_flags_save_keeps_a_baseline_with_conflict_markers() {
        let dir = tempfile::tempdir().unwrap();
        let path = seed_baseline(dir.path(), CONFLICT_SEED.as_bytes());

        let saved = save_flags_regression_baseline(
            &path,
            dir.path(),
            &sample_flags(),
            OutputFormat::Human,
            RegressionArgs::default(),
        );

        assert!(
            saved.is_err(),
            "a partial save must stop on a file it cannot parse"
        );
        assert_eq!(std::fs::read(&path).unwrap(), CONFLICT_SEED.as_bytes());
    }

    #[test]
    fn a_flags_save_keeps_a_baseline_of_a_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = seed_baseline(dir.path(), NEWER_SCHEMA_SEED.as_bytes());

        let saved = save_flags_regression_baseline(
            &path,
            dir.path(),
            &sample_flags(),
            OutputFormat::Human,
            RegressionArgs::default(),
        );

        assert!(saved.is_err(), "a partial save must stop on a newer schema");
        assert_eq!(std::fs::read(&path).unwrap(), NEWER_SCHEMA_SEED.as_bytes());
    }

    #[test]
    fn a_flags_save_merges_into_a_version_1_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = seed_baseline(
            dir.path(),
            br#"{"schema_version":1,"fallow_version":"0.0.0","timestamp":"t","check":{"total_issues":7}}"#,
        );

        save_flags_regression_baseline(
            &path,
            dir.path(),
            &sample_flags(),
            OutputFormat::Human,
            RegressionArgs::default(),
        )
        .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert_eq!(loaded.schema_version, REGRESSION_SCHEMA_VERSION);
        assert_eq!(loaded.check.expect("issue counts kept").total_issues, 7);
        assert!(loaded.flags.is_some());
    }

    #[test]
    fn a_flags_save_creates_a_missing_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("regression-baseline.json");

        save_flags_regression_baseline(
            &path,
            dir.path(),
            &sample_flags(),
            OutputFormat::Human,
            RegressionArgs::default(),
        )
        .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert!(loaded.flags.is_some());
        assert!(loaded.check.is_none());
    }

    #[test]
    fn an_issue_count_save_replaces_a_baseline_that_does_not_parse() {
        let dir = tempfile::tempdir().unwrap();
        let path = seed_baseline(dir.path(), CONFLICT_SEED.as_bytes());
        let counts = sample_check_counts();

        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();

        assert_eq!(loaded.check.expect("new issue counts").total_issues, 1);
    }

    #[test]
    fn an_issue_count_save_keeps_a_baseline_that_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = seed_baseline(dir.path(), &[0xff, 0xfe, 0xfd]);
        let counts = sample_check_counts();

        let saved =
            save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human);

        assert!(saved.is_err(), "a save must stop on a file it cannot read");
        assert_eq!(std::fs::read(&path).unwrap(), [0xff, 0xfe, 0xfd]);
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dir").join("baseline.json");
        let counts = CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };

        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        assert!(path.exists());
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let result = load_regression_baseline(
            Path::new("/tmp/nonexistent-baseline-12345.json"),
            OutputFormat::Human,
        );
        assert!(result.is_err());
    }

    #[test]
    fn load_invalid_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not valid json {{{").unwrap();
        let result = load_regression_baseline(&path, OutputFormat::Human);
        assert!(result.is_err());
    }

    #[test]
    fn save_baseline_to_json_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(&config_path, r#"{"entry": ["src/main.ts"]}"#).unwrap();

        let counts = CheckCounts {
            total_issues: 7,
            unused_files: 3,
            unused_exports: 4,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config(&config_path, &counts, OutputFormat::Human).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("\"regression\""));
        assert!(content.contains("\"totalIssues\": 7"));
        serde_json::from_str::<serde_json::Value>(&content).unwrap();
    }

    #[test]
    fn save_baseline_to_toml_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("fallow.toml");
        std::fs::write(&config_path, "[rules]\nunused-files = \"warn\"\n").unwrap();

        let counts = CheckCounts {
            total_issues: 7,
            unused_files: 3,
            unused_exports: 4,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config(&config_path, &counts, OutputFormat::Human).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("[regression.baseline]"));
        assert!(content.contains("totalIssues = 7"));
        assert!(content.contains("[rules]"));
    }

    #[test]
    fn save_baseline_to_nonexistent_json_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".fallowrc.json");

        let counts = CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config(&config_path, &counts, OutputFormat::Human).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("\"regression\""));
        serde_json::from_str::<serde_json::Value>(&content).unwrap();
    }

    #[test]
    fn save_baseline_to_nonexistent_toml_config() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("fallow.toml");

        let counts = CheckCounts {
            total_issues: 0,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config(&config_path, &counts, OutputFormat::Human).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("[regression.baseline]"));
        assert!(content.contains("totalIssues = 0"));
    }

    #[test]
    fn save_to_json_config_keeps_an_object_after_a_regression_string() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(".fallowrc.json");
        std::fs::write(
            &config_path,
            r#"{"ignorePatterns": ["regression"], "rules": {"unused-files": "warn"}}"#,
        )
        .unwrap();

        let counts = CheckCounts {
            total_issues: 7,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config(&config_path, &counts, OutputFormat::Human).unwrap();

        let content = std::fs::read_to_string(&config_path).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(saved["rules"], serde_json::json!({"unused-files": "warn"}));
        assert_eq!(saved["ignorePatterns"], serde_json::json!(["regression"]));
        assert_eq!(saved["regression"]["baseline"]["totalIssues"], 7);
    }

    #[test]
    fn save_to_toml_config_round_trips_the_identity_and_all_counts() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("fallow.toml");
        std::fs::write(&config_path, "[rules]\nunused-files = \"warn\"\n").unwrap();

        let identity = fallow_types::semantic::SemanticAnalysisIdentity {
            mode: fallow_types::semantic::SemanticAnalysisMode::TypeAware,
            ..fallow_types::semantic::SemanticAnalysisIdentity::syntactic()
        };
        let counts = CheckCounts {
            total_issues: 5,
            boundary_violations: 3,
            policy_violations: 2,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_baseline_to_config_with_identity(
            &config_path,
            &counts,
            OutputFormat::Human,
            &identity,
        )
        .unwrap();
        let content = std::fs::read_to_string(&config_path).unwrap();
        let config: fallow_config::FallowConfig = toml::from_str(&content).unwrap();
        let loaded = config.regression.unwrap().baseline.unwrap();
        assert_eq!(loaded.analysis_identity, identity);
        assert_eq!(loaded.boundary_violations, 3);
        assert_eq!(loaded.policy_violations, 2);
        assert!(
            ensure_regression_identity(
                &loaded.analysis_identity,
                &identity,
                None,
                OutputFormat::Human,
                check_command("dead-code"),
            )
            .is_ok()
        );
    }

    fn make_opts(
        fail: bool,
        tolerance: Tolerance,
        scoped: bool,
        baseline_file: Option<&Path>,
    ) -> RegressionOpts<'_> {
        RegressionOpts {
            fail_on_regression: fail,
            tolerance,
            args: RegressionArgs::default(),
            regression_baseline_file: baseline_file,
            save_target: SaveRegressionTarget::None,
            scoped,
            quiet: true,
            output: OutputFormat::Human,
        }
    }

    #[test]
    fn compare_returns_none_when_disabled() {
        let results = AnalysisResults::default();
        let opts = make_opts(false, Tolerance::Absolute(0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 5,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn compare_returns_skipped_when_scoped() {
        let results = AnalysisResults::default();
        let opts = make_opts(true, Tolerance::Absolute(0), true, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 5,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        assert!(matches!(outcome, Some(RegressionOutcome::Skipped { .. })));
    }

    #[test]
    fn compare_pass_with_config_baseline() {
        let results = AnalysisResults::default(); // 0 issues
        let opts = make_opts(true, Tolerance::Absolute(0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 0,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        match outcome {
            Some(RegressionOutcome::Pass {
                baseline_total,
                current_total,
            }) => {
                assert_eq!(baseline_total, 0);
                assert_eq!(current_total, 0);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn compare_exceeded_with_config_baseline() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("b.ts"),
            }));
        let opts = make_opts(true, Tolerance::Absolute(0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 0,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        match outcome {
            Some(RegressionOutcome::Exceeded {
                baseline_total,
                current_total,
                ..
            }) => {
                assert_eq!(baseline_total, 0);
                assert_eq!(current_total, 2);
            }
            other => panic!("expected Exceeded, got {other:?}"),
        }
    }

    #[test]
    fn compare_pass_within_tolerance() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        let opts = make_opts(true, Tolerance::Absolute(5), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 0,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        assert!(matches!(outcome, Some(RegressionOutcome::Pass { .. })));
    }

    #[test]
    fn compare_improvement_is_pass() {
        let results = AnalysisResults::default(); // 0 issues
        let opts = make_opts(true, Tolerance::Absolute(0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 10,
            unused_files: 5,
            unused_exports: 5,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        match outcome {
            Some(RegressionOutcome::Pass {
                baseline_total,
                current_total,
            }) => {
                assert_eq!(baseline_total, 10);
                assert_eq!(current_total, 0);
            }
            other => panic!("expected Pass, got {other:?}"),
        }
    }

    #[test]
    fn compare_with_file_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let baseline_path = dir.path().join("baseline.json");

        let counts = CheckCounts {
            total_issues: 5,
            unused_files: 5,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_regression_baseline(
            &baseline_path,
            dir.path(),
            Some(&counts),
            None,
            OutputFormat::Human,
        )
        .unwrap();

        let results = AnalysisResults::default();
        let opts = make_opts(true, Tolerance::Absolute(0), false, Some(&baseline_path));
        let outcome = compare_check_regression(&results, &opts, None).unwrap();
        assert!(matches!(outcome, Some(RegressionOutcome::Pass { .. })));
    }

    #[test]
    fn compare_file_baseline_missing_check_data_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let baseline_path = dir.path().join("baseline.json");

        save_regression_baseline(
            &baseline_path,
            dir.path(),
            None,
            Some(&DupesCounts {
                clone_groups: 1,
                duplication_percentage: 1.0,
            }),
            OutputFormat::Human,
        )
        .unwrap();

        let results = AnalysisResults::default();
        let opts = make_opts(true, Tolerance::Absolute(0), false, Some(&baseline_path));
        let outcome = compare_check_regression(&results, &opts, None);
        assert!(outcome.is_err());
    }

    #[test]
    fn compare_no_baseline_source_returns_error() {
        let results = AnalysisResults::default();
        let opts = make_opts(true, Tolerance::Absolute(0), false, None);
        let outcome = compare_check_regression(&results, &opts, None);
        assert!(outcome.is_err());
    }

    #[test]
    fn compare_exceeded_includes_type_deltas() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("b.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("c.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));

        let opts = make_opts(true, Tolerance::Absolute(0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 0,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();

        match outcome {
            Some(RegressionOutcome::Exceeded { type_deltas, .. }) => {
                assert!(type_deltas.contains(&("unused_files", 2)));
                assert!(type_deltas.contains(&("unused_exports", 1)));
            }
            other => panic!("expected Exceeded, got {other:?}"),
        }
    }

    #[test]
    fn compare_with_percentage_tolerance() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));

        let opts = make_opts(true, Tolerance::Percentage(50.0), false, None);
        let config_baseline = fallow_config::RegressionBaseline {
            total_issues: 10,
            unused_files: 10,
            ..Default::default()
        };
        let outcome = compare_check_regression(&results, &opts, Some(&config_baseline)).unwrap();
        assert!(matches!(outcome, Some(RegressionOutcome::Pass { .. })));
    }

    fn write_baseline_with_schema_version(dir: &Path, version: u32) -> PathBuf {
        let path = dir.join("baseline.json");
        let body = format!(
            r#"{{
  "schema_version": {version},
  "fallow_version": "3.0.0",
  "timestamp": "2026-05-21T00:00:00Z",
  "check": {{
    "total_issues": 0,
    "unused_files": 0
  }}
}}"#
        );
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn load_rejects_schema_version_too_high() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_baseline_with_schema_version(dir.path(), REGRESSION_SCHEMA_VERSION + 1);
        let result = load_regression_baseline(&path, OutputFormat::Human);
        assert!(result.is_err());
    }

    #[test]
    fn load_rejects_schema_version_zero_predates_versioning() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_baseline_with_schema_version(dir.path(), 0);
        let result = load_regression_baseline(&path, OutputFormat::Human);
        assert!(result.is_err());
    }

    #[test]
    fn load_accepts_current_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_baseline_with_schema_version(dir.path(), REGRESSION_SCHEMA_VERSION);
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();
        assert_eq!(loaded.schema_version, REGRESSION_SCHEMA_VERSION);
    }

    #[test]
    fn load_rewrites_missing_schema_version_field_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        std::fs::write(
            &path,
            r#"{
  "fallow_version": "1.0.0",
  "timestamp": "2026-05-21T00:00:00Z",
  "check": {}
}"#,
        )
        .unwrap();
        let result = load_regression_baseline(&path, OutputFormat::Human);
        assert!(result.is_err());
    }

    #[test]
    fn format_schema_mismatch_error_too_high() {
        let msg = format_schema_mismatch_error(
            Path::new("/repo/.fallow-baseline.json"),
            1,
            99,
            "3.0.0",
            check_command("dead-code"),
        );
        assert!(msg.contains("schema_version 99"));
        assert!(msg.contains("expects 1"));
        assert!(msg.contains("fallow 3.0.0"));
        assert!(
            msg.contains("fallow dead-code --save-regression-baseline /repo/.fallow-baseline.json")
        );
        assert!(!msg.to_lowercase().contains("refresh"));
        assert!(msg.contains("schema_version"));
    }

    #[test]
    fn format_schema_mismatch_error_actual_zero_special_case() {
        let msg = format_schema_mismatch_error(
            Path::new("/repo/.fallow-baseline.json"),
            1,
            0,
            "2.0.0",
            BaselineCommand::Check {
                name: "dead-code",
                mode: fallow_types::semantic::SemanticAnalysisMode::TypeAware,
                args: RegressionArgs {
                    config: Some(Path::new("ci.json")),
                    ..Default::default()
                },
            },
        );
        assert!(msg.contains("predate"));
        assert!(msg.contains("fallow 2.0.0"));
        assert!(
            msg.contains("fallow dead-code --type-aware --save-regression-baseline /repo/.fallow-baseline.json --config ci.json")
        );
    }

    #[test]
    fn format_missing_schema_version_error_includes_regenerate_command() {
        let msg = format_missing_schema_version_error(
            Path::new("/repo/baseline.json"),
            check_command("dead-code"),
        );
        assert!(msg.contains("missing the schema_version field"));
        assert!(msg.contains("fallow dead-code --save-regression-baseline /repo/baseline.json"));
    }

    #[test]
    fn regenerate_hints_name_the_architecture_command() {
        let path = Path::new("/repo/baseline.json");
        let expected = "fallow architecture --save-regression-baseline /repo/baseline.json";
        assert!(
            format_missing_schema_version_error(path, check_command("architecture"))
                .contains(expected)
        );
        assert!(
            format_schema_mismatch_error(path, 1, 0, "2.0.0", check_command("architecture"))
                .contains(expected)
        );
        assert!(
            format_schema_mismatch_error(path, 1, 99, "3.0.0", check_command("architecture"))
                .contains(expected)
        );
    }

    #[test]
    fn save_load_preserves_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("baseline.json");
        let counts = CheckCounts {
            total_issues: 1,
            unused_files: 1,
            ..CheckCounts::from_config_baseline(&fallow_config::RegressionBaseline::default())
        };
        save_regression_baseline(&path, dir.path(), Some(&counts), None, OutputFormat::Human)
            .unwrap();
        let loaded = load_regression_baseline(&path, OutputFormat::Human).unwrap();
        assert_eq!(loaded.schema_version, REGRESSION_SCHEMA_VERSION);
    }
}
