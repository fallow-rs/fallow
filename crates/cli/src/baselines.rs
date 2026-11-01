//! `fallow baselines prune`: remove the entries of the configured baselines
//! that match no current finding.
//!
//! One whole-project run feeds all three formats. The prune rules live in
//! `fallow_engine::baseline` and keep exactly the entries that `--baseline`
//! matches, so a pruned file hides the same findings and has no stale entry.
//! Prune never adds an entry, so it needs no growth gate. A file saved in another
//! mode than the run is refused, because the run cannot see its findings.

#![expect(
    clippy::print_stderr,
    reason = "the human report goes to stderr like the other baseline notes"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_engine::baseline::{BaselineKind, BaselinePrune, BaselinePruneRefusal, PrunedEntry};
use fallow_output::BaselineMode;

use crate::combined::{CombinedOptions, CombinedResults, collect_combined_results};
use fallow_engine::shell_quote::shell_quote;

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

/// The files to prune: each flag wins over the config key of that kind.
pub fn prune_targets(
    root: &Path,
    audit: &fallow_config::AuditConfig,
    [dead_code, health, dupes]: [&Option<PathBuf>; 3],
) -> BaselineTargets {
    BaselineTargets {
        dead_code: crate::resolve_audit_baseline_path(
            root,
            dead_code.as_deref(),
            audit.dead_code_baseline.as_deref(),
        ),
        health: crate::resolve_audit_baseline_path(
            root,
            health.as_deref(),
            audit.health_baseline.as_deref(),
        ),
        dupes: crate::resolve_audit_baseline_path(
            root,
            dupes.as_deref(),
            audit.dupes_baseline.as_deref(),
        ),
    }
}

/// The first flag or environment variable that would narrow the prune run.
pub fn narrowing_flag(cli: &crate::cli_args::Cli) -> Option<&'static str> {
    [
        (cli.changed_since.is_some(), "--changed-since"),
        (cli.workspace.is_some(), "--workspace"),
        (cli.changed_workspaces.is_some(), "--changed-workspaces"),
        (cli.diff_file.is_some(), "--diff-file"),
        (cli.diff_stdin, "--diff-stdin"),
        (
            std::env::var_os("FALLOW_DIFF_FILE").is_some_and(|value| !value.is_empty()),
            "FALLOW_DIFF_FILE",
        ),
    ]
    .into_iter()
    .find_map(|(present, flag)| present.then_some(flag))
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
    /// The mode flags this command received, so the run uses them and a printed
    /// prune command repeats them. A mode from the config needs no flag.
    pub mode_flags: BaselineMode,
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
    /// The mismatch of a `scope-mismatch` refusal, which has its own remedy.
    mismatch: Option<ScopeMismatch>,
}

/// How the mode of a refused file differs from the run.
#[derive(Clone, Copy)]
struct ScopeMismatch {
    saved_only: BaselineMode,
    run_only: BaselineMode,
    run: BaselineMode,
}

impl ScopeMismatch {
    /// The mode of the file, which a prune must use to judge it.
    const fn saved(self) -> BaselineMode {
        self.run.minus(self.run_only).union(self.saved_only)
    }

    /// The mode the file must record before this run can prune it.
    const fn resave(self) -> BaselineMode {
        self.run.union(self.saved_only)
    }
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
                Ok(content) => prune_one(kind, &content, &results, opts),
                Err(error) => Err(PruneFailure::Unreadable(error.to_string())),
            };
            outcome_for(
                kind,
                path.to_path_buf(),
                pruned,
                opts.check,
                run_mode(kind, &results, opts),
            )
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
            &prune_json(&outcomes, opts.check, opts.root, opts.mode_flags),
            "baselines prune",
            opts.json_style,
        );
        return if exit == ExitCode::SUCCESS {
            code
        } else {
            exit
        };
    }
    print_human(&outcomes, opts);
    exit
}

/// A quiet whole-project run of the analyses that the targets need. The
/// production modes come from the flag or the config, and
/// `workspaces.changedSince` stays off, so the run sees every finding that a
/// baseline saved in this mode can hold.
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
        include_entry_exports: opts.mode_flags.include_entry_exports,
        show_cascade: false,
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
    opts: &BaselinesPruneOptions<'_>,
) -> Result<BaselinePrune, PruneFailure> {
    let root = opts.root;
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
                BaselineMode {
                    production: check.config.production,
                    include_entry_exports: check.config.include_entry_exports,
                },
            )
        }
        BaselineKind::Dupes => {
            let Some(dupes) = results.dupes.as_ref() else {
                return Err(PruneFailure::MissingAnalysis(
                    "the duplication analysis did not run",
                ));
            };
            fallow_engine::baseline::prune_dupes_baseline(
                content,
                &dupes.report,
                opts.production.dupes,
            )
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
            fallow_engine::baseline::prune_health_baseline(
                content,
                &findings,
                root,
                opts.production.health,
            )
        }
    };
    pruned.map_err(PruneFailure::Refused)
}

/// The mode this run analyzed one baseline kind in.
fn run_mode(
    kind: BaselineKind,
    results: &CombinedResults,
    opts: &BaselinesPruneOptions<'_>,
) -> BaselineMode {
    match kind {
        BaselineKind::DeadCode => results.check.as_ref().map_or_else(
            || BaselineMode {
                production: opts.production.dead_code,
                include_entry_exports: opts.mode_flags.include_entry_exports,
            },
            |check| BaselineMode {
                production: check.config.production,
                include_entry_exports: check.config.include_entry_exports,
            },
        ),
        BaselineKind::Health => BaselineMode {
            production: opts.production.health,
            include_entry_exports: false,
        },
        BaselineKind::Dupes => BaselineMode {
            production: opts.production.dupes,
            include_entry_exports: false,
        },
    }
}

