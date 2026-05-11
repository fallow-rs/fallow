use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;
use fallow_config::{RulesConfig, Severity};
use fallow_core::results::{
    AnalysisResults, DuplicateExport, PrivateTypeLeak, TestOnlyDependency, TypeOnlyDependency,
    UnusedDependency, UnusedExport, UnusedMember,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    MAX_FLAT_ITEMS, build_grouped_by_file, build_section_header, format_path,
    print_explain_tip_if_tty, push_section_footer_rollup, push_section_footer_with_count,
};
use crate::report::grouping::OwnershipResolver;
use crate::report::shared::NAMESPACE_BARREL_HINT;
use crate::report::{
    Level, elide_common_prefix, plural, relative_path, severity_to_level, split_dir_filename,
};

/// Minimum number of duplicate-export findings before the human section is
/// allowed to surface the namespace-barrel orientation hint. Below this floor
/// the hint is noise outweighing the value it provides.
const NAMESPACE_BARREL_HINT_MIN_FINDINGS: usize = 3;

/// Minimum ratio of barrel-shaped findings (locations all match
/// `**/<dir>/index.{ts,tsx,js,jsx,mjs,cjs}`, case-insensitive on the extension)
/// before the hint fires.
const NAMESPACE_BARREL_HINT_MIN_RATIO: f32 = 0.8;

/// Whether a duplicate-export location's path is shaped like a namespace-barrel
/// `index` file. The basename must be exactly `index`; the extension may be any
/// of the documented JS / TS module forms in any case (the case-insensitivity
/// applies to the EXTENSION only, so `Index.ts` does not match).
fn is_namespace_barrel_location(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    if stem != "index" {
        return false;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
    )
}

/// Ratio of `items` whose every `DuplicateLocation` matches the namespace-barrel
/// shape. Findings with fewer than two locations (already excluded from the
/// human render) are skipped to keep the denominator aligned with what the user
/// actually sees on screen.
fn namespace_barrel_match_ratio(items: &[DuplicateExport]) -> f32 {
    let renderable: Vec<&DuplicateExport> =
        items.iter().filter(|d| d.locations.len() >= 2).collect();
    if renderable.is_empty() {
        return 0.0;
    }
    let matches = renderable
        .iter()
        .filter(|dup| {
            dup.locations
                .iter()
                .all(|loc| is_namespace_barrel_location(&loc.path))
        })
        .count();
    matches as f32 / renderable.len() as f32
}

/// Whether the namespace-barrel hint should fire for this section. Gate
/// is `findings >= NAMESPACE_BARREL_HINT_MIN_FINDINGS` AND
/// `ratio >= NAMESPACE_BARREL_HINT_MIN_RATIO`. The floor prevents the hint
/// from spamming small projects where the user already knows the layout; the
/// ratio guards against false positives in mixed codebases.
fn should_show_namespace_barrel_hint(items: &[DuplicateExport]) -> bool {
    let renderable_count = items.iter().filter(|d| d.locations.len() >= 2).count();
    if renderable_count < NAMESPACE_BARREL_HINT_MIN_FINDINGS {
        return false;
    }
    namespace_barrel_match_ratio(items) >= NAMESPACE_BARREL_HINT_MIN_RATIO
}

/// Maximum files shown per grouped section (unused exports, types, etc.).
const MAX_GROUPED_FILES: usize = 10;
/// Maximum detail items shown per file within a grouped section.
const MAX_ITEMS_PER_FILE: usize = 5;
/// Threshold above which unused files switch to directory-grouped rollup.
const DIR_ROLLUP_THRESHOLD: usize = 200;
/// Threshold above which truncation hints suggest scoping flags.
const SCOPING_HINT_THRESHOLD: usize = 500;

/// Build a truncation message, adding scoping suggestions for very high counts.
///
/// The `total_issues` parameter is the total across ALL categories (not just this section).
/// The scoping hint fires when either the per-section overflow OR the total issue count
/// exceeds the threshold, so medium-sized projects with dispersed issues still see the hint.
fn truncation_hint(remaining: usize, total_issues: usize) -> String {
    if remaining > SCOPING_HINT_THRESHOLD || total_issues > SCOPING_HINT_THRESHOLD {
        format!(
            "... and {remaining} more \u{2014} try --workspace <name> or --changed-since main to scope"
        )
    } else {
        format!("... and {remaining} more (--format json for full list)")
    }
}

/// Check if a path contains a test directory segment.
fn is_test_path(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            "test"
                | "tests"
                | "__tests__"
                | "__test__"
                | "spec"
                | "specs"
                | "__mocks__"
                | "__fixtures__"
                | "fixtures"
        )
    })
}

/// Insert a dimmed test/src breakdown line when the majority of items are in test paths.
///
/// The annotation is inserted before the last blank line of the current section
/// so it appears just before the section gap.
fn insert_test_src_split<T>(lines: &mut Vec<String>, items: &[T], get_path: impl Fn(&T) -> &Path) {
    if items.len() < 5 {
        return;
    }
    let test_count = items
        .iter()
        .filter(|item| is_test_path(get_path(item)))
        .count();
    let src_count = items.len() - test_count;
    // Only show when there's a meaningful split (both > 0 and test is >=30%)
    if test_count == 0 || src_count == 0 {
        return;
    }
    let test_pct = (test_count * 100) / items.len();
    if test_pct < 30 {
        return;
    }
    let annotation = format!(
        "  {}",
        format!("{src_count} in src, {test_count} in test directories").dimmed()
    );
    // Insert before the trailing blank line (if present)
    if lines.last().is_some_and(String::is_empty) {
        let pos = lines.len() - 1;
        lines.insert(pos, annotation);
    } else {
        lines.push(annotation);
    }
}

pub(in crate::report) fn print_human(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    elapsed: Duration,
    quiet: bool,
    top: Option<usize>,
    show_explain_tip: bool,
) {
    if !quiet {
        eprintln!();
        // Config quality signal: warn when findings are dominated by one directory
        emit_config_quality_signal(results, root);
    }

    let total = results.total_issues();
    print_explain_tip_if_tty(show_explain_tip && total > 0, quiet);

    // Human output always includes section footers with doc links.
    for line in build_human_lines(results, root, rules, top) {
        println!("{line}");
    }

    if !quiet {
        if total == 0 {
            eprintln!(
                "{}",
                format!("\u{2713} No issues found ({:.2}s)", elapsed.as_secs_f64())
                    .green()
                    .bold()
            );
        } else {
            // Compute suppressed counts so the footer reflects visible items
            let unused_file_set: FxHashSet<&std::path::Path> = results
                .unused_files
                .iter()
                .map(|f| f.path.as_path())
                .collect();
            let suppressed_exports = results
                .unused_exports
                .iter()
                .filter(|e| unused_file_set.contains(e.path.as_path()))
                .count();
            let suppressed_types = results
                .unused_types
                .iter()
                .filter(|e| unused_file_set.contains(e.path.as_path()))
                .count();
            let summary = build_summary_footer(results, suppressed_exports, suppressed_types);
            eprintln!(
                "{}",
                format!("\u{2717} {summary} ({:.2}s)", elapsed.as_secs_f64())
                    .red()
                    .bold()
            );
            print_suppression_footer(results);
        }
    }
}

fn print_suppression_footer(results: &AnalysisResults) {
    if results.suppression_count == 0 && results.stale_suppressions.is_empty() {
        return;
    }
    let total = results.total_issues();
    let stale = results.stale_suppressions.len();
    eprintln!(
        "  {}",
        format!(
            "{total} issue{} \u{00b7} {} suppressed \u{00b7} {stale} stale suppression{}",
            plural(total),
            results.suppression_count,
            plural(stale)
        )
        .dimmed()
    );
}

/// Build human-readable output lines for analysis results.
///
/// Each section (unused files, exports, etc.) produces a header line followed by
/// detail lines. Empty sections are omitted entirely.
pub(in crate::report) fn build_human_lines(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    top: Option<usize>,
) -> Vec<String> {
    let max_items = top.unwrap_or(MAX_FLAT_ITEMS);
    let max_grouped_files = top.unwrap_or(MAX_GROUPED_FILES);
    let total_issues = results.total_issues();
    let mut lines = Vec::new();

    build_unused_code_section(
        &mut lines,
        results,
        root,
        rules,
        max_items,
        max_grouped_files,
        total_issues,
    );
    build_dependencies_section(
        &mut lines,
        results,
        root,
        rules,
        max_items,
        max_grouped_files,
        total_issues,
    );
    build_structure_section(&mut lines, results, root, rules, total_issues);
    build_maintenance_section(&mut lines, results, root, rules, total_issues);

    lines
}

