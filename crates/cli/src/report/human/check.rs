use crate::report::sink::outln;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;
use fallow_config::{RulesConfig, Severity};
use fallow_core::results::{
    AnalysisResults, DuplicateExport, DuplicateExportFinding, TestOnlyDependency,
    TestOnlyDependencyFinding, TypeOnlyDependency, TypeOnlyDependencyFinding,
    UnusedClassMemberFinding, UnusedDependency, UnusedDependencyFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding,
    UnusedMember, UnusedOptionalDependencyFinding, UnusedStoreMemberFinding, UnusedTypeFinding,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    GroupedByFileInput, MAX_FLAT_ITEMS, build_grouped_by_file, build_section_header, format_path,
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
fn namespace_barrel_match_ratio(items: &[DuplicateExportFinding]) -> f32 {
    let renderable: Vec<&DuplicateExport> = items
        .iter()
        .map(|d| &d.export)
        .filter(|d| d.locations.len() >= 2)
        .collect();
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
fn should_show_namespace_barrel_hint(items: &[DuplicateExportFinding]) -> bool {
    let renderable_count = items
        .iter()
        .filter(|d| d.export.locations.len() >= 2)
        .count();
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
    if lines.last().is_some_and(String::is_empty) {
        let pos = lines.len() - 1;
        lines.insert(pos, annotation);
    } else {
        lines.push(annotation);
    }
}

pub(in crate::report) struct PrintHumanInput<'a> {
    pub results: &'a AnalysisResults,
    pub root: &'a Path,
    pub rules: &'a RulesConfig,
    pub elapsed: Duration,
    pub quiet: bool,
    pub top: Option<usize>,
    pub show_explain_tip: bool,
    pub explain: bool,
}

pub(in crate::report) fn print_human(input: &PrintHumanInput<'_>) {
    if !input.quiet {
        eprintln!();
        emit_config_quality_signal(input.results, input.root);
    }

    let total = input.results.total_issues();
    print_explain_tip_if_tty(input.show_explain_tip && total > 0, input.quiet);

    for line in build_human_lines_with_explain(
        input.results,
        input.root,
        input.rules,
        input.top,
        input.explain,
    ) {
        outln!("{line}");
    }

    if !input.quiet {
        if total == 0 {
            eprintln!(
                "{}",
                format!(
                    "\u{2713} No issues found ({:.2}s)",
                    input.elapsed.as_secs_f64()
                )
                .green()
                .bold()
            );
        } else {
            let unused_file_set: FxHashSet<&std::path::Path> = input
                .results
                .unused_files
                .iter()
                .map(|f| f.file.path.as_path())
                .collect();
            let suppressed_exports = input
                .results
                .unused_exports
                .iter()
                .filter(|e| unused_file_set.contains(e.export.path.as_path()))
                .count();
            let suppressed_types = input
                .results
                .unused_types
                .iter()
                .filter(|e| unused_file_set.contains(e.export.path.as_path()))
                .count();
            let summary = build_summary_footer(input.results, suppressed_exports, suppressed_types);
            eprintln!(
                "{}",
                format!("\u{2717} {summary} ({:.2}s)", input.elapsed.as_secs_f64())
                    .red()
                    .bold()
            );
            print_suppression_footer(input.results);
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
#[cfg(test)]
pub(in crate::report) fn build_human_lines(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    top: Option<usize>,
) -> Vec<String> {
    build_human_lines_with_explain(results, root, rules, top, false)
}

fn build_human_lines_with_explain(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    top: Option<usize>,
    explain: bool,
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
    build_policy_section(&mut lines, results, root, rules, total_issues);
    build_maintenance_section(&mut lines, results, root, rules, total_issues);

    if explain {
        inject_explain_blocks(lines)
    } else {
        lines
    }
}

fn inject_explain_blocks(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let explain = check_explain_for_header(&line);
        out.push(line);
        if let Some(rule) = explain {
            out.push(format!(
                "  {}",
                format!("Description: {}", rule.full).dimmed()
            ));
        }
    }
    out
}

fn check_explain_for_header(line: &str) -> Option<&'static crate::explain::RuleDef> {
    let mappings = [
        ("Unused files", "fallow/unused-file"),
        ("Unused exports", "fallow/unused-export"),
        ("Unused type exports", "fallow/unused-type"),
        ("Private type leaks", "fallow/private-type-leak"),
        ("Unused dependencies", "fallow/unused-dependency"),
        ("Unused devDependencies", "fallow/unused-dev-dependency"),
        (
            "Unused optionalDependencies",
            "fallow/unused-optional-dependency",
        ),
        ("Type-only dependencies", "fallow/type-only-dependency"),
        (
            "Test-only production dependencies",
            "fallow/test-only-dependency",
        ),
        ("Unused enum members", "fallow/unused-enum-member"),
        ("Unused class members", "fallow/unused-class-member"),
        ("Unused store members", "fallow/unused-store-member"),
        ("Unresolved imports", "fallow/unresolved-import"),
        ("Unlisted dependencies", "fallow/unlisted-dependency"),
        ("Duplicate exports", "fallow/duplicate-export"),
        ("Circular dependencies", "fallow/circular-dependency"),
        ("Re-Export Cycles", "fallow/re-export-cycle"),
        ("Boundary violations", "fallow/boundary-violation"),
        ("Stale suppressions", "fallow/stale-suppression"),
        ("Unused catalog entries", "fallow/unused-catalog-entry"),
        ("Empty catalog groups", "fallow/empty-catalog-group"),
        (
            "Unresolved catalog references",
            "fallow/unresolved-catalog-reference",
        ),
        (
            "Unused dependency overrides",
            "fallow/unused-dependency-override",
        ),
        (
            "Misconfigured dependency overrides",
            "fallow/misconfigured-dependency-override",
        ),
        ("Invalid client exports", "fallow/invalid-client-export"),
        (
            "Mixed client/server barrels",
            "fallow/mixed-client-server-barrel",
        ),
        ("Unprovided injects", "fallow/unprovided-inject"),
        ("Unrendered components", "fallow/unrendered-component"),
        ("Misplaced directives", "fallow/misplaced-directive"),
    ];
    let (_, rule_id) = mappings
        .iter()
        .find(|(title, _)| line.contains(&format!("{title} (")))?;
    crate::explain::rule_by_id(rule_id)
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

fn format_private_type_leak(
    entry: &fallow_types::output_dead_code::PrivateTypeLeakFinding,
) -> String {
    let e = &entry.leak;
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

impl NamedPkgDep for UnusedDependencyFinding {
    fn pkg_name(&self) -> &str {
        &self.dep.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.dep.path
    }
    fn used_in_workspaces(&self) -> &[PathBuf] {
        &self.dep.used_in_workspaces
    }
}

impl NamedPkgDep for UnusedDevDependencyFinding {
    fn pkg_name(&self) -> &str {
        &self.dep.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.dep.path
    }
    fn used_in_workspaces(&self) -> &[PathBuf] {
        &self.dep.used_in_workspaces
    }
}

impl NamedPkgDep for UnusedOptionalDependencyFinding {
    fn pkg_name(&self) -> &str {
        &self.dep.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.dep.path
    }
    fn used_in_workspaces(&self) -> &[PathBuf] {
        &self.dep.used_in_workspaces
    }
}

impl NamedPkgDep for TypeOnlyDependencyFinding {
    fn pkg_name(&self) -> &str {
        &self.dep.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.dep.path
    }
}

impl NamedPkgDep for TestOnlyDependencyFinding {
    fn pkg_name(&self) -> &str {
        &self.dep.package_name
    }
    fn pkg_path(&self) -> &Path {
        &self.dep.path
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
        .map(|f| f.file.path.as_path())
        .collect();
    let filtered_exports: Vec<UnusedExportFinding> = results
        .unused_exports
        .iter()
        .filter(|e| !unused_file_set.contains(e.export.path.as_path()))
        .cloned()
        .collect();
    let filtered_types: Vec<UnusedTypeFinding> = results
        .unused_types
        .iter()
        .filter(|e| !unused_file_set.contains(e.export.path.as_path()))
        .cloned()
        .collect();
    let suppressed_exports = results.unused_exports.len() - filtered_exports.len();
    let suppressed_types = results.unused_types.len() - filtered_types.len();

    let has_unused_code = !results.unused_files.is_empty()
        || !filtered_exports.is_empty()
        || !filtered_types.is_empty()
        || !results.private_type_leaks.is_empty()
        || !results.unused_enum_members.is_empty()
        || !results.unused_class_members.is_empty()
        || !results.unused_store_members.is_empty();
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
                let path_str = relative_path(&file.file.path, root).display().to_string();
                vec![format!("  {}", format_path(&path_str))]
            },
        );
    }
    insert_test_src_split(lines, &results.unused_files, |f| &f.file.path);

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &filtered_exports,
        title: "Unused exports",
        level: severity_to_level(rules.unused_exports),
        root,
        max_files: max_grouped_files,
        get_path: |e| e.export.path.as_path(),
        format_detail: &|e: &UnusedExportFinding| format_unused_export(&e.export),
    });
    push_suppressed_count_note(lines, suppressed_exports);
    insert_test_src_split(lines, &filtered_exports, |e| &e.export.path);

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &filtered_types,
        title: "Unused type exports",
        level: severity_to_level(rules.unused_types),
        root,
        max_files: max_grouped_files,
        get_path: |e| e.export.path.as_path(),
        format_detail: &|e: &UnusedTypeFinding| format_unused_export(&e.export),
    });
    push_suppressed_count_note(lines, suppressed_types);

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.private_type_leaks,
        title: "Private type leaks",
        level: severity_to_level(rules.private_type_leaks),
        root,
        max_files: max_grouped_files,
        get_path: |e| e.leak.path.as_path(),
        format_detail: &format_private_type_leak,
    });

    build_unused_member_sections(lines, results, root, rules, max_grouped_files);
}

