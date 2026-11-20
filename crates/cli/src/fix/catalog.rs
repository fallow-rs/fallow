//! Auto-fix for `unused-catalog-entries` and `empty-catalog-groups` findings.
//!
//! The fixer groups the findings by file and reads each `pnpm-workspace.yaml`
//! one time. It asks `fallow_config::pnpm_catalog_edit::plan_catalog_edits`
//! for all edits of that file, stages the result one time, and writes one
//! record for each finding. The planner finds each key by its path in the
//! text that the fixer reads, and it refuses an edit that would change
//! other parts of the YAML. A dry run and an apply run use the same plan.
//!
//! Entries whose `hardcoded_consumers` is non-empty are skipped: removing
//! the catalog entry while a workspace package still references it via
//! the `catalog:` protocol would break the user's next `pnpm install`.
//! The skip is reported in the fix output (and in human stderr) so the
//! user knows to migrate the consumer first.
//!
//! Multi-document YAML files (`---` document separators) are rejected
//! with a skip record because line edits cannot reliably attribute lines
//! to documents.

use std::path::Path;

use fallow_config::pnpm_catalog_edit::{
    CatalogEditPlan, CatalogEditRefusal, CatalogRemoval, CatalogRemovalOutcome,
    PlannedCatalogRemoval, apply_line_edits, is_multi_document_yaml, plan_catalog_edits,
};
use fallow_config::{CatalogPrecedingCommentPolicy, OutputFormat};
use fallow_types::catalog_source::is_pnpm_catalog_source;
use fallow_types::output_dead_code::{EmptyCatalogGroupFinding, UnusedCatalogEntryFinding};
use fallow_types::results::{EmptyCatalogGroup, UnusedCatalogEntry};

use super::plan::{CapturedHashes, FixPlan, read_source_with_hash_check, stage_fixed_content};

const MULTI_DOCUMENT_DESCRIPTION: &str = "Skipped: pnpm-workspace.yaml contains a `---` document separator; fallow fix does not support multi-document YAML";

/// Inputs for [`apply_catalog_fixes`].
pub(super) struct CatalogFixInput<'a> {
    pub(super) root: &'a Path,
    pub(super) entries: &'a [UnusedCatalogEntryFinding],
    pub(super) groups: &'a [EmptyCatalogGroupFinding],
    pub(super) preceding_comment_policy: CatalogPrecedingCommentPolicy,
    pub(super) hashes: &'a CapturedHashes,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

/// Apply the unused-catalog-entry and empty-catalog-group fixes.
///
/// `fixes` gets the entry records of all files first, then the group
/// records of all files. The returned `skipped` counts the findings that
/// the fixer did not remove on purpose.
pub(super) fn apply_catalog_fixes(input: CatalogFixInput<'_>) -> CatalogFixSummary {
    let CatalogFixInput {
        root,
        entries,
        groups,
        preceding_comment_policy,
        hashes,
        plan,
        output,
        dry_run,
        fixes,
    } = input;
    let mut summary = CatalogFixSummary::default();
    let mut by_path: rustc_hash::FxHashMap<&Path, FileFindings<'_>> =
        rustc_hash::FxHashMap::default();
    for finding in entries {
        let entry = &finding.entry;
        by_path
            .entry(entry.path.as_path())
            .or_default()
            .entries
            .push(entry);
    }
    for finding in groups {
        let group = &finding.group;
        by_path
            .entry(group.path.as_path())
            .or_default()
            .groups
            .push(group);
    }

    let mut records = CatalogRecords::default();
    for (relative_path, findings) in super::in_path_order(by_path) {
        fix_catalog_file(&mut CatalogFile {
            root,
            relative_path,
            findings: &findings,
            policy: preceding_comment_policy,
            hashes,
            plan: &mut *plan,
            output,
            dry_run,
            records: &mut records,
            summary: &mut summary,
        });
    }
    fixes.append(&mut records.entries);
    fixes.append(&mut records.groups);
    summary
}

#[derive(Default)]
struct FileFindings<'a> {
    entries: Vec<&'a UnusedCatalogEntry>,
    groups: Vec<&'a EmptyCatalogGroup>,
}

#[derive(Default)]
struct CatalogRecords {
    entries: Vec<serde_json::Value>,
    groups: Vec<serde_json::Value>,
}

struct CatalogFile<'a, 'b> {
    root: &'b Path,
    relative_path: &'b Path,
    findings: &'b FileFindings<'a>,
    policy: CatalogPrecedingCommentPolicy,
    hashes: &'b CapturedHashes,
    plan: &'b mut FixPlan,
    output: OutputFormat,
    dry_run: bool,
    records: &'b mut CatalogRecords,
    summary: &'b mut CatalogFixSummary,
}