/// `── Label ───...` header followed by a blank line, dimmed.
/// Matches the pre-refactor literal byte-for-byte: 2 leading bars, the
/// space-wrapped label, then exactly 37 trailing bars.
fn push_category_header(lines: &mut Vec<String>, label: &str) {
    let mut header = String::from("\u{2500}\u{2500} ");
    header.push_str(label);
    header.push(' ');
    for _ in 0..37 {
        header.push('\u{2500}');
    }
    lines.push(header.dimmed().to_string());
    lines.push(String::new());
}

/// Insert "(N more in files already reported as unused)" note before the
/// trailing blank line of a section (so any test/src split annotation stays
/// last). No-op when `suppressed` is zero.
fn push_suppressed_count_note(lines: &mut Vec<String>, suppressed: usize) {
    if suppressed == 0 {
        return;
    }
    let pos = if lines.last().is_some_and(String::is_empty) {
        lines.len() - 1
    } else {
        lines.len()
    };
    lines.insert(
        pos,
        format!(
            "  {}",
            format!("({suppressed} more in files already reported as unused)").dimmed()
        ),
    );
}

fn format_unused_export(e: &UnusedExport) -> String {
    let tag = if e.is_re_export {
        " (re-export)".dimmed().to_string()
    } else {
        String::new()
    };
    format!(
        "{} {}{}",
        format!(":{}", e.line).dimmed(),
        e.export_name.bold(),
        tag
    )
}

fn format_private_type_leak(e: &PrivateTypeLeak) -> String {
    format!(
        "{} {} references private type {}",
        format!(":{}", e.line).dimmed(),
        e.export_name.bold(),
        e.type_name.bold()
    )
}

fn format_unused_member(m: &UnusedMember) -> String {
    format!(
        "{} {}",
        format!(":{}", m.line).dimmed(),
        format!("{}.{}", m.parent_name, m.member_name).bold()
    )
}

fn format_dep_with_pkg(
    name: &str,
    pkg_path: &Path,
    used_in_workspaces: &[PathBuf],
    root: &Path,
) -> String {
    let pkg_label = relative_path(pkg_path, root).display().to_string();
    let workspace_context = if used_in_workspaces.is_empty() {
        String::new()
    } else {
        let workspaces = used_in_workspaces
            .iter()
            .map(|path| relative_path(path, root).display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!("; imported in {workspaces}")
    };
    if pkg_label == "package.json" && workspace_context.is_empty() {
        format!("{}", name.bold())
    } else {
        let label = if pkg_label == "package.json" {
            workspace_context.trim_start_matches("; ").to_string()
        } else {
            format!("{pkg_label}{workspace_context}")
        };
        format!("{} ({})", name.bold(), label.dimmed())
    }
}

/// Shared accessor for the dep types rendered with `format_dep_with_pkg`
/// (package name + owning package.json path). Kept crate-private since it
/// exists only to deduplicate the closures inside build_dependencies_section.
trait NamedPkgDep {
    fn pkg_name(&self) -> &str;
    fn pkg_path(&self) -> &Path;
    fn used_in_workspaces(&self) -> &[PathBuf] {
        &[]
    }
}

impl NamedPkgDep for UnusedDependency {
    fn pkg_name(&self) -> &str {
        &self.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.path
    }
    fn used_in_workspaces(&self) -> &[PathBuf] {
        &self.used_in_workspaces
    }
}

impl NamedPkgDep for TypeOnlyDependency {
    fn pkg_name(&self) -> &str {
        &self.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.path
    }
}

impl NamedPkgDep for TestOnlyDependency {
    fn pkg_name(&self) -> &str {
        &self.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.path
    }
}

fn push_human_pkg_dep_section<T: NamedPkgDep>(
    lines: &mut Vec<String>,
    items: &[T],
    title: &'static str,
    severity: Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    build_human_section_ex(
        lines,
        items,
        title,
        severity_to_level(severity),
        max_items,
        total_issues,
        |dep| {
            vec![format!(
                "  {}",
                format_dep_with_pkg(
                    dep.pkg_name(),
                    dep.pkg_path(),
                    dep.used_in_workspaces(),
                    root
                )
            )]
        },
    );
}

fn build_unused_code_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_items: usize,
    max_grouped_files: usize,
    total_issues: usize,
) {
    let unused_file_set: FxHashSet<&Path> = results
        .unused_files
        .iter()
        .map(|f| f.path.as_path())
        .collect();
    let filtered_exports: Vec<UnusedExport> = results
        .unused_exports
        .iter()
        .filter(|e| !unused_file_set.contains(e.path.as_path()))
        .cloned()
        .collect();
    let filtered_types: Vec<UnusedExport> = results
        .unused_types
        .iter()
        .filter(|e| !unused_file_set.contains(e.path.as_path()))
        .cloned()
        .collect();
    let suppressed_exports = results.unused_exports.len() - filtered_exports.len();
    let suppressed_types = results.unused_types.len() - filtered_types.len();

    let has_unused_code = !results.unused_files.is_empty()
        || !filtered_exports.is_empty()
        || !filtered_types.is_empty()
        || !results.private_type_leaks.is_empty()
        || !results.unused_enum_members.is_empty()
        || !results.unused_class_members.is_empty();
    if !has_unused_code {
        return;
    }
    push_category_header(lines, "Unused Code");

    if results.unused_files.len() > DIR_ROLLUP_THRESHOLD {
        build_dir_rollup_section(lines, &results.unused_files, root, rules, total_issues);
    } else {
        build_human_section_ex(
            lines,
            &results.unused_files,
            "Unused files",
            severity_to_level(rules.unused_files),
            max_items,
            total_issues,
            |file| {
                let path_str = relative_path(&file.path, root).display().to_string();
                vec![format!("  {}", format_path(&path_str))]
            },
        );
    }
    insert_test_src_split(lines, &results.unused_files, |f| &f.path);

    build_human_grouped_section(
        lines,
        &filtered_exports,
        "Unused exports",
        severity_to_level(rules.unused_exports),
        root,
        max_grouped_files,
        |e| e.path.as_path(),
        &format_unused_export,
    );
    push_suppressed_count_note(lines, suppressed_exports);
    insert_test_src_split(lines, &filtered_exports, |e| &e.path);

    build_human_grouped_section(
        lines,
        &filtered_types,
        "Unused type exports",
        severity_to_level(rules.unused_types),
        root,
        max_grouped_files,
        |e| e.path.as_path(),
        &format_unused_export,
    );
    push_suppressed_count_note(lines, suppressed_types);

    build_human_grouped_section(
        lines,
        &results.private_type_leaks,
        "Private type leaks",
        severity_to_level(rules.private_type_leaks),
        root,
        max_grouped_files,
        |e| e.path.as_path(),
        &format_private_type_leak,
    );

    build_human_grouped_section(
        lines,
        &results.unused_enum_members,
        "Unused enum members",
        severity_to_level(rules.unused_enum_members),
        root,
        max_grouped_files,
        |m| m.path.as_path(),
        &format_unused_member,
    );

    build_human_grouped_section(
        lines,
        &results.unused_class_members,
        "Unused class members",
        severity_to_level(rules.unused_class_members),
        root,
        max_grouped_files,
        |m| m.path.as_path(),
        &format_unused_member,
    );
}

