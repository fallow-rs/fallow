use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;

use super::enum_helpers::{
    EnumDeclarationRange, declares_exported_enum, removable_exported_enum_range,
};
use super::plan::{CapturedHashes, FixPlan, read_source_with_hash_check, stage_fixed_content};

pub(super) struct EnumMemberFix {
    line_idx: usize,
    member_name: String,
    parent_name: String,
}

struct FoldedEnum {
    parent_name: String,
    decl_line: usize,
    range: EnumDeclarationRange,
}

/// Locate `export enum <name>` (allowing `const` / `declare` modifiers) in
/// the file's source lines. Returns the line index of the declaration.
fn find_enum_declaration_line(lines: &[&str], enum_name: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| declares_exported_enum(line, enum_name))
}

/// Returns true if removing every member name in `removed_members` from the
/// enum body would leave the body entirely free of member declarations.
/// Comments and blank lines do not count as remaining content.
fn enum_body_drained_after_removal(
    lines: &[&str],
    range: EnumDeclarationRange,
    removed_members: &[&str],
) -> bool {
    if range.start_line == range.end_line {
        let line = lines[range.start_line];
        let Some(open) = line.find('{') else {
            return false;
        };
        let Some(close) = line.rfind('}') else {
            return false;
        };
        if open >= close {
            return false;
        }
        line[open + 1..close]
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .all(|spec| {
                let ident = spec.split('=').next().unwrap_or(spec).trim();
                removed_members.contains(&ident)
            })
    } else {
        (range.start_line + 1..range.end_line).all(|i| {
            let trimmed = lines[i].trim();
            if trimmed.is_empty()
                || trimmed.starts_with("//")
                || trimmed.starts_with('*')
                || trimmed.starts_with("/*")
            {
                return true;
            }
            let token = trimmed
                .split(|c: char| c == ',' || c == '=' || c.is_whitespace())
                .next()
                .unwrap_or("");
            !token.is_empty() && removed_members.contains(&token)
        })
    }
}

