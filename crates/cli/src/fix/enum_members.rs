use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;
use fallow_engine::source_edits::SourceEditIndex;

use super::enum_helpers::{EnumDeclarationRange, removable_exported_enum_range};
use super::plan::{CapturedHashes, FixPlan, skip_generated_source};
use super::source_edits::{QueueOutcome, SourceEdit, line_deletions, read_source_snapshot};

struct EnumMemberFix {
    line_idx: usize,
    member_name: String,
    parent_name: String,
}

struct FoldedEnum {
    parent_name: String,
    decl_line: usize,
    range: EnumDeclarationRange,
}

/// Determine which enums in the file should have their entire declaration
/// removed because every member is in the fix list. Each entry corresponds to
/// one folded enum; per-member edits for these enums are skipped in favour of
/// a single whole-block delete.
fn detect_folded_enums(
    index: &SourceEditIndex,
    lines: &[&str],
    member_fixes: &[EnumMemberFix],
) -> Vec<FoldedEnum> {
    let mut by_parent: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for fix in member_fixes {
        by_parent
            .entry(&fix.parent_name)
            .or_default()
            .push(&fix.member_name);
    }

    let mut folded = Vec::new();
    for (parent_name, member_names) in &by_parent {
        let Some(range) = removable_exported_enum_range(index, lines, parent_name) else {
            continue;
        };
        let in_range = |offset: u32| {
            let line = index.line_range(offset, offset + 1).0;
            (range.start_line..=range.end_line).contains(&line)
        };
        let drained = index
            .enums_named(parent_name)
            .find(|enumeration| in_range(enumeration.body_start))
            .is_some_and(|enumeration| {
                enumeration.complete
                    && enumeration
                        .members
                        .iter()
                        .all(|(member, _)| member_names.contains(&member.as_str()))
            });
        if !drained {
            continue;
        }
        folded.push(FoldedEnum {
            parent_name: (*parent_name).to_string(),
            decl_line: range.start_line,
            range,
        });
    }
    folded
}

