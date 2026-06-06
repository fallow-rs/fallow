use rustc_hash::{FxHashMap, FxHashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;

mod catalog;
mod config;
mod deps;
mod enum_helpers;
mod enum_members;
mod exports;
mod io;
mod plan;

pub use config::is_config_fixable;

use plan::{CapturedHashes, CommitOutcome, FixPlan, SkippedFile};

fn run_analyze(
    config: &fallow_config::ResolvedConfig,
    output: OutputFormat,
) -> Result<(fallow_core::results::AnalysisResults, CapturedHashes), ExitCode> {
    #[expect(
        deprecated,
        reason = "ADR-008 deprecates fallow_core::analyze_with_file_hashes externally; the CLI still uses the workspace path dependency"
    )]
    let output_struct = fallow_core::analyze_with_file_hashes(config)
        .map_err(|e| crate::error::emit_error(&format!("Analysis error: {e}"), 2, output))?;
    Ok((output_struct.results, output_struct.file_hashes))
}

pub struct FixOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub dry_run: bool,
    pub yes: bool,
    pub production: bool,
    /// Refuse to create a new fallow config file when none exists. The
    /// duplicate-export config-add path is skipped with an explanatory
    /// entry; source-file fixes proceed normally. Honored by
    /// `fix::config::apply_config_fixes`.
    pub no_create_config: bool,
}