/// What the fixer does with one finding of a file.
#[derive(Clone, Copy)]
enum Slot {
    HardcodedConsumers,
    Removal(usize),
    Duplicate,
}

fn slot_for<'a>(removal: CatalogRemoval<'a>, removals: &mut Vec<CatalogRemoval<'a>>) -> Slot {
    if removals.contains(&removal) {
        return Slot::Duplicate;
    }
    removals.push(removal);
    Slot::Removal(removals.len() - 1)
}

/// Plan, record and stage the catalog fixes of one file.
fn fix_catalog_file(file: &mut CatalogFile<'_, '_>) {
    let FileFindings { entries, groups } = file.findings;
    if !is_pnpm_catalog_source(file.relative_path) {
        skip_unsupported_catalog_source_entries(entries, file);
        skip_unsupported_empty_catalog_groups(groups, file);
        return;
    }

    let absolute = file.root.join(file.relative_path);
    let Some((content, meta)) =
        read_source_with_hash_check(file.root, &absolute, file.hashes, file.plan)
    else {
        return;
    };

    if is_multi_document_yaml(&content) {
        skip_multi_document(entries, groups, file);
        return;
    }

    let (removals, entry_slots, group_slots) = removals_of(entries, groups);
    let edit_plan = plan_catalog_edits(&content, &removals, file.policy);
    let outcome = |index: usize| -> Result<&PlannedCatalogRemoval, CatalogEditRefusal> {
        match &edit_plan {
            Err(refusal) => Err(*refusal),
            Ok(plan) => match &plan.outcomes[index] {
                CatalogRemovalOutcome::Planned(planned) => Ok(planned),
                CatalogRemovalOutcome::Refused(refusal) => Err(*refusal),
            },
        }
    };

    let mut planned = PlannedRemovals::default();
    for (entry, slot) in entries.iter().zip(&entry_slots) {
        match slot {
            Slot::HardcodedConsumers => skip_hardcoded_catalog_consumers(entry, file),
            Slot::Duplicate => {}
            Slot::Removal(index) => match outcome(*index) {
                Ok(removal) => planned.entries.push((removal, entry)),
                Err(refusal) => {
                    let (reason, description) = refusal_reason(refusal);
                    file.summary.skipped += 1;
                    let record =
                        skip_record(entry, reason, description, file.output, file.relative_path);
                    file.records.entries.push(record);
                }
            },
        }
    }
    for (group, slot) in groups.iter().zip(&group_slots) {
        let Slot::Removal(index) = slot else {
            continue;
        };
        match outcome(*index) {
            Ok(removal) => planned.groups.push((removal, group)),
            Err(refusal) => {
                let (reason, description) = refusal_reason(refusal);
                file.summary.skipped += 1;
                let record =
                    skip_group_record(group, reason, description, file.output, file.relative_path);
                file.records.groups.push(record);
            }
        }
    }

    if let (false, Ok(plan)) = (file.dry_run, &edit_plan) {
        stage_catalog_edits(file, &absolute, &content, &meta, plan);
    }
    record_planned_removals(file, &absolute, planned);
}