fn build_unused_member_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_grouped_files: usize,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_enum_members,
        title: "Unused enum members",
        level: severity_to_level(rules.unused_enum_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedEnumMemberFinding| format_unused_member(&m.member),
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_class_members,
        title: "Unused class members",
        level: severity_to_level(rules.unused_class_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedClassMemberFinding| format_unused_member(&m.member),
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_store_members,
        title: "Unused store members",
        level: severity_to_level(rules.unused_store_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedStoreMemberFinding| format_unused_member(&m.member),
    });
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
    if !has_dependency_findings(results) {
        return;
    }
    push_category_header(lines, "Dependencies");

    push_package_dependency_sections(lines, results, root, rules, max_items, total_issues);
    push_import_dependency_sections(ImportDependencySectionInput {
        lines,
        results,
        root,
        rules,
        max_items,
        max_grouped_files,
        total_issues,
    });
    push_catalog_dependency_sections(lines, results, root, rules, max_items, total_issues);
    push_dependency_override_sections(lines, results, root, rules, max_items, total_issues);
}

fn has_dependency_findings(results: &AnalysisResults) -> bool {
    !results.unused_dependencies.is_empty()
        || !results.unused_dev_dependencies.is_empty()
        || !results.unused_optional_dependencies.is_empty()
        || !results.unresolved_imports.is_empty()
        || !results.unlisted_dependencies.is_empty()
        || !results.type_only_dependencies.is_empty()
        || !results.test_only_dependencies.is_empty()
        || !results.unused_catalog_entries.is_empty()
        || !results.empty_catalog_groups.is_empty()
        || !results.unresolved_catalog_references.is_empty()
        || !results.unused_dependency_overrides.is_empty()
        || !results.misconfigured_dependency_overrides.is_empty()
}

fn push_package_dependency_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_items: usize,
    total_issues: usize,
) {
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
}

struct ImportDependencySectionInput<'a> {
    lines: &'a mut Vec<String>,
    results: &'a AnalysisResults,
    root: &'a Path,
    rules: &'a RulesConfig,
    max_items: usize,
    max_grouped_files: usize,
    total_issues: usize,
}

fn push_import_dependency_sections(input: ImportDependencySectionInput<'_>) {
    let ImportDependencySectionInput {
        lines,
        results,
        root,
        rules,
        max_items,
        max_grouped_files,
        total_issues,
    } = input;

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unresolved_imports,
        title: "Unresolved imports",
        level: severity_to_level(rules.unresolved_imports),
        root,
        max_files: max_grouped_files,
        get_path: |i| i.import.path.as_path(),
        format_detail: &|i| {
            format!(
                "{} {}",
                format!(":{}", i.import.line).dimmed(),
                i.import.specifier.bold()
            )
        },
    });
    build_human_section_ex(
        lines,
        &results.unlisted_dependencies,
        "Unlisted dependencies",
        severity_to_level(rules.unlisted_dependencies),
        max_items,
        total_issues,
        |dep| vec![format!("  {}", dep.dep.package_name.bold())],
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
}

fn push_catalog_dependency_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_items: usize,
    total_issues: usize,
) {
    push_unused_catalog_entries_section(
        lines,
        &results.unused_catalog_entries,
        rules.unused_catalog_entries,
        max_items,
        total_issues,
        root,
    );
    push_empty_catalog_groups_section(
        lines,
        &results.empty_catalog_groups,
        rules.empty_catalog_groups,
        max_items,
        total_issues,
        root,
    );
    push_unresolved_catalog_references_section(
        lines,
        &results.unresolved_catalog_references,
        rules.unresolved_catalog_references,
        max_items,
        total_issues,
        root,
    );
}

fn push_dependency_override_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_items: usize,
    total_issues: usize,
) {
    push_unused_dependency_overrides_section(
        lines,
        &results.unused_dependency_overrides,
        rules.unused_dependency_overrides,
        max_items,
        total_issues,
        root,
    );
    push_misconfigured_dependency_overrides_section(
        lines,
        &results.misconfigured_dependency_overrides,
        rules.misconfigured_dependency_overrides,
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
    entries: &[fallow_core::results::UnusedCatalogEntryFinding],
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
            let entry = &entry.entry;
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

fn push_empty_catalog_groups_section(
    lines: &mut Vec<String>,
    groups: &[fallow_core::results::EmptyCatalogGroupFinding],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if groups.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        lines,
        groups,
        "Empty catalog groups",
        level,
        max_items,
        total_issues,
        |group| {
            let group = &group.group;
            let path_display = root.join(&group.path);
            vec![format!(
                "  {catalog}  {loc}",
                catalog = group.catalog_name.bold(),
                loc = format!(
                    "{}:{}",
                    path_display
                        .strip_prefix(root)
                        .unwrap_or(&path_display)
                        .display(),
                    group.line
                )
                .dimmed(),
            )]
        },
    );
}

/// Render unresolved pnpm catalog references using the same two-tier shape as
/// `unused-catalog-entries`: a headline `entry_name  catalog_name  path:line`
/// row, then an indented "not in catalog ...; available in: ..." second line.
/// The default catalog gets a special case: the indented text reads "not in the
/// default catalog" instead of "not in catalog 'default'" because users who
/// write bare `catalog:` think of it as "the catalog", not as a named one.
fn push_unresolved_catalog_references_section(
    lines: &mut Vec<String>,
    findings: &[fallow_core::results::UnresolvedCatalogReferenceFinding],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        lines,
        findings,
        "Unresolved catalog references",
        level,
        max_items,
        total_issues,
        |finding| {
            let finding = &finding.reference;
            let path_display = root.join(&finding.path);
            let catalog_label = if finding.catalog_name == "default" {
                "default".to_string()
            } else {
                finding.catalog_name.clone()
            };
            let row = format!(
                "  {entry_name}  {catalog}  {loc}",
                entry_name = finding.entry_name.bold(),
                catalog = catalog_label.dimmed(),
                loc = format!(
                    "{}:{}",
                    path_display
                        .strip_prefix(root)
                        .unwrap_or(&path_display)
                        .display(),
                    finding.line
                )
                .dimmed(),
            );
            let mut out = vec![row];
            let detail = if finding.catalog_name == "default" {
                "not in the default catalog".to_string()
            } else {
                format!("not in catalog '{}'", finding.catalog_name)
            };
            let detail_line = if finding.available_in_catalogs.is_empty() {
                format!("    {}", detail.dimmed())
            } else {
                format!(
                    "    {}; available in: {}",
                    detail.dimmed(),
                    finding.available_in_catalogs.join(", ").bold(),
                )
            };
            out.push(detail_line);
            if finding.available_in_catalogs.len() == 1 {
                let target = &finding.available_in_catalogs[0];
                out.push(format!(
                    "    {}",
                    format!("Suggested: switch to `catalog:{target}`").dimmed(),
                ));
            }
            out
        },
    );
}

/// Render unused pnpm dependency overrides as a two-tier block: a headline row
/// shows `raw_key  source  path:line`, then an indented detail row shows the
/// forced version, target package, and optional CVE hint that the
/// conservative-static algorithm flags.
fn push_unused_dependency_overrides_section(
    lines: &mut Vec<String>,
    findings: &[fallow_core::results::UnusedDependencyOverrideFinding],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        lines,
        findings,
        "Unused dependency overrides",
        level,
        max_items,
        total_issues,
        |finding| {
            let finding = &finding.entry;
            let path_display = root.join(&finding.path);
            let row = format!(
                "  {key}  {source}  {loc}",
                key = finding.raw_key.bold(),
                source = finding.source.as_label().dimmed(),
                loc = format!(
                    "{}:{}",
                    path_display
                        .strip_prefix(root)
                        .unwrap_or(&path_display)
                        .display(),
                    finding.line
                )
                .dimmed(),
            );
            let mut out = vec![row];
            let detail = format!(
                "forces {} to {}",
                finding.target_package, finding.version_range
            );
            out.push(format!("    {}", detail.dimmed()));
            if let Some(hint) = &finding.hint {
                out.push(format!("    {}", hint.as_str().dimmed()));
            }
            out
        },
    );
}

