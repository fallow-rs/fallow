use rustc_hash::FxHashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;

mod catalog;
mod deps;
mod enum_helpers;
mod enum_members;
mod exports;
mod io;

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
}

pub fn run_fix(opts: &FixOptions<'_>) -> ExitCode {
    // In non-TTY environments (CI, AI agents), require --yes or --dry-run
    // to prevent accidental destructive operations.
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

    let results = match fallow_core::analyze(&config) {
        Ok(r) => r,
        Err(e) => {
            return crate::error::emit_error(&format!("Analysis error: {e}"), 2, opts.output);
        }
    };

    if results.total_issues() == 0 {
        if matches!(opts.output, OutputFormat::Json) {
            match serde_json::to_string_pretty(&serde_json::json!({
                "dry_run": opts.dry_run,
                "fixes": [],
                "total_fixed": 0,
                "skipped": 0,
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

    // Group exports by file path so we can apply all fixes to a single in-memory copy.
    let mut exports_by_file: FxHashMap<PathBuf, Vec<&fallow_core::results::UnusedExport>> =
        FxHashMap::default();
    for export in &results.unused_exports {
        exports_by_file
            .entry(export.path.clone())
            .or_default()
            .push(export);
    }

    let mut had_write_error = exports::apply_export_fixes(
        opts.root,
        &exports_by_file,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );

    had_write_error |=
        deps::apply_dependency_fixes(opts.root, &results, opts.output, opts.dry_run, &mut fixes);

    // Group unused enum members by file path for batch editing.
    if !results.unused_enum_members.is_empty() {
        let mut enum_members_by_file: FxHashMap<PathBuf, Vec<&fallow_core::results::UnusedMember>> =
            FxHashMap::default();
        for member in &results.unused_enum_members {
            enum_members_by_file
                .entry(member.path.clone())
                .or_default()
                .push(member);
        }

        had_write_error |= enum_members::apply_enum_member_fixes(
            opts.root,
            &enum_members_by_file,
            opts.output,
            opts.dry_run,
            &mut fixes,
        );
    }

    let catalog_summary = catalog::apply_catalog_entry_fixes(
        opts.root,
        &results.unused_catalog_entries,
        opts.output,
        opts.dry_run,
        &mut fixes,
    );
    had_write_error |= catalog_summary.write_error;
    let catalog_applied = catalog_summary.applied;
    let catalog_skipped = catalog_summary.skipped;

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
                f.get("skipped")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            })
            .count();
        match serde_json::to_string_pretty(&serde_json::json!({
            "dry_run": opts.dry_run,
            "fixes": fixes,
            "total_fixed": applied_count,
            "skipped": skipped_count,
        })) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Error: failed to serialize fix output: {e}");
                return ExitCode::from(2);
            }
        }
    } else if !opts.quiet {
        emit_human_summary(opts.dry_run, &fixes, catalog_applied, catalog_skipped);
    }

    if had_write_error {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

/// Print the human stderr summary block at the end of a fix run.
///
/// Ordering rationale: the most actionable next step (`pnpm install`)
/// follows the success line so users see what to do next before any
/// residual-work warnings. Skipped-entry counts come last because they
/// describe work the user opted out of rather than work they need to
/// do right now.
fn emit_human_summary(
    dry_run: bool,
    fixes: &[serde_json::Value],
    catalog_applied: usize,
    catalog_skipped: usize,
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
        eprintln!("Fixed {fixed_count} issue(s).");
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
}
