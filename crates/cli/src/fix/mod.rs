use rustc_hash::{FxHashMap, FxHashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{CatalogPrecedingCommentPolicy, OutputFormat};

mod catalog;
mod config;
mod deps;
mod enum_helpers;
mod enum_members;
mod exports;
mod io;
mod plan;

pub use fallow_config::is_config_fixable;

use plan::{CapturedHashes, CommitOutcome, FixPlan, SkippedFile};

fn run_analyze(
    config: &fallow_config::ResolvedConfig,
    output: OutputFormat,
) -> Result<(fallow_types::results::AnalysisResults, CapturedHashes), ExitCode> {
    let output_struct = fallow_engine::analyze_with_file_hashes(config)
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

pub fn run_fix(opts: &FixOptions<'_>) -> ExitCode {
    if !opts.dry_run && !opts.yes && !std::io::stdin().is_terminal() {
        let msg = "fix command requires --yes (or --force) in non-interactive environments. \
                   Use --dry-run to preview changes first, then pass --yes to confirm.";
        return crate::error::emit_error(msg, 2, opts.output);
    }

    let config = match crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        crate::runtime_support::LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
        },
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let (results, file_hashes) = match run_analyze(&config, opts.output) {
        Ok(r) => r,
        Err(code) => return code,
    };

    if results.total_issues() == 0 {
        return emit_empty_fix_output(opts);
    }

    let mut fixes: Vec<serde_json::Value> = Vec::new();
    let mut plan = FixPlan::new();

    let (had_write_error, catalog_totals) =
        apply_all_fixes(opts, &config, &results, &file_hashes, &mut plan, &mut fixes);

    finalize_fix_run(opts, plan, &mut fixes, had_write_error, &catalog_totals)
}

/// Commit the plan, emit output, and compute the exit code after every fixer ran.
fn finalize_fix_run(
    opts: &FixOptions<'_>,
    plan: FixPlan,
    fixes: &mut Vec<serde_json::Value>,
    mut had_write_error: bool,
    catalog_totals: &CatalogFixTotals,
) -> ExitCode {
    let plan_skip_records = build_skipped_records(opts.root, plan.skipped(), opts.quiet);
    fixes.extend(plan_skip_records.iter().cloned());

    let has_recoverable_skip = plan
        .skipped()
        .iter()
        .any(|skip| !skip.reason.is_intentional());

    let commit_outcome = commit_fix_plan(opts, plan, fixes);

    strip_target_sidechannel(fixes);

    let skip_counts = count_fix_skips(&plan_skip_records);
    if commit_outcome.had_failures() {
        had_write_error = true;
    }
    if has_recoverable_skip {
        had_write_error = true;
    }

    if let Err(code) = emit_fix_output(&FixOutputInput {
        output: opts.output,
        quiet: opts.quiet,
        dry_run: opts.dry_run,
        fixes,
        catalog_applied: catalog_totals.applied,
        catalog_skipped: catalog_totals.skipped,
        catalog_comment_lines_removed: catalog_totals.comment_lines_removed,
        content_changed_count: skip_counts.content_changed,
        mixed_line_endings_count: skip_counts.mixed_line_endings,
        low_confidence_count: skip_counts.low_confidence,
    }) {
        return code;
    }

    if had_write_error {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

/// Run every per-issue-type fixer, returning `(had_write_error, catalog_totals)`.
fn apply_all_fixes(
    opts: &FixOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    results: &fallow_types::results::AnalysisResults,
    file_hashes: &CapturedHashes,
    plan: &mut FixPlan,
    fixes: &mut Vec<serde_json::Value>,
) -> (bool, CatalogFixTotals) {
    apply_unused_export_fixes(&mut FixApplicationInput {
        root: opts.root,
        results,
        file_hashes,
        plan: &mut *plan,
        output: opts.output,
        dry_run: opts.dry_run,
        fixes: &mut *fixes,
    });

    deps::apply_dependency_fixes(&mut deps::DependencyFixInput {
        root: opts.root,
        results,
        hashes: file_hashes,
        plan: &mut *plan,
        output: opts.output,
        dry_run: opts.dry_run,
        fixes: &mut *fixes,
    });

    let mut had_write_error = config::apply_config_fixes(config::ConfigFixInput {
        root: opts.root,
        config_path: opts.config_path.as_ref(),
        results,
        output: opts.output,
        dry_run: opts.dry_run,
        no_create_config: opts.no_create_config,
        fixes,
    });

    apply_unused_enum_member_fixes(&mut FixApplicationInput {
        root: opts.root,
        results,
        file_hashes,
        plan: &mut *plan,
        output: opts.output,
        dry_run: opts.dry_run,
        fixes: &mut *fixes,
    });

    let catalog_totals = apply_catalog_fixes(&mut CatalogFixRequest {
        root: opts.root,
        results,
        file_hashes,
        plan,
        delete_preceding_comments: config.fix.catalog.delete_preceding_comments,
        output: opts.output,
        dry_run: opts.dry_run,
        fixes,
    });
    had_write_error |= catalog_totals.write_error;

    (had_write_error, catalog_totals)
}

fn emit_empty_fix_output(opts: &FixOptions<'_>) -> ExitCode {
    if matches!(opts.output, OutputFormat::Json) {
        let fixes = [];
        match fallow_output::serialize_fix_json_output(fallow_output::FixJsonOutputInput {
            dry_run: opts.dry_run,
            fixes: &fixes,
            skipped_content_changed: 0,
            skipped_mixed_line_endings: 0,
            skipped_low_confidence_exports: 0,
        })
        .and_then(|value| serde_json::to_string_pretty(&value))
        {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Error: failed to serialize fix output: {e}");
                return ExitCode::from(2);
            }
        }
    } else if !opts.quiet {
        eprintln!("No issues to fix.");
    }
    ExitCode::SUCCESS
}

struct FixApplicationInput<'a> {
    root: &'a Path,
    results: &'a fallow_types::results::AnalysisResults,
    file_hashes: &'a CapturedHashes,
    plan: &'a mut FixPlan,
    output: OutputFormat,
    dry_run: bool,
    fixes: &'a mut Vec<serde_json::Value>,
}

fn apply_unused_export_fixes(input: &mut FixApplicationInput<'_>) {
    let mut exports_by_file: FxHashMap<PathBuf, Vec<&fallow_types::results::UnusedExport>> =
        FxHashMap::default();
    for finding in &input.results.unused_exports {
        exports_by_file
            .entry(finding.export.path.clone())
            .or_default()
            .push(&finding.export);
    }
    let unresolved_import_files: FxHashSet<PathBuf> = input
        .results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.path.clone())
        .collect();
    exports::apply_export_fixes(&mut exports::ExportFixInput {
        root: input.root,
        exports_by_file: &exports_by_file,
        hashes: input.file_hashes,
        unresolved_import_files: &unresolved_import_files,
        plan: input.plan,
        output: input.output,
        dry_run: input.dry_run,
        fixes: input.fixes,
    });
}

