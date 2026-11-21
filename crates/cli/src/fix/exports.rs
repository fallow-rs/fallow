use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;

use super::enum_helpers::{EnumDeclarationRange, enum_statement_range};
use super::export_list::{ExportListIndex, ExportListStatement, ListRewrite, ListRewriteOutcome};
use super::list_imports::{ImportRemoval, plan_import_removals};
use fallow_engine::module_graph::NamedReExport;
use fallow_engine::source_edits::{ExportStatement, ExportStatementKind, SourceEditIndex};
use fallow_types::output_dead_code::ReachabilityCaveat;

use super::plan::{CapturedHashes, FixPlan, SkipReason, skip_generated_source};
use super::source_edits::{
    QueueOutcome, SourceEdit, line_deletions, line_edit, read_source_snapshot,
};

/// Directory names whose contents are commonly consumed through paths
/// fallow's static graph cannot see: Vitest / Jest `__mocks__` aliases,
/// Playwright / Cypress e2e suites, published-package `examples`, and
/// fixture / golden harnesses wired up by a build step. Removing an
/// `export` from a file under any of these is low confidence; `fallow fix`
/// withholds the rewrite (see [`SkipReason::LowConfidenceOffGraph`]).
///
/// Matched against every directory component of the file's
/// project-root-relative path.
///
/// Deliberately EXCLUDED (documented so the set does not silently grow):
/// `test` / `tests` / `__tests__` (genuinely-dead test helpers are common
/// and SHOULD auto-remove), bare `mocks` (too generic), `stories` /
/// `.storybook` (frequently a declared entry point, so skipping would
/// regress). Issue #602.
const OFF_GRAPH_CONSUMER_DIRS: &[&str] = &[
    "__mocks__",
    "__fixtures__",
    "fixtures",
    "e2e",
    "e2e-tests",
    "cypress",
    "playwright",
    "examples",
    "evals",
    "golden",
];

/// True when any directory component of `relative` is an off-graph
/// consumer surface (see [`OFF_GRAPH_CONSUMER_DIRS`]). The final component
/// (the file name) is matched too, which is harmless: source files are not
/// named exactly `e2e` / `golden` / etc. in practice, and a directory by
/// that name anywhere in the path is the signal we want.
fn is_off_graph_consumer_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|segment| OFF_GRAPH_CONSUMER_DIRS.contains(&segment))
    })
}

/// Decide whether export removals in `path` should be withheld as low
/// confidence, and why. Off-graph directory membership is checked first
/// (more specific, names the surface for the user); a file that itself has
/// an unresolved import is the second-tier signal (its local usage graph is
/// incomplete); a finding the analysis already marked with a reachability
/// caveat is the third, and the least deniable, since the run itself recorded
/// that the verdict rests on an import graph it knows is incomplete. Returns
/// `None` when the file is high confidence and the fixer should proceed
/// normally. Issue #602.
fn low_confidence_skip_reason(
    relative: &Path,
    absolute: &Path,
    unresolved_import_files: &FxHashSet<PathBuf>,
    caveats: &[ReachabilityCaveat],
) -> Option<SkipReason> {
    if is_off_graph_consumer_path(relative) {
        return Some(SkipReason::LowConfidenceOffGraph);
    }
    if unresolved_import_files.contains(absolute) {
        return Some(SkipReason::LowConfidenceUnresolvedImports);
    }
    if !caveats.is_empty() {
        return Some(SkipReason::LowConfidenceIncompleteAnalysis);
    }
    None
}

struct ExportFix {
    line_idx: usize,
    export_name: String,
    edit: ExportEdit,
}

#[derive(Clone, Copy)]
enum ExportEdit {
    /// Remove the export of the statement whose `export` keyword is at
    /// byte `start` of the file, on the 0-based line `line`.
    Keyword { start: u32, line: usize },
    /// Delete the exported enum declaration.
    EnumDeclaration(EnumDeclarationRange),
    /// Remove the specifier from the list statement. The statement can span
    /// several lines.
    List(ExportListStatement),
}

impl ExportFix {
    /// The line that holds the edit. All names of one list statement share
    /// the first line of that statement, so one rewrite handles them.
    const fn edit_line(&self) -> usize {
        match self.edit {
            ExportEdit::List(statement) => statement.start_line,
            ExportEdit::Keyword { line, .. } => line,
            ExportEdit::EnumDeclaration(_) => self.line_idx,
        }
    }
}

pub(super) struct ExportFixInput<'a, 'export> {
    pub(super) root: &'a Path,
    pub(super) exports_by_file:
        &'a FxHashMap<PathBuf, Vec<&'export fallow_types::results::UnusedExport>>,
    pub(super) hashes: &'a CapturedHashes,
    pub(super) unresolved_import_files: &'a FxHashSet<PathBuf>,
    /// Degraded-parse caveats per file, from the findings the analysis
    /// annotated. A file present here has its export removals withheld.
    pub(super) caveats_by_file: &'a FxHashMap<PathBuf, Vec<ReachabilityCaveat>>,
    /// Every named re-export of the project. A removal of an export stays
    /// withheld while a re-export that this run keeps still names it.
    pub(super) re_exports: &'a [NamedReExport],
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

fn emit_dry_run_export_fix(relative: &Path, fix: &ExportFix) {
    let relative_display = relative.to_string_lossy().replace('\\', "/");
    if matches!(fix.edit, ExportEdit::EnumDeclaration(_)) {
        eprintln!(
            "Would remove enum declaration from {relative_display}:{} `{}`",
            fix.line_idx + 1,
            fix.export_name,
        );
    } else {
        eprintln!(
            "Would remove export from {relative_display}:{} `{}`",
            fix.line_idx + 1,
            fix.export_name,
        );
    }
}

fn push_export_fix_json(
    fixes: &mut Vec<serde_json::Value>,
    relative: &Path,
    absolute: &Path,
    fix: &ExportFix,
    applied: Option<bool>,
) {
    let mut value = serde_json::json!({
        "type": "remove_export",
        "path": relative.to_string_lossy().replace('\\', "/"),
        "line": fix.line_idx + 1,
        "name": fix.export_name,
    });
    if let Some(applied) = applied {
        value["applied"] = serde_json::json!(applied);
        value["__target"] = serde_json::json!(absolute.display().to_string());
    }
    fixes.push(value);
}

/// The planned export removals of one file, with the source they edit.
struct FileExportPlan<'a> {
    path: &'a Path,
    relative: &'a Path,
    content: String,
    meta: super::io::EncodingMetadata,
    /// The parsed export list statements of the file.
    lists: ExportListIndex,
    /// The export statements of the file, or `None` when the file does not
    /// parse.
    index: Option<SourceEditIndex>,
    fixes: Vec<ExportFix>,
    /// Removals withheld, with the reason of each.
    withheld: Vec<(ExportFix, SkipReason)>,
}

