//! `fallow baselines prune`: remove the entries of the configured baselines
//! that match no current finding.
//!
//! One whole-project run feeds all three formats. The prune rules live in
//! `fallow_engine::baseline` and keep exactly the entries that `--baseline`
//! matches, so a pruned file hides the same findings and has no stale entry.
//! Prune never adds an entry, so it needs no growth gate.

#![expect(
    clippy::print_stderr,
    reason = "the human report goes to stderr like the other baseline notes"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_engine::baseline::{BaselineKind, BaselinePrune, BaselinePruneRefusal, PrunedEntry};

use crate::combined::{CombinedOptions, CombinedResults, collect_combined_results};
use crate::report;

/// The most removed keys that the human report lists for one file.
const HUMAN_REMOVED_KEY_LIMIT: usize = 10;

/// The baseline files that the prune reads, resolved against the root.
pub struct BaselineTargets {
    pub dead_code: Option<PathBuf>,
    pub health: Option<PathBuf>,
    pub dupes: Option<PathBuf>,
}

impl BaselineTargets {
    fn iter(&self) -> impl Iterator<Item = (BaselineKind, &Path)> {
        [
            (BaselineKind::DeadCode, self.dead_code.as_deref()),
            (BaselineKind::Health, self.health.as_deref()),
            (BaselineKind::Dupes, self.dupes.as_deref()),
        ]
        .into_iter()
        .filter_map(|(kind, path)| path.map(|path| (kind, path)))
    }
}

pub struct BaselinesPruneOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    pub check: bool,
    pub targets: BaselineTargets,
    pub production: crate::cli_production::ProductionModes,
    pub coverage: Option<&'a Path>,
    pub coverage_root: Option<&'a Path>,
    pub regression_opts: crate::regression::RegressionOpts<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pruned,
    WouldPrune,
    Unchanged,
    Refused,
    Error,
}

impl Status {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Pruned => "pruned",
            Self::WouldPrune => "would-prune",
            Self::Unchanged => "unchanged",
            Self::Refused => "refused",
            Self::Error => "error",
        }
    }
}

struct FileOutcome {
    kind: BaselineKind,
    path: PathBuf,
    status: Status,
    entries_before: usize,
    entries_after: usize,
    removed: Vec<PrunedEntry>,
    content: Option<String>,
    reason: Option<String>,
    reason_code: Option<&'static str>,
}

