use rustc_hash::{FxHashMap, FxHashSet};
use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;

use super::enum_helpers::{EnumDeclarationRange, removable_exported_enum_range};
use super::plan::{
    CapturedHashes, FixPlan, SkipReason, read_source_with_hash_check, stage_fixed_content,
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
/// incomplete). Returns `None` when the file is high confidence and the
/// fixer should proceed normally. Issue #602.
fn low_confidence_skip_reason(
    relative: &Path,
    absolute: &Path,
    unresolved_import_files: &FxHashSet<PathBuf>,
) -> Option<SkipReason> {
    if is_off_graph_consumer_path(relative) {
        return Some(SkipReason::LowConfidenceOffGraph);
    }
    if unresolved_import_files.contains(absolute) {
        return Some(SkipReason::LowConfidenceUnresolvedImports);
    }
    None
}

pub(super) struct ExportFix {
    line_idx: usize,
    export_name: String,
    enum_declaration: Option<EnumDeclarationRange>,
}

pub(super) struct ExportFixInput<'a, 'export> {
    pub(super) root: &'a Path,
    pub(super) exports_by_file:
        &'a FxHashMap<PathBuf, Vec<&'export fallow_types::results::UnusedExport>>,
    pub(super) hashes: &'a CapturedHashes,
    pub(super) unresolved_import_files: &'a FxHashSet<PathBuf>,
    pub(super) plan: &'a mut FixPlan,
    pub(super) output: OutputFormat,
    pub(super) dry_run: bool,
    pub(super) fixes: &'a mut Vec<serde_json::Value>,
}

/// Check if a line (after stripping `export `) is a named export list like `{ A, B } ...`
fn is_export_list(after_export: &str) -> bool {
    let s = after_export.trim_start();
    let s = if let Some(rest) = s.strip_prefix("type") {
        rest.trim_start()
    } else {
        s
    };
    s.starts_with('{')
}

/// Given a line like `export { A, B, C } from "./mod";` or `export { A, B, C };`,
/// remove the specified specifiers. If all specifiers are removed, returns `None`
/// (meaning the entire line should be deleted). Otherwise returns the updated line.
fn remove_specifiers_from_export_list(line: &str, names_to_remove: &[&str]) -> Option<String> {
    let indent = line.len() - line.trim_start().len();
    let trimmed = line.trim_start();

    let after_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (type_prefix, after_type) = if let Some(rest) = after_export.strip_prefix("type") {
        if rest.trim_start().starts_with('{') {
            ("type ", rest.trim_start())
        } else {
            ("", after_export)
        }
    } else {
        ("", after_export)
    };

    let brace_start = after_type.find('{')?;
    let brace_end = after_type.find('}')?;

    let inside = &after_type[brace_start + 1..brace_end];
    let after_brace = &after_type[brace_end + 1..];

    let remaining: Vec<&str> = inside
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|spec| {
            let exported_name = if let Some((original, _alias)) = spec.split_once(" as ") {
                original.trim()
            } else {
                spec.trim()
            };
            !names_to_remove.contains(&exported_name)
        })
        .collect();

    if remaining.is_empty() {
        None
    } else {
        let prefix = &line[..indent];
        let new_inside = remaining.join(", ");
        Some(format!(
            "{prefix}export {type_prefix}{{ {new_inside} }}{after_brace}"
        ))
    }
}