fn outcome_for(
    kind: BaselineKind,
    path: PathBuf,
    pruned: Result<BaselinePrune, PruneFailure>,
    check: bool,
    run: BaselineMode,
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
                mismatch: None,
            }
        }
        Err(failure) => {
            let mismatch = match &failure {
                PruneFailure::Refused(BaselinePruneRefusal::ScopeMismatch {
                    saved_only,
                    run_only,
                }) => Some(ScopeMismatch {
                    saved_only: *saved_only,
                    run_only: *run_only,
                    run,
                }),
                _ => None,
            };
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
                mismatch,
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

/// The command that saves this file again. It keeps the mode of the run, so
/// the new file records the mode that this prune compared in.
fn save_command(outcome: &FileOutcome, root: &Path, mode: BaselineMode) -> String {
    format!(
        "fallow {}{} --save-baseline {}",
        outcome.kind.as_str(),
        mode.flags(),
        shell_quote(&display_path(&outcome.path, root))
    )
}

/// The command that prunes one refused file in the mode that the file records.
fn prune_in_saved_mode(outcome: &FileOutcome, root: &Path, saved: BaselineMode) -> String {
    format!(
        "fallow baselines prune{} --{}-baseline {}",
        saved.flags(),
        outcome.kind.as_str(),
        shell_quote(&display_path(&outcome.path, root))
    )
}

/// The remedy of a `scope-mismatch` refusal. The run can prune the file when it
/// adds the mode that the file records. A file without the record of a mode that
/// the run uses must first be saved again, and a person must review that save,
/// because it can record findings that the file did not hold.
fn mismatch_remedy(outcome: &FileOutcome, root: &Path, mismatch: ScopeMismatch) -> MismatchRemedy {
    if mismatch.run_only.is_empty() {
        MismatchRemedy::Prune(prune_in_saved_mode(outcome, root, mismatch.saved()))
    } else {
        MismatchRemedy::Save(save_command(outcome, root, mismatch.resave()))
    }
}

enum MismatchRemedy {
    Prune(String),
    Save(String),
}

fn resave_description(mode: BaselineMode) -> String {
    match (mode.production, mode.include_entry_exports) {
        (true, false) => {
            "Save this baseline again in production mode, after a review of its findings".to_owned()
        }
        _ => format!(
            "Save this baseline again with{}, after a review of its findings",
            mode.flags()
        ),
    }
}

fn prune_json(
    outcomes: &[FileOutcome],
    check: bool,
    root: &Path,
    mode_flags: BaselineMode,
) -> serde_json::Value {
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
        "actions": prune_actions(outcomes, check, root, mode_flags),
    })
}

fn prune_actions(
    outcomes: &[FileOutcome],
    check: bool,
    root: &Path,
    mode_flags: BaselineMode,
) -> Vec<serde_json::Value> {
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
            "command": format!("fallow baselines prune{}", mode_flags.flags()),
        }));
    }
    for outcome in outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::Refused)
    {
        if let Some(mismatch) = outcome.mismatch {
            let path = display_path(&outcome.path, root);
            actions.push(match mismatch_remedy(outcome, root, mismatch) {
                MismatchRemedy::Prune(command) => serde_json::json!({
                    "type": "prune-baselines",
                    "auto_fixable": false,
                    "description": "Prune this baseline in the mode that it was saved in",
                    "path": path,
                    "command": command,
                }),
                MismatchRemedy::Save(command) => serde_json::json!({
                    "type": "resave-baseline",
                    "auto_fixable": false,
                    "description": resave_description(mismatch.resave()),
                    "path": path,
                    "command": command,
                }),
            });
            continue;
        }
        actions.push(serde_json::json!({
            "type": "resave-baseline",
            "auto_fixable": false,
            "description": "Save this baseline again with the current key form, after a review of its findings",
            "path": display_path(&outcome.path, root),
            "command": save_command(outcome, root, mode_flags),
        }));
    }
    actions
}

/// Print the report. `--quiet` keeps only the skipped and failed files, so a
/// run that exits 2 always says why.
fn print_human(outcomes: &[FileOutcome], opts: &BaselinesPruneOptions<'_>) {
    let (check, root, quiet) = (opts.check, opts.root, opts.quiet);
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
            Status::Refused => {
                let remedy = match outcome.mismatch {
                    Some(mismatch) => match mismatch_remedy(outcome, root, mismatch) {
                        MismatchRemedy::Prune(command) => format!("Run: {command}"),
                        MismatchRemedy::Save(command) => {
                            format!("Review the findings, then save it again with: {command}")
                        }
                    },
                    None => format!(
                        "Save it again with: {}",
                        save_command(outcome, root, opts.mode_flags)
                    ),
                };
                eprintln!(
                    "Skipped the {kind} baseline {path}: {}. {remedy}",
                    outcome.reason.as_deref().unwrap_or_default(),
                );
            }
            Status::Error => eprintln!(
                "Error: {kind} baseline {path}: {}. To save it: {}",
                outcome.reason.as_deref().unwrap_or_default(),
                save_command(outcome, root, opts.mode_flags)
            ),
        }
    }
    if check
        && !quiet
        && outcomes
            .iter()
            .any(|outcome| outcome.status == Status::WouldPrune)
    {
        eprintln!(
            "Run `fallow baselines prune{}` to remove these entries.",
            opts.mode_flags.flags()
        );
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