/// Why one file could not be pruned.
enum PruneFailure {
    Refused(BaselinePruneRefusal),
    MissingAnalysis(&'static str),
    Unreadable(String),
}

pub fn run_baselines_prune(opts: &BaselinesPruneOptions<'_>) -> ExitCode {
    let results = match collect_combined_results(&combined_options(opts)) {
        Ok(results) => results,
        Err(code) => return code,
    };

    let mut outcomes: Vec<FileOutcome> = opts
        .targets
        .iter()
        .map(|(kind, path)| {
            let pruned = match std::fs::read_to_string(path) {
                Ok(content) => prune_one(kind, &content, &results, opts.root),
                Err(error) => Err(PruneFailure::Unreadable(error.to_string())),
            };
            outcome_for(kind, path.to_path_buf(), pruned, opts.check)
        })
        .collect();
    if !opts.check {
        for outcome in &mut outcomes {
            write_outcome(outcome, opts.root);
        }
    }

    let exit = exit_code(&outcomes, opts.check);
    if matches!(opts.output, OutputFormat::Json) {
        let code = report::emit_report_json(
            &prune_json(&outcomes, opts.check, opts.root),
            "baselines prune",
            opts.json_style,
        );
        return if exit == ExitCode::SUCCESS {
            code
        } else {
            exit
        };
    }
    print_human(&outcomes, opts.check, opts.root, opts.quiet);
    exit
}

/// A quiet whole-project run of the analyses that the targets need. The
/// production modes come from config, and `workspaces.changedSince` stays
/// off, so the run sees every finding that a saved baseline can hold.
fn combined_options<'a>(opts: &'a BaselinesPruneOptions<'a>) -> CombinedOptions<'a> {
    CombinedOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        json_style: opts.json_style,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: true,
        allow_remote_extends: opts.allow_remote_extends,
        fail_on_issues: false,
        sarif_file: None,
        changed_since: None,
        no_package_baselines: true,
        churn_file: None,
        baseline: None,
        save_baseline: None,
        dupes_baseline: None,
        health_baseline: None,
        health_baseline_mode: fallow_engine::baseline::HealthBaselineMode::Count,
        health_baseline_mode_explicit: false,
        fail_on_stale_baseline: false,
        production: false,
        production_dead_code: Some(opts.production.dead_code),
        production_health: Some(opts.production.health),
        production_dupes: Some(opts.production.dupes),
        workspace: None,
        changed_workspaces: None,
        group_by: None,
        type_aware: None,
        type_aware_projects: &[],
        type_aware_require: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        summary: false,
        run_check: opts.targets.dead_code.is_some(),
        run_dupes: opts.targets.dupes.is_some(),
        run_health: opts.targets.health.is_some(),
        architecture: crate::check::ArchitectureSelection::All,
        dupes: crate::dupes::DupesOverrides::default(),
        score: false,
        trend: false,
        trend_from: None,
        save_snapshot: None,
        coverage: opts.coverage,
        coverage_root: opts.coverage_root,
        include_entry_exports: false,
        fail_on_parse_error: false,
        scope: None,
        regression_opts: crate::regression::RegressionOpts {
            fail_on_regression: false,
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: true,
            quiet: true,
            output: opts.output,
            ..opts.regression_opts
        },
    }
}

fn prune_one(
    kind: BaselineKind,
    content: &str,
    results: &CombinedResults,
    root: &Path,
) -> Result<BaselinePrune, PruneFailure> {
    let pruned = match kind {
        BaselineKind::DeadCode => {
            let Some(check) = results.check.as_ref() else {
                return Err(PruneFailure::MissingAnalysis(
                    "the dead-code analysis did not run",
                ));
            };
            let identity = check
                .type_aware_meta
                .as_ref()
                .and_then(|meta| meta.identity.clone())
                .unwrap_or_default();
            fallow_engine::baseline::prune_dead_code_baseline(
                content,
                &check.results,
                root,
                &identity,
            )
        }
        BaselineKind::Dupes => {
            let Some(dupes) = results.dupes.as_ref() else {
                return Err(PruneFailure::MissingAnalysis(
                    "the duplication analysis did not run",
                ));
            };
            fallow_engine::baseline::prune_dupes_baseline(content, &dupes.report)
        }
        BaselineKind::Health => {
            let Some(health) = results.health.as_ref() else {
                return Err(PruneFailure::MissingAnalysis(
                    "the health analysis did not run",
                ));
            };
            let findings: Vec<_> = health
                .report
                .findings
                .iter()
                .map(|finding| finding.violation.clone())
                .collect();
            fallow_engine::baseline::prune_health_baseline(content, &findings, root)
        }
    };
    pruned.map_err(PruneFailure::Refused)
}

fn outcome_for(
    kind: BaselineKind,
    path: PathBuf,
    pruned: Result<BaselinePrune, PruneFailure>,
    check: bool,
) -> FileOutcome {
    match pruned {
        Ok(pruned) => {
            let status = if pruned.removed.is_empty() {
                Status::Unchanged
            } else if check {
                Status::WouldPrune
            } else {
                Status::Pruned
            };
            FileOutcome {
                kind,
                path,
                status,
                entries_before: pruned.entries_before,
                entries_after: pruned.entries_after,
                removed: pruned.removed,
                content: pruned.content,
                reason: None,
                reason_code: None,
            }
        }
        Err(failure) => {
            let (status, reason, reason_code) = match failure {
                PruneFailure::Refused(refusal) => {
                    (Status::Refused, refusal.reason(), Some(refusal.code()))
                }
                PruneFailure::MissingAnalysis(reason) => (Status::Error, reason.to_owned(), None),
                PruneFailure::Unreadable(error) => (
                    Status::Error,
                    format!("cannot read the file ({error})"),
                    None,
                ),
            };
            FileOutcome {
                kind,
                path,
                status,
                entries_before: 0,
                entries_after: 0,
                removed: Vec::new(),
                content: None,
                reason: Some(reason),
                reason_code,
            }
        }
    }
}