/// Determine which enums in the file should have their entire declaration
/// removed because every member is in the fix list. Each entry corresponds to
/// one folded enum; per-member edits for these enums are skipped in favour of
/// a single whole-block delete.
fn detect_folded_enums(lines: &[&str], member_fixes: &[EnumMemberFix]) -> Vec<FoldedEnum> {
    let mut by_parent: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for fix in member_fixes {
        by_parent
            .entry(&fix.parent_name)
            .or_default()
            .push(&fix.member_name);
    }

    let mut folded = Vec::new();
    for (parent_name, member_names) in &by_parent {
        let Some(decl_line) = find_enum_declaration_line(lines, parent_name) else {
            continue;
        };
        let Some(range) = removable_exported_enum_range(lines, decl_line, parent_name) else {
            continue;
        };
        if !enum_body_drained_after_removal(lines, range, member_names) {
            continue;
        }
        folded.push(FoldedEnum {
            parent_name: (*parent_name).to_string(),
            decl_line,
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
/// Removes unused enum members from their declarations. Handles:
/// - Multi-line enums: removes the entire line containing the member
/// - Single-line enums: removes the member token from the line
/// - Trailing commas: cleans up when the last member is removed
/// - All members removed: leaves the enum body empty (`enum Foo {}`)
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
    for (path, file_members) in members_by_file {
        let Some((content, meta)) = read_source_with_hash_check(root, path, hashes, plan) else {
            continue;
        };
        let lines: Vec<&str> = content.split(meta.line_ending).collect();

        let member_fixes = collect_enum_member_fixes(&lines, file_members);
        if member_fixes.is_empty() {
            continue;
        }

        let relative = path.strip_prefix(root).unwrap_or(path);

        let folded = detect_folded_enums(&lines, &member_fixes);
        let folded_parents: rustc_hash::FxHashSet<&str> =
            folded.iter().map(|f| f.parent_name.as_str()).collect();

        if dry_run {
            record_enum_member_dry_run(
                &member_fixes,
                &folded,
                &folded_parents,
                relative,
                output,
                fixes,
            );
        } else {
            let mut new_lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
            let mut apply_ctx = EnumMemberApplyContext {
                path,
                relative,
                output,
                fixes,
            };
            apply_enum_member_file_fixes(
                &mut new_lines,
                &member_fixes,
                &folded,
                &folded_parents,
                &mut apply_ctx,
            );
            stage_fixed_content(plan, path, &new_lines, &meta, &content);
        }
    }
}

/// Build the sorted, deduped per-line enum-member fix list for one file from
/// the unused-member findings whose reported line actually contains the
/// member name. Sorted descending by line index so later in-place edits do
/// not shift earlier indices.
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

        let line = lines[line_idx];
        if !line.contains(&member.member_name) {
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

fn record_enum_member_dry_run(
    member_fixes: &[EnumMemberFix],
    folded: &[FoldedEnum],
    folded_parents: &rustc_hash::FxHashSet<&str>,
    relative: &Path,
    output: OutputFormat,
    fixes: &mut Vec<serde_json::Value>,
) {
    for fix in member_fixes {
        if folded_parents.contains(fix.parent_name.as_str()) {
            continue;
        }
        if !matches!(output, OutputFormat::Json) {
            eprintln!(
                "Would remove enum member from {}:{} `{}.{}`",
                relative.display(),
                fix.line_idx + 1,
                fix.parent_name,
                fix.member_name,
            );
        }
        fixes.push(serde_json::json!({
            "type": "remove_enum_member",
            "path": relative.display().to_string(),
            "line": fix.line_idx + 1,
            "parent": fix.parent_name,
            "name": fix.member_name,
        }));
    }
    for fold in folded {
        if !matches!(output, OutputFormat::Json) {
            eprintln!(
                "Would remove enum declaration from {}:{} `{}` (every member is unused; \
                 importers in other files will need cleanup, run your TypeScript build to find them)",
                relative.display(),
                fold.decl_line + 1,
                fold.parent_name,
            );
        }
        fixes.push(serde_json::json!({
            "type": "remove_export",
            "path": relative.display().to_string(),
            "line": fold.decl_line + 1,
            "name": fold.parent_name,
        }));
    }
}

struct EnumMemberApplyContext<'a> {
    path: &'a Path,
    relative: &'a Path,
    output: OutputFormat,
    fixes: &'a mut Vec<serde_json::Value>,
}

fn apply_enum_member_file_fixes(
    new_lines: &mut Vec<String>,
    member_fixes: &[EnumMemberFix],
    folded: &[FoldedEnum],
    folded_parents: &rustc_hash::FxHashSet<&str>,
    ctx: &mut EnumMemberApplyContext<'_>,
) {
    let mut lines_to_delete = enum_member_lines_to_delete(new_lines, member_fixes, folded_parents);
    for fold in folded {
        lines_to_delete.extend(fold.range.start_line..=fold.range.end_line);
    }
    lines_to_delete.sort_unstable();
    lines_to_delete.dedup();
    for &idx in lines_to_delete.iter().rev() {
        new_lines.remove(idx);
    }

    record_applied_enum_member_fixes(&mut AppliedEnumMemberRecordInput {
        member_fixes,
        folded,
        folded_parents,
        path: ctx.path,
        relative: ctx.relative,
        output: ctx.output,
        fixes: ctx.fixes,
    });
}

fn enum_member_lines_to_delete(
    new_lines: &mut [String],
    member_fixes: &[EnumMemberFix],
    folded_parents: &rustc_hash::FxHashSet<&str>,
) -> Vec<usize> {
    let mut lines_to_delete = Vec::new();
    for fix in member_fixes {
        if folded_parents.contains(fix.parent_name.as_str()) {
            continue;
        }
        let line = &new_lines[fix.line_idx];
        if line.contains('{') && line.contains('}') {
            new_lines[fix.line_idx] = remove_member_from_single_line(line, &fix.member_name);
        } else {
            new_lines[fix.line_idx] = String::new();
            lines_to_delete.push(fix.line_idx);
        }
    }
    lines_to_delete
}

struct AppliedEnumMemberRecordInput<'a> {
    member_fixes: &'a [EnumMemberFix],
    folded: &'a [FoldedEnum],
    folded_parents: &'a rustc_hash::FxHashSet<&'a str>,
    path: &'a Path,
    relative: &'a Path,
    output: OutputFormat,
    fixes: &'a mut Vec<serde_json::Value>,
}

fn record_applied_enum_member_fixes(input: &mut AppliedEnumMemberRecordInput<'_>) {
    let target = input.path.display().to_string();
    for fix in input.member_fixes {
        if input.folded_parents.contains(fix.parent_name.as_str()) {
            continue;
        }
        input.fixes.push(serde_json::json!({
            "type": "remove_enum_member",
            "path": input.relative.display().to_string(),
            "line": fix.line_idx + 1,
            "parent": fix.parent_name,
            "name": fix.member_name,
            "applied": true,
            "__target": target,
        }));
    }
    for fold in input.folded {
        if !matches!(input.output, OutputFormat::Json) {
            eprintln!(
                "Removed unused enum `{}` from {}; importers in other files will need cleanup, run your TypeScript build to find them.",
                fold.parent_name,
                input.relative.display(),
            );
        }
        input.fixes.push(serde_json::json!({
            "type": "remove_export",
            "path": input.relative.display().to_string(),
            "line": fold.decl_line + 1,
            "name": fold.parent_name,
            "applied": true,
            "__target": target,
        }));
    }
}

/// Remove a single member from a single-line enum like `enum Foo { A, B, C }`.
///
/// Returns the modified line with the member removed and commas cleaned up.
fn remove_member_from_single_line(line: &str, member_name: &str) -> String {
    let Some(open) = line.find('{') else {
        return line.to_string();
    };
    let Some(close) = line.rfind('}') else {
        return line.to_string();
    };
    if open >= close {
        return line.to_string();
    }

    let prefix = &line[..=open];
    let suffix = &line[close..];
    let inner = &line[open + 1..close];

    let parts: Vec<&str> = inner.split(',').collect();

    let filtered: Vec<String> = parts
        .iter()
        .filter(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return false;
            }
            let ident = trimmed.split('=').next().unwrap_or(trimmed).trim();
            ident != member_name
        })
        .map(|part| part.trim().to_string())
        .collect();

    if filtered.is_empty() {
        format!("{}{}", prefix.trim_end(), suffix.trim_start())
    } else {
        let members_str = filtered.join(", ");
        format!("{prefix} {members_str} {suffix}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::extract::MemberKind;
    use fallow_types::results::UnusedMember;

    fn make_enum_member(path: &Path, parent: &str, name: &str, line: u32) -> UnusedMember {
        UnusedMember {
            path: path.to_path_buf(),
            parent_name: parent.to_string(),
            member_name: name.to_string(),
            kind: MemberKind::EnumMember,
            line,
            col: 0,
        }
    }

    fn fix_single_member(
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
        let mut plan = FixPlan::new();
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

    /// Helper mirrored from `exports.rs`. The fix tests need the
    /// captured-hashes map to be populated for every file the test
    /// considers freshly analyzed.
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
        assert_eq!(content, "export enum Status {\n  Active,\n  Pending,\n}\n");
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
        let mut plan = FixPlan::new();
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
        assert_eq!(content, "export enum Status {\n  Inactive,\n}\n");
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
        let mut plan = FixPlan::new();
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
        assert_eq!(content, "enum Status { Active, Pending }\n");
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
        let mut plan = FixPlan::new();
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
            "export enum Status {\r\n  Active,\r\n  Pending,\r\n}\r\n"
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
            "    export enum Status {\n        Inactive,\n    }\n"
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
    fn single_line_remove_first_member() {
        let result = remove_member_from_single_line("enum Foo { A, B, C }", "A");
        assert_eq!(result, "enum Foo { B, C }");
    }

    #[test]
    fn single_line_remove_middle_member() {
        let result = remove_member_from_single_line("enum Foo { A, B, C }", "B");
        assert_eq!(result, "enum Foo { A, C }");
    }

    #[test]
    fn single_line_remove_last_member() {
        let result = remove_member_from_single_line("enum Foo { A, B, C }", "C");
        assert_eq!(result, "enum Foo { A, B }");
    }

    #[test]
    fn single_line_remove_only_member() {
        let result = remove_member_from_single_line("enum Foo { A }", "A");
        assert_eq!(result, "enum Foo {}");
    }

    #[test]
    fn single_line_remove_member_with_value() {
        let result = remove_member_from_single_line("enum Foo { A = 1, B = 2, C = 3 }", "B");
        assert_eq!(result, "enum Foo { A = 1, C = 3 }");
    }

    #[test]
    fn single_line_remove_member_with_string_value() {
        let result = remove_member_from_single_line("enum Foo { A = \"a\", B = \"b\" }", "A");
        assert_eq!(result, "enum Foo { B = \"b\" }");
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
        let mut plan = FixPlan::new();
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
        assert_eq!(content, "enum Status { A, C }\n");
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
        assert_eq!(content, "enum Status {\n  Inactive,\n  Pending,\n}\n");
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
    fn enum_fix_single_line_with_trailing_comma() {
        let result = remove_member_from_single_line("enum Foo { A, B, C, }", "B");
        assert_eq!(result, "enum Foo { A, C }");
    }

    #[test]
    fn enum_fix_single_line_no_braces() {
        let result = remove_member_from_single_line("enum Foo A, B, C", "B");
        assert_eq!(result, "enum Foo A, B, C");
    }

    #[test]
    fn enum_fix_single_line_close_before_open() {
        let result = remove_member_from_single_line("} enum Foo { A }", "A");
        assert!(!result.is_empty());
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
            "const enum Direction {\n  Up,\n  Down,\n  Right,\n}\n"
        );
    }

    #[test]
    fn single_line_remove_member_preserves_export_keyword() {
        let result =
            remove_member_from_single_line("export enum Status { Active, Inactive }", "Active");
        assert_eq!(result, "export enum Status { Inactive }");
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
        let mut plan = FixPlan::new();
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
            "export enum Status {\n  Inactive,\n  Pending,\n}\n"
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
