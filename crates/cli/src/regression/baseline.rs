use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_core::git_env::clear_ambient_git_env;
use fallow_core::results::AnalysisResults;

use super::counts::{CheckCounts, DupesCounts, REGRESSION_SCHEMA_VERSION, RegressionBaseline};
use super::outcome::RegressionOutcome;
use super::tolerance::Tolerance;

use crate::error::emit_error;

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
    let mut command = std::process::Command::new("git");
    command
        .args(["check-ignore", "-q"])
        .arg(path)
        .current_dir(root);
    clear_ambient_git_env(&mut command);
    command.output().ok().is_some_and(|o| o.status.success())
}

/// Get the current git SHA, if available.
fn current_git_sha(root: &Path) -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.args(["rev-parse", "HEAD"]).current_dir(root);
    clear_ambient_git_env(&mut command);
    command
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Save the current analysis results as a regression baseline.
///
/// # Errors
///
/// Returns an error if the baseline cannot be serialized or written to disk.
pub fn save_regression_baseline(
    path: &Path,
    root: &Path,
    check_counts: Option<&CheckCounts>,
    dupes_counts: Option<&DupesCounts>,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let baseline = RegressionBaseline {
        schema_version: REGRESSION_SCHEMA_VERSION,
        fallow_version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: chrono_now(),
        git_sha: current_git_sha(root),
        check: check_counts.cloned(),
        dupes: dupes_counts.cloned(),
    };
    let json = serde_json::to_string_pretty(&baseline).map_err(|e| {
        emit_error(
            &format!("failed to serialize regression baseline: {e}"),
            2,
            output,
        )
    })?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, json).map_err(|e| {
        emit_error(
            &format!("failed to save regression baseline: {e}"),
            2,
            output,
        )
    })?;
    eprintln!("Regression baseline saved to {}", path.display());
    if is_likely_gitignored(path, root) {
        eprintln!(
            "Warning: '{}' may be gitignored. Commit this file so CI can compare against it.",
            path.display()
        );
    }
    Ok(())
}

/// Save regression baseline counts into the project's config file.
///
/// Reads the existing config, adds/updates the `regression.baseline` section,
/// and writes it back. For JSONC files, comments are preserved using a targeted
/// insertion/replacement strategy.
///
/// # Errors
///
/// Returns an error if the config file cannot be read, updated, or written back.
pub fn save_baseline_to_config(
    config_path: &Path,
    counts: &CheckCounts,
    output: OutputFormat,
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
            return Err(emit_error(
                &format!(
                    "failed to read config file '{}': {e}",
                    config_path.display()
                ),
                2,
                output,
            ));
        }
    };

    let baseline = counts.to_config_baseline();
    let is_toml = config_path.extension().is_some_and(|ext| ext == "toml");

    let updated = if is_toml {
        Ok(update_toml_regression(&content, &baseline))
    } else {
        update_json_regression(&content, &baseline)
    }
    .map_err(|e| {
        emit_error(
            &format!(
                "failed to update config file '{}': {e}",
                config_path.display()
            ),
            2,
            output,
        )
    })?;

    std::fs::write(config_path, updated).map_err(|e| {
        emit_error(
            &format!(
                "failed to write config file '{}': {e}",
                config_path.display()
            ),
            2,
            output,
        )
    })?;

    eprintln!(
        "Regression baseline saved to {} (regression.baseline section)",
        config_path.display()
    );
    Ok(())
}

/// Update a JSONC config file with regression baseline, preserving comments.
/// Find a JSON key in content, skipping `//` line comments and `/* */` block comments.
/// Returns the byte offset of the opening `"` of the key.
fn find_json_key(content: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find(&needle) {
        let abs_pos = search_from + pos;
        let line_start = content[..abs_pos].rfind('\n').map_or(0, |i| i + 1);
        let line_prefix = content[line_start..abs_pos].trim_start();
        if line_prefix.starts_with("//") {
            search_from = abs_pos + needle.len();
            continue;
        }
        let before = &content[..abs_pos];
        let last_open = before.rfind("/*");
        let last_close = before.rfind("*/");
        if let Some(open_pos) = last_open
            && last_close.is_none_or(|close_pos| close_pos < open_pos)
        {
            search_from = abs_pos + needle.len();
            continue;
        }
        return Some(abs_pos);
    }
    None
}

