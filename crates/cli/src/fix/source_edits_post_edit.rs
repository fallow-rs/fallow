//! The post-edit check of the queued source edits.
//!
//! Before the plan writes a file, this check applies the queued edits of the
//! file and compares the facts of the text before and after the edits (see
//! [`fallow_engine::post_edit`]). It also checks that each export that the
//! edits remove is an export that the analysis found unused, and that no kept
//! re-export of another file still reads it. The check drops each group of
//! edits that fails, and keeps the other groups of the file. A file that the
//! check cannot parse before the edit keeps all its text.

use std::path::{Path, PathBuf};

use fallow_engine::module_graph::NamedReExport;
use fallow_engine::post_edit::{PostEditViolation, SourceFacts, checks_file};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{SourceEdit, SourceSnapshot, SourceSnapshots, apply_edits};

/// The analysis facts that the cross-file part of the check reads.
pub(in crate::fix) struct PostEditInput<'a> {
    /// The names of the exports that the analysis found unused, by absolute
    /// path. An edit may remove only these exports.
    pub(in crate::fix) unused_exports: &'a FxHashMap<PathBuf, FxHashSet<String>>,
    /// Every named re-export of the project.
    pub(in crate::fix) re_exports: &'a [NamedReExport],
}

/// One group of edits that the check dropped.
#[derive(Debug)]
pub(in crate::fix) struct DroppedEdit {
    /// The absolute path of the file.
    pub(in crate::fix) path: PathBuf,
    /// The 1-based lines of the analysed text that the edits of the group
    /// touch.
    pub(in crate::fix) lines: Vec<usize>,
    /// Why the check dropped the group.
    pub(in crate::fix) violation: PostEditViolation,
}

/// The check state of one edited file.
struct FileCheck {
    path: PathBuf,
    before: SourceFacts,
    /// The number of each queued group, in queue order.
    groups: Vec<usize>,
    /// The 1-based lines that each group touches, by position in `groups`.
    lines: Vec<Vec<usize>>,
    /// True for each group that passes the check, by position in `groups`.
    accepted: Vec<bool>,
    /// The exports of the text with the accepted groups applied.
    exports: FxHashSet<String>,
    /// The position of each dropped group, with the violation.
    dropped: Vec<(usize, PostEditViolation)>,
}

/// The exports of each checked file with its accepted groups applied.
type ExportsNow = FxHashMap<PathBuf, FxHashSet<String>>;

/// The re-exports that read each `(source path, imported name)`.
type ReExportIndex<'a> = FxHashMap<(&'a Path, &'a str), Vec<&'a NamedReExport>>;

impl SourceSnapshots {
    /// Check the queued edits of each file, drop each group that fails, and
    /// return the dropped groups.
    pub(in crate::fix) fn check_edits(&mut self, input: &PostEditInput<'_>) -> Vec<DroppedEdit> {
        let mut re_exports: ReExportIndex<'_> = FxHashMap::default();
        for re_export in input.re_exports {
            re_exports
                .entry((
                    re_export.source_path.as_path(),
                    re_export.imported_name.as_str(),
                ))
                .or_default()
                .push(re_export);
        }
        let (mut checks, unchecked) = self.file_checks();
        self.settle_exports(&mut checks, input.unused_exports, &re_exports);
        self.take_dropped(checks, unchecked)
    }

    /// One check per edited file, with the groups that pass the file-local
    /// part of the check. Also gives each file of a parsed type that does not
    /// parse before the edit.
    fn file_checks(&self) -> (Vec<FileCheck>, Vec<PathBuf>) {
        let mut checks: Vec<FileCheck> = Vec::new();
        let mut unchecked: Vec<PathBuf> = Vec::new();
        for path in &self.order {
            let Some(snapshot) = self.by_path.get(path) else {
                continue;
            };
            if snapshot.edits.is_empty() {
                continue;
            }
            let Some(before) = SourceFacts::collect(path, &snapshot.content) else {
                if checks_file(path) {
                    unchecked.push(path.clone());
                }
                continue;
            };
            let groups = group_numbers(snapshot);
            let mut check = FileCheck {
                path: path.clone(),
                before,
                lines: groups
                    .iter()
                    .map(|group| group_lines(snapshot, *group))
                    .collect(),
                accepted: vec![true; groups.len()],
                groups,
                exports: FxHashSet::default(),
                dropped: Vec::new(),
            };
            check.settle(snapshot, |_| Ok(()));
            checks.push(check);
        }
        (checks, unchecked)
    }

    /// Drop the groups that remove an export in use, until no file changes.
    fn settle_exports(
        &self,
        checks: &mut [FileCheck],
        unused_exports: &FxHashMap<PathBuf, FxHashSet<String>>,
        re_exports: &ReExportIndex<'_>,
    ) {
        let mut changed = true;
        while changed {
            changed = false;
            let exports_now: ExportsNow = checks
                .iter()
                .map(|check| (check.path.clone(), check.exports.clone()))
                .collect();
            for check in checks.iter_mut() {
                let snapshot = &self.by_path[&check.path];
                let path = check.path.clone();
                let before_exports = check.before.exports().clone();
                let exports_check = |exports: &FxHashSet<String>| {
                    removed_exports_are_unused(
                        &path,
                        (&before_exports, exports),
                        unused_exports,
                        re_exports,
                        &exports_now,
                    )
                };
                if exports_check(&check.exports).is_ok() {
                    continue;
                }
                check.settle(snapshot, exports_check);
                changed = true;
            }
        }
    }