fn apply_unused_enum_member_fixes(input: &mut FixApplicationInput<'_>) {
    if input.results.unused_enum_members.is_empty() {
        return;
    }
    let mut enum_members_by_file: FxHashMap<PathBuf, Vec<&fallow_types::results::UnusedMember>> =
        FxHashMap::default();
    for finding in &input.results.unused_enum_members {
        enum_members_by_file
            .entry(finding.member.path.clone())
            .or_default()
            .push(&finding.member);
    }
    enum_members::apply_enum_member_fixes(enum_members::EnumMemberFixInput {
        root: input.root,
        members_by_file: &enum_members_by_file,
        hashes: input.file_hashes,
        plan: input.plan,
        output: input.output,
        dry_run: input.dry_run,
        fixes: input.fixes,
    });
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

struct FixOutputInput<'a> {
    output: OutputFormat,
    quiet: bool,
    dry_run: bool,
    fixes: &'a [serde_json::Value],
    catalog_applied: usize,
    catalog_skipped: usize,
    catalog_comment_lines_removed: usize,
    content_changed_count: usize,
    mixed_line_endings_count: usize,
    low_confidence_count: usize,
}

struct CatalogFixTotals {
    applied: usize,
    skipped: usize,
    comment_lines_removed: usize,
    write_error: bool,
}

struct CatalogFixRequest<'a> {
    root: &'a Path,
    results: &'a fallow_types::results::AnalysisResults,
    file_hashes: &'a CapturedHashes,
    plan: &'a mut FixPlan,
    delete_preceding_comments: CatalogPrecedingCommentPolicy,
    output: OutputFormat,
    dry_run: bool,
    fixes: &'a mut Vec<serde_json::Value>,
}

struct FixSkipCounts {
    content_changed: usize,
    mixed_line_endings: usize,
    low_confidence: usize,
}

fn count_fix_skips(records: &[serde_json::Value]) -> FixSkipCounts {
    let count_reason = |reason: &str| {
        records
            .iter()
            .filter(|record| {
                record
                    .get("skip_reason")
                    .and_then(serde_json::Value::as_str)
                    == Some(reason)
            })
            .count()
    };
    let low_confidence = records
        .iter()
        .filter(|record| {
            matches!(
                record
                    .get("skip_reason")
                    .and_then(serde_json::Value::as_str),
                Some("low_confidence_off_graph" | "low_confidence_unresolved_imports")
            )
        })
        .count();
    FixSkipCounts {
        content_changed: count_reason("content_changed"),
        mixed_line_endings: count_reason("mixed_line_endings"),
        low_confidence,
    }
}