fn build_dependencies_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_items: usize,
    max_grouped_files: usize,
    total_issues: usize,
) {
    let has_dependencies = !results.unused_dependencies.is_empty()
        || !results.unused_dev_dependencies.is_empty()
        || !results.unused_optional_dependencies.is_empty()
        || !results.unresolved_imports.is_empty()
        || !results.unlisted_dependencies.is_empty()
        || !results.type_only_dependencies.is_empty()
        || !results.test_only_dependencies.is_empty()
        || !results.unused_catalog_entries.is_empty();
    if !has_dependencies {
        return;
    }
    push_category_header(lines, "Dependencies");

    push_human_pkg_dep_section(
        lines,
        &results.unused_dependencies,
        "Unused dependencies",
        rules.unused_dependencies,
        max_items,
        total_issues,
        root,
    );
    push_human_pkg_dep_section(
        lines,
        &results.unused_dev_dependencies,
        "Unused devDependencies",
        rules.unused_dev_dependencies,
        max_items,
        total_issues,
        root,
    );
    push_human_pkg_dep_section(
        lines,
        &results.unused_optional_dependencies,
        "Unused optionalDependencies",
        rules.unused_optional_dependencies,
        max_items,
        total_issues,
        root,
    );
    build_human_grouped_section(
        lines,
        &results.unresolved_imports,
        "Unresolved imports",
        severity_to_level(rules.unresolved_imports),
        root,
        max_grouped_files,
        |i| i.path.as_path(),
        &|i| format!("{} {}", format!(":{}", i.line).dimmed(), i.specifier.bold()),
    );
    build_human_section_ex(
        lines,
        &results.unlisted_dependencies,
        "Unlisted dependencies",
        severity_to_level(rules.unlisted_dependencies),
        max_items,
        total_issues,
        |dep| vec![format!("  {}", dep.package_name.bold())],
    );
    push_human_pkg_dep_section(
        lines,
        &results.type_only_dependencies,
        "Type-only dependencies (consider moving to devDependencies)",
        rules.type_only_dependencies,
        max_items,
        total_issues,
        root,
    );
    push_human_pkg_dep_section(
        lines,
        &results.test_only_dependencies,
        "Test-only production dependencies (consider moving to devDependencies)",
        rules.test_only_dependencies,
        max_items,
        total_issues,
        root,
    );
    push_unused_catalog_entries_section(
        lines,
        &results.unused_catalog_entries,
        rules.unused_catalog_entries,
        max_items,
        total_issues,
        root,
    );
}

/// Render unused pnpm catalog entries in a flat column layout (matches knip's
/// shape): `entry_name  catalog_name  path:line`. Skipped when the list is
/// empty or the rule is `Off` (which already removed entries upstream).
fn push_unused_catalog_entries_section(
    lines: &mut Vec<String>,
    entries: &[fallow_core::results::UnusedCatalogEntry],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if entries.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        lines,
        entries,
        "Unused catalog entries",
        level,
        max_items,
        total_issues,
        |entry| {
            let path_display = root.join(&entry.path);
            let mut row = format!(
                "  {entry_name}  {catalog}  {loc}",
                entry_name = entry.entry_name.bold(),
                catalog = entry.catalog_name.dimmed(),
                loc = format!(
                    "{}:{}",
                    path_display
                        .strip_prefix(root)
                        .unwrap_or(&path_display)
                        .display(),
                    entry.line
                )
                .dimmed(),
            );
            let mut out = vec![row];
            if !entry.hardcoded_consumers.is_empty() {
                let consumers = entry
                    .hardcoded_consumers
                    .iter()
                    .map(|p| p.strip_prefix(root).unwrap_or(p).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                row = format!("    {}: {consumers}", "hardcoded in".dimmed());
                out.push(row);
            }
            out
        },
    );
}

fn build_structure_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    total_issues: usize,
) {
    let has_structure = !results.duplicate_exports.is_empty()
        || !results.circular_dependencies.is_empty()
        || !results.boundary_violations.is_empty();
    if !has_structure {
        return;
    }
    push_category_header(lines, "Structure");

    build_duplicate_exports_section(
        lines,
        &results.duplicate_exports,
        severity_to_level(rules.duplicate_exports),
        root,
        total_issues,
    );
    build_circular_deps_section(
        lines,
        &results.circular_dependencies,
        severity_to_level(rules.circular_dependencies),
        root,
        total_issues,
    );
    build_boundary_violations_section(
        lines,
        &results.boundary_violations,
        severity_to_level(rules.boundary_violation),
        root,
        total_issues,
    );
}

fn build_maintenance_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    total_issues: usize,
) {
    if results.stale_suppressions.is_empty() {
        return;
    }
    push_category_header(lines, "Maintenance");

    build_stale_suppressions_section(
        lines,
        &results.stale_suppressions,
        severity_to_level(rules.stale_suppressions),
        root,
        total_issues,
    );
}

/// Directory-grouped rollup for large unused file counts.
///
/// Instead of listing individual files (which is overwhelming at 200+), groups
/// by top-level directory and shows file counts per directory.
fn build_dir_rollup_section(
    lines: &mut Vec<String>,
    unused_files: &[fallow_core::results::UnusedFile],
    root: &Path,
    rules: &RulesConfig,
    total_issues: usize,
) {
    if unused_files.is_empty() {
        return;
    }
    let title = "Unused files";
    let level = severity_to_level(rules.unused_files);
    lines.push(build_section_header(title, unused_files.len(), level));

    // Group by first directory component (root-level files go under "(project root)")
    let mut dir_counts: Vec<(String, usize, bool)> = Vec::new();
    let mut dir_map: FxHashMap<String, usize> = FxHashMap::default();
    for f in unused_files {
        let rel = relative_path(&f.path, root);
        // Detect root-level files: only one path component means no parent directory
        let (dir, is_dir) = if rel.components().count() <= 1 {
            ("(project root)".to_string(), false)
        } else {
            (
                rel.components().next().map_or_else(
                    || ".".to_string(),
                    |c| c.as_os_str().to_string_lossy().to_string(),
                ),
                true,
            )
        };
        if let Some(&idx) = dir_map.get(&dir) {
            dir_counts[idx].1 += 1;
        } else {
            dir_map.insert(dir.clone(), dir_counts.len());
            dir_counts.push((dir, 1, is_dir));
        }
    }
    dir_counts.sort_by_key(|b| std::cmp::Reverse(b.1));

    // Second-level rollup: when one directory holds >80% of files, expand it
    // into two-level sub-directories (e.g. `packages/react-query/`) for clarity.
    let total = unused_files.len();
    let dominant = dir_counts
        .iter()
        .find(|(_, count, is_dir)| *is_dir && count * 100 / total.max(1) > 80)
        .map(|(dir, _, _)| dir.clone());

    let display_entries: Vec<(String, usize, bool)> = if let Some(ref dom_dir) = dominant {
        let mut sub_counts: Vec<(String, usize, bool)> = Vec::new();
        let mut sub_map: FxHashMap<String, usize> = FxHashMap::default();
        for f in unused_files {
            let rel = relative_path(&f.path, root);
            let mut components = rel.components();
            let first = components
                .next()
                .map(|c| c.as_os_str().to_string_lossy().to_string());
            if first.as_deref() == Some(dom_dir.as_str()) {
                let sub_key = components.next().map_or_else(
                    || dom_dir.clone(),
                    |c| format!("{}/{}", dom_dir, c.as_os_str().to_string_lossy()),
                );
                if let Some(&idx) = sub_map.get(&sub_key) {
                    sub_counts[idx].1 += 1;
                } else {
                    sub_map.insert(sub_key.clone(), sub_counts.len());
                    sub_counts.push((sub_key, 1, true));
                }
            }
        }
        sub_counts.sort_by_key(|b| std::cmp::Reverse(b.1));
        // Combine: sub-entries for the dominant dir + remaining first-level entries
        let mut combined = sub_counts;
        for entry in &dir_counts {
            if entry.0 != *dom_dir {
                combined.push(entry.clone());
            }
        }
        combined
    } else {
        dir_counts.clone()
    };

    let shown = display_entries.len().min(MAX_FLAT_ITEMS);
    for (dir, count, is_dir) in &display_entries[..shown] {
        let label = if *is_dir {
            format!("{dir}/").bold().to_string()
        } else {
            dir.dimmed().to_string()
        };
        lines.push(format!("  {}  {} file{}", label, count, plural(*count)));
    }
    if display_entries.len() > MAX_FLAT_ITEMS {
        let remaining = display_entries.len() - MAX_FLAT_ITEMS;
        // Use directory-specific wording and scoping hint when total issues are high
        let hint = if remaining > SCOPING_HINT_THRESHOLD || total_issues > SCOPING_HINT_THRESHOLD {
            format!(
                "... and {remaining} more director{} \u{2014} try --workspace <name> or --changed-since main to scope",
                if remaining == 1 { "y" } else { "ies" }
            )
        } else {
            format!(
                "... and {remaining} more director{} (--format json for full list)",
                if remaining == 1 { "y" } else { "ies" }
            )
        };
        lines.push(format!("  {}", hint.dimmed()));
    }
    push_section_footer_rollup(lines, title, unused_files.len());
    lines.push(String::new());
}