#[expect(
    clippy::too_many_lines,
    reason = "orchestrator threads results across 5 per-issue-type fixers + the post-#454 commit + envelope assembly; splitting harms locality of the wire-format authoring"
)]
pub fn run_fix(opts: &FixOptions<'_>) -> ExitCode {
    if !opts.dry_run && !opts.yes && !std::io::stdin().is_terminal() {
        let msg = "fix command requires --yes (or --force) in non-interactive environments. \
                   Use --dry-run to preview changes first, then pass --yes to confirm.";
        return crate::error::emit_error(msg, 2, opts.output);
    }

    let config = match crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production,
        opts.quiet,
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let (results, file_hashes) = match run_analyze(&config, opts.output) {
        Ok(r) => r,
        Err(code) => return code,
    };

    if results.total_issues() == 0 {
        if matches!(opts.output, OutputFormat::Json) {
            match serde_json::to_string_pretty(&serde_json::json!({
                "dry_run": opts.dry_run,
                "fixes": [],
                "total_fixed": 0,
                "skipped": 0,
                "skipped_content_changed": 0,
                "skipped_mixed_line_endings": 0,
                "skipped_low_confidence_exports": 0,
            })) {
                Ok(json) => println!("{json}"),
                Err(e) => {
                    eprintln!("Error: failed to serialize fix output: {e}");
                    return ExitCode::from(2);
                }
            }
        } else if !opts.quiet {
            eprintln!("No issues to fix.");
        }
        return ExitCode::SUCCESS;
    }

    let mut fixes: Vec<serde_json::Value> = Vec::new();
    let mut plan = FixPlan::new();

    let mut exports_by_file: FxHashMap<PathBuf, Vec<&fallow_core::results::UnusedExport>> =
        FxHashMap::default();
    for finding in &results.unused_exports {
        exports_by_file
            .entry(finding.export.path.clone())
            .or_default()
            .push(&finding.export);
    }

    let unresolved_import_files: FxHashSet<PathBuf> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.path.clone())
        .collect();

    exports::apply_export_fixes(
        opts.root,
        &exports_by_file,
        &file_hashes,
        &unresolved_import_files,
        &mut plan,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );

    deps::apply_dependency_fixes(
        opts.root,
        &results,
        &file_hashes,
        &mut plan,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );

    let mut had_write_error = config::apply_config_fixes(
        opts.root,
        opts.config_path.as_ref(),
        &results,
        opts.output,
        opts.dry_run,
        opts.no_create_config,
        &mut fixes,
    );

    if !results.unused_enum_members.is_empty() {
        let mut enum_members_by_file: FxHashMap<PathBuf, Vec<&fallow_core::results::UnusedMember>> =
            FxHashMap::default();
        for finding in &results.unused_enum_members {
            enum_members_by_file
                .entry(finding.member.path.clone())
                .or_default()
                .push(&finding.member);
        }

        enum_members::apply_enum_member_fixes(
            opts.root,
            &enum_members_by_file,
            &file_hashes,
            &mut plan,
            opts.output,
            opts.dry_run,
            &mut fixes,
        );
    }

    let catalog_summary = catalog::apply_catalog_entry_fixes(
        opts.root,
        &results.unused_catalog_entries,
        config.fix.catalog.delete_preceding_comments,
        &file_hashes,
        &mut plan,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );
    had_write_error |= catalog_summary.write_error;
    let empty_catalog_summary = catalog::apply_empty_catalog_group_fixes(
        opts.root,
        &results.empty_catalog_groups,
        &file_hashes,
        &mut plan,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );
    had_write_error |= empty_catalog_summary.write_error;
    let catalog_applied = catalog_summary.applied + empty_catalog_summary.applied;
    let catalog_skipped = catalog_summary.skipped + empty_catalog_summary.skipped;
    let catalog_comment_lines_removed = catalog_summary.comment_lines_removed;

    let plan_skip_records = build_skipped_records(opts.root, plan.skipped(), opts.quiet);
    fixes.extend(plan_skip_records.iter().cloned());

    let has_recoverable_skip = plan
        .skipped()
        .iter()
        .any(|skip| !skip.reason.is_intentional());

    let commit_outcome = if opts.dry_run {
        CommitOutcome::empty_for_dry_run()
    } else {
        let outcome = plan.commit();
        patch_applied_field_on_failure(&mut fixes, opts.root, &outcome.failed);
        outcome
    };

    strip_target_sidechannel(&mut fixes);

    let content_changed_count = plan_skip_records
        .iter()
        .filter(|r| {
            r.get("skip_reason").and_then(serde_json::Value::as_str) == Some("content_changed")
        })
        .count();
    let mixed_line_endings_count = plan_skip_records
        .iter()
        .filter(|r| {
            r.get("skip_reason").and_then(serde_json::Value::as_str) == Some("mixed_line_endings")
        })
        .count();
    let low_confidence_count = plan_skip_records
        .iter()
        .filter(|r| {
            matches!(
                r.get("skip_reason").and_then(serde_json::Value::as_str),
                Some("low_confidence_off_graph" | "low_confidence_unresolved_imports")
            )
        })
        .count();
    if commit_outcome.had_failures() {
        had_write_error = true;
    }
    if has_recoverable_skip {
        had_write_error = true;
    }

    if matches!(opts.output, OutputFormat::Json) {
        let applied_count = fixes
            .iter()
            .filter(|f| {
                f.get("applied")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        let skipped_count = fixes
            .iter()
            .filter(|f| {
                let is_skipped = f
                    .get("skipped")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let reason = f.get("skip_reason").and_then(serde_json::Value::as_str);
                let is_plan_skip = matches!(
                    reason,
                    Some(
                        "content_changed"
                            | "mixed_line_endings"
                            | "low_confidence_off_graph"
                            | "low_confidence_unresolved_imports"
                    )
                );
                is_skipped && !is_plan_skip
            })
            .count();
        match serde_json::to_string_pretty(&serde_json::json!({
            "dry_run": opts.dry_run,
            "fixes": fixes,
            "total_fixed": applied_count,
            "skipped": skipped_count,
            "skipped_content_changed": content_changed_count,
            "skipped_mixed_line_endings": mixed_line_endings_count,
            "skipped_low_confidence_exports": low_confidence_count,
        })) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Error: failed to serialize fix output: {e}");
                return ExitCode::from(2);
            }
        }
    } else if !opts.quiet {
        emit_human_summary(
            opts.dry_run,
            &fixes,
            catalog_applied,
            catalog_skipped,
            catalog_comment_lines_removed,
            content_changed_count,
            mixed_line_endings_count,
            low_confidence_count,
        );
    }

    if had_write_error {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

impl CommitOutcome {
    /// Sentinel used by the orchestrator on the dry-run path to avoid
    /// touching disk while still satisfying the post-commit code shape.
    pub(super) fn empty_for_dry_run() -> Self {
        Self {
            written: rustc_hash::FxHashSet::default(),
            failed: Vec::new(),
        }
    }

    pub(super) fn had_failures(&self) -> bool {
        !self.failed.is_empty()
    }
}

/// Build JSON entries for files the FixPlan decided to skip during the
/// hash-precondition check. One entry per skipped file; the orchestrator
/// surfaces them in the same `fixes` array used for applied fixes so
/// downstream consumers (JSON renderer, human summary, jq scripts) see
/// the diagnostic in one stream.
///
/// `quiet` suppresses the per-file stderr diagnostic, matching how
/// `opts.quiet` gates the rest of the human summary. JSON consumers
/// always see the skip records via the returned vec; only the streaming
/// stderr line is gated.
fn build_skipped_records(
    root: &Path,
    skipped: &[SkippedFile],
    quiet: bool,
) -> Vec<serde_json::Value> {
    skipped
        .iter()
        .map(|skip| {
            let relative = skip.path.strip_prefix(root).unwrap_or(&skip.path);
            if !quiet {
                eprintln!("{}", skip.reason.human_message(relative));
            }
            serde_json::json!({
                "type": "skipped",
                "path": relative.display().to_string(),
                "skipped": true,
                "skip_reason": skip.reason.as_wire_str(),
            })
        })
        .collect()
}

/// Walk every fix entry produced by the per-issue-type fixers and flip
/// `applied` to false for any entry whose target path landed in the
/// commit's `failed` set. The fixer pushed entries with optimistic
/// `applied: true`; this is the post-commit correction.
fn patch_applied_field_on_failure(
    fixes: &mut [serde_json::Value],
    root: &Path,
    failed: &[(PathBuf, std::io::Error)],
) {
    if failed.is_empty() {
        return;
    }
    let failed_paths: rustc_hash::FxHashSet<PathBuf> =
        failed.iter().map(|(p, _)| p.clone()).collect();
    for (path, err) in failed {
        let relative = path.strip_prefix(root).unwrap_or(path);
        eprintln!("Error: failed to write {}: {err}", relative.display());
    }
    for entry in fixes.iter_mut() {
        let target = entry.get("__target").and_then(|v| v.as_str());
        let Some(target_str) = target else { continue };
        if failed_paths.contains(&PathBuf::from(target_str)) {
            entry["applied"] = serde_json::json!(false);
        }
    }
}

/// Remove the orchestrator-private `__target` correlation field from
/// every fix entry before serialization. The field is an implementation
/// detail; the public JSON shape stays unchanged.
fn strip_target_sidechannel(fixes: &mut [serde_json::Value]) {
    for entry in fixes.iter_mut() {
        if let Some(obj) = entry.as_object_mut() {
            obj.remove("__target");
        }
    }
}

/// Print the human stderr summary block at the end of a fix run.
///
/// Ordering rationale: the most actionable next step (`pnpm install`)
/// follows the success line so users see what to do next before any
/// residual-work warnings. Skipped-entry counts come last because they
/// describe work the user opted out of rather than work they need to
/// do right now.
#[expect(
    clippy::too_many_arguments,
    reason = "each count drives an independent stderr line; grouping into a struct would just move the same scalars behind a name with no readability gain at the single call site"
)]
fn emit_human_summary(
    dry_run: bool,
    fixes: &[serde_json::Value],
    catalog_applied: usize,
    catalog_skipped: usize,
    catalog_comment_lines_removed: usize,
    content_changed_count: usize,
    mixed_line_endings_count: usize,
    low_confidence_count: usize,
) {
    if dry_run {
        eprintln!("Dry run complete. No files were modified.");
    } else {
        let fixed_count = fixes
            .iter()
            .filter(|f| {
                f.get("applied")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        if catalog_comment_lines_removed > 0 {
            let line_word = if catalog_comment_lines_removed == 1 {
                "line"
            } else {
                "lines"
            };
            eprintln!(
                "Fixed {fixed_count} issue(s) (+{catalog_comment_lines_removed} catalog comment {line_word})."
            );
        } else {
            eprintln!("Fixed {fixed_count} issue(s).");
        }
    }
    if !dry_run && catalog_applied > 0 {
        eprintln!(
            "Catalog entries were removed from pnpm-workspace.yaml. Run `pnpm install` to refresh pnpm-lock.yaml.",
        );
    }
    if catalog_skipped > 0 {
        let entries_word = if catalog_skipped == 1 {
            "entry"
        } else {
            "entries"
        };
        eprintln!(
            "Skipped {catalog_skipped} catalog {entries_word} with hardcoded consumers or other guards (run with --format json for details).",
        );
    }
    if content_changed_count > 0 {
        let files_word = if content_changed_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Skipped {content_changed_count} {files_word} that changed since `fallow dead-code` ran. Re-run `fallow fix` to refresh the analysis."
        );
    }
    if mixed_line_endings_count > 0 {
        let files_word = if mixed_line_endings_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Skipped {mixed_line_endings_count} {files_word} with mixed CRLF/LF line endings. Normalize each file (`dos2unix <path>` or `git config core.autocrlf input` + re-checkout) before re-running.",
        );
    }
    if low_confidence_count > 0 {
        let files_word = if low_confidence_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Kept unused exports in {low_confidence_count} {files_word} where consumers may be invisible to fallow (test, mock, and fixture directories, or files with unresolved imports). Still listed by `fallow dead-code`; remove by hand if you have confirmed they are unused.",
        );
    }
}