fn apply_catalog_fixes(request: &mut CatalogFixRequest<'_>) -> CatalogFixTotals {
    let catalog_summary = catalog::apply_catalog_entry_fixes(
        request.root,
        &request.results.unused_catalog_entries,
        request.delete_preceding_comments,
        catalog::CatalogFixContext {
            hashes: request.file_hashes,
            plan: &mut *request.plan,
            output: request.output,
            dry_run: request.dry_run,
            fixes: &mut *request.fixes,
        },
    );
    let empty_catalog_summary =
        catalog::apply_empty_catalog_group_fixes(catalog::EmptyCatalogGroupFixInput {
            root: request.root,
            groups: &request.results.empty_catalog_groups,
            hashes: request.file_hashes,
            plan: request.plan,
            output: request.output,
            dry_run: request.dry_run,
            fixes: request.fixes,
        });
    CatalogFixTotals {
        applied: catalog_summary.applied + empty_catalog_summary.applied,
        skipped: catalog_summary.skipped + empty_catalog_summary.skipped,
        comment_lines_removed: catalog_summary.comment_lines_removed,
        write_error: catalog_summary.write_error || empty_catalog_summary.write_error,
    }
}

fn commit_fix_plan(
    opts: &FixOptions<'_>,
    plan: FixPlan,
    fixes: &mut [serde_json::Value],
) -> CommitOutcome {
    if opts.dry_run {
        return CommitOutcome::empty_for_dry_run();
    }
    let outcome = plan.commit();
    patch_applied_field_on_failure(fixes, opts.root, &outcome.failed);
    outcome
}

fn emit_fix_output(input: &FixOutputInput<'_>) -> Result<(), ExitCode> {
    if matches!(input.output, OutputFormat::Json) {
        match fallow_output::serialize_fix_json_output(fallow_output::FixJsonOutputInput {
            dry_run: input.dry_run,
            fixes: input.fixes,
            skipped_content_changed: input.content_changed_count,
            skipped_mixed_line_endings: input.mixed_line_endings_count,
            skipped_low_confidence_exports: input.low_confidence_count,
        })
        .and_then(|value| serde_json::to_string_pretty(&value))
        {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Error: failed to serialize fix output: {e}");
                return Err(ExitCode::from(2));
            }
        }
    } else if !input.quiet {
        emit_human_summary(&HumanSummaryInput {
            dry_run: input.dry_run,
            fixes: input.fixes,
            catalog_applied: input.catalog_applied,
            catalog_skipped: input.catalog_skipped,
            catalog_comment_lines_removed: input.catalog_comment_lines_removed,
            content_changed_count: input.content_changed_count,
            mixed_line_endings_count: input.mixed_line_endings_count,
            low_confidence_count: input.low_confidence_count,
        });
    }
    Ok(())
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
struct HumanSummaryInput<'a> {
    dry_run: bool,
    fixes: &'a [serde_json::Value],
    catalog_applied: usize,
    catalog_skipped: usize,
    catalog_comment_lines_removed: usize,
    content_changed_count: usize,
    mixed_line_endings_count: usize,
    low_confidence_count: usize,
}

fn emit_human_summary(input: &HumanSummaryInput<'_>) {
    emit_fix_count_line(
        input.dry_run,
        input.fixes,
        input.catalog_comment_lines_removed,
    );
    if !input.dry_run && input.catalog_applied > 0 {
        eprintln!(
            "Catalog entries were removed from pnpm-workspace.yaml. Run `pnpm install` to refresh pnpm-lock.yaml.",
        );
    }
    emit_residual_skip_warnings(input);
}

/// Print the leading dry-run notice or `Fixed N issue(s)` count line.
fn emit_fix_count_line(
    dry_run: bool,
    fixes: &[serde_json::Value],
    catalog_comment_lines_removed: usize,
) {
    if dry_run {
        eprintln!("Dry run complete. No files were modified.");
        return;
    }
    let fixed_count = fallow_output::count_applied_fixes(fixes);
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

/// Print the trailing skipped-entry warning lines (catalog guards, hash
/// mismatch, mixed line endings, low-confidence exports).
fn emit_residual_skip_warnings(input: &HumanSummaryInput<'_>) {
    if input.catalog_skipped > 0 {
        let entries_word = if input.catalog_skipped == 1 {
            "entry"
        } else {
            "entries"
        };
        eprintln!(
            "Skipped {} catalog {entries_word} with hardcoded consumers or other guards (run with --format json for details).",
            input.catalog_skipped,
        );
    }
    if input.content_changed_count > 0 {
        let files_word = if input.content_changed_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Skipped {} {files_word} that changed since `fallow dead-code` ran. Re-run `fallow fix` to refresh the analysis.",
            input.content_changed_count,
        );
    }
    if input.mixed_line_endings_count > 0 {
        let files_word = if input.mixed_line_endings_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Skipped {} {files_word} with mixed CRLF/LF line endings. Normalize each file (`dos2unix <path>` or `git config core.autocrlf input` + re-checkout) before re-running.",
            input.mixed_line_endings_count,
        );
    }
    if input.low_confidence_count > 0 {
        let files_word = if input.low_confidence_count == 1 {
            "file"
        } else {
            "files"
        };
        eprintln!(
            "Kept unused exports in {} {files_word} where consumers may be invisible to fallow (test, mock, and fixture directories, or files with unresolved imports). Still listed by `fallow dead-code`; remove by hand if you have confirmed they are unused.",
            input.low_confidence_count,
        );
    }
}