/// Render misconfigured pnpm dependency overrides as a two-tier block: a
/// headline row shows `raw_key  source  path:line`, then an indented detail
/// row shows the parsed reason. pnpm refuses to install on these shapes so the
/// rule defaults to error.
fn push_misconfigured_dependency_overrides_section(
    lines: &mut Vec<String>,
    findings: &[fallow_core::results::MisconfiguredDependencyOverrideFinding],
    severity: fallow_config::Severity,
    max_items: usize,
    total_issues: usize,
    root: &Path,
) {
    if findings.is_empty() {
        return;
    }
    let level = severity_to_level(severity);
    build_human_section_ex(
        lines,
        findings,
        "Misconfigured dependency overrides",
        level,
        max_items,
        total_issues,
        |finding| {
            let finding = &finding.entry;
            let path_display = root.join(&finding.path);
            let row = format!(
                "  {key}  {source}  {loc}",
                key = finding.raw_key.bold(),
                source = finding.source.as_label().dimmed(),
                loc = format!(
                    "{}:{}",
                    path_display
                        .strip_prefix(root)
                        .unwrap_or(&path_display)
                        .display(),
                    finding.line
                )
                .dimmed(),
            );
            vec![row, format!("    {}", finding.reason.describe().dimmed())]
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
        || !results.re_export_cycles.is_empty()
        || !results.boundary_violations.is_empty()
        || !results.boundary_coverage_violations.is_empty()
        || !results.boundary_call_violations.is_empty();
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
    build_re_export_cycles_section(
        lines,
        &results.re_export_cycles,
        severity_to_level(rules.re_export_cycle),
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
    build_boundary_coverage_violations_section(
        lines,
        &results.boundary_coverage_violations,
        severity_to_level(rules.boundary_violation),
        root,
        total_issues,
    );
    build_boundary_call_violations_section(
        lines,
        &results.boundary_call_violations,
        severity_to_level(rules.boundary_violation),
        root,
        total_issues,
    );
}

/// Render the three opt-in React/Preact component-health grouped sections
/// (prop-drilling, thin-wrapper, duplicate-prop-shape). Each defaults to `off`,
/// so an empty collection renders nothing. Extracted from `build_policy_section`
/// to keep that function under the unit-size ceiling.
fn push_react_component_health_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.prop_drilling_chains,
        title: "Prop drilling",
        level: severity_to_level(rules.prop_drilling),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: prop_drilling_anchor,
        format_detail: &format_prop_drilling_chain,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.thin_wrappers,
        title: "Thin wrappers",
        level: severity_to_level(rules.thin_wrapper),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |w: &fallow_types::output_dead_code::ThinWrapperFinding| w.wrapper.file.as_path(),
        format_detail: &format_thin_wrapper,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.duplicate_prop_shapes,
        title: "Duplicate prop shapes",
        level: severity_to_level(rules.duplicate_prop_shape),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |d: &fallow_types::output_dead_code::DuplicatePropShapeFinding| {
            d.shape.file.as_path()
        },
        format_detail: &format_duplicate_prop_shape,
    });
}

/// Build the Policy category (rule-pack findings). Separate from Structure
/// because policy is user-authored project rules, not architecture analysis.
fn build_policy_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    total_issues: usize,
) {
    if results.policy_violations.is_empty()
        && results.invalid_client_exports.is_empty()
        && results.mixed_client_server_barrels.is_empty()
        && results.misplaced_directives.is_empty()
        && results.unprovided_injects.is_empty()
        && results.unrendered_components.is_empty()
        && results.unused_component_props.is_empty()
        && results.unused_component_emits.is_empty()
        && results.unused_server_actions.is_empty()
        && results.unused_load_data_keys.is_empty()
        && results.route_collisions.is_empty()
        && results.dynamic_segment_name_conflicts.is_empty()
        && results.prop_drilling_chains.is_empty()
        && results.thin_wrappers.is_empty()
        && results.duplicate_prop_shapes.is_empty()
    {
        return;
    }
    push_category_header(lines, "Policy");
    build_policy_violations_section(lines, &results.policy_violations, root, total_issues);
    build_framework_policy_section(lines, results, root, rules);
    build_component_policy_section(lines, results, root, rules);
    build_route_policy_section(lines, results, root, rules);
}

fn build_framework_policy_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.invalid_client_exports,
        title: "Invalid client exports",
        level: severity_to_level(rules.invalid_client_export),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::InvalidClientExportFinding| {
            e.export.path.as_path()
        },
        format_detail: &format_invalid_client_export,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.mixed_client_server_barrels,
        title: "Mixed client/server barrels",
        level: severity_to_level(rules.mixed_client_server_barrel),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |b: &fallow_types::output_dead_code::MixedClientServerBarrelFinding| {
            b.barrel.path.as_path()
        },
        format_detail: &format_mixed_client_server_barrel,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.misplaced_directives,
        title: "Misplaced directives",
        level: severity_to_level(rules.misplaced_directive),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |d: &fallow_types::output_dead_code::MisplacedDirectiveFinding| {
            d.directive_site.path.as_path()
        },
        format_detail: &format_misplaced_directive,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unprovided_injects,
        title: "Unprovided injects",
        level: severity_to_level(rules.unprovided_injects),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |i: &fallow_types::output_dead_code::UnprovidedInjectFinding| {
            i.inject.path.as_path()
        },
        format_detail: &format_unprovided_inject,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unrendered_components,
        title: "Unrendered components",
        level: severity_to_level(rules.unrendered_components),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |c: &fallow_types::output_dead_code::UnrenderedComponentFinding| {
            c.component.path.as_path()
        },
        format_detail: &format_unrendered_component,
    });
}

fn build_component_policy_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_component_props,
        title: "Unused component props",
        level: severity_to_level(rules.unused_component_props),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |p: &fallow_types::output_dead_code::UnusedComponentPropFinding| {
            p.prop.path.as_path()
        },
        format_detail: &format_unused_component_prop,
    });

    push_react_component_health_sections(lines, results, root, rules);

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_component_emits,
        title: "Unused component emits",
        level: severity_to_level(rules.unused_component_emits),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::UnusedComponentEmitFinding| {
            e.emit.path.as_path()
        },
        format_detail: &format_unused_component_emit,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_server_actions,
        title: "Unused server actions",
        level: severity_to_level(rules.unused_server_actions),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |a: &fallow_types::output_dead_code::UnusedServerActionFinding| {
            a.action.path.as_path()
        },
        format_detail: &format_unused_server_action,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_load_data_keys,
        title: "Unused load data keys",
        level: severity_to_level(rules.unused_load_data_keys),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |k: &fallow_types::output_dead_code::UnusedLoadDataKeyFinding| {
            k.key.path.as_path()
        },
        format_detail: &format_unused_load_data_key,
    });
}

/// Render the Next.js App Router route-tree policy sections (route collisions
/// and dynamic-segment name conflicts). Split out of `build_policy_section` to
/// keep that orchestrator under the unit-size limit.
fn build_route_policy_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.route_collisions,
        title: "Route collisions",
        level: severity_to_level(rules.route_collision),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |c: &fallow_types::output_dead_code::RouteCollisionFinding| {
            c.collision.path.as_path()
        },
        format_detail: &format_route_collision,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.dynamic_segment_name_conflicts,
        title: "Dynamic segment conflicts",
        level: severity_to_level(rules.dynamic_segment_name_conflict),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |c: &fallow_types::output_dead_code::DynamicSegmentNameConflictFinding| {
            c.conflict.path.as_path()
        },
        format_detail: &format_dynamic_segment_name_conflict,
    });
}

fn format_invalid_client_export(
    entry: &fallow_types::output_dead_code::InvalidClientExportFinding,
) -> String {
    let e = &entry.export;
    format!(
        "{} {} {}",
        format!(":{}", e.line).dimmed(),
        e.export_name.bold(),
        format!("(from \"{}\")", e.directive).dimmed(),
    )
}

fn format_mixed_client_server_barrel(
    entry: &fallow_types::output_dead_code::MixedClientServerBarrelFinding,
) -> String {
    let b = &entry.barrel;
    format!(
        "{} {}",
        format!(":{}", b.line).dimmed(),
        format!(
            "re-exports client \"{}\" and server-only \"{}\"",
            b.client_origin, b.server_origin
        )
        .dimmed(),
    )
}

fn format_misplaced_directive(
    entry: &fallow_types::output_dead_code::MisplacedDirectiveFinding,
) -> String {
    let d = &entry.directive_site;
    format!(
        "{} {}",
        format!(":{}", d.line).dimmed(),
        format!(
            "\"{}\" is not in the leading position and is ignored",
            d.directive
        )
        .dimmed(),
    )
}

