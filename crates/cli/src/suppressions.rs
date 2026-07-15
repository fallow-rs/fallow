//! `fallow suppressions` subcommand: read-only suppression inventory.
//!
//! Lists every `fallow-ignore-next-line` / `fallow-ignore-file` marker present
//! in analyzed files, grouped per file with line, kind, level, and reason,
//! plus project totals and a stale cross-reference. A governance surface, not
//! a detector: the command always exits 0.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_output::{
    SuppressionInventoryFile, SuppressionInventoryLevel, SuppressionInventoryOutput,
    SuppressionInventoryOutputInput, build_suppression_inventory_output,
    serialize_suppression_inventory_json_output,
};
use fallow_types::results::ActiveSuppression;

use crate::error::emit_error;

/// Options for the `fallow suppressions` subcommand.
pub struct SuppressionsOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub allow_remote_extends: bool,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub changed_since: Option<&'a str>,
    pub file: &'a [PathBuf],
}

/// Run the `fallow suppressions` subcommand. Read-only inventory: exits 0
/// whenever the analysis itself succeeds, regardless of what it lists.
pub fn run_suppressions(opts: &SuppressionsOptions<'_>) -> ExitCode {
    let start = Instant::now();

    if let Err(code) = validate_suppressions_output(opts.output) {
        return code;
    }
    let config = match load_suppressions_config(opts) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let session = fallow_engine::session::AnalysisSession::from_resolved_config(config);
    // The dead-code execute path is what populates `active_suppressions` and
    // `stale_suppressions`; presence capture is uniform across all suppression
    // kinds (complexity and code-duplication markers included).
    let results = match session.analyze_dead_code_with_artifacts(false, false) {
        Ok(artifacts) => artifacts.results,
        Err(err) => return emit_error(&format!("Analysis error: {err}"), 2, opts.output),
    };

    let mut active = results.active_suppressions;
    if let Err(code) = apply_suppression_scopes(&mut active, opts) {
        return code;
    }
    crate::telemetry::note_result_count(active.len());

    let output = build_suppression_inventory_output(SuppressionInventoryOutputInput {
        active: &active,
        // The stale join keys on the scoped active set, so the raw run-level
        // findings are safe to pass unscoped.
        stale: &results.stale_suppressions,
        root: &session.config().root,
    });

    let elapsed = start.elapsed();
    match opts.output {
        OutputFormat::Human => print_suppressions_human(&output, elapsed, opts.quiet),
        OutputFormat::Json => print_suppressions_json(output, opts.json_style),
        _ => unreachable!("validated above"),
    }

    ExitCode::SUCCESS
}

fn validate_suppressions_output(output: OutputFormat) -> Result<(), ExitCode> {
    if matches!(output, OutputFormat::Human | OutputFormat::Json) {
        Ok(())
    } else {
        Err(emit_error(
            "fallow suppressions supports --format human or json only.",
            2,
            output,
        ))
    }
}

fn load_suppressions_config(opts: &SuppressionsOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        crate::runtime_support::LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
    )
}

/// Apply `--changed-since`, `--workspace` / `--changed-workspaces`, and
/// `--file` scoping to the active suppression set, mirroring `fallow flags`.
fn apply_suppression_scopes(
    active: &mut Vec<ActiveSuppression>,
    opts: &SuppressionsOptions<'_>,
) -> Result<(), ExitCode> {
    if let Some(git_ref) = opts.changed_since
        && let Some(changed) = crate::check::get_changed_files(opts.root, git_ref)
    {
        active.retain(|s| changed.contains(&s.path));
    }

    let ws_scope = crate::check::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    if let Some(ref ws_roots) = ws_scope {
        active.retain(|s| ws_roots.iter().any(|r| s.path.starts_with(r)));
    }

    apply_file_scope(active, opts);
    Ok(())
}

/// Retain only suppressions in the `--file` selection. Relative paths resolve
/// against the project root; a missing file warns but never errors.
fn apply_file_scope(active: &mut Vec<ActiveSuppression>, opts: &SuppressionsOptions<'_>) {
    if opts.file.is_empty() {
        return;
    }

    let resolved_files: Vec<PathBuf> = opts
        .file
        .iter()
        .map(|path| {
            if crate::path_util::is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                opts.root.join(path)
            }
        })
        .collect();

    if !opts.quiet {
        for (original, resolved) in opts.file.iter().zip(&resolved_files) {
            if !resolved.exists() {
                eprintln!(
                    "Warning: --file '{}' (resolved to '{}') was not found in the project",
                    original.display(),
                    resolved.display()
                );
            }
        }
    }

    let file_set: rustc_hash::FxHashSet<PathBuf> = resolved_files.into_iter().collect();
    active.retain(|s| file_set.contains(&s.path));
}

/// Human-facing kind label: blanket markers read as the word "blanket". The
/// JSON contract deliberately keeps `null` instead (machine consumers branch
/// on `null`); this asymmetry is by design.
fn kind_label(kind: Option<&str>) -> &str {
    kind.unwrap_or("blanket")
}

/// Print a file path with dimmed directory and bold filename.
fn print_file_path(display: &str) {
    use colored::Colorize;
    if let Some(parent) = Path::new(display).parent() {
        let parent_str = parent.to_string_lossy();
        let file_name = Path::new(display)
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().to_string());
        if parent_str.is_empty() {
            println!("  {}", file_name.bold());
        } else {
            println!(
                "  {}{}{}",
                parent_str.dimmed(),
                "/".dimmed(),
                file_name.bold()
            );
        }
    } else {
        println!("  {}", display.bold());
    }
}