fn emit_dry_run_export_fix(relative: &Path, fix: &ExportFix) {
    if fix.enum_declaration.is_some() {
        eprintln!(
            "Would remove enum declaration from {}:{} `{}`",
            relative.display(),
            fix.line_idx + 1,
            fix.export_name,
        );
    } else {
        eprintln!(
            "Would remove export from {}:{} `{}`",
            relative.display(),
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
        "path": relative.display().to_string(),
        "line": fix.line_idx + 1,
        "name": fix.export_name,
    });
    if let Some(applied) = applied {
        value["applied"] = serde_json::json!(applied);
        value["__target"] = serde_json::json!(absolute.display().to_string());
    }
    fixes.push(value);
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
/// least one unresolved import. A file in that set, or under an off-graph
/// consumer directory, has its export removals withheld as low confidence
/// (issue #602): the rewrite would risk breaking a consumer fallow's graph
/// cannot see. The skip is recorded on `plan` so the orchestrator surfaces
/// it; the export stays reported by `fallow dead-code`.
pub(super) fn apply_export_fixes(input: &mut ExportFixInput<'_, '_>) {
    let root = input.root;
    let exports_by_file = input.exports_by_file;
    let hashes = input.hashes;
    let unresolved_import_files = input.unresolved_import_files;
    let output = input.output;
    let dry_run = input.dry_run;
    let plan = &mut *input.plan;
    let fixes = &mut *input.fixes;

    for (path, file_exports) in exports_by_file {
        let relative = path.strip_prefix(root).unwrap_or(path);

        if let Some(reason) = low_confidence_skip_reason(relative, path, unresolved_import_files) {
            plan.skip(path.clone(), reason);
            continue;
        }

        let Some((content, meta)) = read_source_with_hash_check(root, path, hashes, plan) else {
            continue;
        };
        let lines: Vec<&str> = content.split(meta.line_ending).collect();

        let mut line_fixes = collect_export_line_fixes(&lines, file_exports);
        if line_fixes.is_empty() {
            continue;
        }

        line_fixes.sort_by_key(|f| std::cmp::Reverse(f.line_idx));
        let grouped = group_export_fixes_by_line(&line_fixes);

        if dry_run {
            push_dry_run_export_fixes(output, fixes, relative, path, &line_fixes);
        } else {
            apply_grouped_export_fixes(GroupedExportFixInput {
                plan,
                path,
                fixes,
                relative,
                content: &content,
                meta: &meta,
                lines: &lines,
                line_fixes: &line_fixes,
                grouped: &grouped,
            });
        }
    }
}

fn collect_export_line_fixes(
    lines: &[&str],
    file_exports: &[&fallow_types::results::UnusedExport],
) -> Vec<ExportFix> {
    file_exports
        .iter()
        .filter_map(|export| export_line_fix(lines, export))
        .collect()
}

fn export_line_fix(
    lines: &[&str],
    export: &fallow_types::results::UnusedExport,
) -> Option<ExportFix> {
    let line_idx = export.line.saturating_sub(1) as usize;
    let line = *lines.get(line_idx)?;
    let trimmed = line.trim_start();
    if !trimmed.starts_with("export ") {
        return None;
    }

    let after_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    if !is_removable_default_export(after_export) {
        return None;
    }

    Some(ExportFix {
        line_idx,
        export_name: export.export_name.clone(),
        enum_declaration: removable_exported_enum_range(lines, line_idx, &export.export_name),
    })
}

fn is_removable_default_export(after_export: &str) -> bool {
    if !after_export.starts_with("default ") {
        return true;
    }
    let after_default = after_export
        .strip_prefix("default ")
        .unwrap_or(after_export);
    after_default.starts_with("function ")
        || after_default.starts_with("async function ")
        || after_default.starts_with("class ")
        || after_default.starts_with("abstract class ")
}

fn group_export_fixes_by_line(line_fixes: &[ExportFix]) -> Vec<(usize, Vec<String>)> {
    let mut grouped: Vec<(usize, Vec<String>)> = Vec::new();
    for fix in line_fixes {
        if let Some(last) = grouped.last_mut()
            && last.0 == fix.line_idx
        {
            last.1.push(fix.export_name.clone());
            continue;
        }
        grouped.push((fix.line_idx, vec![fix.export_name.clone()]));
    }
    grouped
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

struct GroupedExportFixInput<'a> {
    plan: &'a mut FixPlan,
    path: &'a Path,
    fixes: &'a mut Vec<serde_json::Value>,
    relative: &'a Path,
    content: &'a str,
    meta: &'a super::io::EncodingMetadata,
    lines: &'a [&'a str],
    line_fixes: &'a [ExportFix],
    grouped: &'a [(usize, Vec<String>)],
}