fn format_unprovided_inject(
    entry: &fallow_types::output_dead_code::UnprovidedInjectFinding,
) -> String {
    let i = &entry.inject;
    format!(
        "{} {} {}",
        format!(":{}", i.line).dimmed(),
        i.key_name.bold(),
        format!(
            "has no matching provide({}) in this project; at runtime it returns undefined (provide the key or remove this inject)",
            i.key_name
        )
        .dimmed(),
    )
}

fn format_unrendered_component(
    entry: &fallow_types::output_dead_code::UnrenderedComponentFinding,
) -> String {
    let c = &entry.component;
    format!(
        "{} {} {}",
        format!(":{}", c.line).dimmed(),
        c.component_name.bold(),
        "is reachable but rendered nowhere in this project (render it somewhere or remove it)"
            .dimmed(),
    )
}

fn format_unused_component_prop(
    entry: &fallow_types::output_dead_code::UnusedComponentPropFinding,
) -> String {
    let p = &entry.prop;
    format!(
        "{} {} {}",
        format!(":{}", p.line).dimmed(),
        p.prop_name.bold(),
        "is declared but referenced nowhere in this component (remove it or use it)".dimmed(),
    )
}

/// Anchor a prop-drilling chain to its SOURCE hop file for grouped display.
fn prop_drilling_anchor(
    c: &fallow_types::output_dead_code::PropDrillingChainFinding,
) -> &std::path::Path {
    c.chain
        .hops
        .first()
        .map_or_else(|| std::path::Path::new(""), |h| h.file.as_path())
}

fn format_prop_drilling_chain(
    entry: &fallow_types::output_dead_code::PropDrillingChainFinding,
) -> String {
    let c = &entry.chain;
    let trail = c
        .hops
        .iter()
        .map(|h| h.component.as_str())
        .collect::<Vec<_>>()
        .join(" \u{2192} ");
    let line = c.hops.first().map_or(0, |h| h.line);
    format!(
        "{} {} {}",
        format!(":{line}").dimmed(),
        c.prop.bold(),
        format!(
            "is forwarded unused through {trail} (depth {}); colocate the consumer or lift it to a \
             context at a mid-chain hop",
            c.depth
        )
        .dimmed(),
    )
}

fn format_thin_wrapper(entry: &fallow_types::output_dead_code::ThinWrapperFinding) -> String {
    let w = &entry.wrapper;
    format!(
        "{} {} {}",
        format!(":{}", w.line).dimmed(),
        w.component.bold(),
        format!(
            "is a thin wrapper around {} (candidate for inlining at call sites or deleting)",
            w.child_component
        )
        .dimmed(),
    )
}

fn format_duplicate_prop_shape(
    entry: &fallow_types::output_dead_code::DuplicatePropShapeFinding,
) -> String {
    let d = &entry.shape;
    format!(
        "{} {} {}",
        format!(":{}", d.line).dimmed(),
        d.component.bold(),
        format!(
            "shares an identical prop shape {{{}}} with {} other component(s) (extract a shared Props type)",
            d.shape.join(", "),
            d.group_size.saturating_sub(1)
        )
        .dimmed(),
    )
}

fn format_unused_component_emit(
    entry: &fallow_types::output_dead_code::UnusedComponentEmitFinding,
) -> String {
    let e = &entry.emit;
    format!(
        "{} {} {}",
        format!(":{}", e.line).dimmed(),
        e.emit_name.bold(),
        "is declared but emitted nowhere in this component (remove it or emit it)".dimmed(),
    )
}

fn format_unused_server_action(
    entry: &fallow_types::output_dead_code::UnusedServerActionFinding,
) -> String {
    let a = &entry.action;
    format!(
        "{} {} {}",
        format!(":{}", a.line).dimmed(),
        a.action_name.bold(),
        "is exported from a \"use server\" file but no code in this project references it".dimmed(),
    )
}

fn format_unused_load_data_key(
    entry: &fallow_types::output_dead_code::UnusedLoadDataKeyFinding,
) -> String {
    let k = &entry.key;
    format!(
        "{} {} {}",
        format!(":{}", k.line).dimmed(),
        k.key_name.bold(),
        "is returned from load() but no consumer reads it (sibling +page.svelte data.<key> or \
         project-wide page.data.<key>)"
            .dimmed(),
    )
}

fn format_route_collision(entry: &fallow_types::output_dead_code::RouteCollisionFinding) -> String {
    let c = &entry.collision;
    let others = c.conflicting_paths.len();
    let plural = if others == 1 { "" } else { "s" };
    format!(
        "{}",
        format!(
            "resolves to {} (shared with {others} other route file{plural}; route groups and \
             slots do not change the URL)",
            c.url
        )
        .dimmed(),
    )
}

fn format_dynamic_segment_name_conflict(
    entry: &fallow_types::output_dead_code::DynamicSegmentNameConflictFinding,
) -> String {
    let c = &entry.conflict;
    format!(
        "{}",
        format!(
            "crashes at runtime: different slug names ({}) at the same dynamic path {}; \
             next build passes but the route fails on its first request (rename to one \
             consistent slug)",
            c.conflicting_segments.join(" vs "),
            c.position
        )
        .dimmed(),
    )
}