    /// Remove the edits of each dropped group, and return the dropped groups.
    fn take_dropped(
        &mut self,
        checks: Vec<FileCheck>,
        unchecked: Vec<PathBuf>,
    ) -> Vec<DroppedEdit> {
        let mut dropped = Vec::new();
        for path in unchecked {
            let Some(snapshot) = self.by_path.get_mut(&path) else {
                continue;
            };
            for group in group_numbers(snapshot) {
                dropped.push(DroppedEdit {
                    path: path.clone(),
                    lines: group_lines(snapshot, group),
                    violation: PostEditViolation::DoesNotParse,
                });
            }
            snapshot.edits.clear();
        }
        for check in checks {
            let Some(snapshot) = self.by_path.get_mut(&check.path) else {
                continue;
            };
            for (position, violation) in check.dropped {
                let group = check.groups[position];
                snapshot.edits.retain(|(number, _)| *number != group);
                dropped.push(DroppedEdit {
                    path: check.path.clone(),
                    lines: check.lines[position].clone(),
                    violation,
                });
            }
        }
        dropped
    }
}

impl FileCheck {
    /// Keep the accepted groups when they pass together. Otherwise split
    /// them in halves until each failing group is alone, and drop it. Then
    /// drop each accepted group that shares a line with a dropped group, so
    /// that each line of the fix output belongs to written edits only.
    fn settle(
        &mut self,
        snapshot: &SourceSnapshot,
        extra: impl Fn(&FxHashSet<String>) -> Result<(), PostEditViolation>,
    ) {
        let run = |mask: &[bool]| {
            let facts = evaluate(&self.path, &self.before, snapshot, &self.groups, mask)?;
            extra(facts.exports())?;
            Ok::<SourceFacts, PostEditViolation>(facts)
        };
        if let Ok(facts) = run(&self.accepted) {
            self.exports.clone_from(facts.exports());
            return;
        }
        let candidates: Vec<usize> = (0..self.groups.len())
            .filter(|position| self.accepted[*position])
            .collect();
        let mut mask = vec![false; self.groups.len()];
        let mut dropped = Vec::new();
        bisect(&candidates, &mut mask, &mut dropped, &run);
        self.drop_shared_lines(&mut mask, &mut dropped);
        match run(&mask) {
            Ok(facts) => self.exports.clone_from(facts.exports()),
            Err(violation) => {
                for (position, accepted) in mask.iter_mut().enumerate() {
                    if *accepted {
                        *accepted = false;
                        dropped.push((position, violation.clone()));
                    }
                }
                self.exports.clone_from(self.before.exports());
            }
        }
        self.accepted = mask;
        self.dropped.extend(dropped);
    }

    fn drop_shared_lines(&self, mask: &mut [bool], dropped: &mut Vec<(usize, PostEditViolation)>) {
        let mut position = 0;
        while position < dropped.len() {
            let (gone, violation) = dropped[position].clone();
            for kept in 0..mask.len() {
                if mask[kept]
                    && self.lines[kept]
                        .iter()
                        .any(|line| self.lines[gone].contains(line))
                {
                    mask[kept] = false;
                    dropped.push((kept, violation.clone()));
                }
            }
            position += 1;
        }
    }
}

/// Accept `candidates` on top of `mask` when they pass together. Otherwise
/// split them, and drop a candidate that fails alone.
fn bisect(
    candidates: &[usize],
    mask: &mut Vec<bool>,
    dropped: &mut Vec<(usize, PostEditViolation)>,
    run: &impl Fn(&[bool]) -> Result<SourceFacts, PostEditViolation>,
) {
    let Some((&first, rest)) = candidates.split_first() else {
        return;
    };
    let mut trial = mask.clone();
    for position in candidates {
        trial[*position] = true;
    }
    match run(&trial) {
        Ok(_) => *mask = trial,
        Err(violation) if rest.is_empty() => dropped.push((first, violation)),
        Err(_) => {
            let (left, right) = candidates.split_at(candidates.len() / 2);
            bisect(left, mask, dropped, run);
            bisect(right, mask, dropped, run);
        }
    }
}

/// The distinct group numbers of the queued edits, in queue order.
fn group_numbers(snapshot: &SourceSnapshot) -> Vec<usize> {
    let mut groups: Vec<usize> = snapshot.edits.iter().map(|(group, _)| *group).collect();
    groups.sort_unstable();
    groups.dedup();
    groups
}