fn update_json_regression(
    content: &str,
    baseline: &fallow_config::RegressionBaseline,
) -> Result<String, String> {
    let baseline_json =
        serde_json::to_string_pretty(baseline).map_err(|e| format!("serialization error: {e}"))?;

    let indented: String = baseline_json
        .lines()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                format!("    {line}")
            } else {
                format!("\n    {line}")
            }
        })
        .collect();

    let regression_block = format!("  \"regression\": {{\n    \"baseline\": {indented}\n  }}");

    if let Some(start) = find_json_key(content, "regression") {
        let after_key = &content[start..];
        if let Some(brace_start) = after_key.find('{') {
            let abs_brace = start + brace_start;
            let mut depth = 0;
            let mut end = abs_brace;
            let mut found_close = false;
            for (i, ch) in content[abs_brace..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = abs_brace + i + 1;
                            found_close = true;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if !found_close {
                return Err("malformed JSON: unmatched brace in regression object".to_string());
            }
            let mut result = String::new();
            result.push_str(&content[..start]);
            result.push_str(&regression_block[2..]); // skip leading "  " — reuse original indent
            result.push_str(&content[end..]);
            return Ok(result);
        }
    }

    if let Some(last_brace) = content.rfind('}') {
        let before_brace = content[..last_brace].trim_end();
        let needs_comma = !before_brace.ends_with('{') && !before_brace.ends_with(',');

        let mut result = String::new();
        result.push_str(before_brace);
        if needs_comma {
            result.push(',');
        }
        result.push('\n');
        result.push_str(&regression_block);
        result.push('\n');
        result.push_str(&content[last_brace..]);
        Ok(result)
    } else {
        Err("config file has no closing brace".to_string())
    }
}

/// Update a TOML config file with regression baseline.
fn update_toml_regression(content: &str, baseline: &fallow_config::RegressionBaseline) -> String {
    use std::fmt::Write;
    let mut section = String::from("[regression.baseline]\n");
    let _ = writeln!(section, "totalIssues = {}", baseline.total_issues);
    let _ = writeln!(section, "unusedFiles = {}", baseline.unused_files);
    let _ = writeln!(section, "unusedExports = {}", baseline.unused_exports);
    let _ = writeln!(section, "unusedTypes = {}", baseline.unused_types);
    let _ = writeln!(
        section,
        "unusedDependencies = {}",
        baseline.unused_dependencies
    );
    let _ = writeln!(
        section,
        "unusedDevDependencies = {}",
        baseline.unused_dev_dependencies
    );
    let _ = writeln!(
        section,
        "unusedOptionalDependencies = {}",
        baseline.unused_optional_dependencies
    );
    let _ = writeln!(
        section,
        "unusedEnumMembers = {}",
        baseline.unused_enum_members
    );
    let _ = writeln!(
        section,
        "unusedClassMembers = {}",
        baseline.unused_class_members
    );
    let _ = writeln!(
        section,
        "unresolvedImports = {}",
        baseline.unresolved_imports
    );
    let _ = writeln!(
        section,
        "unlistedDependencies = {}",
        baseline.unlisted_dependencies
    );
    let _ = writeln!(section, "duplicateExports = {}", baseline.duplicate_exports);
    let _ = writeln!(
        section,
        "circularDependencies = {}",
        baseline.circular_dependencies
    );
    let _ = writeln!(
        section,
        "typeOnlyDependencies = {}",
        baseline.type_only_dependencies
    );
    let _ = writeln!(
        section,
        "testOnlyDependencies = {}",
        baseline.test_only_dependencies
    );

    if let Some(start) = content.find("[regression.baseline]") {
        let after = &content[start + "[regression.baseline]".len()..];
        let end_offset = after.find("\n[").map_or(content.len(), |i| {
            start + "[regression.baseline]".len() + i + 1
        });

        let mut result = String::new();
        result.push_str(&content[..start]);
        result.push_str(&section);
        if end_offset < content.len() {
            result.push_str(&content[end_offset..]);
        }
        result
    } else {
        let mut result = content.to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push('\n');
        result.push_str(&section);
        result
    }
}