/// Write a pruned file through a temporary file in the same directory and a
/// rename, so a reader never sees a half-written baseline.
fn write_outcome(outcome: &mut FileOutcome, root: &Path) {
    if outcome.status != Status::Pruned {
        return;
    }
    let Some(content) = outcome.content.take() else {
        return;
    };
    if let Err(error) = write_atomically(&outcome.path, content.as_bytes()) {
        outcome.status = Status::Error;
        outcome.reason = Some(format!(
            "failed to write {}: {error}",
            display_path(&outcome.path, root)
        ));
    }
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    // A rename would replace the link itself with a regular file. A save
    // refuses a symbolic link too.
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("the path is a symbolic link".to_owned());
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| "the path has no file name".to_owned())?
        .to_string_lossy();
    let temp = path.with_file_name(format!(
        ".{file_name}.fallow-prune-{}.tmp",
        std::process::id()
    ));
    fallow_engine::write_guard::write_file(
        &temp,
        contents,
        fallow_engine::write_guard::WriteTarget::Path,
    )
    .and_then(|()| {
        std::fs::rename(&temp, path).map_err(fallow_engine::write_guard::WriteFailure::File)
    })
    .map_err(|error| {
        let _ = std::fs::remove_file(&temp);
        error.to_string()
    })
}

fn exit_code(outcomes: &[FileOutcome], check: bool) -> ExitCode {
    if outcomes
        .iter()
        .any(|outcome| matches!(outcome.status, Status::Refused | Status::Error))
    {
        return ExitCode::from(2);
    }
    if check
        && outcomes
            .iter()
            .any(|outcome| outcome.status == Status::WouldPrune)
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn save_command(outcome: &FileOutcome, root: &Path) -> String {
    format!(
        "fallow {} --save-baseline {}",
        outcome.kind.as_str(),
        shell_quote(&display_path(&outcome.path, root))
    )
}

/// Quote a word for a POSIX shell. The paths come from the project config, and
/// an agent can run an action `command` in a shell, so a path must never add
/// a command of its own.
fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '@' | '+'));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn prune_json(outcomes: &[FileOutcome], check: bool, root: &Path) -> serde_json::Value {
    let files: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|outcome| {
            let removed: Vec<serde_json::Value> = outcome
                .removed
                .iter()
                .map(|entry| {
                    serde_json::json!({
                        "category": entry.category,
                        "key": entry.key,
                        "count": entry.count,
                    })
                })
                .collect();
            let failed = matches!(outcome.status, Status::Refused | Status::Error);
            serde_json::json!({
                "kind": outcome.kind.as_str(),
                "path": display_path(&outcome.path, root),
                "status": outcome.status.as_str(),
                "entries_before": outcome.entries_before,
                "entries_after": outcome.entries_after,
                "removed": removed,
                "reason_code": outcome.reason_code,
                "reason": outcome.reason.as_deref().filter(|_| failed),
            })
        })
        .collect();
    serde_json::json!({
        "kind": "baselines-prune",
        "schema_version": 1,
        "check": check,
        "files": files,
        "actions": prune_actions(outcomes, check, root),
    })
}