/// Check the text with the edits of the groups that `mask` selects applied.
fn evaluate(
    path: &Path,
    before: &SourceFacts,
    snapshot: &SourceSnapshot,
    groups: &[usize],
    mask: &[bool],
) -> Result<SourceFacts, PostEditViolation> {
    let selected: FxHashSet<usize> = groups
        .iter()
        .zip(mask)
        .filter(|(_, selected)| **selected)
        .map(|(group, _)| *group)
        .collect();
    let edits: Vec<SourceEdit> = snapshot
        .edits
        .iter()
        .filter(|(group, _)| selected.contains(group))
        .map(|(_, edit)| edit.clone())
        .collect();
    let deleted: Vec<(u32, u32)> = edits
        .iter()
        .filter(|edit| edit.replacement.is_empty())
        .map(|edit| {
            (
                u32::try_from(edit.start).unwrap_or(u32::MAX),
                u32::try_from(edit.end).unwrap_or(u32::MAX),
            )
        })
        .collect();
    let after = apply_edits(&snapshot.content, snapshot.meta.line_ending, edits);
    before.check(path, &after, &deleted)
}

/// Check that each export that the edits remove is unused, and that no kept
/// re-export reads it. `exports` holds the exports before and after the edits.
fn removed_exports_are_unused(
    path: &Path,
    (before, after): (&FxHashSet<String>, &FxHashSet<String>),
    unused_exports: &FxHashMap<PathBuf, FxHashSet<String>>,
    re_exports: &ReExportIndex<'_>,
    exports_now: &ExportsNow,
) -> Result<(), PostEditViolation> {
    let unused = unused_exports.get(path);
    let mut removed: Vec<&String> = before.difference(after).collect();
    removed.sort();
    for name in removed {
        let found_unused = unused.is_some_and(|names| names.contains(name));
        let re_exported = re_exports
            .get(&(path, name.as_str()))
            .is_some_and(|readers| {
                readers.iter().any(|re_export| {
                    if re_export.path == path {
                        after.contains(&re_export.exported_name)
                    } else {
                        exports_now
                            .get(&re_export.path)
                            .is_none_or(|exports| exports.contains(&re_export.exported_name))
                    }
                })
            });
        if !found_unused || re_exported {
            return Err(PostEditViolation::ExportInUse { name: name.clone() });
        }
    }
    Ok(())
}

/// The 1-based lines that the edits of `group` touch. A deletion that starts
/// at the line ending before a line touches that line, not the line before.
fn group_lines(snapshot: &SourceSnapshot, group: usize) -> Vec<usize> {
    let content = snapshot.content.as_str();
    let line_ending = snapshot.meta.line_ending;
    let line_of = |offset: usize| {
        content
            .get(..offset)
            .map_or(0, |before| before.matches('\n').count())
            + 1
    };
    let mut lines = Vec::new();
    for (_, edit) in snapshot.edits.iter().filter(|(number, _)| *number == group) {
        let mut start = edit.start;
        if edit.replacement.is_empty()
            && content
                .get(start..edit.end)
                .is_some_and(|text| text.len() > line_ending.len() && text.starts_with(line_ending))
        {
            start += line_ending.len();
        }
        let last = edit.end.saturating_sub(1).max(start);
        lines.extend(line_of(start)..=line_of(last));
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// The names of the exports that the analysis found unused, by absolute path.
pub(in crate::fix) fn unused_export_names(
    results: &fallow_types::results::AnalysisResults,
) -> FxHashMap<PathBuf, FxHashSet<String>> {
    let mut names: FxHashMap<PathBuf, FxHashSet<String>> = FxHashMap::default();
    for finding in &results.unused_exports {
        names
            .entry(finding.export.path.clone())
            .or_default()
            .insert(finding.export.export_name.clone());
    }
    names
}

/// The fix entry types that a source edit group produces.
const SOURCE_FIX_TYPES: [&str; 3] = ["remove_export", "remove_enum_member", "remove_class_member"];

/// Remove the fix entries of each dropped group, so that the output lists
/// only the edits that the plan writes. For a human format, print one line per
/// dropped group on stderr.
pub(in crate::fix) fn withdraw_dropped_fixes(
    root: &Path,
    fixes: &mut Vec<serde_json::Value>,
    dropped: &[DroppedEdit],
    output: fallow_config::OutputFormat,
) {
    for edit in dropped {
        let relative = edit
            .path
            .strip_prefix(root)
            .unwrap_or(&edit.path)
            .to_string_lossy()
            .replace('\\', "/");
        if !matches!(output, fallow_config::OutputFormat::Json) {
            let line = edit.lines.first().copied().unwrap_or(1);
            eprintln!(
                "Kept {relative}:{line} as it is, so the planned edit listed above for this line does not happen: {}. Edit the file by hand, then run `fallow fix --dry-run` again.",
                edit.violation.describe(),
            );
        }
        fixes.retain(|fix| {
            let source_fix = fix
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| SOURCE_FIX_TYPES.contains(&kind));
            let in_group = fix.get("path").and_then(serde_json::Value::as_str)
                == Some(relative.as_str())
                && fix
                    .get("line")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|line| usize::try_from(line).ok())
                    .is_some_and(|line| edit.lines.contains(&line));
            !(source_fix && in_group && fix.get("skipped").is_none())
        });
    }
}