/// Append a non-empty section with a header, doc-link footer, and truncated items.
fn build_human_section_ex<T>(
    lines: &mut Vec<String>,
    items: &[T],
    title: &str,
    level: Level,
    max: usize,
    total_issues: usize,
    format_lines: impl Fn(&T) -> Vec<String>,
) {
    if items.is_empty() {
        return;
    }
    lines.push(build_section_header(title, items.len(), level));
    let shown = items.len().min(max);
    for item in &items[..shown] {
        for line in format_lines(item) {
            lines.push(line);
        }
    }
    if items.len() > max {
        let remaining = items.len() - max;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

/// Append a non-empty section whose items are grouped by file path (truncated).
///
/// Files are sorted by item count descending. Shows `(N exports)` next to each
/// file header. Truncates to `max_files` files and `MAX_ITEMS_PER_FILE`
/// items per file.
#[expect(
    clippy::too_many_arguments,
    reason = "section renderer needs all display parameters"
)]
fn build_human_grouped_section<'a, T>(
    lines: &mut Vec<String>,
    items: &'a [T],
    title: &str,
    level: Level,
    root: &Path,
    max_files: usize,
    get_path: impl Fn(&'a T) -> &'a Path,
    format_detail: &impl Fn(&T) -> String,
) {
    if items.is_empty() {
        return;
    }
    lines.push(build_section_header(title, items.len(), level));
    build_grouped_by_file(
        lines,
        items,
        root,
        get_path,
        format_detail,
        max_files,
        MAX_ITEMS_PER_FILE,
    );
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

/// Build duplicate exports grouped by file pair instead of flat list.
fn build_duplicate_exports_section(
    lines: &mut Vec<String>,
    items: &[fallow_core::results::DuplicateExport],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Duplicate exports";
    lines.push(build_section_header(title, items.len(), level));

    // Group by sorted file-pair key
    let mut pair_groups: Vec<(String, String, Vec<&str>)> = Vec::new();
    let mut pair_map: rustc_hash::FxHashMap<(String, String), usize> =
        rustc_hash::FxHashMap::default();

    for dup in items {
        if dup.locations.len() < 2 {
            continue;
        }
        let mut paths: Vec<String> = dup
            .locations
            .iter()
            .map(|loc| relative_path(&loc.path, root).display().to_string())
            .collect();
        paths.sort();
        paths.dedup();

        // For multi-file duplicates, pair the first two
        let key = (paths[0].clone(), paths.get(1).cloned().unwrap_or_default());
        if let Some(&group_idx) = pair_map.get(&key) {
            pair_groups[group_idx].2.push(&dup.export_name);
        } else {
            pair_map.insert(key, pair_groups.len());
            pair_groups.push((
                paths[0].clone(),
                paths.get(1).cloned().unwrap_or_default(),
                vec![&dup.export_name],
            ));
        }
    }

    // Sort by count descending
    pair_groups.sort_by_key(|b| std::cmp::Reverse(b.2.len()));

    let shown = pair_groups.len().min(MAX_FLAT_ITEMS);
    for (file_a, file_b, exports) in &pair_groups[..shown] {
        let export_list = if exports.len() <= 5 {
            exports
                .iter()
                .map(|e| e.bold().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            let mut display: Vec<String> =
                exports[..5].iter().map(|e| e.bold().to_string()).collect();
            display.push(format!("... +{}", exports.len() - 5).dimmed().to_string());
            display.join(", ")
        };

        // Vertical layout: file_a on line 1, <-> file_b on line 2, exports on line 3
        let elided_b = elide_common_prefix(file_a, file_b);
        lines.push(format!("  {}", format_path(file_a)));
        lines.push(format!(
            "    {} {} ({} export{})",
            "\u{2194}".dimmed(),
            format_path(elided_b),
            exports.len(),
            plural(exports.len())
        ));
        lines.push(format!("    {export_list}"));
        lines.push(String::new());
    }

    let truncation_emitted = pair_groups.len() > MAX_FLAT_ITEMS;
    if truncation_emitted {
        let remaining = pair_groups.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    if should_show_namespace_barrel_hint(items) {
        if truncation_emitted {
            // Keep the truncation hint and the namespace-barrel hint visually
            // distinct; without this blank line both render as one block of
            // dim text and read as a single run-on note.
            lines.push(String::new());
        }
        lines.push(format!("  {}", NAMESPACE_BARREL_HINT.dimmed()));
    }
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

/// Build circular dependencies grouped by hub file with path elision.
fn build_circular_deps_section(
    lines: &mut Vec<String>,
    items: &[fallow_core::results::CircularDependency],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Circular dependencies";
    lines.push(build_section_header(title, items.len(), level));

    // Group cycles by their first file (hub)
    let mut hub_groups: Vec<(String, Vec<&fallow_core::results::CircularDependency>)> = Vec::new();
    let mut hub_map: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();

    for cycle in items {
        let hub = cycle
            .files
            .first()
            .map(|p| relative_path(p, root).display().to_string())
            .unwrap_or_default();
        if let Some(&idx) = hub_map.get(&hub) {
            hub_groups[idx].1.push(cycle);
        } else {
            hub_map.insert(hub.clone(), hub_groups.len());
            hub_groups.push((hub, vec![cycle]));
        }
    }

    // Sort by cycle count descending, alphabetical tiebreaker
    hub_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    let shown = hub_groups.len().min(MAX_FLAT_ITEMS);
    for (hub_path, cycles) in &hub_groups[..shown] {
        let count_tag = if cycles.len() > 1 {
            format!(" ({} cycles)", cycles.len()).dimmed().to_string()
        } else {
            String::new()
        };
        lines.push(format!("  {}{}", format_path(hub_path), count_tag));

        for cycle in cycles {
            let rel_paths: Vec<String> = cycle
                .files
                .iter()
                .map(|p| relative_path(p, root).display().to_string())
                .collect();

            // Build chain: elide common prefix with hub, add closing return to hub
            let mut chain_parts: Vec<String> = Vec::new();
            for path in &rel_paths[1..] {
                let elided = elide_common_prefix(hub_path, path);
                chain_parts.push(format_path(elided));
            }
            // Close the cycle back to hub filename
            let (_, hub_filename) = split_dir_filename(hub_path);
            chain_parts.push(hub_filename.bold().to_string());

            // When every file in the cycle is a .d.ts, tag it as type-only
            let type_only_tag = if cycle
                .files
                .iter()
                .all(|p| p.to_str().is_some_and(|s| s.ends_with(".d.ts")))
            {
                format!(" {}", "(type-only)".dimmed())
            } else {
                String::new()
            };

            let cross_pkg_tag = if cycle.is_cross_package {
                format!(" {}", "(cross-package)".dimmed())
            } else {
                String::new()
            };

            lines.push(format!(
                "    {} {}{}{}",
                "\u{2192}".dimmed(),
                chain_parts.join(&format!(" {} ", "\u{2192}".dimmed())),
                type_only_tag,
                cross_pkg_tag,
            ));
        }
        lines.push(String::new());
    }

    if hub_groups.len() > MAX_FLAT_ITEMS {
        let hidden: usize = hub_groups[MAX_FLAT_ITEMS..]
            .iter()
            .map(|(_, cycles)| cycles.len())
            .sum();
        lines.push(format!(
            "  {}",
            truncation_hint(hidden, total_issues).dimmed()
        ));
        lines.push(String::new());
    }
    push_section_footer_with_count(lines, title, items.len());
    if !lines.last().is_some_and(String::is_empty) {
        lines.push(String::new());
    }
}

/// Build boundary violations section grouped by importing file.
fn build_boundary_violations_section(
    lines: &mut Vec<String>,
    items: &[fallow_core::results::BoundaryViolation],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Boundary violations";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for v in &items[..shown] {
        let from = relative_path(&v.from_path, root).display().to_string();
        let to = relative_path(&v.to_path, root).display().to_string();
        lines.push(format!(
            "  {}:{} {} {} {} {}",
            from,
            v.line,
            "\u{2192}".dimmed(),
            to,
            format!("({}", v.from_zone).dimmed(),
            format!("\u{2192} {})", v.to_zone).dimmed(),
        ));
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

fn build_stale_suppressions_section(
    lines: &mut Vec<String>,
    items: &[fallow_core::results::StaleSuppression],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Stale suppressions";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for s in &items[..shown] {
        let path_str = relative_path(&s.path, root).display().to_string();
        lines.push(format!(
            "  {}:{}:{} {} {}",
            path_str,
            s.line,
            s.col,
            s.description().bold(),
            format!("({})", s.explanation()).dimmed(),
        ));
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

/// Collect the unique CODEOWNERS patterns that matched files in a result set.
///
/// Returns up to 3 sorted patterns. Only meaningful for `Owner` mode.
fn collect_matching_rules(
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) -> Vec<String> {
    let mut rules: FxHashSet<String> = FxHashSet::default();

    let mut check = |path: &Path| {
        if let (_, Some(rule)) = resolver.resolve_with_rule(relative_path(path, root)) {
            rules.insert(rule);
        }
    };

    for f in &results.unused_files {
        check(&f.path);
    }
    for e in &results.unused_exports {
        check(&e.path);
    }
    for e in &results.unused_types {
        check(&e.path);
    }
    for e in &results.private_type_leaks {
        check(&e.path);
    }
    for m in &results.unused_enum_members {
        check(&m.path);
    }
    for m in &results.unused_class_members {
        check(&m.path);
    }
    for u in &results.unresolved_imports {
        check(&u.path);
    }
    for c in &results.circular_dependencies {
        if let Some(first) = c.files.first() {
            check(first);
        }
    }
    for b in &results.boundary_violations {
        check(&b.from_path);
    }
    for s in &results.stale_suppressions {
        check(&s.path);
    }

    let mut sorted: Vec<String> = rules.into_iter().collect();
    sorted.sort();
    sorted.truncate(3);
    sorted
}

/// Print analysis results grouped by owner or directory.
///
/// Each group gets a colored header with its key and issue count, followed by
/// the same section output that `print_human` produces. Unowned groups get
/// an advisory footer. Doc URL footers are deduplicated across groups.
pub(in crate::report) fn print_grouped_human(
    groups: &[crate::report::grouping::ResultGroup],
    root: &Path,
    rules: &RulesConfig,
    elapsed: Duration,
    quiet: bool,
    resolver: Option<&OwnershipResolver>,
) {
    if !quiet {
        eprintln!();
    }

    // ── Summary line: groups sorted by issue count descending ───────
    let mut group_counts: Vec<(&str, usize)> = groups
        .iter()
        .map(|g| (g.key.as_str(), g.results.total_issues()))
        .filter(|(_, count)| *count > 0)
        .collect();
    group_counts.sort_by_key(|b| std::cmp::Reverse(b.1));

    if !group_counts.is_empty() {
        let summary_parts: Vec<String> = group_counts
            .iter()
            .map(|(key, count)| format!("{key} {count}"))
            .collect();
        let summary = format!(
            "{} group{}: {}",
            group_counts.len(),
            plural(group_counts.len()),
            summary_parts.join(" \u{00b7} ")
        );
        println!("{}", summary.dimmed());
        println!();
    }

    let mut grand_total: usize = 0;
    let mut seen_footers: FxHashSet<String> = FxHashSet::default();

    for group in groups {
        let total = group.results.total_issues();
        if total == 0 {
            continue;
        }
        grand_total += total;

        // Group header: bold cyan key with issue count and per-type breakdown
        let issue_word = if total == 1 { "issue" } else { "issues" };
        let breakdown = build_summary_footer(&group.results, 0, 0);
        let header_text = if breakdown.is_empty() {
            format!("{} ({total} {issue_word})", group.key)
        } else {
            format!("{} ({total} {issue_word}: {breakdown})", group.key)
        };

        // Optionally append matching CODEOWNERS rules for Owner mode
        let header_text = match resolver {
            Some(r @ OwnershipResolver::Owner(_)) => {
                let matched = collect_matching_rules(&group.results, root, r);
                if matched.is_empty() {
                    header_text
                } else {
                    format!("{header_text} \u{2014} matched by {}", matched.join(", "))
                }
            }
            _ => header_text,
        };

        println!("{}", header_text.cyan().bold());

        // Section-mode: list the section's default owners under the heading
        // so human output mirrors the `owners` metadata emitted in JSON.
        if let Some(ref owners) = group.owners
            && !owners.is_empty()
        {
            println!("  {} {}", "owners:".dimmed(), owners.join(" ").dimmed());
        }

        // Build lines and dedup doc URL footers across groups
        let lines = build_human_lines(&group.results, root, rules, None);
        for line in &lines {
            if line.contains("docs.fallow.tools") && !seen_footers.insert(line.clone()) {
                continue;
            }
            println!("{line}");
        }

        if group.key == crate::codeowners::UNOWNED_LABEL {
            eprintln!(
                "  {}",
                "Files with no CODEOWNERS entry \u{2014} add ownership or verify before removing"
                    .dimmed()
            );
            eprintln!();
        }
    }

    if !quiet {
        if grand_total == 0 {
            eprintln!(
                "{}",
                format!("\u{2713} No issues found ({:.2}s)", elapsed.as_secs_f64())
                    .green()
                    .bold()
            );
        } else {
            eprintln!(
                "{}",
                format!(
                    "\u{2717} {grand_total} issue{} across {} group{} ({:.2}s)",
                    plural(grand_total),
                    groups
                        .iter()
                        .filter(|g| g.results.total_issues() > 0)
                        .count(),
                    plural(
                        groups
                            .iter()
                            .filter(|g| g.results.total_issues() > 0)
                            .count()
                    ),
                    elapsed.as_secs_f64()
                )
                .red()
                .bold()
            );
        }
    }
}

/// Emit a config-quality advisory to stderr when unused files are dominated by one directory.
///
/// Called from `print_human` (not `build_human_lines`) so it respects the `quiet` flag
/// and doesn't fire as a side effect during line-building.
fn emit_config_quality_signal(results: &AnalysisResults, root: &Path) {
    if results.unused_files.len() <= 50 {
        return;
    }
    let mut dir_counts: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();
    for f in &results.unused_files {
        let rel = relative_path(&f.path, root);
        if let Some(first) = rel.components().next() {
            *dir_counts
                .entry(first.as_os_str().to_string_lossy().to_string())
                .or_insert(0) += 1;
        }
    }
    let total = results.unused_files.len();
    if let Some((dominant_dir, count)) = dir_counts.iter().max_by_key(|(_, c)| **c) {
        let pct = (*count as f64 / total as f64) * 100.0;
        if pct > 80.0 {
            // Source-heavy directories get different advice than test/example dirs
            let is_source_dir =
                matches!(dominant_dir.as_str(), "packages" | "src" | "lib" | "apps");
            let advice = if is_source_dir {
                format!(
                    "Note: {pct:.0}% of unused files are under {dominant_dir}/ \
                     \u{2014} run `fallow list --entry-points` to verify entry-point detection \
                     \u{2014} https://docs.fallow.tools/explanations/dead-code#unused-files"
                )
            } else {
                format!(
                    "Note: {pct:.0}% of unused files are under {dominant_dir}/ \
                     \u{2014} consider adding it to ignorePatterns or using --production \
                     (analyzes only production entry points) \
                     \u{2014} https://docs.fallow.tools/explanations/dead-code#unused-files"
                )
            };
            eprintln!("  {}", advice.yellow());
        }
    }
}

/// Build a one-line summary footer showing counts per issue type.
///
/// `suppressed_exports` / `suppressed_types` are subtracted from the raw
/// counts so the footer reflects the *visible* items when export suppression
/// is active (exports from unused files are hidden).
fn build_summary_footer(
    results: &AnalysisResults,
    suppressed_exports: usize,
    suppressed_types: usize,
) -> String {
    let mut parts = Vec::new();
    let mut add = |count: usize, label: &str| {
        if count > 0 {
            let display_label = if count == 1 && label.ends_with("ies") {
                // Singularize -ies plurals: "dependencies" → "dependency"
                format!("{}y", &label[..label.len() - 3])
            } else if count == 1 && label.ends_with('s') {
                // Singularize simple plurals: "enum members" → "enum member"
                label[..label.len() - 1].to_string()
            } else {
                label.to_string()
            };
            let mut s = String::new();
            let _ = write!(s, "{count} {display_label}");
            if count != 1 && !label.ends_with('s') {
                s.push('s');
            }
            parts.push(s);
        }
    };

    add(results.unused_files.len(), "file");
    add(
        results
            .unused_exports
            .len()
            .saturating_sub(suppressed_exports),
        "export",
    );
    add(
        results.unused_types.len().saturating_sub(suppressed_types),
        "type",
    );
    add(results.unused_dependencies.len(), "unused dependencies");
    add(
        results.unused_dev_dependencies.len() + results.unused_optional_dependencies.len(),
        "dev/optional dependencies",
    );
    add(results.unused_enum_members.len(), "enum members");
    add(results.unused_class_members.len(), "class members");
    add(results.unresolved_imports.len(), "unresolved imports");
    add(results.unlisted_dependencies.len(), "unlisted dependencies");
    // Count unique file-pairs (consistent with the section renderer's grouping)
    {
        let mut pair_set = rustc_hash::FxHashSet::default();
        for dup in &results.duplicate_exports {
            if dup.locations.len() >= 2 {
                let mut paths: Vec<&std::path::Path> =
                    dup.locations.iter().map(|l| l.path.as_path()).collect();
                paths.sort();
                paths.dedup();
                if paths.len() >= 2 {
                    pair_set.insert((paths[0].to_path_buf(), paths[1].to_path_buf()));
                }
            }
        }
        add(pair_set.len(), "duplicate pair");
    }
    add(
        results.type_only_dependencies.len(),
        "type-only dependencies",
    );
    add(
        results.test_only_dependencies.len(),
        "test-only dependencies",
    );
    add(results.circular_dependencies.len(), "circular dependencies");
    add(results.boundary_violations.len(), "violations");
    add(results.stale_suppressions.len(), "stale suppressions");

    parts.join(" \u{00b7} ")
}

/// Print a concise summary showing only category counts, no individual items.
pub(in crate::report) fn print_check_summary(
    results: &AnalysisResults,
    rules: &RulesConfig,
    elapsed: Duration,
    quiet: bool,
) {
    let total = results.total_issues();
    if total == 0 {
        if !quiet {
            eprintln!(
                "{}",
                format!("\u{2713} No issues found ({:.2}s)", elapsed.as_secs_f64())
                    .green()
                    .bold()
            );
        }
        return;
    }

    println!("{}", "Dead Code Summary".bold());
    println!();

    let categories: &[(&str, usize, Level)] = &[
        (
            "Unused files",
            results.unused_files.len(),
            severity_to_level(rules.unused_files),
        ),
        (
            "Unused exports",
            results.unused_exports.len(),
            severity_to_level(rules.unused_exports),
        ),
        (
            "Unused types",
            results.unused_types.len(),
            severity_to_level(rules.unused_types),
        ),
        (
            "Private type leaks",
            results.private_type_leaks.len(),
            severity_to_level(rules.private_type_leaks),
        ),
        (
            "Unused dependencies",
            results.unused_dependencies.len(),
            severity_to_level(rules.unused_dependencies),
        ),
        (
            "Unused dev dependencies",
            results.unused_dev_dependencies.len(),
            severity_to_level(rules.unused_dev_dependencies),
        ),
        (
            "Unused optional dependencies",
            results.unused_optional_dependencies.len(),
            severity_to_level(rules.unused_optional_dependencies),
        ),
        (
            "Unused enum members",
            results.unused_enum_members.len(),
            severity_to_level(rules.unused_enum_members),
        ),
        (
            "Unused class members",
            results.unused_class_members.len(),
            severity_to_level(rules.unused_class_members),
        ),
        (
            "Unresolved imports",
            results.unresolved_imports.len(),
            severity_to_level(rules.unresolved_imports),
        ),
        (
            "Unlisted dependencies",
            results.unlisted_dependencies.len(),
            severity_to_level(rules.unlisted_dependencies),
        ),
        (
            "Duplicate exports",
            results.duplicate_exports.len(),
            severity_to_level(rules.duplicate_exports),
        ),
        (
            "Type-only dependencies",
            results.type_only_dependencies.len(),
            severity_to_level(rules.type_only_dependencies),
        ),
        (
            "Test-only dependencies",
            results.test_only_dependencies.len(),
            severity_to_level(rules.test_only_dependencies),
        ),
        (
            "Circular dependencies",
            results.circular_dependencies.len(),
            severity_to_level(rules.circular_dependencies),
        ),
        (
            "Boundary violations",
            results.boundary_violations.len(),
            severity_to_level(rules.boundary_violation),
        ),
        (
            "Stale suppressions",
            results.stale_suppressions.len(),
            severity_to_level(rules.stale_suppressions),
        ),
    ];

    for (name, count, level) in categories {
        if *count == 0 {
            continue;
        }
        let count_str = format!("{count:>6}");
        let colored = match level {
            Level::Error => count_str.red().bold().to_string(),
            Level::Warn => count_str.yellow().to_string(),
            Level::Info => count_str.dimmed().to_string(),
        };
        println!("  {colored}  {name}");
    }

    println!();
    let total_str = format!("{total:>6}");
    println!("  {}  {}", total_str.bold(), "Total".bold());

    if !quiet {
        eprintln!(
            "{}",
            format!("\u{2717} {total} issues ({:.2}s)", elapsed.as_secs_f64())
                .red()
                .bold()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::{plain, strip_ansi};
    use super::*;
    use fallow_config::{RulesConfig, Severity};
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use std::path::PathBuf;

    /// Build sample results including optional deps (extends the shared helper).
    fn sample_results(root: &Path) -> AnalysisResults {
        crate::report::test_helpers::sample_results(root)
    }

    // ── Empty results ──

    #[test]
    fn empty_results_produce_no_lines() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        assert!(lines.is_empty());
    }

    // ── Section headers contain title and count ──

    #[test]
    fn section_headers_contain_title_and_count() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);

        assert!(text.contains("Unused files (1)"));
        assert!(text.contains("Unused exports (1)"));
        assert!(text.contains("Unused type exports (1)"));
        assert!(text.contains("Unused dependencies (1)"));
        assert!(text.contains("Unused devDependencies (1)"));
        assert!(text.contains("Unused optionalDependencies (1)"));
        assert!(text.contains("Unused enum members (1)"));
        assert!(text.contains("Unused class members (1)"));
        assert!(text.contains("Unresolved imports (1)"));
        assert!(text.contains("Unlisted dependencies (1)"));
        assert!(text.contains("Duplicate exports (1)"));
        assert!(text.contains("Type-only dependencies (consider moving to devDependencies) (1)"));
        assert!(text.contains("Circular dependencies (1)"));
    }

    // ── Multiple items show correct counts ──

    #[test]
    fn section_header_shows_correct_count_for_multiple_items() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..5 {
            results.unused_files.push(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            });
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (5)"));
    }

    // ── Unused files display relative paths ──

    #[test]
    fn unused_files_show_relative_paths() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/components/Button.tsx"),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("src/components/Button.tsx"));
        assert!(!text.contains("/project/"));
    }

    // ── Unused exports show file grouping, line, and name ──

    #[test]
    fn unused_exports_grouped_by_file_with_line_and_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "anotherFn".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);

        // Count of 2 in header
        assert!(text.contains("Unused exports (2)"));
        // File path appears as group header
        assert!(text.contains("src/utils.ts"));
        // Both export names appear
        assert!(text.contains(":10 helperFn"));
        assert!(text.contains(":25 anotherFn"));
    }

    // ── Re-exports are tagged ──

    #[test]
    fn re_exports_are_tagged() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExported".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("(re-export)"));
    }

    #[test]
    fn non_re_exports_have_no_tag() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helper".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(!text.contains("(re-export)"));
    }

    // ── Unused members show parent.member format ──

    #[test]
    fn unused_enum_members_show_parent_dot_member() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Color".to_string(),
            member_name: "Purple".to_string(),
            kind: MemberKind::EnumMember,
            line: 5,
            col: 2,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Color.Purple"));
        assert!(text.contains(":5"));
    }

    #[test]
    fn unused_class_members_show_parent_dot_member() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_class_members.push(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "ApiService".to_string(),
            member_name: "disconnect".to_string(),
            kind: MemberKind::ClassMethod,
            line: 99,
            col: 4,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("ApiService.disconnect"));
        assert!(text.contains(":99"));
    }

    // ── Dependencies display ──

    #[test]
    fn unused_deps_at_root_show_package_name_only() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("lodash"));
        // Should NOT show "(package.json)" for root deps
        assert!(!text.contains("(package.json)"));
    }

    #[test]
    fn unused_deps_in_workspace_show_workspace_path() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "axios".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/web/package.json"),
            line: 8,
            used_in_workspaces: Vec::new(),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("axios"));
        assert!(text.contains("(packages/web/package.json)"));
    }

    #[test]
    fn unused_deps_show_cross_workspace_context() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/shared/package.json"),
            line: 8,
            used_in_workspaces: vec![root.join("packages/consumer")],
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("lodash-es"));
        assert!(text.contains("packages/shared/package.json; imported in packages/consumer"));
    }

    #[test]
    fn unused_root_dep_with_cross_workspace_context_uses_context_label() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 8,
            used_in_workspaces: vec![root.join("packages/consumer")],
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("lodash-es"));
        assert!(text.contains("(imported in packages/consumer)"));
        assert!(!text.contains("(package.json; imported in packages/consumer)"));
    }

    // ── Unresolved imports show specifier ──

    #[test]
    fn unresolved_imports_show_specifier_and_line() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "@org/missing-pkg".to_string(),
            line: 7,
            col: 0,
            specifier_col: 0,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("src/app.ts"));
        assert!(text.contains(":7"));
        assert!(text.contains("@org/missing-pkg"));
    }

    // ── Namespace-barrel hint helpers ──

    fn make_dup(name: &str, paths: &[&str]) -> DuplicateExport {
        DuplicateExport {
            export_name: name.to_string(),
            locations: paths
                .iter()
                .map(|p| DuplicateLocation {
                    path: PathBuf::from(p),
                    line: 1,
                    col: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn is_namespace_barrel_location_matches_documented_extensions() {
        assert!(is_namespace_barrel_location(Path::new(
            "components/ui/button/index.ts"
        )));
        assert!(is_namespace_barrel_location(Path::new(
            "components/ui/button/index.tsx"
        )));
        assert!(is_namespace_barrel_location(Path::new("src/x/index.mjs")));
        assert!(is_namespace_barrel_location(Path::new("src/x/index.cjs")));
        assert!(is_namespace_barrel_location(Path::new("src/x/index.jsx")));
        // Case-insensitive on the extension only.
        assert!(is_namespace_barrel_location(Path::new(
            "components/ui/button/index.TS"
        )));
        assert!(is_namespace_barrel_location(Path::new(
            "components/ui/button/index.Tsx"
        )));
    }

    #[test]
    fn is_namespace_barrel_location_rejects_non_index_files() {
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/Button.ts"
        )));
        // basename must be exactly `index`; uppercase `Index` does not match.
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/Index.ts"
        )));
        // Unsupported extensions.
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/index.svelte"
        )));
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/index.vue"
        )));
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/index"
        )));
    }

    #[test]
    fn namespace_barrel_hint_fires_when_4_of_5_findings_match() {
        let items = vec![
            make_dup(
                "Root",
                &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
            ),
            make_dup(
                "Content",
                &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
            ),
            make_dup(
                "Trigger",
                &["packages/ui/e/index.ts", "packages/ui/f/index.ts"],
            ),
            make_dup(
                "Item",
                &["packages/ui/g/index.ts", "packages/ui/h/index.ts"],
            ),
            make_dup("Config", &["src/config.ts", "src/types.ts"]),
        ];
        assert!(should_show_namespace_barrel_hint(&items));
    }

    #[test]
    fn namespace_barrel_hint_does_not_fire_when_2_of_5_findings_match() {
        let items = vec![
            make_dup(
                "Root",
                &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
            ),
            make_dup("Content", &["packages/ui/c/index.ts", "src/types.ts"]),
            make_dup("Trigger", &["src/a.ts", "src/b.ts"]),
            make_dup("Item", &["src/c.ts", "src/d.ts"]),
            make_dup("Config", &["src/config.ts", "src/types.ts"]),
        ];
        assert!(!should_show_namespace_barrel_hint(&items));
    }

    #[test]
    fn namespace_barrel_hint_does_not_fire_below_findings_floor() {
        // 2 of 2 findings match the barrel shape, but the floor is 3 findings.
        let items = vec![
            make_dup(
                "Root",
                &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
            ),
            make_dup(
                "Content",
                &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
            ),
        ];
        assert!(!should_show_namespace_barrel_hint(&items));
    }

    #[test]
    fn namespace_barrel_hint_fires_when_47_of_47_findings_match() {
        let items: Vec<DuplicateExport> = (0..47)
            .map(|i| {
                let path_a = format!("packages/ui/dir_{i}/index.ts");
                let path_b = format!("packages/ui/other_{i}/index.tsx");
                make_dup(&format!("Sym{i}"), &[path_a.as_str(), path_b.as_str()])
            })
            .collect();
        assert!(should_show_namespace_barrel_hint(&items));
    }

    #[test]
    fn namespace_barrel_hint_skips_single_location_findings_when_computing_ratio() {
        // Single-location findings are filtered out of the human render and
        // should not affect the ratio. Three barrel-shaped renderable findings
        // alongside a single-location finding still satisfy the gate.
        let items = vec![
            make_dup(
                "Root",
                &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
            ),
            make_dup(
                "Content",
                &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
            ),
            make_dup(
                "Trigger",
                &["packages/ui/e/index.ts", "packages/ui/f/index.ts"],
            ),
            make_dup("Lonely", &["src/lonely.ts"]),
        ];
        assert!(should_show_namespace_barrel_hint(&items));
    }

    #[test]
    fn duplicate_exports_section_emits_hint_when_gate_passes() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..4 {
            results.duplicate_exports.push(make_dup(
                &format!("Sym{i}"),
                &[
                    &format!("/project/packages/ui/dir_{i}/index.ts"),
                    &format!("/project/packages/ui/other_{i}/index.tsx"),
                ],
            ));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(
            text.contains("namespace-barrel"),
            "expected hint substring in output: {text}"
        );
    }

    #[test]
    fn duplicate_exports_section_omits_hint_when_gate_fails() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // Only one finding -> below floor.
        results.duplicate_exports.push(make_dup(
            "Sym",
            &[
                "/project/packages/ui/a/index.ts",
                "/project/packages/ui/b/index.ts",
            ],
        ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(
            !text.contains("namespace-barrel"),
            "hint must not fire below the 3-finding floor: {text}"
        );
    }

    // ── Duplicate exports show locations ──

    #[test]
    fn duplicate_exports_show_name_and_locations() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/config.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/types.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Config"));
        assert!(text.contains("src/config.ts"));
        // file_b shown with common prefix elided
        assert!(text.contains("types.ts"));
    }

    // ── Circular dependencies show cycle with arrow ──

    #[test]
    fn circular_dependencies_show_cycle_with_arrow_and_repeat() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                root.join("src/a.ts"),
                root.join("src/b.ts"),
                root.join("src/c.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // Hub file shown first, chain with elided paths and arrows
        assert!(text.contains("a.ts"));
        assert!(text.contains("b.ts"));
        assert!(text.contains("c.ts"));
        assert!(text.contains("\u{2192}"));
    }

    // ── Empty sections are omitted ──

    #[test]
    fn empty_sections_are_omitted() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // Only add unused files, no other issues
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (1)"));
        assert!(!text.contains("Unused exports"));
        assert!(!text.contains("Unused dependencies"));
        assert!(!text.contains("Unresolved imports"));
    }

    // ── Severity levels affect section header indicator ──

    #[test]
    fn section_header_uses_bullet_indicator() {
        // The section header always contains the bullet character
        let header = build_section_header("Test section", 3, Level::Error);
        let text = strip_ansi(&header);
        assert!(text.contains("\u{25cf}"));
        assert!(text.contains("Test section (3)"));
    }

    #[test]
    fn section_header_formats_for_all_levels() {
        // Verify all three levels produce valid headers (not panicking, contain the title)
        for level in [Level::Error, Level::Warn, Level::Info] {
            let header = build_section_header("Items", 7, level);
            let text = strip_ansi(&header);
            assert!(
                text.contains("Items (7)"),
                "Missing title for level {level:?}"
            );
        }
    }

    // ── Grouped sections sort by file path ──

    #[test]
    fn grouped_exports_from_different_files_sorted_by_path() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // Add exports in non-alphabetical order
        results.unused_exports.push(UnusedExport {
            path: root.join("src/z-file.ts"),
            export_name: "zExport".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: root.join("src/a-file.ts"),
            export_name: "aExport".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // a-file should appear before z-file in output
        let a_pos = text.find("src/a-file.ts").unwrap();
        let z_pos = text.find("src/z-file.ts").unwrap();
        assert!(a_pos < z_pos, "Files should be sorted alphabetically");
    }

    // ── File grouping deduplicates file headers ──

    #[test]
    fn grouped_items_from_same_file_share_one_file_header() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..3 {
            results.unused_exports.push(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: format!("fn{i}"),
                is_type_only: false,
                line: (i + 1) as u32,
                col: 0,
                span_start: 0,
                is_re_export: false,
            });
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // "src/utils.ts" should appear exactly once as a group header
        let count = text.matches("src/utils.ts").count();
        assert_eq!(count, 1, "File header should appear once, found {count}");
    }

    // ── Severity affects which sections appear ──

    #[test]
    fn off_severity_still_shows_section_when_items_present() {
        // When severity is Off, the items are normally filtered before reaching
        // the reporter. But if items ARE present, the section should still render
        // (with Info-level styling).
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let rules = RulesConfig {
            unused_files: Severity::Off,
            ..RulesConfig::default()
        };
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (1)"));
    }

    // ── Deeply nested paths display correctly ──

    #[test]
    fn deeply_nested_paths_display_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("packages/ui/src/components/forms/inputs/TextInput.tsx"),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("packages/ui/src/components/forms/inputs/TextInput.tsx"));
    }

    // ── All section types produce output when populated ──

    #[test]
    fn all_issue_types_produce_output_lines() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // Every populated section must produce a header with a count
        assert!(text.contains("Unused files (1)"));
        assert!(text.contains("Unused exports (1)"));
        assert!(text.contains("Unused type exports (1)"));
        assert!(text.contains("Unused dependencies (1)"));
        assert!(text.contains("Unused devDependencies (1)"));
        assert!(text.contains("Unused optionalDependencies (1)"));
        assert!(text.contains("Unused enum members (1)"));
        assert!(text.contains("Unused class members (1)"));
        assert!(text.contains("Unresolved imports (1)"));
        assert!(text.contains("Unlisted dependencies (1)"));
        assert!(text.contains("Duplicate exports (1)"));
        assert!(text.contains("Type-only dependencies (consider moving to devDependencies) (1)"));
        assert!(text.contains(
            "Test-only production dependencies (consider moving to devDependencies) (1)"
        ));
        assert!(text.contains("Circular dependencies (1)"));
    }

    // ── Sections end with empty line separator ──

    #[test]
    fn each_section_ends_with_empty_line_separator() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/a.ts"),
        });
        results.unused_dependencies.push(UnusedDependency {
            package_name: "pkg".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 1,
            used_in_workspaces: Vec::new(),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        // Category headers + issue sections each add an empty line separator.
        // Unused Code header + unused_files + Dependencies header + unused_deps = 4 empty lines.
        let empty_count = lines.iter().filter(|l| l.is_empty()).count();
        assert_eq!(
            empty_count, 4,
            "Expected 4 empty separators (2 category headers + 2 sections), got {empty_count}"
        );
    }

    // ── Type-only dependencies section has specific title ──

    #[test]
    fn type_only_deps_section_title_includes_suggestion() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Type-only dependencies (consider moving to devDependencies)"));
    }

    // ── Warn severity renders with correct indicator for section header ──

    #[test]
    fn warn_severity_produces_header_with_bullet() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        });
        // type_only_dependencies defaults to Warn
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // Verify the section appears with the correct title (the styling differs
        // between Warn and Error, but the structural content is the same)
        assert!(text.contains("\u{25cf}"));
        assert!(text.contains("Type-only dependencies"));
    }

    // ── Unlisted dependencies show package name ──

    #[test]
    fn unlisted_deps_show_package_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "@scope/unknown-pkg".to_string(),
            imported_from: vec![],
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("@scope/unknown-pkg"));
    }

    // ── Hub-grouped circular deps ──

    #[test]
    fn circular_deps_grouped_by_hub() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // Two cycles sharing the same hub file
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/hub.ts"), root.join("src/a.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        results.circular_dependencies.push(CircularDependency {
            files: vec![root.join("src/hub.ts"), root.join("src/b.ts")],
            length: 2,
            line: 5,
            col: 0,
            is_cross_package: false,
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // Should show "(2 cycles)" for the hub
        assert!(text.contains("(2 cycles)"));
        // Hub file appears once
        assert_eq!(text.matches("hub.ts").count(), 3); // header + 2 chain endings
    }

    // ── Summary footer ──

    #[test]
    fn summary_footer_uses_short_labels() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let footer = build_summary_footer(&results, 0, 0);
        // Should use short labels, not "unused file" etc.
        assert!(footer.contains("1 file"));
        assert!(footer.contains("1 export"));
        assert!(footer.contains("1 circular"));
        assert!(!footer.contains("unused file"));
    }

    #[test]
    fn summary_footer_singularizes_pre_pluralized_labels_for_count_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // Add exactly 1 of each pre-pluralized category
        results
            .unused_enum_members
            .push(fallow_core::results::UnusedMember {
                path: root.join("src/types.ts"),
                parent_name: "Status".to_string(),
                member_name: "Unused".to_string(),
                line: 10,
                col: 0,
                kind: MemberKind::EnumMember,
            });
        results
            .unused_class_members
            .push(fallow_core::results::UnusedMember {
                path: root.join("src/foo.ts"),
                parent_name: "Foo".to_string(),
                member_name: "bar".to_string(),
                line: 5,
                col: 0,
                kind: MemberKind::ClassMethod,
            });
        let footer = build_summary_footer(&results, 0, 0);
        // Pre-pluralized labels should be singularized for count=1
        assert!(
            footer.contains("1 enum member"),
            "Expected '1 enum member' but got: {footer}"
        );
        assert!(
            !footer.contains("1 enum members"),
            "Should not contain '1 enum members': {footer}"
        );
        assert!(
            footer.contains("1 class member"),
            "Expected '1 class member' but got: {footer}"
        );
        assert!(
            !footer.contains("1 class members"),
            "Should not contain '1 class members': {footer}"
        );
    }

    // ── Section footers with docs links ──

    #[test]
    fn section_footer_contains_docs_link() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: root.join("src/dead.ts"),
        });
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        // Human output always includes section footers with doc links
        assert!(text.contains("docs.fallow.tools/explanations/dead-code"));
        assert!(text.contains("Files not reachable from any entry point"));
    }

    // ── Truncation tests ──

    #[test]
    fn flat_section_truncates_at_max() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..15 {
            results.unused_files.push(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            });
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("... and 5 more"));
    }

    #[test]
    fn grouped_section_truncates_files() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        // 15 files with 1 export each
        for i in 0..15 {
            results.unused_exports.push(UnusedExport {
                path: root.join(format!("src/file{i:02}.ts")),
                export_name: format!("fn{i}"),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            });
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("... and 5 more in 5 files"));
    }

    // ── --top flag limits items shown ──

    #[test]
    fn top_flag_limits_unused_files_shown() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..5 {
            results.unused_files.push(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            });
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, Some(2));
        let text = plain(&lines);

        // Header still shows the full count
        assert!(text.contains("Unused files (5)"));

        // Only 2 of the 5 files should be listed
        let file_lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("src/dead") && l.contains(".ts"))
            .collect();
        assert_eq!(
            file_lines.len(),
            2,
            "Expected 2 file lines with top=2, got {}: {file_lines:?}",
            file_lines.len()
        );

        // Truncation hint for the remaining 3
        assert!(
            text.contains("... and 3 more"),
            "Expected truncation hint, got:\n{text}"
        );
    }
}