/// Inputs for [`apply_enum_member_fixes`], bundled so the entry point takes one
/// parameter struct instead of seven (mirrors the `*FixInput` convention used
/// by the dependency and export fixers in this module).
pub(super) struct EnumMemberFixInput<'a, 'member> {
    pub(super) root: &'a Path,
    pub(super) members_by_file:
        &'a FxHashMap<PathBuf, Vec<&'member fallow_types::results::UnusedMember>>,
    pub(super) hashes: &'a CapturedHashes,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

/// Apply enum member fixes to source files, returning JSON fix entries.
///
/// The edits come from the syntax spans of the members
/// (`SourceEditIndex::remove_enum_members`). A member that owns its lines is
/// deleted with its lines, also an initializer over several lines. A member
/// that shares a line is deleted with one separating comma, and each other
/// byte of the enum stays as it is. A kept member that counts on from a
/// removed member gets an initializer with its old value, so no kept value
/// changes. When that old value is not a known integer, the enum gets no
/// edit and no record. An exported enum whose members are all unused is
/// removed as a whole. A file that does not parse gets no edit.
pub(super) fn apply_enum_member_fixes(input: EnumMemberFixInput<'_, '_>) {
    let EnumMemberFixInput {
        root,
        members_by_file,
        hashes,
        plan,
        output,
        dry_run,
        fixes,
    } = input;
    for (path, file_members) in super::in_path_order(members_by_file) {
        let Some((content, meta)) = read_source_snapshot(root, path, hashes, plan) else {
            continue;
        };
        if skip_generated_source(root, path, &content, plan) {
            continue;
        }
        let lines: Vec<&str> = content.split(meta.line_ending).collect();
        let Some(index) = SourceEditIndex::parse(path, &content) else {
            continue;
        };

        let member_fixes = collect_enum_member_fixes(&lines, file_members);
        if member_fixes.is_empty() {
            continue;
        }

        let relative = path.strip_prefix(root).unwrap_or(path);
        let folded = detect_folded_enums(&index, &lines, &member_fixes);

        let kept = queue_enum_edits(EnumFileEdits {
            path,
            content: &content,
            line_ending: meta.line_ending,
            index: &index,
            member_fixes: &member_fixes,
            folded: &folded,
            plan,
        });

        if dry_run {
            record_enum_member_dry_run(EnumMemberDryRunInput {
                member_fixes: &member_fixes,
                folded: &folded,
                kept: &kept,
                relative,
                output,
                fixes,
            });
        } else {
            record_applied_enum_member_fixes(&mut AppliedEnumMemberRecordInput {
                member_fixes: &member_fixes,
                folded: &folded,
                kept: &kept,
                path,
                relative,
                output,
                fixes,
            });
        }
    }
}

/// The parsed text of one file and its planned enum fixes.
struct EnumFileEdits<'a, 'p> {
    path: &'a Path,
    content: &'a str,
    line_ending: &'a str,
    index: &'a SourceEditIndex,
    member_fixes: &'a [EnumMemberFix],
    folded: &'a [FoldedEnum],
    plan: &'p mut FixPlan,
}

/// Queue one group of edits for each folded enum and for each enum with
/// member fixes, and return the fixes whose group went in.
fn queue_enum_edits<'a>(input: EnumFileEdits<'a, '_>) -> KeptEnumFixes<'a> {
    let EnumFileEdits {
        path,
        content,
        line_ending,
        index,
        member_fixes,
        folded,
        plan,
    } = input;
    let mut parents: Vec<&str> = input
        .member_fixes
        .iter()
        .map(|fix| fix.parent_name.as_str())
        .collect();
    parents.sort_unstable();
    parents.dedup();
    let mut kept: KeptEnumFixes<'a> = rustc_hash::FxHashSet::default();
    for parent in parents {
        if let Some(fold) = folded.iter().find(|fold| fold.parent_name == parent) {
            let lines: Vec<usize> = (fold.range.start_line..=fold.range.end_line).collect();
            let group = line_deletions(content, line_ending, &lines);
            if plan.sources_mut().queue(path, group) == QueueOutcome::Queued {
                kept.insert((parent, None));
            }
            continue;
        }
        for enum_fixes in fixes_by_enum(index, member_fixes, parent) {
            let Some(group) = enum_member_edits(index, parent, &enum_fixes) else {
                continue;
            };
            if plan.sources_mut().queue(path, group) == QueueOutcome::Queued {
                kept.extend(enum_fixes.iter().map(|fix| (parent, Some(fix.line_idx))));
            }
        }
    }
    kept
}

/// The fixes whose queued edits went in: `(parent, None)` for the fold of an
/// enum, `(parent, Some(line))` for one member fix.
type KeptEnumFixes<'a> = rustc_hash::FxHashSet<(&'a str, Option<usize>)>;

/// The member fixes of `parent`, grouped by the enum declaration that covers
/// the line of each fix. Two enums can share a name, for example in two
/// namespaces of one file. A fix on a line that no single enum covers gets
/// no group.
fn fixes_by_enum<'f>(
    index: &SourceEditIndex,
    member_fixes: &'f [EnumMemberFix],
    parent: &str,
) -> Vec<Vec<&'f EnumMemberFix>> {
    let mut groups: Vec<(u32, Vec<&EnumMemberFix>)> = Vec::new();
    for fix in member_fixes.iter().filter(|fix| fix.parent_name == parent) {
        let Ok(line) = u32::try_from(fix.line_idx + 1) else {
            continue;
        };
        let Some(enumeration) = index.enum_on_line(parent, line) else {
            continue;
        };
        match groups
            .iter_mut()
            .find(|(body_start, _)| *body_start == enumeration.body_start)
        {
            Some((_, group)) => group.push(fix),
            None => groups.push((enumeration.body_start, vec![fix])),
        }
    }
    groups.into_iter().map(|(_, group)| group).collect()
}