fn apply_grouped_export_fixes(input: GroupedExportFixInput<'_>) {
    let GroupedExportFixInput {
        plan,
        path,
        fixes,
        relative,
        content,
        meta,
        lines,
        line_fixes,
        grouped,
    } = input;

    let mut new_lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
    let mut lines_to_delete = Vec::new();
    let mut ranges_to_delete = Vec::new();

    for (line_idx, names) in grouped {
        apply_export_line_group(
            &mut new_lines,
            &mut lines_to_delete,
            &mut ranges_to_delete,
            line_fixes,
            *line_idx,
            names,
        );
    }

    delete_export_lines(&mut new_lines, lines_to_delete, ranges_to_delete);
    stage_fixed_content(plan, path, &new_lines, meta, content);
    for fix in line_fixes {
        push_export_fix_json(fixes, relative, path, fix, Some(true));
    }
}

fn apply_export_line_group(
    new_lines: &mut [String],
    lines_to_delete: &mut Vec<usize>,
    ranges_to_delete: &mut Vec<EnumDeclarationRange>,
    line_fixes: &[ExportFix],
    line_idx: usize,
    names: &[String],
) {
    if let Some(range) = line_fixes
        .iter()
        .find(|fix| fix.line_idx == line_idx && fix.enum_declaration.is_some())
        .and_then(|fix| fix.enum_declaration)
    {
        ranges_to_delete.push(range);
        return;
    }

    let line = new_lines[line_idx].clone();
    let trimmed = line.trim_start();
    let after_export = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    if is_export_list(after_export) {
        apply_export_list_line(new_lines, lines_to_delete, line_idx, &line, names);
    } else {
        new_lines[line_idx] = remove_direct_export_keyword(&line, trimmed, after_export);
    }
}

fn apply_export_list_line(
    new_lines: &mut [String],
    lines_to_delete: &mut Vec<usize>,
    line_idx: usize,
    line: &str,
    names: &[String],
) {
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    match remove_specifiers_from_export_list(line, &name_refs) {
        None => lines_to_delete.push(line_idx),
        Some(new_line) => {
            new_lines[line_idx] = new_line;
        }
    }
}

fn remove_direct_export_keyword(line: &str, trimmed: &str, after_export: &str) -> String {
    let indent = line.len() - trimmed.len();
    let replacement = if after_export.starts_with("default function ")
        || after_export.starts_with("default async function ")
        || after_export.starts_with("default class ")
        || after_export.starts_with("default abstract class ")
    {
        after_export
            .strip_prefix("default ")
            .unwrap_or(after_export)
    } else {
        after_export
    };

    let prefix = &line[..indent];
    format!("{prefix}{replacement}")
}

fn delete_export_lines(
    new_lines: &mut Vec<String>,
    lines_to_delete: Vec<usize>,
    ranges_to_delete: Vec<EnumDeclarationRange>,
) {
    let mut delete_indices = lines_to_delete;
    for range in ranges_to_delete {
        delete_indices.extend(range.start_line..=range.end_line);
    }
    delete_indices.sort_unstable();
    delete_indices.dedup();
    for &idx in delete_indices.iter().rev() {
        new_lines.remove(idx);
    }
}

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
            plan,
            output,
            dry_run,
            fixes,
        });
    }

    fn make_export(path: &Path, name: &str, line: u32) -> UnusedExport {
        UnusedExport {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
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
    fn fix_single(
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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

    #[test]
    fn export_fix_handles_aliased_specifiers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let file = root.join("index.ts");
        std::fs::write(&file, "export { Foo as MyFoo, Bar } from \"./mod\";\n").unwrap();

        let (_, _) = fix_single(root, &file, "Foo", 1, false);

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
            low_confidence_skip_reason(Path::new("e2e/foo.ts"), &abs, &unresolved),
            Some(SkipReason::LowConfidenceOffGraph)
        );
    }

    #[test]
    fn low_confidence_reason_unresolved_when_not_off_graph() {
        let abs = PathBuf::from("/proj/src/foo.ts");
        let mut unresolved = FxHashSet::default();
        unresolved.insert(abs.clone());
        assert_eq!(
            low_confidence_skip_reason(Path::new("src/foo.ts"), &abs, &unresolved),
            Some(SkipReason::LowConfidenceUnresolvedImports)
        );
    }

    #[test]
    fn low_confidence_reason_none_for_clean_file() {
        let abs = PathBuf::from("/proj/src/foo.ts");
        assert_eq!(
            low_confidence_skip_reason(Path::new("src/foo.ts"), &abs, &FxHashSet::default()),
            None
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
        let mut plan = FixPlan::new();
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