/// The removal requests of one file, with the slot of each entry finding
/// and each group finding.
fn removals_of<'a>(
    entries: &[&'a UnusedCatalogEntry],
    groups: &[&'a EmptyCatalogGroup],
) -> (Vec<CatalogRemoval<'a>>, Vec<Slot>, Vec<Slot>) {
    let mut removals: Vec<CatalogRemoval<'a>> = Vec::new();
    let entry_slots: Vec<Slot> = entries
        .iter()
        .map(|entry| {
            if entry.hardcoded_consumers.is_empty() {
                let removal = CatalogRemoval::Entry {
                    catalog: &entry.catalog_name,
                    entry: &entry.entry_name,
                };
                slot_for(removal, &mut removals)
            } else {
                Slot::HardcodedConsumers
            }
        })
        .collect();
    let group_slots: Vec<Slot> = groups
        .iter()
        .map(|group| {
            let removal = CatalogRemoval::Group {
                catalog: &group.catalog_name,
            };
            slot_for(removal, &mut removals)
        })
        .collect();
    (removals, entry_slots, group_slots)
}

#[derive(Default)]
struct PlannedRemovals<'p, 'a> {
    entries: Vec<(&'p PlannedCatalogRemoval, &'a UnusedCatalogEntry)>,
    groups: Vec<(&'p PlannedCatalogRemoval, &'a EmptyCatalogGroup)>,
}

/// Write the removal records of one file: entries by first deleted line and
/// groups by key line, both in descending order.
fn record_planned_removals(
    file: &mut CatalogFile<'_, '_>,
    absolute: &Path,
    mut planned: PlannedRemovals<'_, '_>,
) {
    planned
        .entries
        .sort_by_key(|(removal, _)| std::cmp::Reverse(removal.lines.start));
    planned
        .groups
        .sort_by_key(|(removal, _)| std::cmp::Reverse(removal.key_line));
    let applied = !file.dry_run;
    let target = applied.then(|| serde_json::json!(absolute.display().to_string()));
    let display_path = file.relative_path.to_string_lossy().replace('\\', "/");
    for (removal, entry) in planned.entries {
        if file.dry_run && !matches!(file.output, OutputFormat::Json) {
            eprintln!(
                "Would remove catalog entry from {display_path}:{} `{}` (catalog: {})",
                removal.lines.start + 1,
                entry.entry_name,
                entry.catalog_name,
            );
        }
        let mut record = remove_record(
            entry,
            &removal.lines,
            removal.key_line,
            applied,
            file.relative_path,
        );
        if let Some(target) = &target {
            record["__target"] = target.clone();
            file.summary.comment_lines_removed += removal.key_line - removal.lines.start;
        }
        file.records.entries.push(record);
        file.summary.applied += 1;
    }
    for (removal, group) in planned.groups {
        if file.dry_run && !matches!(file.output, OutputFormat::Json) {
            eprintln!(
                "Would remove empty catalog group from {display_path}:{} `{}`",
                removal.key_line + 1,
                group.catalog_name,
            );
        }
        let mut record = remove_group_record(group, removal.key_line, applied, file.relative_path);
        if let Some(target) = &target {
            record["__target"] = target.clone();
        }
        file.records.groups.push(record);
        file.summary.applied += 1;
    }
}

fn stage_catalog_edits(
    file: &mut CatalogFile<'_, '_>,
    absolute: &Path,
    content: &str,
    meta: &super::io::EncodingMetadata,
    plan: &CatalogEditPlan,
) {
    if plan.edits.is_empty() {
        return;
    }
    let mut new_lines: Vec<String> = content
        .split(meta.line_ending)
        .map(ToString::to_string)
        .collect();
    apply_line_edits(&mut new_lines, &plan.edits);
    stage_fixed_content(file.plan, absolute, &new_lines, meta, content);
}

/// The wire value and the description of the skip record for a refusal.
fn refusal_reason(refusal: CatalogEditRefusal) -> (&'static str, &'static str) {
    match refusal {
        CatalogEditRefusal::NotFound => (
            "not_found",
            "Skipped: the catalog key is not in pnpm-workspace.yaml; the file may have been edited since fallow dead-code ran",
        ),
        CatalogEditRefusal::NotEmpty => (
            "not_empty",
            "Skipped: the catalog group has entries; the file may have been edited since fallow dead-code ran",
        ),
        CatalogEditRefusal::UnsupportedStyle => (
            "unsupported_yaml_style",
            "Skipped: fallow fix only edits block-style catalog keys; edit this catalog manually",
        ),
        CatalogEditRefusal::MultiDocument => ("multi_document_yaml", MULTI_DOCUMENT_DESCRIPTION),
        CatalogEditRefusal::InvalidYaml | CatalogEditRefusal::DeltaMismatch => (
            "unsafe_edit",
            "Skipped: the edit would change other parts of pnpm-workspace.yaml; edit the catalog manually",
        ),
    }
}

const UNSUPPORTED_SOURCE_DESCRIPTION: &str = "Skipped: fallow fix only edits pnpm-workspace.yaml catalog entries; edit Bun package.json catalogs manually";

fn skip_unsupported_catalog_source_entries(
    entries: &[&UnusedCatalogEntry],
    file: &mut CatalogFile<'_, '_>,
) {
    for entry in entries {
        file.summary.skipped += 1;
        file.records.entries.push(skip_record(
            entry,
            "unsupported_catalog_source",
            UNSUPPORTED_SOURCE_DESCRIPTION,
            file.output,
            file.relative_path,
        ));
    }
}

fn skip_unsupported_empty_catalog_groups(
    groups: &[&EmptyCatalogGroup],
    file: &mut CatalogFile<'_, '_>,
) {
    for group in groups {
        file.summary.skipped += 1;
        file.records.groups.push(skip_group_record(
            group,
            "unsupported_catalog_source",
            UNSUPPORTED_SOURCE_DESCRIPTION,
            file.output,
            file.relative_path,
        ));
    }
}

fn skip_multi_document(
    entries: &[&UnusedCatalogEntry],
    groups: &[&EmptyCatalogGroup],
    file: &mut CatalogFile<'_, '_>,
) {
    for entry in entries {
        file.summary.skipped += 1;
        file.records.entries.push(skip_record(
            entry,
            "multi_document_yaml",
            MULTI_DOCUMENT_DESCRIPTION,
            file.output,
            file.relative_path,
        ));
    }
    for group in groups {
        file.summary.skipped += 1;
        file.records.groups.push(skip_group_record(
            group,
            "multi_document_yaml",
            MULTI_DOCUMENT_DESCRIPTION,
            file.output,
            file.relative_path,
        ));
    }
}

fn skip_hardcoded_catalog_consumers(entry: &UnusedCatalogEntry, file: &mut CatalogFile<'_, '_>) {
    file.summary.skipped += 1;
    let consumer_summary = format_consumer_summary(&entry.hardcoded_consumers);
    let description = format!(
        "Skipped: {consumer_summary} still pin `{}` with a hardcoded version. Switch the consumer(s) to \"{}\": \"catalog:{}\" first, then rerun fallow fix.",
        entry.entry_name,
        entry.entry_name,
        if entry.catalog_name == "default" {
            String::new()
        } else {
            entry.catalog_name.clone()
        },
    );
    file.records.entries.push(skip_record(
        entry,
        "hardcoded_consumers",
        &description,
        file.output,
        file.relative_path,
    ));
}

/// Output of [`apply_catalog_fixes`] consumed by the orchestrator.
#[derive(Debug, Default)]
pub(super) struct CatalogFixSummary {
    pub applied: usize,
    pub skipped: usize,
    pub write_error: bool,
    /// Total leading-comment lines absorbed across all applied fixes.
    /// Surfaced in the human summary so users see that comments were
    /// removed alongside entries (`Fixed N issue(s) (+M comment lines)`).
    pub comment_lines_removed: usize,
}

fn skip_record(
    entry: &UnusedCatalogEntry,
    skip_reason: &str,
    description: &str,
    output: OutputFormat,
    relative_path: &Path,
) -> serde_json::Value {
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Skipped catalog entry {}:{} `{}` ({skip_reason})",
            relative_path.to_string_lossy().replace('\\', "/"),
            entry.line,
            entry.entry_name,
        );
    }
    let consumers: Option<serde_json::Value> =
        if skip_reason == "hardcoded_consumers" && !entry.hardcoded_consumers.is_empty() {
            Some(serde_json::Value::Array(
                entry
                    .hardcoded_consumers
                    .iter()
                    .map(|p| serde_json::Value::String(p.to_string_lossy().replace('\\', "/")))
                    .collect(),
            ))
        } else {
            None
        };
    let mut value = serde_json::json!({
        "type": "remove_catalog_entry",
        "entry_name": entry.entry_name,
        "catalog_name": entry.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": entry.line,
        "applied": false,
        "skipped": true,
        "skip_reason": skip_reason,
        "description": description,
    });
    if let Some(consumers) = consumers
        && let serde_json::Value::Object(map) = &mut value
    {
        map.insert("consumers".to_string(), consumers);
    }
    value
}