/// Build the human-readable schema-version mismatch message. Factored out so
/// tests can assert on the wording without capturing stderr.
fn format_schema_mismatch_error(
    path: &Path,
    expected: u32,
    actual: u32,
    writer_version: &str,
) -> String {
    let path_display = path.display();
    if actual == 0 {
        format!(
            "regression baseline '{path_display}' appears to predate schema versioning \
             (schema_version is 0; this fallow build expects {expected}).\n\
             The baseline was written by fallow {writer_version}.\n\
             Regenerate it by running: fallow dead-code --save-regression-baseline {path_display}"
        )
    } else {
        format!(
            "regression baseline '{path_display}' has schema_version {actual} but this fallow build expects {expected}.\n\
             The baseline was written by fallow {writer_version}.\n\
             Regenerate it by running: fallow dead-code --save-regression-baseline {path_display}"
        )
    }
}

/// Build the message for a baseline missing `schema_version` entirely. Pre-versioning
/// baselines (hand-edited or written by a very old fallow) hit this path; the raw
/// serde error ("missing field `schema_version`") is unhelpful to a CI user.
fn format_missing_schema_version_error(path: &Path) -> String {
    let path_display = path.display();
    let expected = REGRESSION_SCHEMA_VERSION;
    format!(
        "regression baseline '{path_display}' is missing the schema_version field; \
         this fallow build expects schema_version {expected}.\n\
         The baseline likely predates schema versioning or was hand-edited.\n\
         Regenerate it by running: fallow dead-code --save-regression-baseline {path_display}"
    )
}

/// Load a regression baseline from disk.
///
/// Validates that `schema_version` matches `REGRESSION_SCHEMA_VERSION`. Mismatches
/// (including baselines missing the field entirely) fail loud with an actionable
/// regenerate hint rather than silently loading default-zero fields, which would
/// mask real regressions.
///
/// # Errors
///
/// Returns an error if the file does not exist, cannot be read, contains invalid
/// JSON, or has a `schema_version` that does not match the current build's
/// `REGRESSION_SCHEMA_VERSION`.
pub fn load_regression_baseline(
    path: &Path,
    output: OutputFormat,
) -> Result<RegressionBaseline, ExitCode> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            emit_error(
                &format!(
                    "no regression baseline found at '{}'.\n\
                     Run with --save-regression-baseline on your main branch to create one.",
                    path.display()
                ),
                2,
                output,
            )
        } else {
            emit_error(
                &format!(
                    "failed to read regression baseline '{}': {e}",
                    path.display()
                ),
                2,
                output,
            )
        }
    })?;
    let baseline: RegressionBaseline = serde_json::from_str(&content).map_err(|e| {
        let message = if e.to_string().contains("missing field `schema_version`") {
            format_missing_schema_version_error(path)
        } else {
            format!(
                "failed to parse regression baseline '{}': {e}",
                path.display()
            )
        };
        emit_error(&message, 2, output)
    })?;
    if baseline.schema_version != REGRESSION_SCHEMA_VERSION {
        let message = format_schema_mismatch_error(
            path,
            REGRESSION_SCHEMA_VERSION,
            baseline.schema_version,
            &baseline.fallow_version,
        );
        return Err(emit_error(&message, 2, output));
    }
    Ok(baseline)
}