/// The byte edits that remove the members of `enum_fixes` from their enum.
/// `None` when the index refuses the edit.
fn enum_member_edits(
    index: &SourceEditIndex,
    parent: &str,
    enum_fixes: &[&EnumMemberFix],
) -> Option<Vec<SourceEdit>> {
    let names: Vec<&str> = enum_fixes
        .iter()
        .map(|fix| fix.member_name.as_str())
        .collect();
    let line = u32::try_from(enum_fixes.first()?.line_idx + 1).ok()?;
    let edits = index.remove_enum_members(parent, &names, line).ok()?;
    Some(
        edits
            .into_iter()
            .map(|edit| SourceEdit {
                start: edit.start as usize,
                end: edit.end as usize,
                replacement: edit.replacement,
            })
            .collect(),
    )
}

/// Build the sorted, deduped enum-member fix list for one file from the
/// unused-member findings inside the file. Sorted descending by line index.
fn collect_enum_member_fixes(
    lines: &[&str],
    file_members: &[&fallow_types::results::UnusedMember],
) -> Vec<EnumMemberFix> {
    let mut member_fixes: Vec<EnumMemberFix> = Vec::new();
    for member in file_members {
        let line_idx = member.line.saturating_sub(1) as usize;
        if line_idx >= lines.len() {
            continue;
        }

        member_fixes.push(EnumMemberFix {
            line_idx,
            member_name: member.member_name.clone(),
            parent_name: member.parent_name.clone(),
        });
    }

    member_fixes.sort_by(|a, b| {
        b.line_idx
            .cmp(&a.line_idx)
            .then_with(|| a.parent_name.cmp(&b.parent_name))
            .then_with(|| a.member_name.cmp(&b.member_name))
    });
    member_fixes.dedup_by(|a, b| {
        a.line_idx == b.line_idx && a.parent_name == b.parent_name && a.member_name == b.member_name
    });
    member_fixes
}

struct EnumMemberDryRunInput<'a, 'b> {
    member_fixes: &'a [EnumMemberFix],
    folded: &'a [FoldedEnum],
    kept: &'a KeptEnumFixes<'a>,
    relative: &'a Path,
    output: OutputFormat,
    fixes: &'b mut Vec<serde_json::Value>,
}

fn record_enum_member_dry_run(input: EnumMemberDryRunInput<'_, '_>) {
    let fixes = input.fixes;
    let relative_display = input.relative.to_string_lossy().replace('\\', "/");
    let folded = |parent: &str| input.folded.iter().any(|fold| fold.parent_name == parent);
    for fix in input.member_fixes {
        if folded(&fix.parent_name)
            || !input
                .kept
                .contains(&(fix.parent_name.as_str(), Some(fix.line_idx)))
        {
            continue;
        }
        if !matches!(input.output, OutputFormat::Json) {
            eprintln!(
                "Would remove enum member from {relative_display}:{} `{}.{}`",
                fix.line_idx + 1,
                fix.parent_name,
                fix.member_name,
            );
        }
        fixes.push(serde_json::json!({
            "type": "remove_enum_member",
            "path": relative_display,
            "line": fix.line_idx + 1,
            "parent": fix.parent_name,
            "name": fix.member_name,
        }));
    }
    for fold in input.folded {
        if !input.kept.contains(&(fold.parent_name.as_str(), None)) {
            continue;
        }
        if !matches!(input.output, OutputFormat::Json) {
            eprintln!(
                "Would remove enum declaration from {relative_display}:{} `{}` (every member is unused; \
                 importers in other files will need cleanup, run your TypeScript build to find them)",
                fold.decl_line + 1,
                fold.parent_name,
            );
        }
        fixes.push(serde_json::json!({
            "type": "remove_export",
            "path": relative_display,
            "line": fold.decl_line + 1,
            "name": fold.parent_name,
        }));
    }
}

struct AppliedEnumMemberRecordInput<'a> {
    member_fixes: &'a [EnumMemberFix],
    folded: &'a [FoldedEnum],
    kept: &'a KeptEnumFixes<'a>,
    path: &'a Path,
    relative: &'a Path,
    output: OutputFormat,
    fixes: &'a mut Vec<serde_json::Value>,
}