/// Apply export fixes to source files, returning JSON fix entries.
///
/// Stages every per-file rewrite on `plan` instead of writing directly;
/// the orchestrator commits the plan after all fixers run, so a single
/// stage failure in any fixer leaves the project untouched. Hash mismatch
/// against `hashes` (captured during the in-process analysis read) marks
/// the file as skipped instead of overwriting bytes the analysis never saw.
///
/// `unresolved_import_files` is the set of absolute paths that have at
/// least one unresolved import. A file in that set, under an off-graph
/// consumer directory, or carrying a reachability caveat in
/// `caveats_by_file`, has its export removals withheld as low confidence
/// (issue #602): the rewrite would risk breaking a consumer fallow's graph
/// cannot see. The skip is recorded on `plan` so the orchestrator surfaces
/// it; the export stays reported by `fallow dead-code`.
///
/// A removal is also withheld while a re-export that the run keeps still
/// names the export (issue #4636). Without this rule the kept re-export
/// points at a declaration that is no longer exported, and the type check
/// of the fixed code fails. For the same reason, the `export` keyword of a
/// declaration that exports more than one name stays while the run keeps
/// one of these names (issue #4659).
pub(super) fn apply_export_fixes(input: &mut ExportFixInput<'_, '_>) {
    let mut file_plans = plan_export_fixes(input);
    withhold_removals_of_kept_exports(&mut file_plans, input.re_exports);

    for mut file_plan in file_plans {
        file_plan
            .withheld
            .sort_by_key(|(f, _)| std::cmp::Reverse(f.line_idx));
        for (fix, reason) in &file_plan.withheld {
            push_withheld_export_entry(input, file_plan.relative, fix, *reason);
        }
        let rewrites = rewrite_list_statements(&file_plan.lists, &mut file_plan.fixes);
        if file_plan.fixes.is_empty() {
            continue;
        }
        let import_removals = plan_import_removals(
            file_plan.path,
            &file_plan.content,
            file_plan.meta.line_ending,
            &removed_list_references(&rewrites),
        );
        file_plan.fixes.sort_by_key(|f| {
            (
                std::cmp::Reverse(f.edit_line()),
                std::cmp::Reverse(f.line_idx),
            )
        });

        let edits = export_source_edits(
            &file_plan.content,
            file_plan.meta.line_ending,
            file_plan.index.as_ref(),
            &file_plan.fixes,
            &rewrites,
            &import_removals,
        );
        if input.plan.sources_mut().queue(file_plan.path, edits) != QueueOutcome::Queued {
            continue;
        }
        if input.dry_run {
            push_dry_run_export_fixes(
                input.output,
                input.fixes,
                file_plan.relative,
                file_plan.path,
                &file_plan.fixes,
            );
            if !matches!(input.output, OutputFormat::Json) {
                emit_dry_run_import_removals(file_plan.relative, &import_removals);
            }
        } else {
            for fix in &file_plan.fixes {
                push_export_fix_json(
                    input.fixes,
                    file_plan.relative,
                    file_plan.path,
                    fix,
                    Some(true),
                );
            }
        }
    }
}

/// Read each file with findings and collect the edits that its text allows.
fn plan_export_fixes<'a>(input: &mut ExportFixInput<'a, '_>) -> Vec<FileExportPlan<'a>> {
    let root = input.root;
    let mut file_plans = Vec::new();
    for (path, file_exports) in super::in_path_order(input.exports_by_file) {
        let relative = path.strip_prefix(root).unwrap_or(path);

        let caveats = input
            .caveats_by_file
            .get(path)
            .map_or(&[][..], |caveats| caveats.as_slice());
        if let Some(reason) =
            low_confidence_skip_reason(relative, path, input.unresolved_import_files, caveats)
        {
            input
                .plan
                .skip_with_caveats(path.clone(), reason, caveats.to_vec());
            continue;
        }

        let Some((content, meta)) = read_source_snapshot(root, path, input.hashes, input.plan)
        else {
            continue;
        };
        if skip_generated_source(root, path, &content, input.plan) {
            continue;
        }
        let lines: Vec<&str> = content.split(meta.line_ending).collect();
        let mut lists = ExportListIndex::default();
        let index = SourceEditIndex::parse(path, &content);
        let mut fixes = collect_export_line_fixes(&lines, file_exports, &mut lists, index.as_ref());
        drop(lines);
        rewrite_list_statements(&lists, &mut fixes);
        if fixes.is_empty() {
            continue;
        }
        file_plans.push(FileExportPlan {
            path,
            relative,
            content,
            meta,
            lists,
            index,
            fixes,
            withheld: Vec::new(),
        });
    }
    file_plans
}