/// Human-readable output for `fallow suppressions`.
fn print_suppressions_human(
    output: &SuppressionInventoryOutput,
    elapsed: std::time::Duration,
    quiet: bool,
) {
    use colored::Colorize;

    if output.summary.total == 0 {
        if !quiet {
            eprintln!(
                "{} No suppression markers found ({:.2}s)",
                "\u{2713}".green().bold(),
                elapsed.as_secs_f64()
            );
        }
        return;
    }

    let label = format!("Suppression inventory ({})", output.summary.total);
    println!("{} {}", "\u{25cf}".cyan(), label.cyan().bold());
    for file in &output.files {
        print_suppression_file(file);
    }

    println!();
    println!("{} {}", "\u{25cf}".cyan(), "Totals by kind".cyan().bold());
    for row in &output.summary.by_kind {
        println!("  {}: {}", kind_label(row.kind.as_deref()), row.count);
    }

    if !quiet {
        let elapsed_str = format!("{:.2}s", elapsed.as_secs_f64());
        eprintln!(
            "\n{} {} suppression{} in {} file{}, {} without reason{}, {} stale ({})",
            "\u{2713}".green().bold(),
            output.summary.total,
            crate::report::plural(output.summary.total),
            output.summary.files,
            crate::report::plural(output.summary.files),
            output.summary.without_reason,
            crate::report::plural(output.summary.without_reason),
            output.summary.stale,
            elapsed_str.dimmed(),
        );
    }
}

/// Print one file's suppression listing (human format).
fn print_suppression_file(file: &SuppressionInventoryFile) {
    use colored::Colorize;

    let display = file.path.to_string_lossy().replace('\\', "/");
    print_file_path(&display);

    for entry in &file.suppressions {
        let level_tag = if entry.level == SuppressionInventoryLevel::File {
            " (file-wide)"
        } else {
            ""
        };
        let reason = entry
            .reason
            .as_deref()
            .map_or_else(|| "reason: missing".to_string(), |r| format!("reason: {r}"));
        println!(
            "    {} {}{} {}",
            format!(":{}", entry.line).dimmed(),
            kind_label(entry.kind.as_deref()).bold(),
            level_tag.dimmed(),
            reason.dimmed(),
        );
    }
}

/// JSON output for `fallow suppressions`.
#[expect(
    clippy::expect_used,
    reason = "suppression inventory JSON output is built from serializable literals"
)]
fn print_suppressions_json(
    output: SuppressionInventoryOutput,
    json_style: crate::json_style::JsonStyle,
) {
    let value = serialize_suppression_inventory_json_output(
        output,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .expect("JSON serialization should not fail");

    println!(
        "{}",
        json_style
            .serialize(&value)
            .expect("JSON serialization should not fail")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No explicit `--config`; static so the `&Option<PathBuf>` field borrows it.
    const NO_CONFIG: Option<PathBuf> = None;

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/suppression-reasons")
    }

    fn suppressions_opts(root: &Path, output: OutputFormat) -> SuppressionsOptions<'_> {
        SuppressionsOptions {
            root,
            config_path: &NO_CONFIG,
            output,
            json_style: crate::json_style::JsonStyle::Compact,
            no_cache: true,
            threads: 1,
            quiet: true,
            production: false,
            allow_remote_extends: false,
            workspace: None,
            changed_workspaces: None,
            changed_since: None,
            file: &[],
        }
    }

    #[test]
    fn run_suppressions_renders_human_and_json() {
        colored::control::set_override(false);
        let root = fixture_root();
        for output in [OutputFormat::Human, OutputFormat::Json] {
            assert_eq!(
                run_suppressions(&suppressions_opts(&root, output)),
                ExitCode::SUCCESS,
                "format {output:?} should render and exit 0"
            );
        }
    }

    #[test]
    fn run_suppressions_rejects_unsupported_format() {
        let root = fixture_root();
        for output in [
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::Badge,
        ] {
            assert_eq!(
                run_suppressions(&suppressions_opts(&root, output)),
                ExitCode::from(2),
                "format {output:?} should be rejected"
            );
        }
    }

    #[test]
    fn file_scope_retains_matching_paths_only() {
        let root = Path::new("/proj");
        let mut active = vec![supp("/proj/src/a.ts"), supp("/proj/src/b.ts")];
        let files = vec![PathBuf::from("src/a.ts")];
        let opts = SuppressionsOptions {
            file: &files,
            ..suppressions_opts(root, OutputFormat::Json)
        };

        apply_file_scope(&mut active, &opts);

        assert_eq!(active.len(), 1);
        assert!(active[0].path.ends_with("src/a.ts"));
    }

    #[test]
    fn kind_label_renders_blanket_for_none() {
        assert_eq!(kind_label(None), "blanket");
        assert_eq!(kind_label(Some("unused-export")), "unused-export");
    }

    fn supp(path: &str) -> ActiveSuppression {
        ActiveSuppression {
            path: PathBuf::from(path),
            kind: Some("unused-export".to_owned()),
            is_file_level: false,
            reason: None,
            comment_line: 1,
        }
    }
}