fn record_applied_enum_member_fixes(input: &mut AppliedEnumMemberRecordInput<'_>) {
    let target = input.path.display().to_string();
    let relative_display = input.relative.to_string_lossy().replace('\\', "/");
    let folded = |parent: &str| input.folded.iter().any(|fold| fold.parent_name == parent);
    for fix in input.member_fixes {
        if folded(&fix.parent_name)
            || !input
                .kept
                .contains(&(fix.parent_name.as_str(), Some(fix.line_idx)))
        {
            continue;
        }
        input.fixes.push(serde_json::json!({
            "type": "remove_enum_member",
            "path": relative_display,
            "line": fix.line_idx + 1,
            "parent": fix.parent_name,
            "name": fix.member_name,
            "applied": true,
            "__target": target,
        }));
    }
    for fold in input.folded {
        if !input.kept.contains(&(fold.parent_name.as_str(), None)) {
            continue;
        }
        if !matches!(input.output, OutputFormat::Json) {
            eprintln!(
                "Removed unused enum `{}` from {relative_display}; importers in other files will need cleanup, run your TypeScript build to find them.",
                fold.parent_name,
            );
        }
        input.fixes.push(serde_json::json!({
            "type": "remove_export",
            "path": relative_display,
            "line": fold.decl_line + 1,
            "name": fold.parent_name,
            "applied": true,
            "__target": target,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::extract::MemberKind;
    use fallow_types::results::UnusedMember;

    pub(super) fn make_enum_member(
        path: &Path,
        parent: &str,
        name: &str,
        line: u32,
    ) -> UnusedMember {
        UnusedMember {
            path: path.to_path_buf(),
            parent_name: parent.to_string(),
            member_name: name.to_string(),
            kind: MemberKind::EnumMember,
            line,
            col: 0,
        }
    }

    pub(super) fn fix_single_member(
        root: &Path,
        file: &Path,
        enum_name: &str,
        member_name: &str,
        line: u32,
        dry_run: bool,
    ) -> Vec<serde_json::Value> {
        let member = make_enum_member(file, enum_name, member_name, line);
        let mut map: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        map.insert(file.to_path_buf(), vec![&member]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &map,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run,
            fixes: &mut fixes,
        });
        if !dry_run {
            let _ = plan.commit();
        }
        fixes
    }

    #[test]
    fn enum_member_fixes_follow_path_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let files: Vec<PathBuf> = ["h", "c", "f", "a", "g", "b", "e", "d"]
            .iter()
            .map(|name| {
                let file = root.join(format!("{name}.ts"));
                std::fs::write(&file, "enum Status {\n  Active,\n  Unused,\n}\n").unwrap();
                file
            })
            .collect();
        let members: Vec<UnusedMember> = files
            .iter()
            .map(|file| make_enum_member(file, "Status", "Unused", 3))
            .collect();
        let mut map: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        for member in &members {
            map.insert(member.path.clone(), vec![member]);
        }
        let file_refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &map,
            hashes: &capture_hashes(&file_refs),
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: true,
            fixes: &mut fixes,
        });

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

    /// Helper mirrored from `exports.rs`. The fix tests need the
    /// captured-hashes map to be populated for every file the test
    /// considers freshly analyzed.
    pub(super) fn capture_hashes(paths: &[&Path]) -> CapturedHashes {
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

    #[test]
    fn enum_fix_removes_single_member_from_multi_member_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active,\n  Inactive,\n  Pending,\n}\n",
        )
        .unwrap();

        let fixes = fix_single_member(root, &file, "Status", "Inactive", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "export enum Status {\n  Active,\n  Pending = 2,\n}\n"
        );
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
        assert_eq!(fixes[0]["parent"], "Status");
        assert_eq!(fixes[0]["name"], "Inactive");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn enum_fix_removes_multiple_members_from_same_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active,\n  Inactive,\n  Pending,\n}\n",
        )
        .unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let m2 = make_enum_member(&file, "Status", "Pending", 4);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export enum Status {\n  Inactive = 1,\n}\n");
        assert_eq!(fixes.len(), 2);
    }

    #[test]
    fn enum_fix_folds_when_every_member_of_exported_enum_unused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "export enum Status {\n  Active,\n  Inactive,\n}\n").unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let m2 = make_enum_member(&file, "Status", "Inactive", 3);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert_eq!(fixes[0]["name"], "Status");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn enum_fix_handles_members_with_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active = \"active\",\n  Inactive = \"inactive\",\n  Pending = 2,\n}\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Status", "Inactive", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "export enum Status {\n  Active = \"active\",\n  Pending = 2,\n}\n"
        );
    }

    #[test]
    fn enum_fix_single_line_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status { Active, Inactive, Pending }\n").unwrap();

        fix_single_member(root, &file, "Status", "Inactive", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status { Active, Pending = 2 }\n");
    }

    #[test]
    fn enum_fix_single_line_removes_all_members() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status { Active }\n").unwrap();

        fix_single_member(root, &file, "Status", "Active", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status {}\n");
    }

    #[test]
    fn enum_fix_single_line_with_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "enum Status { Active = \"active\", Inactive = \"inactive\" }\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Status", "Active", 1, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status { Inactive = \"inactive\" }\n");
    }

    #[test]
    fn enum_fix_dry_run_does_not_modify_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        let original = "export enum Status {\n  Active,\n  Inactive,\n}\n";
        std::fs::write(&file, original).unwrap();

        let member = make_enum_member(&file, "Status", "Active", 2);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&member]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: true,
            fixes: &mut fixes,
        });

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
        assert_eq!(fixes[0]["name"], "Active");
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn enum_fix_preserves_crlf_line_endings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\r\n  Active,\r\n  Inactive,\r\n  Pending,\r\n}\r\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Status", "Inactive", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "export enum Status {\r\n  Active,\r\n  Pending = 2,\r\n}\r\n"
        );
    }

    #[test]
    fn enum_fix_preserves_indentation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "    export enum Status {\n        Active,\n        Inactive,\n    }\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Status", "Active", 2, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "    export enum Status {\n        Inactive = 1,\n    }\n"
        );
    }

    #[test]
    fn enum_fix_skips_path_outside_project_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside_file = dir.path().join("outside.ts");
        let original = "enum Status {\n  Active,\n  Inactive,\n}\n";
        std::fs::write(&outside_file, original).unwrap();

        let fixes = fix_single_member(&root, &outside_file, "Status", "Active", 2, false);

        assert_eq!(std::fs::read_to_string(&outside_file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn enum_fix_skips_line_without_member_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        let original = "enum Status {\n  Active,\n  Inactive,\n}\n";
        std::fs::write(&file, original).unwrap();

        let fixes = fix_single_member(root, &file, "Status", "Missing", 2, false);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn enum_fix_skips_out_of_bounds_line() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        let original = "enum Status {\n  Active,\n}\n";
        std::fs::write(&file, original).unwrap();

        let fixes = fix_single_member(root, &file, "Status", "Active", 999, false);

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert!(fixes.is_empty());
    }

    #[test]
    fn enum_fix_removes_last_member_of_multi_line_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status {\n  Active,\n  Inactive,\n}\n").unwrap();

        fix_single_member(root, &file, "Status", "Inactive", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status {\n  Active,\n}\n");
    }

    #[test]
    fn enum_fix_handles_numeric_values() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("priority.ts");
        std::fs::write(
            &file,
            "enum Priority {\n  Low = 0,\n  Medium = 1,\n  High = 2,\n}\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Priority", "Medium", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Priority {\n  Low = 0,\n  High = 2,\n}\n");
    }

    #[test]
    fn single_line_remove_two_members_sequentially() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status { A, B, C, D }\n").unwrap();

        let m1 = make_enum_member(&file, "Status", "B", 1);
        let m2 = make_enum_member(&file, "Status", "D", 1);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status { A, C = 2 }\n");
        assert_eq!(fixes.len(), 2);
        assert!(fixes.iter().any(|fix| fix["name"] == "B"));
        assert!(fixes.iter().any(|fix| fix["name"] == "D"));
    }

    #[test]
    fn enum_fix_removes_first_member_of_multi_line_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "enum Status {\n  Active,\n  Inactive,\n  Pending,\n}\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Status", "Active", 2, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status {\n  Inactive = 1,\n  Pending,\n}\n");
    }

    #[test]
    fn enum_fix_keeps_the_value_of_each_kept_member() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("level.ts");
        std::fs::write(
            &file,
            "export enum Level {\n  Low,\n  Medium,\n  High,\n}\n",
        )
        .unwrap();

        let fixes = fix_single_member(root, &file, "Level", "Low", 2, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export enum Level {\n  Medium = 1,\n  High,\n}\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
        assert_eq!(fixes[0]["name"], "Low");
        assert_eq!(fixes[0]["applied"], true);
    }

    #[test]
    fn enum_fix_nonexistent_file_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("missing.ts"); // Does not exist

        let fixes = fix_single_member(root, &file, "Status", "Active", 2, false);

        assert!(fixes.is_empty());
    }

    #[test]
    fn enum_fix_member_with_computed_value() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("computed.ts");
        std::fs::write(
            &file,
            "enum Bits {\n  A = 1 << 0,\n  B = 1 << 1,\n  C = 1 << 2,\n}\n",
        )
        .unwrap();

        fix_single_member(root, &file, "Bits", "B", 3, false);

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Bits {\n  A = 1 << 0,\n  C = 1 << 2,\n}\n");
    }

    #[test]
    fn enum_fix_returns_relative_path_in_json() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("src").join("status.ts");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(&file, "enum Status {\n  Active,\n  Inactive,\n}\n").unwrap();

        let member = make_enum_member(&file, "Status", "Active", 2);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&member]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let path_str = fixes[0]["path"].as_str().unwrap().replace('\\', "/");
        assert_eq!(path_str, "src/status.ts");
    }

    #[test]
    fn dry_run_enum_fix_with_human_output() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        let original = "enum Status {\n  Active,\n  Inactive,\n}\n";
        std::fs::write(&file, original).unwrap();

        let member = make_enum_member(&file, "Status", "Active", 2);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&member]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: true,
            fixes: &mut fixes,
        });

        assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn enum_fix_line_zero_saturating_sub() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status { Active }\n").unwrap();

        let member = make_enum_member(&file, "Status", "Active", 0);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&member]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status {}\n");
    }

    #[test]
    fn enum_fix_const_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("direction.ts");
        std::fs::write(
            &file,
            "const enum Direction {\n  Up,\n  Down,\n  Left,\n  Right,\n}\n",
        )
        .unwrap();

        let member = make_enum_member(&file, "Direction", "Left", 4);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&member]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "const enum Direction {\n  Up,\n  Down,\n  Right = 3,\n}\n"
        );
    }

    #[test]
    fn fold_does_not_fire_when_only_some_members_are_unused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active,\n  Inactive,\n  Pending,\n}\n",
        )
        .unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "export enum Status {\n  Inactive = 1,\n  Pending,\n}\n"
        );
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
    }

    #[test]
    fn fold_fires_on_single_line_exported_enum_with_all_members_unused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "export enum Status { Active, Inactive }\n").unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 1);
        let m2 = make_enum_member(&file, "Status", "Inactive", 1);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert_eq!(fixes[0]["name"], "Status");
    }

    #[test]
    fn fold_does_not_fire_when_enum_name_is_used_locally() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(
            &file,
            "export enum Status {\n  Active,\n  Inactive,\n}\nconsole.log(typeof Status);\n",
        )
        .unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let m2 = make_enum_member(&file, "Status", "Inactive", 3);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            content,
            "export enum Status {\n}\nconsole.log(typeof Status);\n"
        );
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
    }

    #[test]
    fn fold_dry_run_emits_remove_export_not_remove_enum_member() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "export enum Status {\n  Active,\n  Inactive,\n}\n").unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let m2 = make_enum_member(&file, "Status", "Inactive", 3);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: true,
            fixes: &mut fixes,
        });

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "export enum Status {\n  Active,\n  Inactive,\n}\n");
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0]["type"], "remove_export");
        assert_eq!(fixes[0]["name"], "Status");
        assert!(fixes[0].get("applied").is_none());
    }

    #[test]
    fn enum_fix_reports_no_fold_for_an_enum_that_a_queued_edit_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("e.ts");
        std::fs::write(&file, "export enum E {\n  A,\n  B,\n}\n").unwrap();
        let m1 = make_enum_member(&file, "E", "A", 2);
        let m2 = make_enum_member(&file, "E", "B", 3);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);
        let hashes = capture_hashes(&[&file]);

        for dry_run in [true, false] {
            let mut plan = FixPlan::for_root(root).unwrap();
            let (content, _) = read_source_snapshot(root, &file, &hashes, &mut plan).unwrap();
            let deletion = line_deletions(&content, "\n", &[0, 1, 2, 3]);
            assert_eq!(
                plan.sources_mut().queue(&file, deletion),
                QueueOutcome::Queued
            );
            let mut fixes = Vec::new();
            apply_enum_member_fixes(EnumMemberFixInput {
                root,
                members_by_file: &members_by_file,
                hashes: &hashes,
                plan: &mut plan,
                output: OutputFormat::Json,
                dry_run,
                fixes: &mut fixes,
            });
            assert!(fixes.is_empty(), "dry_run {dry_run}: {fixes:?}");
            if !dry_run {
                assert!(plan.commit().failed.is_empty());
                assert_eq!(std::fs::read_to_string(&file).unwrap(), "\n");
            }
        }
    }

    #[test]
    fn enum_fix_folds_two_neighbour_enums_at_the_end_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("e.ts");
        std::fs::write(&file, "export enum A {\n  a,\n}\nexport enum B {\n  b,\n}").unwrap();
        let m1 = make_enum_member(&file, "A", "a", 2);
        let m2 = make_enum_member(&file, "B", "b", 5);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);
        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &capture_hashes(&[&file]),
            plan: &mut plan,
            output: OutputFormat::Json,
            dry_run: false,
            fixes: &mut fixes,
        });
        assert!(plan.commit().failed.is_empty());

        let mut names: Vec<&str> = fixes
            .iter()
            .map(|fix| fix["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["A", "B"]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "");
    }

    #[test]
    fn fold_skipped_for_non_exported_enum() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("status.ts");
        std::fs::write(&file, "enum Status {\n  Active,\n  Inactive,\n}\n").unwrap();

        let m1 = make_enum_member(&file, "Status", "Active", 2);
        let m2 = make_enum_member(&file, "Status", "Inactive", 3);
        let mut members_by_file: FxHashMap<PathBuf, Vec<&UnusedMember>> = FxHashMap::default();
        members_by_file.insert(file.clone(), vec![&m1, &m2]);

        let mut fixes = Vec::new();
        let mut plan = FixPlan::for_root(root).unwrap();
        let hashes = capture_hashes(&[&file]);
        apply_enum_member_fixes(EnumMemberFixInput {
            root,
            members_by_file: &members_by_file,
            hashes: &hashes,
            plan: &mut plan,
            output: OutputFormat::Human,
            dry_run: false,
            fixes: &mut fixes,
        });
        let _ = plan.commit();

        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "enum Status {\n}\n");
        assert_eq!(fixes.len(), 2);
        assert_eq!(fixes[0]["type"], "remove_enum_member");
    }
}

#[cfg(test)]
#[path = "enum_members_syntax_tests.rs"]
mod syntax_tests;