/// Withhold each planned removal whose export the run must keep.
///
/// A removal is withheld when a kept re-export still names the export, or
/// when the `export` keyword of its declaration also exports a name that the
/// run keeps. A re-export is kept when the run does not remove it. Each
/// withheld removal keeps more exports, so the loop runs until no removal
/// changes.
///
/// The check sees only the re-exports in the module graph. A file that
/// `ignorePatterns` excludes from the analysis is not in the graph, so a
/// re-export in such a file cannot hold a removal back.
fn withhold_removals_of_kept_exports(
    file_plans: &mut [FileExportPlan<'_>],
    re_exports: &[NamedReExport],
) {
    loop {
        let planned: FxHashSet<(&Path, &str)> = file_plans
            .iter()
            .flat_map(|file_plan| {
                file_plan
                    .fixes
                    .iter()
                    .map(|fix| (file_plan.path, fix.export_name.as_str()))
            })
            .collect();
        let named_by_kept: FxHashSet<(&Path, &str)> = re_exports
            .iter()
            .filter(|re_export| {
                !planned.contains(&(re_export.path.as_path(), re_export.exported_name.as_str()))
            })
            .map(|re_export| {
                (
                    re_export.source_path.as_path(),
                    re_export.imported_name.as_str(),
                )
            })
            .collect();

        let mut changed = false;
        for file_plan in file_plans.iter_mut() {
            let path = file_plan.path;
            let (withheld, kept): (Vec<ExportFix>, Vec<ExportFix>) =
                std::mem::take(&mut file_plan.fixes)
                    .into_iter()
                    .partition(|fix| named_by_kept.contains(&(path, fix.export_name.as_str())));
            changed |= !withheld.is_empty();
            file_plan.fixes = kept;
            file_plan.withheld.extend(
                withheld
                    .into_iter()
                    .map(|fix| (fix, SkipReason::KeptReExport)),
            );
            changed |= withhold_partial_shared_declarations(file_plan);
        }
        if !changed {
            return;
        }
    }
}

/// Withhold each removal of a shared declaration that the file plan does not
/// remove for every name of the declaration. A statement is shared when its
/// `export` keyword exports more than one name. Returns true when it
/// withholds a removal.
fn withhold_partial_shared_declarations(file_plan: &mut FileExportPlan<'_>) -> bool {
    let Some(index) = file_plan.index.as_ref() else {
        return false;
    };
    let fixes = std::mem::take(&mut file_plan.fixes);
    let planned: FxHashSet<&str> = fixes.iter().map(|fix| fix.export_name.as_str()).collect();
    let partial: FxHashSet<u32> = fixes
        .iter()
        .filter_map(|fix| match fix.edit {
            ExportEdit::Keyword { start, .. } => Some(start),
            ExportEdit::EnumDeclaration(_) | ExportEdit::List(_) => None,
        })
        .filter(|start| {
            index.statement_at(*start).is_some_and(|statement| {
                statement.names.len() > 1
                    && !statement
                        .names
                        .iter()
                        .all(|name| planned.contains(name.as_str()))
            })
        })
        .collect();
    drop(planned);
    let (withheld, kept): (Vec<ExportFix>, Vec<ExportFix>) = fixes.into_iter().partition(
        |fix| matches!(fix.edit, ExportEdit::Keyword { start, .. } if partial.contains(&start)),
    );
    file_plan.fixes = kept;
    let changed = !withheld.is_empty();
    file_plan.withheld.extend(
        withheld
            .into_iter()
            .map(|fix| (fix, SkipReason::SharedDeclarationInUse)),
    );
    changed
}

/// Rewrite each list statement for the names that its fixes remove, and
/// keep only the list fixes that the rewrite applies.
///
/// The rewrite matches the exported name of each specifier. A fix whose name
/// the statement does not export, or a statement that has no safe text edit,
/// is not planned, so the report never claims an edit that does not happen.
/// Returns the rewrite of each statement, keyed by its first line.
fn rewrite_list_statements(
    lists: &ExportListIndex,
    fixes: &mut Vec<ExportFix>,
) -> FxHashMap<usize, ListRewriteOutcome> {
    let mut names_by_statement: FxHashMap<usize, (ExportListStatement, Vec<&str>)> =
        FxHashMap::default();
    for fix in fixes.iter() {
        if let ExportEdit::List(statement) = fix.edit {
            names_by_statement
                .entry(statement.start_line)
                .or_insert_with(|| (statement, Vec::new()))
                .1
                .push(fix.export_name.as_str());
        }
    }
    let rewrites: FxHashMap<usize, ListRewriteOutcome> = names_by_statement
        .into_iter()
        .filter_map(|(start, (statement, names))| {
            lists
                .rewrite(statement, &names)
                .map(|outcome| (start, outcome))
        })
        .collect();
    fixes.retain(|fix| match fix.edit {
        ExportEdit::List(statement) => rewrites
            .get(&statement.start_line)
            .is_some_and(|outcome| outcome.removed.contains(&fix.export_name)),
        ExportEdit::Keyword { .. } | ExportEdit::EnumDeclaration(_) => true,
    });
    rewrites
}

/// Count, for each local name, the removed export list specifiers that read
/// it.
fn removed_list_references(
    rewrites: &FxHashMap<usize, ListRewriteOutcome>,
) -> FxHashMap<String, usize> {
    let mut counts: FxHashMap<String, usize> = FxHashMap::default();
    for local in rewrites
        .values()
        .flat_map(|outcome| &outcome.removed_locals)
    {
        *counts.entry(local.clone()).or_default() += 1;
    }
    counts
}

fn emit_dry_run_import_removals(relative: &Path, removals: &[ImportRemoval]) {
    let relative_display = relative.to_string_lossy().replace('\\', "/");
    for removal in removals {
        for name in &removal.names {
            eprintln!(
                "Would remove import from {relative_display}:{} `{name}`",
                removal.start_line + 1,
            );
        }
    }
}

/// Emit the entry of an export removal that the run withholds. Withholding
/// is per export, not per file rewrite, so the entry never reaches the
/// per-file skip list of the plan.
fn push_withheld_export_entry(
    input: &mut ExportFixInput<'_, '_>,
    relative: &Path,
    fix: &ExportFix,
    reason: SkipReason,
) {
    let relative_display = relative.to_string_lossy().replace('\\', "/");
    if input.dry_run && !matches!(input.output, OutputFormat::Json) {
        let why = if reason == SkipReason::SharedDeclarationInUse {
            "the same `export` keyword also exports a name that is in use"
        } else {
            "a re-export that this run keeps still names it"
        };
        eprintln!(
            "Would keep export in {relative_display}:{} `{}`: {why}",
            fix.line_idx + 1,
            fix.export_name,
        );
    }
    input.fixes.push(serde_json::json!({
        "type": "remove_export",
        "path": relative_display,
        "line": fix.line_idx + 1,
        "name": fix.export_name,
        "applied": false,
        "skipped": true,
        "skip_reason": reason.as_wire_str(),
        "help": reason.help(),
    }));
}

fn collect_export_line_fixes(
    lines: &[&str],
    file_exports: &[&fallow_types::results::UnusedExport],
    lists: &mut ExportListIndex,
    index: Option<&SourceEditIndex>,
) -> Vec<ExportFix> {
    file_exports
        .iter()
        .filter_map(|export| export_line_fix(lines, export, lists, index))
        .collect()
}

fn export_line_fix(
    lines: &[&str],
    export: &fallow_types::results::UnusedExport,
    lists: &mut ExportListIndex,
    index: Option<&SourceEditIndex>,
) -> Option<ExportFix> {
    let line_idx = export.line.saturating_sub(1) as usize;
    let line = *lines.get(line_idx)?;
    let after_export = line.trim_start().strip_prefix("export ");
    let list_fix = |lists: &mut ExportListIndex| {
        let statement = lists.statement_for_line(lines, line_idx)?;
        Some(ExportFix {
            line_idx,
            export_name: export.export_name.clone(),
            edit: ExportEdit::List(statement),
        })
    };
    if after_export.is_some_and(is_export_list) {
        return list_fix(lists);
    }
    let statement = index.and_then(|index| {
        index
            .export_statement(&export.export_name, export.line, export.col)
            .ok()
    });
    let (Some(index), Some(statement)) = (index, statement) else {
        // The specifier of a list over several lines is on its own line, so
        // the reported line does not start with `export`.
        return if after_export.is_none() {
            list_fix(lists)
        } else {
            None
        };
    };
    let edit = statement_edit(index, lines, statement)?;
    Some(ExportFix {
        line_idx,
        export_name: export.export_name.clone(),
        edit,
    })
}

/// The edit that removes the export of `statement`: the whole enum when no
/// other text needs it, or else the `export` keyword.
fn statement_edit(
    index: &SourceEditIndex,
    lines: &[&str],
    statement: &ExportStatement,
) -> Option<ExportEdit> {
    if statement.kind == ExportStatementKind::Enum
        && let Some(range) = enum_statement_range(index, lines, statement)
    {
        return Some(ExportEdit::EnumDeclaration(range));
    }
    statement.keyword_edit()?;
    Some(ExportEdit::Keyword {
        start: statement.start,
        line: index.line_range(statement.start, statement.start + 1).0,
    })
}

/// Check if a line (after stripping `export `) is a named export list like `{ A, B } ...`
fn is_export_list(after_export: &str) -> bool {
    let s = after_export.trim_start();
    let s = s.strip_prefix("type").map_or(s, str::trim_start);
    s.starts_with('{')
}

fn push_dry_run_export_fixes(
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
    relative: &Path,
    path: &Path,
    line_fixes: &[ExportFix],
) {
    for fix in line_fixes {
        if !matches!(output, OutputFormat::Json) {
            emit_dry_run_export_fix(relative, fix);
        }
        push_export_fix_json(fixes, relative, path, fix, None);
    }
}

/// The byte edits of the planned export fixes of one file, against the
/// analysed text `content`.
///
/// Fixes are grouped by their edit line. On one line, each keyword edit
/// goes, and then the first other edit of the line.
fn export_source_edits(
    content: &str,
    line_ending: &str,
    index: Option<&SourceEditIndex>,
    line_fixes: &[ExportFix],
    rewrites: &FxHashMap<usize, ListRewriteOutcome>,
    import_removals: &[ImportRemoval],
) -> Vec<SourceEdit> {
    let mut edits = Vec::new();
    let mut line_indexes = Vec::new();

    let mut edit_lines: Vec<usize> = line_fixes.iter().map(ExportFix::edit_line).collect();
    edit_lines.dedup();
    for edit_line in edit_lines {
        let on_line = || line_fixes.iter().filter(|fix| fix.edit_line() == edit_line);
        for fix in on_line() {
            if let ExportEdit::Keyword { start, .. } = fix.edit
                && let Some(edit) = index
                    .and_then(|index| index.statement_at(start))
                    .and_then(ExportStatement::keyword_edit)
            {
                edits.push(SourceEdit {
                    start: edit.start as usize,
                    end: edit.end as usize,
                    replacement: edit.replacement,
                });
            }
        }
        let Some(edit) = on_line()
            .map(|fix| fix.edit)
            .find(|edit| !matches!(edit, ExportEdit::Keyword { .. }))
        else {
            continue;
        };
        match edit {
            ExportEdit::EnumDeclaration(range) => {
                line_indexes.extend(range.start_line..=range.end_line);
            }
            ExportEdit::List(statement) => {
                match rewrites
                    .get(&statement.start_line)
                    .map(|outcome| &outcome.rewrite)
                {
                    Some(ListRewrite::Replace(rewritten)) if !rewritten.is_empty() => {
                        edits.push(line_edit(
                            content,
                            line_ending,
                            statement.start_line,
                            statement.end_line,
                            Some(rewritten),
                        ));
                    }
                    Some(_) => line_indexes.extend(statement.start_line..=statement.end_line),
                    None => {}
                }
            }
            ExportEdit::Keyword { .. } => {}
        }
    }

    for removal in import_removals {
        match &removal.replacement {
            Some(replacement) if !replacement.is_empty() => edits.push(line_edit(
                content,
                line_ending,
                removal.start_line,
                removal.end_line,
                Some(replacement),
            )),
            _ => line_indexes.extend(removal.start_line..=removal.end_line),
        }
    }

    edits.extend(line_deletions(content, line_ending, &line_indexes));
    edits
}

#[cfg(test)]
#[path = "exports_unicode_span_oracle.rs"]
mod unicode_span_oracle;

#[cfg(test)]
#[path = "exports_syntax_tests.rs"]
mod syntax_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::UnusedExport;

    #[expect(
        clippy::too_many_arguments,
        reason = "test helper preserves compact fixture setup while production uses ExportFixInput"
    )]
    fn apply_export_fixes(
        root: &Path,
        exports_by_file: &FxHashMap<PathBuf, Vec<&UnusedExport>>,
        hashes: &CapturedHashes,
        unresolved_import_files: &FxHashSet<PathBuf>,
        plan: &mut FixPlan,
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) {
        super::apply_export_fixes(&mut ExportFixInput {
            root,
            exports_by_file,
            hashes,
            unresolved_import_files,
            caveats_by_file: &FxHashMap::default(),
            re_exports: &[],
            plan,
            output,
            dry_run,
            fixes,
        });
    }

    pub(super) fn make_export(path: &Path, name: &str, line: u32) -> UnusedExport {
        UnusedExport {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }
    }

    /// Build a captured-hashes map containing the real on-disk hash of
    /// each path that the test wants to consider "freshly analyzed".
    /// Skipping paths that do not exist on disk keeps the helper compatible
    /// with tests that exercise the missing-file path.
    fn capture_hashes(paths: &[&Path]) -> CapturedHashes {
        let mut hashes = CapturedHashes::default();
        for path in paths {
            if let Ok(content) = std::fs::read_to_string(path) {
                hashes.insert(
                    path.to_path_buf(),
                    xxhash_rust::xxh3::xxh3_64(content.as_bytes()),
                );
            }
        }
        hashes
    }

    /// Run export fix for a single export. Returns (had_error, fixes).
    pub(super) fn fix_single(
        root: &Path,
        file: &Path,
        name: &str,
        line: u32,
        dry_run: bool,
    ) -> (bool, Vec<serde_json::Value>) {
        let format = if dry_run {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        };
        let export = make_export(file, name, line);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(file.to_path_buf(), vec![&export]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[file]);
        apply_export_fixes(
            root,
            &map,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            format,
            dry_run,
            &mut fixes,
        );
        let had_error = if dry_run {
            false
        } else {
            !plan.commit().failed.is_empty()
        };
        (had_error, fixes)
    }

    #[test]
    fn export_fixes_follow_path_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let names = ["h", "c", "f", "a", "g", "b", "e", "d"];
        let files: Vec<PathBuf> = names
            .iter()
            .map(|name| {
                let file = root.join(format!("{name}.ts"));
                std::fs::write(&file, "export const value = 1;\n").unwrap();
                file
            })
            .collect();
        let exports: Vec<UnusedExport> = files
            .iter()
            .map(|file| make_export(file, "value", 1))
            .collect();
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        for export in &exports {
            map.insert(export.path.clone(), vec![export]);
        }
        let file_refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        apply_export_fixes(
            root,
            &map,
            &capture_hashes(&file_refs),
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Json,
            true,
            &mut fixes,
        );

        let paths: Vec<&str> = fixes
            .iter()
            .map(|fix| fix["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            paths,
            [
                "a.ts", "b.ts", "c.ts", "d.ts", "e.ts", "f.ts", "g.ts", "h.ts"
            ]
        );
    }

    #[test]
    fn dry_run_export_fix_does_not_modify_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("src/utils.ts");
        std::fs::create_dir_all(root.join("src")).unwrap();
        let original = "export function foo() {}\nexport function bar() {}\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "foo", 1, true);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert_eq!(fixes[0]["name"], "foo");
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn actual_export_fix_removes_export_keyword() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("utils.ts");
        std::fs::write(&file, "export function foo() {}\nexport const bar = 1;\n").unwrap();

        let (had_error, fixes) = fix_single(root, &file, "foo", 1, false);

        assert!(!had_error);
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "function foo() {}\nexport const bar = 1;\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn export_fix_removes_default_from_function() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("component.ts");
        std::fs::write(&file, "export default function App() {}\n").unwrap();

        let (_, _) = fix_single(root, &file, "default", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "function App() {}\n");
    }

    #[test]
    fn export_fix_removes_default_from_class() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("service.ts");
        std::fs::write(&file, "export default class MyService {}\n").unwrap();

        let (_, _) = fix_single(root, &file, "default", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "class MyService {}\n");
    }

    #[test]
    fn export_fix_removes_default_from_abstract_class() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("base.ts");
        std::fs::write(&file, "export default abstract class Base {}\n").unwrap();

        let (_, _) = fix_single(root, &file, "default", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "abstract class Base {}\n");
    }

    #[test]
    fn export_fix_removes_default_from_async_function() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("handler.ts");
        std::fs::write(&file, "export default async function handler() {}\n").unwrap();

        let (_, _) = fix_single(root, &file, "default", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "async function handler() {}\n");
    }

    #[test]
    fn export_fix_skips_default_expression_export() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("config.ts");
        let original = "export default { key: 'value' };\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "default", 1, false);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_preserves_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("mod.ts");
        std::fs::write(&file, "  export const x = 1;\n").unwrap();

        let (_, _) = fix_single(root, &file, "x", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "  const x = 1;\n");
    }

    #[test]
    fn export_fix_preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("win.ts");
        std::fs::write(
            &file,
            "export function foo() {}\r\nexport function bar() {}\r\n",
        )
        .unwrap();

        let (_, _) = fix_single(root, &file, "foo", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "function foo() {}\r\nexport function bar() {}\r\n");
    }

    #[test]
    fn export_fix_skips_path_outside_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside_file = dir.path().join("outside.ts");
        let original = "export function evil() {}\n";
        std::fs::write(&outside_file, original).unwrap();

        let export = make_export(&outside_file, "evil", 1);
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(outside_file.clone(), vec![&export]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(&root).unwrap();
        let hashes = capture_hashes(&[&outside_file]);
        apply_export_fixes(
            &root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );
        let _ = plan.commit();

        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_skips_line_not_starting_with_export() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("tricky.ts");
        let original = "const foo = 'export something';\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "foo", 1, false);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_handles_multiple_exports_in_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("multi.ts");
        std::fs::write(
            &file,
            "export function a() {}\nexport const b = 1;\nexport class C {}\n",
        )
        .unwrap();

        let e1 = make_export(&file, "a", 1);
        let e2 = make_export(&file, "C", 3);
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(file.clone(), vec![&e1, &e2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "function a() {}\nexport const b = 1;\nclass C {}\n"
        );
        assert_eq!(fixes.len(), 2);
    }

    #[test]
    fn export_fix_skips_out_of_bounds_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("short.ts");
        std::fs::write(&file, "export function a() {}\n").unwrap();

        let (_, fixes) = fix_single(root, &file, "ghost", 999, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export function a() {}\n");
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_removes_export_from_const() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("constants.ts");
        std::fs::write(&file, "export const MAX = 100;\n").unwrap();

        let (_, _) = fix_single(root, &file, "MAX", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "const MAX = 100;\n");
    }

    #[test]
    fn export_fix_removes_export_from_let() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("state.ts");
        std::fs::write(&file, "export let counter = 0;\n").unwrap();

        let (_, _) = fix_single(root, &file, "counter", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "let counter = 0;\n");
    }

    #[test]
    fn export_fix_removes_export_from_type_alias() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("types.ts");
        std::fs::write(&file, "export type Foo = string;\n").unwrap();

        let (_, _) = fix_single(root, &file, "Foo", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "type Foo = string;\n");
    }

    #[test]
    fn export_fix_removes_export_from_interface() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("types.ts");
        std::fs::write(&file, "export interface Bar {\n  name: string;\n}\n").unwrap();

        let (_, _) = fix_single(root, &file, "Bar", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "interface Bar {\n  name: string;\n}\n");
    }

    #[test]
    fn export_fix_removes_export_from_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("enums.ts");
        std::fs::write(&file, "export enum Status { Active, Inactive }\n").unwrap();

        let (_, _) = fix_single(root, &file, "Status", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "\n");
    }

    #[test]
    fn export_fix_removes_multiline_exported_enum_when_unused_locally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("enums.ts");
        std::fs::write(
            &file,
            "const before = 1;\nexport enum Status {\n  Active,\n  Inactive,\n}\nconst after = 2;\n",
        )
        .unwrap();

        let (_, fixes) = fix_single(root, &file, "Status", 2, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "const before = 1;\nconst after = 2;\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert_eq!(fixes[0]["name"], "Status");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn export_fix_only_removes_export_from_enum_when_used_locally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("enums.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active,\n  Inactive,\n}\nconsole.log(Status.Active);\n",
        )
        .unwrap();

        let (_, _) = fix_single(root, &file, "Status", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "enum Status {\n  Active,\n  Inactive,\n}\nconsole.log(Status.Active);\n"
        );
    }

    #[test]
    fn export_fix_removes_const_enum_declaration() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("enums.ts");
        std::fs::write(&file, "export const enum Status { Active }\n").unwrap();

        let (_, _) = fix_single(root, &file, "Status", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "\n");
    }

    #[test]
    fn export_fix_deletes_export_list_before_enum_without_shift() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        std::fs::write(
            &file,
            "export { unused } from './unused';\nexport enum Status {\n  Active,\n}\nexport const kept = 1;\n",
        )
        .unwrap();

        let e1 = make_export(&file, "unused", 1);
        let e2 = make_export(&file, "Status", 2);
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(file.clone(), vec![&e1, &e2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export const kept = 1;\n");
    }

    #[test]
    fn export_fix_deduplicates_same_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("dup.ts");
        std::fs::write(&file, "export function foo() {}\n").unwrap();

        let e1 = make_export(&file, "foo", 1);
        let e2 = make_export(&file, "foo", 1); // duplicate line
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(file.clone(), vec![&e1, &e2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "function foo() {}\n");
        assert_eq!(fixes.len(), 2);
    }

    #[test]
    fn export_fix_preserves_tab_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("tabbed.ts");
        std::fs::write(&file, "\texport const x = 1;\n").unwrap();

        let (_, _) = fix_single(root, &file, "x", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "\tconst x = 1;\n");
    }

    #[test]
    fn export_fix_line_zero_saturating_sub() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("zero.ts");
        std::fs::write(&file, "export function first() {}\n").unwrap();

        let (_, _) = fix_single(root, &file, "first", 0, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "function first() {}\n");
    }

    #[test]
    fn export_fix_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("empty.ts");
        std::fs::write(&file, "").unwrap();

        let (_, fixes) = fix_single(root, &file, "x", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "");
        assert!(fixes.is_empty());
    }

    #[test]
    fn dry_run_with_human_output_reports_fixes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("mod.ts");
        let original = "export function foo() {}\n";
        std::fs::write(&file, original).unwrap();

        let export = make_export(&file, "foo", 1);
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(file.clone(), vec![&export]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            true,
            &mut fixes,
        );

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn export_fix_skips_default_variable_export() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("config.ts");
        let original = "export default someVariable;\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "default", 1, false);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_nonexistent_file_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("missing.ts"); // Does not exist

        let (had_error, fixes) = fix_single(root, &file, "foo", 1, false);

        assert!(!had_error);
        assert!(fixes.is_empty());
    }

    #[test]
    fn export_fix_returns_relative_path_in_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("src").join("utils.ts");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(&file, "export const x = 1;\n").unwrap();

        let (_, fixes) = fix_single(root, &file, "x", 1, false);

        let path_str = fixes[0]["path"].as_str().unwrap().replace('\\', "/");
        assert_eq!(path_str, "src/utils.ts");
    }

    /// Windows-only CI produced `src\util.ts` because `push_export_fix_json`
    /// rendered the relative path with `.display()`, which emits the
    /// platform separator (issue caught in the v3.24.0 release run). This
    /// constructs the backslash directly instead of relying on the host
    /// OS to produce one, so the assertion pins the invariant on every
    /// platform, not only on Windows.
    #[test]
    fn push_export_fix_json_normalizes_backslash_separators() {
        let relative = Path::new("src\\util.ts");
        let absolute = Path::new("/project/src\\util.ts");
        let fix = ExportFix {
            line_idx: 1,
            export_name: "removable".to_owned(),
            edit: ExportEdit::Keyword { start: 0, line: 1 },
        };

        let mut fixes = Vec::new();
        push_export_fix_json(&mut fixes, relative, absolute, &fix, None);

        let path = fixes[0]["path"].as_str().unwrap();
        assert_eq!(
            path, "src/util.ts",
            "the reported path must use forward slashes on every platform: {path}"
        );
    }

    #[test]
    fn export_fix_removes_specifier_from_export_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        std::fs::write(&file, "export { Foo, Bar, Baz } from \"./mod\";\n").unwrap();

        let (_, _) = fix_single(root, &file, "Bar", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export { Foo, Baz } from \"./mod\";\n");
    }

    #[test]
    fn export_fix_removes_all_specifiers_deletes_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        std::fs::write(
            &file,
            "export { Foo, Bar } from \"./mod\";\nexport const x = 1;\n",
        )
        .unwrap();

        let e1 = make_export(&file, "Foo", 1);
        let e2 = make_export(&file, "Bar", 1);
        let mut exports_by_file: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        exports_by_file.insert(file.clone(), vec![&e1, &e2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &exports_by_file,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export const x = 1;\n");
    }

    #[test]
    fn export_fix_handles_export_list_without_from() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("barrel.ts");
        std::fs::write(
            &file,
            "const A = 1;\nconst B = 2;\nconst C = 3;\nexport { A, B, C };\n",
        )
        .unwrap();

        let (_, _) = fix_single(root, &file, "B", 4, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "const A = 1;\nconst B = 2;\nconst C = 3;\nexport { A, C };\n"
        );
    }

    #[test]
    fn export_fix_handles_export_type_list() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("types.ts");
        std::fs::write(&file, "export type { Foo, Bar } from \"./types\";\n").unwrap();

        let (_, _) = fix_single(root, &file, "Foo", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export type { Bar } from \"./types\";\n");
    }

    /// The analysis reports an aliased specifier by its exported name, so
    /// the fixer matches that name and not the local name before `as`.
    #[test]
    fn export_fix_handles_aliased_specifiers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        let original = "export { Foo as MyFoo, Bar } from \"./mod\";\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "Foo", 1, false);
        assert!(
            fixes.is_empty(),
            "the local name is not an export: {fixes:?}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);

        let (_, _) = fix_single(root, &file, "MyFoo", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export { Bar } from \"./mod\";\n");
    }

    #[test]
    fn export_fix_single_specifier_list_deletes_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        std::fs::write(
            &file,
            "export { Foo } from \"./foo\";\nexport { Bar } from \"./bar\";\n",
        )
        .unwrap();

        let (_, _) = fix_single(root, &file, "Foo", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export { Bar } from \"./bar\";\n");
    }

    #[test]
    fn is_off_graph_consumer_path_matches_every_listed_dir() {
        for dir in OFF_GRAPH_CONSUMER_DIRS {
            let p = PathBuf::from(format!("src/{dir}/file.ts"));
            assert!(is_off_graph_consumer_path(&p), "{dir} should match");
        }
        assert!(is_off_graph_consumer_path(Path::new(
            "packages/app/e2e/utils/helper.ts"
        )));
        assert!(is_off_graph_consumer_path(Path::new(
            "src/components/__mocks__/api.ts"
        )));
    }

    #[test]
    fn is_off_graph_consumer_path_rejects_normal_and_excluded_dirs() {
        for p in [
            "src/utils.ts",
            "src/components/App.tsx",
            "test/foo.ts",
            "tests/foo.ts",
            "__tests__/foo.ts",
            "src/stories/Button.stories.ts",
            "src/mocks/handlers.ts",
            "src/fixtured/data.ts",
            "src/golden-path/route.ts",
        ] {
            assert!(!is_off_graph_consumer_path(Path::new(p)), "{p} matched");
        }
    }

    #[test]
    fn low_confidence_reason_prefers_off_graph_over_unresolved() {
        let abs = PathBuf::from("/proj/e2e/foo.ts");
        let mut unresolved = FxHashSet::default();
        unresolved.insert(abs.clone());
        assert_eq!(
            low_confidence_skip_reason(Path::new("e2e/foo.ts"), &abs, &unresolved, &[]),
            Some(SkipReason::LowConfidenceOffGraph)
        );
    }

    #[test]
    fn low_confidence_reason_unresolved_when_not_off_graph() {
        let abs = PathBuf::from("/proj/src/foo.ts");
        let mut unresolved = FxHashSet::default();
        unresolved.insert(abs.clone());
        assert_eq!(
            low_confidence_skip_reason(Path::new("src/foo.ts"), &abs, &unresolved, &[]),
            Some(SkipReason::LowConfidenceUnresolvedImports)
        );
    }

    #[test]
    fn low_confidence_reason_none_for_clean_file() {
        let abs = PathBuf::from("/proj/src/foo.ts");
        assert_eq!(
            low_confidence_skip_reason(Path::new("src/foo.ts"), &abs, &FxHashSet::default(), &[]),
            None
        );
    }

    /// The analysis already recorded that this verdict rests on an import
    /// graph it knows is incomplete. Removing the export on that evidence is
    /// how `fallow fix` breaks a build with a mutation fallow itself flagged,
    /// so the removal is withheld the same way an off-graph export is: the
    /// exit code does not move and `fallow dead-code` keeps reporting it.
    #[test]
    fn a_reachability_caveat_withholds_the_export_removal() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/lib.ts");
        let original = "export const needed = (): void => {};\n";
        std::fs::write(&file, original).unwrap();

        let export = make_export(&file, "needed", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(file.clone(), vec![&export]);
        let mut caveats_by_file: FxHashMap<PathBuf, Vec<ReachabilityCaveat>> = FxHashMap::default();
        caveats_by_file.insert(
            file.clone(),
            vec![ReachabilityCaveat::IncompleteImportGraph],
        );
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);

        super::apply_export_fixes(&mut ExportFixInput {
            root,
            exports_by_file: &map,
            hashes: &hashes,
            unresolved_import_files: &FxHashSet::default(),
            caveats_by_file: &caveats_by_file,
            re_exports: &[],
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });

        assert!(
            fixes.is_empty(),
            "no removal is planned for a caveated export"
        );
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(
            plan.skipped()[0].reason,
            SkipReason::LowConfidenceIncompleteAnalysis
        );
        assert_eq!(
            plan.skipped()[0].caveats,
            vec![ReachabilityCaveat::IncompleteImportGraph],
            "the skip carries the marker so a caller can gate on it"
        );
        assert!(
            plan.skipped()[0].reason.is_intentional(),
            "a withheld low-confidence removal must not move the exit code"
        );
        let _ = plan.commit();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    }

    /// The withholding has to hold on the preview path too: an agent runs
    /// `fix --dry-run` first and must not be told the removal is planned.
    #[test]
    fn a_reachability_caveat_withholds_the_export_removal_in_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/lib.ts");
        let original = "export const needed = (): void => {};\n";
        std::fs::write(&file, original).unwrap();

        let export = make_export(&file, "needed", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(file.clone(), vec![&export]);
        let mut caveats_by_file: FxHashMap<PathBuf, Vec<ReachabilityCaveat>> = FxHashMap::default();
        caveats_by_file.insert(
            file.clone(),
            vec![ReachabilityCaveat::IncompleteFileAnalysis],
        );
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);

        super::apply_export_fixes(&mut ExportFixInput {
            root,
            exports_by_file: &map,
            hashes: &hashes,
            unresolved_import_files: &FxHashSet::default(),
            caveats_by_file: &caveats_by_file,
            re_exports: &[],
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: true,
            fixes: &mut fixes,
        });

        assert!(fixes.is_empty());
        assert_eq!(
            plan.skipped()[0].reason,
            SkipReason::LowConfidenceIncompleteAnalysis
        );
    }

    /// A clean finding still auto-fixes: the caveat gate must not become a
    /// blanket refusal on every export in a project that has one broken file.
    #[test]
    fn an_uncaveated_export_is_still_removed_in_an_incomplete_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let clean = root.join("src/clean.ts");
        std::fs::write(&clean, "export const gone = (): void => {};\n").unwrap();
        let caveated = root.join("src/caveated.ts");

        let export = make_export(&clean, "gone", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(clean.clone(), vec![&export]);
        let mut caveats_by_file: FxHashMap<PathBuf, Vec<ReachabilityCaveat>> = FxHashMap::default();
        caveats_by_file.insert(caveated, vec![ReachabilityCaveat::IncompleteImportGraph]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&clean]);

        super::apply_export_fixes(&mut ExportFixInput {
            root,
            exports_by_file: &map,
            hashes: &hashes,
            unresolved_import_files: &FxHashSet::default(),
            caveats_by_file: &caveats_by_file,
            re_exports: &[],
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });

        assert!(plan.skipped().is_empty());
        let _ = plan.commit();
        assert_eq!(
            std::fs::read_to_string(&clean).unwrap(),
            "const gone = (): void => {};\n"
        );
    }

    #[test]
    fn off_graph_file_export_removal_is_withheld() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("e2e")).unwrap();
        let file = root.join("e2e/utils.ts");
        let original = "export function helper() {}\n";
        std::fs::write(&file, original).unwrap();

        let export = make_export(&file, "helper", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(file.clone(), vec![&export]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &map,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );

        assert!(fixes.is_empty());
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].reason, SkipReason::LowConfidenceOffGraph);
        let _ = plan.commit();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    }

    #[test]
    fn off_graph_file_export_removal_withheld_in_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("__mocks__")).unwrap();
        let file = root.join("__mocks__/sdk.ts");
        let original = "export const client = {};\n";
        std::fs::write(&file, original).unwrap();

        let (_, fixes) = fix_single(root, &file, "client", 1, true);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn unresolved_import_file_export_removal_is_withheld() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let file = root.join("src/foo.ts");
        let original = "export function bar() {}\n";
        std::fs::write(&file, original).unwrap();

        let export = make_export(&file, "bar", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(file.clone(), vec![&export]);
        let mut unresolved = FxHashSet::default();
        unresolved.insert(file.clone());
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_export_fixes(
            root,
            &map,
            &hashes,
            &unresolved,
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );

        assert!(fixes.is_empty());
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(
            plan.skipped()[0].reason,
            SkipReason::LowConfidenceUnresolvedImports
        );
        let _ = plan.commit();

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
    }

    #[test]
    fn normal_file_still_fixed_alongside_off_graph_skip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("e2e")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let e2e_file = root.join("e2e/utils.ts");
        let src_file = root.join("src/utils.ts");
        std::fs::write(&e2e_file, "export function helper() {}\n").unwrap();
        std::fs::write(&src_file, "export function realDead() {}\n").unwrap();

        let e1 = make_export(&e2e_file, "helper", 1);
        let e2 = make_export(&src_file, "realDead", 1);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedExport>> = FxHashMap::default();
        map.insert(e2e_file.clone(), vec![&e1]);
        map.insert(src_file.clone(), vec![&e2]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&e2e_file, &src_file]);
        apply_export_fixes(
            root,
            &map,
            &hashes,
            &FxHashSet::default(),
            &mut plan,
            OutputFormat::Human,
            false,
            &mut fixes,
        );

        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["name"], "realDead");
        assert_eq!(plan.skipped().len(), 1);
        assert_eq!(plan.skipped()[0].reason, SkipReason::LowConfidenceOffGraph);
        let _ = plan.commit();

        assert_eq!(
            std::fs::read_to_string(&e2e_file).unwrap(),
            "export function helper() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(&src_file).unwrap(),
            "function realDead() {}\n"
        );
    }
}