/// Build the rule-pack policy-violation section. The header level reflects
/// the EFFECTIVE per-finding severities (error when any finding is error),
/// because rule-level `severity` overrides the `policy-violation` master.
fn build_policy_violations_section(
    lines: &mut Vec<String>,
    items: &[fallow_types::output_dead_code::PolicyViolationFinding],
    root: &Path,
    total_issues: usize,
) {
    use fallow_types::results::PolicyViolationSeverity;

    if items.is_empty() {
        return;
    }
    let level = if items
        .iter()
        .any(|f| f.violation.severity == PolicyViolationSeverity::Error)
    {
        Level::Error
    } else {
        Level::Warn
    };
    let title = "Policy violations";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for entry in &items[..shown] {
        let v = &entry.violation;
        let path = relative_path(&v.path, root).display().to_string();
        let detail = match &v.message {
            Some(message) => format!("banned by `{}/{}`: {message}", v.pack, v.rule_id),
            None => format!("banned by `{}/{}`", v.pack, v.rule_id),
        };
        lines.push(format!(
            "  {}:{} {} {}",
            path,
            v.line,
            v.matched,
            detail.dimmed(),
        ));
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    lines.push(format!(
        "  {}",
        "suppress: // fallow-ignore-next-line policy-violation:<pack>/<rule-id> (or policy-violation for every rule-pack rule)"
            .dimmed()
    ));
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
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
    unused_files: &[fallow_types::output_dead_code::UnusedFileFinding],
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

    let dir_counts = unused_file_dir_counts(unused_files, root);

    let total = unused_files.len();
    let dominant = dir_counts
        .iter()
        .find(|(_, count, is_dir)| *is_dir && count * 100 / total.max(1) > 80)
        .map(|(dir, _, _)| dir.clone());

    let display_entries =
        unused_file_display_entries(unused_files, root, &dir_counts, dominant.as_deref());

    render_dir_rollup_entries(lines, &display_entries, total_issues);
    push_section_footer_rollup(lines, title, unused_files.len());
    lines.push(String::new());
}

fn unused_file_dir_counts(
    unused_files: &[fallow_types::output_dead_code::UnusedFileFinding],
    root: &Path,
) -> Vec<(String, usize, bool)> {
    let mut dir_counts: Vec<(String, usize, bool)> = Vec::new();
    let mut dir_map: FxHashMap<String, usize> = FxHashMap::default();
    for f in unused_files {
        let rel = relative_path(&f.file.path, root);
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
    dir_counts
}

fn unused_file_display_entries(
    unused_files: &[fallow_types::output_dead_code::UnusedFileFinding],
    root: &Path,
    dir_counts: &[(String, usize, bool)],
    dominant: Option<&str>,
) -> Vec<(String, usize, bool)> {
    let Some(dom_dir) = dominant else {
        return dir_counts.to_vec();
    };

    let mut sub_counts: Vec<(String, usize, bool)> = Vec::new();
    let mut sub_map: FxHashMap<String, usize> = FxHashMap::default();
    for f in unused_files {
        let rel = relative_path(&f.file.path, root);
        let mut components = rel.components();
        let first = components
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string());
        if first.as_deref() == Some(dom_dir) {
            let sub_key = components.next().map_or_else(
                || dom_dir.to_string(),
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
    let mut combined = sub_counts;
    for entry in dir_counts {
        if entry.0 != dom_dir {
            combined.push(entry.clone());
        }
    }
    combined
}

fn render_dir_rollup_entries(
    lines: &mut Vec<String>,
    display_entries: &[(String, usize, bool)],
    total_issues: usize,
) {
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
        let hint = if remaining > SCOPING_HINT_THRESHOLD || total_issues > SCOPING_HINT_THRESHOLD {
            format!(
                "... and {remaining} more director{}, try --workspace <name> or --changed-since main to scope",
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
struct GroupedSectionInput<'a, T, P, F>
where
    P: Fn(&'a T) -> &'a Path,
    F: Fn(&T) -> String,
{
    lines: &'a mut Vec<String>,
    items: &'a [T],
    title: &'a str,
    level: Level,
    root: &'a Path,
    max_files: usize,
    get_path: P,
    format_detail: &'a F,
}

fn build_human_grouped_section<'a, T, P, F>(input: GroupedSectionInput<'a, T, P, F>)
where
    P: Fn(&'a T) -> &'a Path,
    F: Fn(&T) -> String,
{
    let GroupedSectionInput {
        lines,
        items,
        title,
        level,
        root,
        max_files,
        get_path,
        format_detail,
    } = input;
    if items.is_empty() {
        return;
    }
    lines.push(build_section_header(title, items.len(), level));
    build_grouped_by_file(GroupedByFileInput {
        lines,
        items,
        root,
        get_path,
        format_detail,
        max_files,
        max_items_per_file: MAX_ITEMS_PER_FILE,
    });
    push_section_footer_with_count(lines, title, items.len());
    lines.push(String::new());
}

/// Build duplicate exports grouped by file pair instead of flat list.
fn build_duplicate_exports_section(
    lines: &mut Vec<String>,
    items: &[fallow_core::results::DuplicateExportFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Duplicate exports";
    lines.push(build_section_header(title, items.len(), level));

    let mut pair_groups: Vec<(String, String, Vec<&str>)> = Vec::new();
    let mut pair_map: rustc_hash::FxHashMap<(String, String), usize> =
        rustc_hash::FxHashMap::default();

    for dup in items {
        let dup = &dup.export;
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
    items: &[fallow_types::output_dead_code::CircularDependencyFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Circular dependencies";
    lines.push(build_section_header(title, items.len(), level));

    let hub_groups = circular_dependency_hub_groups(items, root);
    let shown = hub_groups.len().min(MAX_FLAT_ITEMS);
    for (hub_path, cycles) in &hub_groups[..shown] {
        lines.push(circular_dependency_hub_line(hub_path, cycles.len()));
        for cycle in cycles {
            lines.push(circular_dependency_cycle_line(hub_path, cycle, root));
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

fn circular_dependency_hub_groups<'a>(
    items: &'a [fallow_types::output_dead_code::CircularDependencyFinding],
    root: &Path,
) -> Vec<(String, Vec<&'a fallow_core::results::CircularDependency>)> {
    let mut hub_groups: Vec<(String, Vec<&'a fallow_core::results::CircularDependency>)> =
        Vec::new();
    let mut hub_map: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();

    for entry in items {
        let cycle = &entry.cycle;
        let hub = cycle
            .files
            .first()
            .map(|path| relative_path(path, root).display().to_string())
            .unwrap_or_default();
        if let Some(&idx) = hub_map.get(&hub) {
            hub_groups[idx].1.push(cycle);
        } else {
            hub_map.insert(hub.clone(), hub_groups.len());
            hub_groups.push((hub, vec![cycle]));
        }
    }

    hub_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    hub_groups
}

fn circular_dependency_hub_line(hub_path: &str, cycle_count: usize) -> String {
    let count_tag = if cycle_count > 1 {
        format!(" ({cycle_count} cycles)").dimmed().to_string()
    } else {
        String::new()
    };
    format!("  {}{}", format_path(hub_path), count_tag)
}

fn circular_dependency_cycle_line(
    hub_path: &str,
    cycle: &fallow_core::results::CircularDependency,
    root: &Path,
) -> String {
    let rel_paths: Vec<String> = cycle
        .files
        .iter()
        .map(|path| relative_path(path, root).display().to_string())
        .collect();
    let mut chain_parts: Vec<String> = rel_paths[1..]
        .iter()
        .map(|path| format_path(elide_common_prefix(hub_path, path)))
        .collect();
    let (_, hub_filename) = split_dir_filename(hub_path);
    chain_parts.push(hub_filename.bold().to_string());

    format!(
        "    {} {}{}{}",
        "\u{2192}".dimmed(),
        chain_parts.join(&format!(" {} ", "\u{2192}".dimmed())),
        circular_type_only_tag(cycle),
        circular_cross_package_tag(cycle),
    )
}

fn circular_type_only_tag(cycle: &fallow_core::results::CircularDependency) -> String {
    if cycle
        .files
        .iter()
        .all(|path| path.to_str().is_some_and(|s| s.ends_with(".d.ts")))
    {
        format!(" {}", "(type-only)".dimmed())
    } else {
        String::new()
    }
}

fn circular_cross_package_tag(cycle: &fallow_core::results::CircularDependency) -> String {
    if cycle.is_cross_package {
        format!(" {}", "(cross-package)".dimmed())
    } else {
        String::new()
    }
}

/// Build re-export cycles section. Each finding renders one path-list block
/// per member, sized as "Cycle (N files)" for multi-node SCCs or
/// "Self-loop (1 file)" for the single-file self-re-export case. The fix
/// hint sits on the second line; the docs link is appended after the path
/// list (matches the SARIF helpUri target so users land on the same anchor
/// from any surface).
fn build_re_export_cycles_section(
    lines: &mut Vec<String>,
    items: &[fallow_types::output_dead_code::ReExportCycleFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Re-Export Cycles";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for entry in &items[..shown] {
        let cycle = &entry.cycle;
        let first_path = cycle
            .files
            .first()
            .map(|p| relative_path(p, root).display().to_string())
            .unwrap_or_default();
        lines.push(format!("  {}", format_path(&first_path)));
        let header_line = match cycle.kind {
            fallow_core::results::ReExportCycleKind::SelfLoop => "Self-loop (1 file):".to_string(),
            fallow_core::results::ReExportCycleKind::MultiNode => {
                format!("Cycle ({} files):", cycle.files.len())
            }
        };
        lines.push(format!("    {}", header_line.dimmed()));
        for path in &cycle.files {
            let rel = relative_path(path, root).display().to_string();
            lines.push(format!("      - {}", format_path(&rel)));
        }
        let fix_hint = match cycle.kind {
            fallow_core::results::ReExportCycleKind::SelfLoop => {
                "To fix: remove the `export * from './'` (or equivalent) inside this file."
            }
            fallow_core::results::ReExportCycleKind::MultiNode => {
                "To fix: remove one `export * from` statement on any member file."
            }
        };
        lines.push(format!("    {}", fix_hint.dimmed()));
        lines.push(String::new());
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
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
    items: &[fallow_types::output_dead_code::BoundaryViolationFinding],
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
    for entry in &items[..shown] {
        let v = &entry.violation;
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

/// Build boundary coverage section for files matched by no zone.
fn build_boundary_coverage_violations_section(
    lines: &mut Vec<String>,
    items: &[fallow_types::output_dead_code::BoundaryCoverageViolationFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Boundary coverage";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for entry in &items[..shown] {
        let v = &entry.violation;
        let path = relative_path(&v.path, root).display().to_string();
        lines.push(format!(
            "  {}:{} {}",
            path,
            v.line,
            "no matching boundary zone".dimmed(),
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

/// Build the forbidden-call section. Renders the written callee path next to
/// the matched pattern and the zone, so users learn the segment-aware
/// matching rule from the output itself.
fn build_boundary_call_violations_section(
    lines: &mut Vec<String>,
    items: &[fallow_types::output_dead_code::BoundaryCallViolationFinding],
    level: Level,
    root: &Path,
    total_issues: usize,
) {
    if items.is_empty() {
        return;
    }
    let title = "Boundary calls";
    lines.push(build_section_header(title, items.len(), level));

    let shown = items.len().min(MAX_FLAT_ITEMS);
    for entry in &items[..shown] {
        let v = &entry.violation;
        let path = relative_path(&v.path, root).display().to_string();
        lines.push(format!(
            "  {}:{} {} {}",
            path,
            v.line,
            v.callee,
            format!("matches forbidden `{}` in zone '{}'", v.pattern, v.zone).dimmed(),
        ));
    }
    if items.len() > MAX_FLAT_ITEMS {
        let remaining = items.len() - MAX_FLAT_ITEMS;
        lines.push(format!(
            "  {}",
            truncation_hint(remaining, total_issues).dimmed()
        ));
    }
    // The rule id is boundary-call-violation but the suppression token is the
    // boundary FAMILY token, so spell it out; users would otherwise derive the
    // wrong token by analogy with every finding where rule id and token align.
    lines.push(format!(
        "  {}",
        "suppress: // fallow-ignore-next-line boundary-violation (one token covers all boundary findings)"
            .dimmed()
    ));
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
    collect_dead_code_rules(&mut rules, results, root, resolver);
    collect_graph_rules(&mut rules, results, root, resolver);
    collect_boundary_rules(&mut rules, results, root, resolver);
    collect_framework_rules(&mut rules, results, root, resolver);
    collect_suppression_rules(&mut rules, results, root, resolver);

    let mut sorted: Vec<String> = rules.into_iter().collect();
    sorted.sort();
    sorted.truncate(3);
    sorted
}

fn insert_matching_rule(
    rules: &mut FxHashSet<String>,
    path: &Path,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    if let (_, Some(rule)) = resolver.resolve_with_rule(relative_path(path, root)) {
        rules.insert(rule);
    }
}

fn collect_dead_code_rules(
    rules: &mut FxHashSet<String>,
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    for f in &results.unused_files {
        insert_matching_rule(rules, &f.file.path, root, resolver);
    }
    for e in &results.unused_exports {
        insert_matching_rule(rules, &e.export.path, root, resolver);
    }
    for e in &results.unused_types {
        insert_matching_rule(rules, &e.export.path, root, resolver);
    }
    for e in &results.private_type_leaks {
        insert_matching_rule(rules, &e.leak.path, root, resolver);
    }
    for m in &results.unused_enum_members {
        insert_matching_rule(rules, &m.member.path, root, resolver);
    }
    for m in &results.unused_class_members {
        insert_matching_rule(rules, &m.member.path, root, resolver);
    }
    for m in &results.unused_store_members {
        insert_matching_rule(rules, &m.member.path, root, resolver);
    }
}

fn collect_graph_rules(
    rules: &mut FxHashSet<String>,
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    for u in &results.unresolved_imports {
        insert_matching_rule(rules, &u.import.path, root, resolver);
    }
    for c in &results.circular_dependencies {
        if let Some(first) = c.cycle.files.first() {
            insert_matching_rule(rules, first, root, resolver);
        }
    }
}

fn collect_boundary_rules(
    rules: &mut FxHashSet<String>,
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    for b in &results.boundary_violations {
        insert_matching_rule(rules, &b.violation.from_path, root, resolver);
    }
    for b in &results.boundary_coverage_violations {
        insert_matching_rule(rules, &b.violation.path, root, resolver);
    }
    for b in &results.boundary_call_violations {
        insert_matching_rule(rules, &b.violation.path, root, resolver);
    }
    for v in &results.policy_violations {
        insert_matching_rule(rules, &v.violation.path, root, resolver);
    }
}

fn collect_framework_rules(
    rules: &mut FxHashSet<String>,
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    for e in &results.invalid_client_exports {
        insert_matching_rule(rules, &e.export.path, root, resolver);
    }
    for b in &results.mixed_client_server_barrels {
        insert_matching_rule(rules, &b.barrel.path, root, resolver);
    }
    for d in &results.misplaced_directives {
        insert_matching_rule(rules, &d.directive_site.path, root, resolver);
    }
    for i in &results.unprovided_injects {
        insert_matching_rule(rules, &i.inject.path, root, resolver);
    }
    for c in &results.unrendered_components {
        insert_matching_rule(rules, &c.component.path, root, resolver);
    }
    for p in &results.unused_component_props {
        insert_matching_rule(rules, &p.prop.path, root, resolver);
    }
    for e in &results.unused_component_emits {
        insert_matching_rule(rules, &e.emit.path, root, resolver);
    }
    for a in &results.unused_server_actions {
        insert_matching_rule(rules, &a.action.path, root, resolver);
    }
    for k in &results.unused_load_data_keys {
        insert_matching_rule(rules, &k.key.path, root, resolver);
    }
    for c in &results.route_collisions {
        insert_matching_rule(rules, &c.collision.path, root, resolver);
    }
    for c in &results.dynamic_segment_name_conflicts {
        insert_matching_rule(rules, &c.conflict.path, root, resolver);
    }
}

fn collect_suppression_rules(
    rules: &mut FxHashSet<String>,
    results: &AnalysisResults,
    root: &Path,
    resolver: &OwnershipResolver,
) {
    for s in &results.stale_suppressions {
        insert_matching_rule(rules, &s.path, root, resolver);
    }
}

/// Print analysis results grouped by owner or directory.
///
/// Each group gets a colored header with its key and issue count, followed by
/// the same section output that `print_human` produces. Unowned groups get
/// an advisory footer. Doc URL footers are deduplicated across groups.
pub(in crate::report) struct PrintGroupedHumanInput<'a> {
    pub(in crate::report) groups: &'a [crate::report::grouping::ResultGroup],
    pub(in crate::report) root: &'a Path,
    pub(in crate::report) rules: &'a RulesConfig,
    pub(in crate::report) elapsed: Duration,
    pub(in crate::report) quiet: bool,
    pub(in crate::report) resolver: Option<&'a OwnershipResolver>,
    pub(in crate::report) explain: bool,
}

fn grouped_issue_counts(groups: &[crate::report::grouping::ResultGroup]) -> Vec<(&str, usize)> {
    let mut group_counts: Vec<(&str, usize)> = groups
        .iter()
        .map(|g| (g.key.as_str(), g.results.total_issues()))
        .filter(|(_, count)| *count > 0)
        .collect();
    group_counts.sort_by_key(|b| std::cmp::Reverse(b.1));
    group_counts
}

fn emit_grouped_summary(group_counts: &[(&str, usize)]) {
    if group_counts.is_empty() {
        return;
    }

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
    outln!("{}", summary.dimmed());
    outln!();
}

fn grouped_header_text(
    group: &crate::report::grouping::ResultGroup,
    root: &Path,
    resolver: Option<&OwnershipResolver>,
    total: usize,
) -> String {
    let issue_word = if total == 1 { "issue" } else { "issues" };
    let breakdown = build_summary_footer(&group.results, 0, 0);
    let header_text = if breakdown.is_empty() {
        format!("{} ({total} {issue_word})", group.key)
    } else {
        format!("{} ({total} {issue_word}: {breakdown})", group.key)
    };

    match resolver {
        Some(r @ OwnershipResolver::Owner(_)) => {
            let matched = collect_matching_rules(&group.results, root, r);
            if matched.is_empty() {
                header_text
            } else {
                format!("{header_text}, matched by {}", matched.join(", "))
            }
        }
        _ => header_text,
    }
}

fn emit_grouped_body(
    group: &crate::report::grouping::ResultGroup,
    root: &Path,
    rules: &RulesConfig,
    seen_footers: &mut FxHashSet<String>,
    explain: bool,
) {
    if let Some(ref owners) = group.owners
        && !owners.is_empty()
    {
        outln!("  {} {}", "owners:".dimmed(), owners.join(" ").dimmed());
    }

    let lines = build_human_lines_with_explain(&group.results, root, rules, None, explain);
    for line in &lines {
        if line.contains("docs.fallow.tools") && !seen_footers.insert(line.clone()) {
            continue;
        }
        outln!("{line}");
    }

    if group.key == crate::codeowners::UNOWNED_LABEL {
        eprintln!(
            "  {}",
            "Files with no CODEOWNERS entry, add ownership or verify before removing".dimmed()
        );
        eprintln!();
    }
}

fn emit_grouped_final_status(
    groups: &[crate::report::grouping::ResultGroup],
    grand_total: usize,
    elapsed: Duration,
) {
    if grand_total == 0 {
        eprintln!(
            "{}",
            format!("\u{2713} No issues found ({:.2}s)", elapsed.as_secs_f64())
                .green()
                .bold()
        );
    } else {
        let non_empty_groups = groups
            .iter()
            .filter(|g| g.results.total_issues() > 0)
            .count();
        eprintln!(
            "{}",
            format!(
                "\u{2717} {grand_total} issue{} across {non_empty_groups} group{} ({:.2}s)",
                plural(grand_total),
                plural(non_empty_groups),
                elapsed.as_secs_f64()
            )
            .red()
            .bold()
        );
    }
}

pub(in crate::report) fn print_grouped_human(input: &PrintGroupedHumanInput<'_>) {
    let groups = input.groups;
    let root = input.root;
    let rules = input.rules;
    let elapsed = input.elapsed;
    let quiet = input.quiet;
    let resolver = input.resolver;
    let explain = input.explain;
    if !quiet {
        eprintln!();
    }

    emit_grouped_summary(&grouped_issue_counts(groups));

    let mut grand_total: usize = 0;
    let mut seen_footers: FxHashSet<String> = FxHashSet::default();

    for group in groups {
        let total = group.results.total_issues();
        if total == 0 {
            continue;
        }
        grand_total += total;

        let header_text = grouped_header_text(group, root, resolver, total);
        outln!("{}", header_text.cyan().bold());
        emit_grouped_body(group, root, rules, &mut seen_footers, explain);
    }

    if !quiet {
        emit_grouped_final_status(groups, grand_total, elapsed);
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
        let rel = relative_path(&f.file.path, root);
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
                format!("{}y", &label[..label.len() - 3])
            } else if count == 1 && label.ends_with('s') {
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
    add(results.unused_store_members.len(), "store members");
    add(results.unresolved_imports.len(), "unresolved imports");
    add(results.unlisted_dependencies.len(), "unlisted dependencies");
    {
        let mut pair_set = rustc_hash::FxHashSet::default();
        for dup in &results.duplicate_exports {
            let dup = &dup.export;
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
    add(results.re_export_cycles.len(), "re-export cycles");
    add(results.boundary_violations.len(), "violations");
    add(results.unprovided_injects.len(), "unprovided injects");
    add(results.unrendered_components.len(), "unrendered components");
    add(
        results.unused_component_props.len(),
        "unused component props",
    );
    add(
        results.unused_component_emits.len(),
        "unused component emits",
    );
    add(results.unused_server_actions.len(), "unused server actions");
    add(results.unused_load_data_keys.len(), "unused load data keys");
    add(results.stale_suppressions.len(), "stale suppressions");

    parts.join(" \u{00b7} ")
}

/// Print a concise summary showing only category counts, no individual items.
pub(in crate::report) fn print_check_summary(
    results: &AnalysisResults,
    rules: &RulesConfig,
    elapsed: Duration,
    quiet: bool,
    heading: bool,
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

    if heading {
        print_check_summary_heading();
    }

    print_check_summary_rows(&check_summary_categories(results, rules));
    print_check_summary_total(total);

    if !quiet {
        print_check_summary_failure(total, elapsed);
    }
}

fn print_check_summary_heading() {
    outln!("{}", "Dead Code Summary".bold());
    outln!();
}

fn check_summary_categories(
    results: &AnalysisResults,
    rules: &RulesConfig,
) -> Vec<(&'static str, usize, Level)> {
    let mut categories = check_summary_core_categories(results, rules);
    categories.extend(check_summary_dependency_categories(results, rules));
    categories.extend(check_summary_framework_categories(results, rules));
    categories
}

fn check_summary_core_categories(
    results: &AnalysisResults,
    rules: &RulesConfig,
) -> Vec<(&'static str, usize, Level)> {
    vec![
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
            "Unused store members",
            results.unused_store_members.len(),
            severity_to_level(rules.unused_store_members),
        ),
        (
            "Unresolved imports",
            results.unresolved_imports.len(),
            severity_to_level(rules.unresolved_imports),
        ),
    ]
}

fn check_summary_dependency_categories(
    results: &AnalysisResults,
    rules: &RulesConfig,
) -> Vec<(&'static str, usize, Level)> {
    vec![
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
            "Re-export cycles",
            results.re_export_cycles.len(),
            severity_to_level(rules.re_export_cycle),
        ),
        (
            "Boundary violations",
            results.boundary_violations.len(),
            severity_to_level(rules.boundary_violation),
        ),
    ]
}

fn check_summary_framework_categories(
    results: &AnalysisResults,
    rules: &RulesConfig,
) -> Vec<(&'static str, usize, Level)> {
    vec![
        (
            "Unprovided injects",
            results.unprovided_injects.len(),
            severity_to_level(rules.unprovided_injects),
        ),
        (
            "Unrendered components",
            results.unrendered_components.len(),
            severity_to_level(rules.unrendered_components),
        ),
        (
            "Unused component props",
            results.unused_component_props.len(),
            severity_to_level(rules.unused_component_props),
        ),
        (
            "Unused component emits",
            results.unused_component_emits.len(),
            severity_to_level(rules.unused_component_emits),
        ),
        (
            "Unused server actions",
            results.unused_server_actions.len(),
            severity_to_level(rules.unused_server_actions),
        ),
        (
            "Unused load data keys",
            results.unused_load_data_keys.len(),
            severity_to_level(rules.unused_load_data_keys),
        ),
        (
            "Stale suppressions",
            results.stale_suppressions.len(),
            severity_to_level(rules.stale_suppressions),
        ),
    ]
}

fn print_check_summary_rows(categories: &[(&str, usize, Level)]) {
    for (name, count, level) in categories {
        if *count > 0 {
            outln!("  {}  {name}", colored_summary_count(*count, *level));
        }
    }
}

fn colored_summary_count(count: usize, level: Level) -> String {
    let count_str = format!("{count:>6}");
    match level {
        Level::Error => count_str.red().bold().to_string(),
        Level::Warn => count_str.yellow().to_string(),
        Level::Info => count_str.dimmed().to_string(),
    }
}

fn print_check_summary_total(total: usize) {
    outln!();
    let total_str = format!("{total:>6}");
    outln!("  {}  {}", total_str.bold(), "Total".bold());
}

fn print_check_summary_failure(total: usize, elapsed: Duration) {
    eprintln!(
        "{}",
        format!("\u{2717} {total} issues ({:.2}s)", elapsed.as_secs_f64())
            .red()
            .bold()
    );
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

    #[test]
    fn empty_results_produce_no_lines() {
        let root = PathBuf::from("/project");
        let results = AnalysisResults::default();
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        assert!(lines.is_empty());
    }

    #[test]
    fn collect_matching_rules_routes_mixed_client_server_barrels() {
        // A file whose ONLY finding is a mixed-client-server-barrel must still
        // surface its CODEOWNERS rule in the `--group-by owner` "matched by"
        // header. Reverting the `mixed_client_server_barrels` loop in
        // `collect_matching_rules` makes this assertion fail (empty rules),
        // pinning the fix that was previously missing alongside the sibling
        // invalid-client-export / misplaced-directive loops.
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .mixed_client_server_barrels
            .push(MixedClientServerBarrelFinding::with_actions(
                MixedClientServerBarrel {
                    path: root.join("src/index.ts"),
                    client_origin: "./Button".to_string(),
                    server_origin: "./fetchUser".to_string(),
                    line: 2,
                    col: 0,
                },
            ));
        let resolver = OwnershipResolver::Owner(
            crate::codeowners::CodeOwners::parse("/src/ @frontend\n").unwrap(),
        );
        let matched = collect_matching_rules(&results, &root, &resolver);
        assert!(
            matched.iter().any(|r| r.contains("src")),
            "mixed-barrel path must route through the ownership resolver, got: {matched:?}"
        );
    }

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

    #[test]
    fn section_header_shows_correct_count_for_multiple_items() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..5 {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join(format!("src/dead{i}.ts")),
                }));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (5)"));
    }

    #[test]
    fn boundary_coverage_alone_renders_structure_section() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: root.join("src/middleware/error.ts"),
                    line: 1,
                    col: 0,
                },
            ));

        let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
        let text = plain(&lines);

        assert!(text.contains("Structure"));
        assert!(text.contains("Boundary coverage (1)"));
        assert!(text.contains("src/middleware/error.ts:1"));
    }

    #[test]
    fn boundary_calls_alone_render_structure_section() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: root.join("src/domain/policy.ts"),
                    line: 5,
                    col: 2,
                    zone: "domain".to_string(),
                    callee: "execSync".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ));

        let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
        let text = plain(&lines);

        assert!(text.contains("Structure"));
        assert!(text.contains("Boundary calls (1)"));
        assert!(text.contains("src/domain/policy.ts:5"));
        assert!(text.contains("execSync"));
        assert!(text.contains("child_process.*"));
        assert!(text.contains("zone 'domain'"));
        // The rule id is boundary-call-violation but the working token is the
        // family token; the section must teach the literal token.
        assert!(text.contains("// fallow-ignore-next-line boundary-violation"));
    }

    #[test]
    fn policy_violations_render_policy_section_with_message() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: root.join("src/app.ts"),
                line: 7,
                col: 2,
                pack: "team-policy".to_string(),
                rule_id: "no-moment".to_string(),
                kind: fallow_types::results::PolicyRuleKind::BannedImport,
                matched: "moment/locale/nl".to_string(),
                severity: fallow_types::results::PolicyViolationSeverity::Error,
                message: Some("Use date-fns.".to_string()),
            }));

        let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
        let text = plain(&lines);

        assert!(text.contains("Policy"));
        assert!(text.contains("Policy violations (1)"));
        assert!(text.contains("src/app.ts:7"));
        assert!(text.contains("moment/locale/nl"));
        assert!(text.contains("team-policy/no-moment"));
        assert!(text.contains("Use date-fns."));
        assert!(text.contains("fallow-ignore-next-line policy-violation:<pack>/<rule-id>"));
    }

    #[test]
    fn unused_files_show_relative_paths() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/components/Button.tsx"),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("src/components/Button.tsx"));
        assert!(!text.contains("/project/"));
    }

    #[test]
    fn unused_files_show_src_test_split_when_mixed() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for path in [
            "src/dead-a.ts",
            "src/dead-b.ts",
            "tests/dead-a.test.ts",
            "tests/dead-b.test.ts",
            "__fixtures__/dead-fixture.ts",
        ] {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join(path),
                }));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);

        assert!(text.contains("2 in src, 3 in test directories"));
    }

    #[test]
    fn unused_exports_grouped_by_file_with_line_and_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helperFn".to_string(),
                is_type_only: false,
                line: 10,
                col: 4,
                span_start: 120,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "anotherFn".to_string(),
                is_type_only: false,
                line: 25,
                col: 0,
                span_start: 300,
                is_re_export: false,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);

        assert!(text.contains("Unused exports (2)"));
        assert!(text.contains("src/utils.ts"));
        assert!(text.contains(":10 helperFn"));
        assert!(text.contains(":25 anotherFn"));
    }

    #[test]
    fn re_exports_are_tagged() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/index.ts"),
                export_name: "reExported".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: true,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("(re-export)"));
    }

    #[test]
    fn non_re_exports_have_no_tag() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(!text.contains("(re-export)"));
    }

    #[test]
    fn unused_enum_members_show_parent_dot_member() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/enums.ts"),
                parent_name: "Color".to_string(),
                member_name: "Purple".to_string(),
                kind: MemberKind::EnumMember,
                line: 5,
                col: 2,
            }));
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
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/service.ts"),
                parent_name: "ApiService".to_string(),
                member_name: "disconnect".to_string(),
                kind: MemberKind::ClassMethod,
                line: 99,
                col: 4,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("ApiService.disconnect"));
        assert!(text.contains(":99"));
    }

    #[test]
    fn unused_deps_at_root_show_package_name_only() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("lodash"));
        assert!(!text.contains("(package.json)"));
    }

    #[test]
    fn unused_deps_in_workspace_show_workspace_path() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "axios".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/web/package.json"),
                line: 8,
                used_in_workspaces: Vec::new(),
            }));
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
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("packages/shared/package.json"),
                line: 8,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }));
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
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 8,
                used_in_workspaces: vec![root.join("packages/consumer")],
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("lodash-es"));
        assert!(text.contains("(imported in packages/consumer)"));
        assert!(!text.contains("(package.json; imported in packages/consumer)"));
    }

    #[test]
    fn unresolved_imports_show_specifier_and_line() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/app.ts"),
                specifier: "@org/missing-pkg".to_string(),
                line: 7,
                col: 0,
                specifier_col: 0,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("src/app.ts"));
        assert!(text.contains(":7"));
        assert!(text.contains("@org/missing-pkg"));
    }

    fn make_dup(name: &str, paths: &[&str]) -> DuplicateExportFinding {
        DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: name.to_string(),
            locations: paths
                .iter()
                .map(|p| DuplicateLocation {
                    path: PathBuf::from(p),
                    line: 1,
                    col: 0,
                })
                .collect(),
        })
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
        assert!(!is_namespace_barrel_location(Path::new(
            "components/ui/button/Index.ts"
        )));
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
        let items: Vec<DuplicateExportFinding> = (0..47)
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

    #[test]
    fn duplicate_exports_show_name_and_locations() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
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
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Config"));
        assert!(text.contains("src/config.ts"));
        assert!(text.contains("types.ts"));
    }

    #[test]
    fn circular_dependencies_show_cycle_with_arrow_and_repeat() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        root.join("src/a.ts"),
                        root.join("src/b.ts"),
                        root.join("src/c.ts"),
                    ],
                    length: 3,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("a.ts"));
        assert!(text.contains("b.ts"));
        assert!(text.contains("c.ts"));
        assert!(text.contains("\u{2192}"));
    }

    #[test]
    fn empty_sections_are_omitted() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (1)"));
        assert!(!text.contains("Unused exports"));
        assert!(!text.contains("Unused dependencies"));
        assert!(!text.contains("Unresolved imports"));
    }

    #[test]
    fn section_header_uses_bullet_indicator() {
        let header = build_section_header("Test section", 3, Level::Error);
        let text = strip_ansi(&header);
        assert!(text.contains("\u{25cf}"));
        assert!(text.contains("Test section (3)"));
    }

    #[test]
    fn section_header_formats_for_all_levels() {
        for level in [Level::Error, Level::Warn, Level::Info] {
            let header = build_section_header("Items", 7, level);
            let text = strip_ansi(&header);
            assert!(
                text.contains("Items (7)"),
                "Missing title for level {level:?}"
            );
        }
    }

    #[test]
    fn grouped_exports_from_different_files_sorted_by_path() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/z-file.ts"),
                export_name: "zExport".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/a-file.ts"),
                export_name: "aExport".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        let a_pos = text.find("src/a-file.ts").unwrap();
        let z_pos = text.find("src/z-file.ts").unwrap();
        assert!(a_pos < z_pos, "Files should be sorted alphabetically");
    }

    #[test]
    fn grouped_items_from_same_file_share_one_file_header() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..3 {
            results
                .unused_exports
                .push(UnusedExportFinding::with_actions(UnusedExport {
                    path: root.join("src/utils.ts"),
                    export_name: format!("fn{i}"),
                    is_type_only: false,
                    line: (i + 1) as u32,
                    col: 0,
                    span_start: 0,
                    is_re_export: false,
                }));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        let count = text.matches("src/utils.ts").count();
        assert_eq!(count, 1, "File header should appear once, found {count}");
    }

    #[test]
    fn off_severity_still_shows_section_when_items_present() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let rules = RulesConfig {
            unused_files: Severity::Off,
            ..RulesConfig::default()
        };
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Unused files (1)"));
    }

    #[test]
    fn deeply_nested_paths_display_correctly() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("packages/ui/src/components/forms/inputs/TextInput.tsx"),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("packages/ui/src/components/forms/inputs/TextInput.tsx"));
    }

    #[test]
    fn all_issue_types_produce_output_lines() {
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
        assert!(text.contains(
            "Test-only production dependencies (consider moving to devDependencies) (1)"
        ));
        assert!(text.contains("Circular dependencies (1)"));
    }

    #[test]
    fn each_section_ends_with_empty_line_separator() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/a.ts"),
            }));
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "pkg".to_string(),
                location: DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 1,
                used_in_workspaces: Vec::new(),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let empty_count = lines.iter().filter(|l| l.is_empty()).count();
        assert_eq!(
            empty_count, 4,
            "Expected 4 empty separators (2 category headers + 2 sections), got {empty_count}"
        );
    }

    #[test]
    fn type_only_deps_section_title_includes_suggestion() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("Type-only dependencies (consider moving to devDependencies)"));
    }

    #[test]
    fn warn_severity_produces_header_with_bullet() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: root.join("package.json"),
                    line: 8,
                },
            ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("\u{25cf}"));
        assert!(text.contains("Type-only dependencies"));
    }

    #[test]
    fn unlisted_deps_show_package_name() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "@scope/unknown-pkg".to_string(),
                    imported_from: vec![],
                },
            ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("@scope/unknown-pkg"));
    }

    #[test]
    fn circular_deps_grouped_by_hub() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("src/hub.ts"), root.join("src/a.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![root.join("src/hub.ts"), root.join("src/b.ts")],
                    length: 2,
                    line: 5,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("(2 cycles)"));
        assert_eq!(text.matches("hub.ts").count(), 3); // header + 2 chain endings
    }

    #[test]
    fn summary_footer_uses_short_labels() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let footer = build_summary_footer(&results, 0, 0);
        assert!(footer.contains("1 file"));
        assert!(footer.contains("1 export"));
        assert!(footer.contains("1 circular"));
        assert!(!footer.contains("unused file"));
    }

    #[test]
    fn summary_footer_singularizes_pre_pluralized_labels_for_count_1() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(
            fallow_core::results::UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: root.join("src/types.ts"),
                parent_name: "Status".to_string(),
                member_name: "Unused".to_string(),
                line: 10,
                col: 0,
                kind: MemberKind::EnumMember,
            }),
        );
        results.unused_class_members.push(
            fallow_core::results::UnusedClassMemberFinding::with_actions(UnusedMember {
                path: root.join("src/foo.ts"),
                parent_name: "Foo".to_string(),
                member_name: "bar".to_string(),
                line: 5,
                col: 0,
                kind: MemberKind::ClassMethod,
            }),
        );
        let footer = build_summary_footer(&results, 0, 0);
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

    #[test]
    fn section_footer_contains_docs_link() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/dead.ts"),
            }));
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("docs.fallow.tools/explanations/dead-code"));
        assert!(text.contains("Files not reachable from any entry point"));
    }

    #[test]
    fn flat_section_truncates_at_max() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..15 {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join(format!("src/dead{i}.ts")),
                }));
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
        for i in 0..15 {
            results
                .unused_exports
                .push(UnusedExportFinding::with_actions(UnusedExport {
                    path: root.join(format!("src/file{i:02}.ts")),
                    export_name: format!("fn{i}"),
                    is_type_only: false,
                    line: 1,
                    col: 0,
                    span_start: 0,
                    is_re_export: false,
                }));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, None);
        let text = plain(&lines);
        assert!(text.contains("... and 5 more in 5 files"));
    }

    #[test]
    fn top_flag_limits_unused_files_shown() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        for i in 0..5 {
            results
                .unused_files
                .push(UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join(format!("src/dead{i}.ts")),
                }));
        }
        let rules = RulesConfig::default();
        let lines = build_human_lines(&results, &root, &rules, Some(2));
        let text = plain(&lines);

        assert!(text.contains("Unused files (5)"));

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

        assert!(
            text.contains("... and 3 more"),
            "Expected truncation hint, got:\n{text}"
        );
    }
}