fn prune_actions(outcomes: &[FileOutcome], check: bool, root: &Path) -> Vec<serde_json::Value> {
    let mut actions = Vec::new();
    let written: Vec<String> = outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::Pruned)
        .map(|outcome| display_path(&outcome.path, root))
        .collect();
    if !written.is_empty() {
        actions.push(serde_json::json!({
            "type": "stage-baselines",
            "auto_fixable": false,
            "description": "Stage the pruned baseline files and commit them with the change that fixed the findings",
            "paths": written,
            "command": format!(
                "git add -- {}",
                written
                    .iter()
                    .map(|path| shell_quote(path))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        }));
    }
    if check
        && outcomes
            .iter()
            .any(|outcome| outcome.status == Status::WouldPrune)
    {
        actions.push(serde_json::json!({
            "type": "prune-baselines",
            "auto_fixable": true,
            "description": "Remove the baseline entries that match no current finding",
            "command": "fallow baselines prune",
        }));
    }
    for outcome in outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::Refused)
    {
        actions.push(serde_json::json!({
            "type": "resave-baseline",
            "auto_fixable": false,
            "description": "Save this baseline again with the current key form, after a review of its findings",
            "path": display_path(&outcome.path, root),
            "command": save_command(outcome, root),
        }));
    }
    actions
}

/// Print the report. `--quiet` keeps only the skipped and failed files, so a
/// run that exits 2 always says why.
fn print_human(outcomes: &[FileOutcome], check: bool, root: &Path, quiet: bool) {
    for outcome in outcomes {
        if quiet && !matches!(outcome.status, Status::Refused | Status::Error) {
            continue;
        }
        let path = display_path(&outcome.path, root);
        let kind = outcome.kind.as_str();
        match outcome.status {
            Status::Unchanged => eprintln!(
                "{kind} baseline {path}: nothing to prune ({} {})",
                outcome.entries_before,
                entry_noun(outcome.entries_before)
            ),
            Status::Pruned | Status::WouldPrune => {
                let removed: usize = outcome.removed.iter().map(|entry| entry.count).sum();
                let verb = if outcome.status == Status::Pruned {
                    "Pruned"
                } else {
                    "Would prune"
                };
                eprintln!(
                    "{verb} {removed} {} from the {kind} baseline {path} ({} -> {})",
                    entry_noun(removed),
                    outcome.entries_before,
                    outcome.entries_after
                );
                print_removed(&outcome.removed);
            }
            Status::Refused => eprintln!(
                "Skipped the {kind} baseline {path}: {}. Save it again with: {}",
                outcome.reason.as_deref().unwrap_or_default(),
                save_command(outcome, root)
            ),
            Status::Error => eprintln!(
                "Error: {kind} baseline {path}: {}. To save it: {}",
                outcome.reason.as_deref().unwrap_or_default(),
                save_command(outcome, root)
            ),
        }
    }
    if check
        && !quiet
        && outcomes
            .iter()
            .any(|outcome| outcome.status == Status::WouldPrune)
    {
        eprintln!("Run `fallow baselines prune` to remove these entries.");
    }
}

fn print_removed(removed: &[PrunedEntry]) {
    for entry in removed.iter().take(HUMAN_REMOVED_KEY_LIMIT) {
        let count = if entry.count > 1 {
            format!(" (x{})", entry.count)
        } else {
            String::new()
        };
        eprintln!(
            "  {}: {}{count}",
            entry.category,
            entry.key.replace('\0', "#")
        );
    }
    if removed.len() > HUMAN_REMOVED_KEY_LIMIT {
        eprintln!(
            "  ... and {} more (use --format json for the full list)",
            removed.len() - HUMAN_REMOVED_KEY_LIMIT
        );
    }
}

const fn entry_noun(count: usize) -> &'static str {
    if count == 1 { "entry" } else { "entries" }
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn shell_quote_keeps_plain_paths_and_quotes_the_rest() {
        assert_eq!(
            shell_quote("baselines/dead-code.json"),
            "baselines/dead-code.json"
        );
        assert_eq!(shell_quote("a b.json"), "'a b.json'");
        assert_eq!(shell_quote("x;rm -rf ~.json"), "'x;rm -rf ~.json'");
        assert_eq!(shell_quote("$(id).json"), "'$(id).json'");
        assert_eq!(shell_quote("it's.json"), "'it'\\''s.json'");
        assert_eq!(shell_quote(""), "''");
    }
}