fn remove_record(
    entry: &UnusedCatalogEntry,
    range: &std::ops::Range<usize>,
    key_line: usize,
    applied: bool,
    relative_path: &Path,
) -> serde_json::Value {
    let removed_lines = range.end - range.start;
    let mut value = serde_json::json!({
        "type": "remove_catalog_entry",
        "entry_name": entry.entry_name,
        "catalog_name": entry.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": range.start + 1,
        "entry_line": key_line + 1,
        "removed_lines": removed_lines,
    });
    if applied && let serde_json::Value::Object(map) = &mut value {
        map.insert("applied".to_string(), serde_json::Value::Bool(true));
    }
    value
}

fn skip_group_record(
    group: &EmptyCatalogGroup,
    skip_reason: &str,
    description: &str,
    output: OutputFormat,
    relative_path: &Path,
) -> serde_json::Value {
    if !matches!(output, OutputFormat::Json) {
        eprintln!(
            "Skipped empty catalog group {}:{} `{}` ({skip_reason})",
            relative_path.to_string_lossy().replace('\\', "/"),
            group.line,
            group.catalog_name,
        );
    }
    serde_json::json!({
        "type": "remove_empty_catalog_group",
        "catalog_name": group.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": group.line,
        "applied": false,
        "skipped": true,
        "skip_reason": skip_reason,
        "description": description,
    })
}