/// Compare current check results against a regression baseline.
///
/// Resolution order for the baseline:
/// 1. Explicit file via `--regression-baseline <PATH>`
/// 2. Config-embedded `regression.baseline` section
/// 3. Error with actionable message
///
/// # Errors
///
/// Returns an error if the baseline file cannot be loaded, is missing check data,
/// or no baseline source is available.
pub fn compare_check_regression(
    results: &AnalysisResults,
    opts: &RegressionOpts<'_>,
    config_baseline: Option<&fallow_config::RegressionBaseline>,
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

    let baseline_counts: CheckCounts = if let Some(baseline_path) = opts.regression_baseline_file {
        let baseline = load_regression_baseline(baseline_path, opts.output)?;
        let Some(counts) = baseline.check else {
            return Err(emit_error(
                &format!(
                    "regression baseline '{}' has no check data",
                    baseline_path.display()
                ),
                2,
                opts.output,
            ));
        };
        counts
    } else if let Some(config_baseline) = config_baseline {
        CheckCounts::from_config_baseline(config_baseline)
    } else {
        return Err(emit_error(
            "no regression baseline found.\n\
             Either add a `regression.baseline` section to your config file\n\
             (run with --save-regression-baseline to generate it),\n\
             or provide an explicit file via --regression-baseline <PATH>.",
            2,
            opts.output,
        ));
    };

    let current_total = results.total_issues();
    let baseline_total = baseline_counts.total_issues;

    if opts.tolerance.exceeded(baseline_total, current_total) {
        let current_counts = CheckCounts::from_results(results);
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

/// ISO 8601 UTC timestamp without external dependencies.
fn chrono_now() -> String {
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
    use fallow_core::results::*;
    use std::path::PathBuf;

    fn sample_baseline() -> fallow_config::RegressionBaseline {
        fallow_config::RegressionBaseline {
            total_issues: 5,
            unused_files: 2,
            ..Default::default()
        }
    }

    #[test]
    fn json_insert_into_empty_object() {
        let result = update_json_regression("{}", &sample_baseline()).unwrap();
        assert!(result.contains("\"regression\""));
        assert!(result.contains("\"totalIssues\": 5"));
        serde_json::from_str::<serde_json::Value>(&result).unwrap();
    }

    #[test]
    fn json_insert_into_existing_config() {
        let config = r#"{
  "entry": ["src/main.ts"],
  "production": true
}"#;
        let result = update_json_regression(config, &sample_baseline()).unwrap();
        assert!(result.contains("\"regression\""));
        assert!(result.contains("\"entry\""));
        serde_json::from_str::<serde_json::Value>(&result).unwrap();
    }

    #[test]
    fn json_replace_existing_regression() {
        let config = r#"{
  "entry": ["src/main.ts"],
  "regression": {
    "baseline": {
      "totalIssues": 99
    }
  }
}"#;
        let result = update_json_regression(config, &sample_baseline()).unwrap();
        assert!(!result.contains("99"));
        assert!(result.contains("\"totalIssues\": 5"));
        serde_json::from_str::<serde_json::Value>(&result).unwrap();
    }

    #[test]
    fn json_skips_regression_in_comment() {
        let config = "{\n  // See \"regression\" docs\n  \"entry\": []\n}";
        let result = update_json_regression(config, &sample_baseline()).unwrap();
        assert!(result.contains("\"regression\":"));
        assert!(result.contains("\"entry\""));
    }

    #[test]
    fn json_malformed_brace_returns_error() {
        let config = r#"{ "regression": { "baseline": { "totalIssues": 1 }"#;
        let result = update_json_regression(config, &sample_baseline());
        assert!(result.is_err());
    }

    #[test]
    fn toml_insert_into_empty() {
        let result = update_toml_regression("", &sample_baseline());
        assert!(result.contains("[regression.baseline]"));
        assert!(result.contains("totalIssues = 5"));
    }

    #[test]
    fn toml_insert_after_existing_content() {
        let config = "[rules]\nunused-files = \"warn\"\n";
        let result = update_toml_regression(config, &sample_baseline());
        assert!(result.contains("[rules]"));
        assert!(result.contains("[regression.baseline]"));
        assert!(result.contains("totalIssues = 5"));
    }

    #[test]
    fn toml_replace_existing_section() {
        let config =
            "[regression.baseline]\ntotalIssues = 99\n\n[rules]\nunused-files = \"warn\"\n";
        let result = update_toml_regression(config, &sample_baseline());
        assert!(!result.contains("99"));
        assert!(result.contains("totalIssues = 5"));
        assert!(result.contains("[rules]"));
    }

    #[test]
    fn find_json_key_basic() {
        assert_eq!(find_json_key(r#"{"foo": 1}"#, "foo"), Some(1));
    }

    #[test]
    fn find_json_key_skips_comment() {
        let content = "{\n  // \"foo\" is important\n  \"bar\": 1\n}";
        assert_eq!(find_json_key(content, "foo"), None);
        assert!(find_json_key(content, "bar").is_some());
    }

    #[test]
    fn find_json_key_not_found() {
        assert_eq!(find_json_key("{}", "missing"), None);
    }

    #[test]
    fn find_json_key_skips_block_comment() {
        let content = "{\n  /* \"foo\": old value */\n  \"foo\": 1\n}";
        let pos = find_json_key(content, "foo").unwrap();
        assert!(content[pos..].starts_with("\"foo\": 1"));
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
            unused_store_members: 0,
            unresolved_imports: 1,
            unlisted_dependencies: 0,
            duplicate_exports: 1,
            circular_dependencies: 0,
            re_export_cycles: 0,
            type_only_dependencies: 0,
            test_only_dependencies: 0,
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
    fn json_insert_with_trailing_comma() {
        let config = r#"{
  "entry": ["src/main.ts"],
}"#;
        let result = update_json_regression(config, &sample_baseline()).unwrap();
        assert!(result.contains("\"regression\""));
    }

    #[test]
    fn json_no_closing_brace_returns_error() {
        let result = update_json_regression("", &sample_baseline());
        assert!(result.is_err());
    }

    #[test]
    fn json_nested_regression_object_replaced_correctly() {
        let config = r#"{
  "regression": {
    "baseline": {
      "totalIssues": 99,
      "unusedFiles": 10
    },
    "tolerance": "5%"
  },
  "entry": ["src/main.ts"]
}"#;
        let result = update_json_regression(config, &sample_baseline()).unwrap();
        assert!(!result.contains("99"));
        assert!(result.contains("\"totalIssues\": 5"));
        assert!(result.contains("\"entry\""));
    }

    #[test]
    fn toml_content_without_trailing_newline() {
        let config = "[rules]\nunused-files = \"warn\"";
        let result = update_toml_regression(config, &sample_baseline());
        assert!(result.contains("[regression.baseline]"));
        assert!(result.contains("[rules]"));
    }

    #[test]
    fn toml_replace_section_not_at_end() {
        let config = "[regression.baseline]\ntotalIssues = 99\nunusedFiles = 10\n\n[rules]\nunused-files = \"warn\"\n";
        let result = update_toml_regression(config, &sample_baseline());
        assert!(!result.contains("99"));
        assert!(result.contains("totalIssues = 5"));
        assert!(result.contains("[rules]"));
        assert!(result.contains("unused-files = \"warn\""));
    }

    #[test]
    fn toml_replace_section_at_end() {
        let config =
            "[rules]\nunused-files = \"warn\"\n\n[regression.baseline]\ntotalIssues = 99\n";
        let result = update_toml_regression(config, &sample_baseline());
        assert!(!result.contains("99"));
        assert!(result.contains("totalIssues = 5"));
        assert!(result.contains("[rules]"));
    }

    #[test]
    fn find_json_key_multiple_same_keys() {
        let content = r#"{"foo": 1, "bar": {"foo": 2}}"#;
        let pos = find_json_key(content, "foo").unwrap();
        assert_eq!(pos, 1);
    }

    #[test]
    fn find_json_key_in_nested_comment_then_real() {
        let content = "{\n  // \"entry\": old\n  /* \"entry\": also old */\n  \"entry\": []\n}";
        let pos = find_json_key(content, "entry").unwrap();
        assert!(content[pos..].starts_with("\"entry\": []"));
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
        let msg =
            format_schema_mismatch_error(Path::new("/repo/.fallow-baseline.json"), 1, 99, "3.0.0");
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
        let msg =
            format_schema_mismatch_error(Path::new("/repo/.fallow-baseline.json"), 1, 0, "2.0.0");
        assert!(msg.contains("predate"));
        assert!(msg.contains("fallow 2.0.0"));
        assert!(
            msg.contains("fallow dead-code --save-regression-baseline /repo/.fallow-baseline.json")
        );
    }

    #[test]
    fn format_missing_schema_version_error_includes_regenerate_command() {
        let msg = format_missing_schema_version_error(Path::new("/repo/baseline.json"));
        assert!(msg.contains("missing the schema_version field"));
        assert!(msg.contains("fallow dead-code --save-regression-baseline /repo/baseline.json"));
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