fn remove_group_record(
    group: &EmptyCatalogGroup,
    line_idx: usize,
    applied: bool,
    relative_path: &Path,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "type": "remove_empty_catalog_group",
        "catalog_name": group.catalog_name,
        "file": relative_path.to_string_lossy().replace('\\', "/"),
        "line": line_idx + 1,
        "removed_lines": 1,
    });
    if applied && let serde_json::Value::Object(map) = &mut value {
        map.insert("applied".to_string(), serde_json::Value::Bool(true));
    }
    value
}

fn format_consumer_summary(consumers: &[std::path::PathBuf]) -> String {
    let display = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/");
    match consumers.len() {
        0 => String::new(),
        1 => format!("`{}`", display(&consumers[0])),
        2 => format!(
            "`{}` and `{}`",
            display(&consumers[0]),
            display(&consumers[1])
        ),
        _ => format!(
            "`{}` and {} other consumer(s)",
            display(&consumers[0]),
            consumers.len() - 1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_entry(name: &str, catalog: &str, line: u32) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: vec![],
        })
    }

    fn make_entry_with_consumers(
        name: &str,
        catalog: &str,
        line: u32,
        consumers: Vec<PathBuf>,
    ) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
            hardcoded_consumers: consumers,
        })
    }

    fn make_package_json_entry(name: &str, catalog: &str, line: u32) -> UnusedCatalogEntryFinding {
        UnusedCatalogEntryFinding::with_actions(UnusedCatalogEntry {
            entry_name: name.to_string(),
            catalog_name: catalog.to_string(),
            path: PathBuf::from("package.json"),
            line,
            hardcoded_consumers: vec![],
        })
    }

    #[test]
    fn format_consumer_summary_normalizes_backslash_separators() {
        let one = format_consumer_summary(&[PathBuf::from("packages\\a\\package.json")]);
        assert_eq!(one, "`packages/a/package.json`");

        let two = format_consumer_summary(&[
            PathBuf::from("packages\\a\\package.json"),
            PathBuf::from("packages\\b\\package.json"),
        ]);
        assert_eq!(
            two,
            "`packages/a/package.json` and `packages/b/package.json`"
        );

        let many = format_consumer_summary(&[
            PathBuf::from("packages\\a\\package.json"),
            PathBuf::from("packages\\b\\package.json"),
            PathBuf::from("packages\\c\\package.json"),
        ]);
        assert_eq!(many, "`packages/a/package.json` and 2 other consumer(s)");
    }

    fn make_group(name: &str, line: u32) -> EmptyCatalogGroupFinding {
        EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: name.to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line,
        })
    }

    fn make_package_json_group(name: &str, line: u32) -> EmptyCatalogGroupFinding {
        EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: name.to_string(),
            path: PathBuf::from("package.json"),
            line,
        })
    }

    fn seed_workspace_file(root: &Path, content: &str) {
        let path = root.join("pnpm-workspace.yaml");
        std::fs::write(&path, content).unwrap();
    }

    fn seed_package_json(root: &Path, content: &str) {
        let path = root.join("package.json");
        std::fs::write(&path, content).unwrap();
    }

    /// Thin wrappers preserving the pre-#454 test API surface: build a
    /// FixPlan + CapturedHashes around the entry-fix / group-fix call
    /// and commit. Commit failures fold into `summary.write_error` so
    /// pre-existing tests that assert on that field keep working.
    #[expect(
        clippy::too_many_arguments,
        reason = "test helper mirrors the fields of CatalogFixInput"
    )]
    fn run_catalog_fix(
        root: &Path,
        entries: &[UnusedCatalogEntryFinding],
        groups: &[EmptyCatalogGroupFinding],
        policy: CatalogPrecedingCommentPolicy,
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> CatalogFixSummary {
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = CapturedHashes::default();
        let mut summary = apply_catalog_fixes(CatalogFixInput {
            root,
            entries,
            groups,
            preceding_comment_policy: policy,
            hashes: &hashes,
            plan: &mut plan,
            output,
            dry_run,
            fixes,
        });
        if !dry_run && !plan.commit().failed.is_empty() {
            summary.write_error = true;
        }
        summary
    }

    fn run_catalog_entry_fix(
        root: &Path,
        entries: &[UnusedCatalogEntryFinding],
        policy: CatalogPrecedingCommentPolicy,
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> CatalogFixSummary {
        run_catalog_fix(root, entries, &[], policy, output, dry_run, fixes)
    }

    fn run_empty_catalog_group_fix(
        root: &Path,
        groups: &[EmptyCatalogGroupFinding],
        output: OutputFormat,
        dry_run: bool,
        fixes: &mut Vec<serde_json::Value>,
    ) -> CatalogFixSummary {
        run_catalog_fix(
            root,
            &[],
            groups,
            CatalogPrecedingCommentPolicy::Auto,
            output,
            dry_run,
            fixes,
        )
    }

    #[test]
    fn removes_empty_named_catalog_group_header_only() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17: {}\n  # keep this note\n  vue3:\n    vue: ^3.4.0\n";
        seed_workspace_file(dir.path(), content);
        let groups = vec![make_group("react17", 2)];
        let mut fixes = Vec::new();

        let summary =
            run_empty_catalog_group_fix(dir.path(), &groups, OutputFormat::Json, false, &mut fixes);

        assert!(!summary.write_error);
        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  # keep this note\n  vue3:\n    vue: ^3.4.0\n"
        );
        assert_eq!(fixes[0]["type"], "remove_empty_catalog_group");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn removes_scalar_form_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n  left-pad: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 1);
        assert_eq!(summary.skipped, 0);
        assert!(!summary.write_error);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  left-pad: ^1.0.0\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["applied"], serde_json::json!(true));
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(1));
    }

    #[test]
    fn skips_bun_package_json_catalog_entries_without_mutating_json() {
        let dir = tempfile::tempdir().unwrap();
        let content = "{\n  \"workspaces\": {\n    \"catalog\": {\n      \"unused\": \"^1.0.0\"\n    }\n  }\n}\n";
        seed_package_json(dir.path(), content);

        let entries = vec![make_package_json_entry("unused", "default", 4)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.write_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("package.json")).unwrap(),
            content
        );
        assert_eq!(fixes[0]["skip_reason"], "unsupported_catalog_source");
        assert_eq!(fixes[0]["file"], "package.json");
    }

    #[test]
    fn skips_bun_package_json_empty_catalog_groups_without_mutating_json() {
        let dir = tempfile::tempdir().unwrap();
        let content =
            "{\n  \"workspaces\": {\n    \"catalogs\": {\n      \"empty\": {}\n    }\n  }\n}\n";
        seed_package_json(dir.path(), content);

        let groups = vec![make_package_json_group("empty", 4)];
        let mut fixes = Vec::new();
        let summary =
            run_empty_catalog_group_fix(dir.path(), &groups, OutputFormat::Json, false, &mut fixes);

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert!(!summary.write_error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("package.json")).unwrap(),
            content
        );
        assert_eq!(fixes[0]["skip_reason"], "unsupported_catalog_source");
        assert_eq!(fixes[0]["file"], "package.json");
    }

    #[test]
    fn removes_object_form_entry_with_nested_keys() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  react:\n    specifier: ^18.2.0\n    publishConfig:\n      access: public\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 1);
        assert_eq!(summary.skipped, 0);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n");
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(4));
    }

    #[test]
    fn skips_entries_with_hardcoded_consumers() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry_with_consumers(
            "is-even",
            "default",
            2,
            vec![PathBuf::from("apps/web/package.json")],
        )];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content, "file must not be modified when skipping");

        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["skipped"], serde_json::json!(true));
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("hardcoded_consumers")
        );
        assert!(
            fixes[0]["description"]
                .as_str()
                .unwrap()
                .contains("apps/web/package.json")
        );
        assert!(fixes[0]["consumers"].is_array());
    }

    #[test]
    fn dry_run_does_not_modify_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            true,
            &mut fixes,
        );

        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content);
        assert_eq!(fixes[0].get("applied"), None);
    }

    #[test]
    fn removes_named_catalog_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 1);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalogs:\n  react17:\n    react-dom: ^17.0.2\n");
    }

    #[test]
    fn catalog_fix_preserves_workspace_changed_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pnpm-workspace.yaml");
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n";
        let external = "packages:\n  - apps/*\n";
        seed_workspace_file(dir.path(), content);
        let entries = vec![make_entry("react", "react17", 3)];
        let hashes = CapturedHashes::default();
        let mut plan = FixPlan::for_root(dir.path()).unwrap();
        let mut fixes = Vec::new();
        let summary = apply_catalog_fixes(CatalogFixInput {
            root: dir.path(),
            entries: &entries,
            groups: &[],
            preceding_comment_policy: CatalogPrecedingCommentPolicy::Auto,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });
        assert_eq!(summary.applied, 1);
        std::fs::write(&path, external).unwrap();

        let outcome = plan.commit();

        assert!(outcome.written.is_empty());
        assert_eq!(outcome.failed.len(), 1);
        assert_eq!(outcome.failed[0].0, path);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap(),
            external
        );
    }

    #[test]
    fn preserves_trailing_inline_comment_on_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0 # keep me\n  is-even: ^1.0.0 # remove me\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0 # keep me\n");
    }

    #[test]
    fn auto_deletes_leading_comment_after_parent_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # mention is-even\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
        assert_eq!(fixes[0]["line"], serde_json::json!(2));
        assert_eq!(fixes[0]["entry_line"], serde_json::json!(3));
        assert_eq!(fixes[0]["removed_lines"], serde_json::json!(2));
        assert_eq!(summary.comment_lines_removed, 1);
    }

    #[test]
    fn auto_preserves_block_with_fallow_keep_marker() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # fallow-keep: audit trail for CVE-2024-XXXX\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalog:\n  # fallow-keep: audit trail for CVE-2024-XXXX\n  is-odd: ^1.0.0\n"
        );
        assert_eq!(summary.comment_lines_removed, 0);
    }

    #[test]
    fn always_preserves_block_with_fallow_keep_marker() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # fallow-keep\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  # fallow-keep\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn auto_preserves_section_banner_block() {
        let dir = tempfile::tempdir().unwrap();
        let content =
            "catalog:\n  # === React 18 production pins ===\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalog:\n  # === React 18 production pins ===\n  is-odd: ^1.0.0\n"
        );
        assert_eq!(summary.comment_lines_removed, 0);
    }

    #[test]
    fn always_deletes_section_banner_block() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # ====\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn auto_deletes_leading_comment_after_blank_separator() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n\n  # mention is-even\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 5)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n\n");
    }

    #[test]
    fn auto_preserves_leading_comment_after_sibling_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  # shared note\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  # shared note\n");
    }

    #[test]
    fn auto_deletes_named_catalog_leading_comment_after_named_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    # pinned for old peer deps\n    react: ^17.0.2\n    react-dom: ^17.0.2\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalogs:\n  react17:\n    react-dom: ^17.0.2\n");
    }

    #[test]
    fn always_deletes_leading_comment_after_sibling_entry() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  # force remove\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 4)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Always,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn never_preserves_leading_comment_after_parent_header() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  # keep always\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Never,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  # keep always\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn removes_multiple_adjacent_entries_in_one_pass() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n  left-pad: ^1.0.0\n  right-pad: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![
            make_entry("is-even", "default", 3),
            make_entry("left-pad", "default", 4),
        ];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 2);
        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n  right-pad: ^1.0.0\n");
    }

    #[test]
    fn rejects_multi_document_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n---\nfoo: bar\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            fixes[0]["skip_reason"],
            serde_json::json!("multi_document_yaml")
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\r\n  is-odd: ^1.0.0\r\n  is-even: ^1.0.0\r\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\r\n  is-odd: ^1.0.0\r\n");
    }

    #[test]
    fn rewrites_emptied_default_catalog_to_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 2)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog: {}\n");
        let doc = fallow_config::yaml::parse(&result).unwrap();
        assert!(
            doc.root()
                .get("catalog")
                .and_then(fallow_config::yaml::YamlNode::as_mapping)
                .is_some_and(fallow_config::yaml::YamlMapping::is_empty),
            "catalog must be `{{}}`, not null"
        );
    }

    #[test]
    fn rewrites_emptied_named_catalog_to_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n  legacy:\n    is-odd: ^3.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![
            make_entry("react", "react17", 3),
            make_entry("react-dom", "react17", 4),
        ];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  react17: {}\n  legacy:\n    is-odd: ^3.0.0\n",
        );
        let doc = fallow_config::yaml::parse(&result).unwrap();
        let react17 = doc.root().get("catalogs").and_then(|c| c.get("react17"));
        assert!(
            react17
                .and_then(fallow_config::yaml::YamlNode::as_mapping)
                .is_some_and(fallow_config::yaml::YamlMapping::is_empty),
            "react17 must be `{{}}`, not null. Got: {result}"
        );
    }

    #[test]
    fn preserves_non_empty_sibling_named_catalogs() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  react17:\n    react: ^17.0.2\n  vue3:\n    vue: ^3.4.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("react", "react17", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(
            result,
            "catalogs:\n  react17: {}\n  vue3:\n    vue: ^3.4.0\n"
        );
    }

    #[test]
    fn leaves_partially_populated_catalog_alone() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-odd: ^1.0.0\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);

        let entries = vec![make_entry("is-even", "default", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  is-odd: ^1.0.0\n");
    }

    #[test]
    fn rewrites_emptied_catalog_before_inline_comment() {
        let dir = tempfile::tempdir().unwrap();
        seed_workspace_file(
            dir.path(),
            "catalogs:\n  ui: # pinned versions\n    react: ^18\n",
        );
        let entries = vec![make_entry("react", "ui", 3)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalogs:\n  ui: {} # pinned versions\n");
        let doc = fallow_config::yaml::parse(&result).unwrap();
        assert!(
            doc.root()
                .get("catalogs")
                .and_then(|catalogs| catalogs.get("ui"))
                .and_then(fallow_config::yaml::YamlNode::as_mapping)
                .is_some_and(fallow_config::yaml::YamlMapping::is_empty),
            "catalogs.ui must be `{{}}`, not null"
        );
    }

    #[test]
    fn removes_object_entry_with_blank_line_after_key() {
        let dir = tempfile::tempdir().unwrap();
        seed_workspace_file(
            dir.path(),
            "catalog:\n  react:\n\n    specifier: ^18\n  vue:\n    specifier: ^3\n",
        );
        let entries = vec![make_entry("react", "default", 2)];
        let mut fixes = Vec::new();
        run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );

        let result = std::fs::read_to_string(dir.path().join("pnpm-workspace.yaml")).unwrap();
        assert_eq!(result, "catalog:\n  vue:\n    specifier: ^3\n");
    }

    fn read_workspace_file(root: &Path) -> String {
        std::fs::read_to_string(root.join("pnpm-workspace.yaml")).unwrap()
    }

    const ENTRY_AND_GROUP: &str =
        "catalog:\n  used: ^1\n  unused: ^1\ncatalogs:\n  old:\nlinkWorkspacePackages: true\n";

    fn run_entry_and_group(
        root: &Path,
        dry_run: bool,
    ) -> (CatalogFixSummary, Vec<serde_json::Value>) {
        seed_workspace_file(root, ENTRY_AND_GROUP);
        let entries = vec![make_entry("unused", "default", 3)];
        let groups = vec![make_group("old", 5)];
        let mut fixes = Vec::new();
        let summary = run_catalog_fix(
            root,
            &entries,
            &groups,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            dry_run,
            &mut fixes,
        );
        (summary, fixes)
    }

    #[test]
    fn entry_and_empty_group_removals_keep_the_next_setting() {
        let dir = tempfile::tempdir().unwrap();
        let (summary, _) = run_entry_and_group(dir.path(), false);
        assert_eq!(
            read_workspace_file(dir.path()),
            "catalog:\n  used: ^1\ncatalogs:\nlinkWorkspacePackages: true\n"
        );
        assert_eq!(summary.applied, 2);
    }

    #[test]
    fn dry_run_and_apply_report_the_same_lines() {
        let dry = tempfile::tempdir().unwrap();
        let apply = tempfile::tempdir().unwrap();
        let (_, dry_fixes) = run_entry_and_group(dry.path(), true);
        let (_, applied_fixes) = run_entry_and_group(apply.path(), false);
        assert_eq!(dry_fixes.len(), 2);
        assert_eq!(dry_fixes.len(), applied_fixes.len());
        for (dry, applied) in dry_fixes.iter().zip(&applied_fixes) {
            assert_eq!(dry["line"], applied["line"]);
            assert_eq!(dry["removed_lines"], applied["removed_lines"]);
        }
        assert_eq!(read_workspace_file(dry.path()), ENTRY_AND_GROUP);
    }

    #[test]
    fn skips_entry_that_is_not_in_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalog:\n  is-even: ^1.0.0\n";
        seed_workspace_file(dir.path(), content);
        let entries = vec![make_entry("is-odd", "default", 2)];
        let mut fixes = Vec::new();
        let summary = run_catalog_entry_fix(
            dir.path(),
            &entries,
            CatalogPrecedingCommentPolicy::Auto,
            OutputFormat::Json,
            false,
            &mut fixes,
        );
        assert_eq!(summary.applied, 0);
        assert_eq!(summary.skipped, 1);
        assert_eq!(fixes[0]["skip_reason"], serde_json::json!("not_found"));
        assert_eq!(read_workspace_file(dir.path()), content);
    }

    #[test]
    fn skips_group_that_has_entries() {
        let dir = tempfile::tempdir().unwrap();
        let content = "catalogs:\n  legacy:\n    react: ^18\n";
        seed_workspace_file(dir.path(), content);
        let groups = vec![make_group("legacy", 2)];
        let mut fixes = Vec::new();
        run_empty_catalog_group_fix(dir.path(), &groups, OutputFormat::Json, false, &mut fixes);
        assert_eq!(fixes[0]["skip_reason"], serde_json::json!("not_empty"));
        assert_eq!(read_workspace_file(dir.path()), content);
    }
}
