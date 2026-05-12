pub(super) mod check;
mod cross_ref;
pub(super) mod dupes;
pub(super) mod health;
mod perf;
mod traces;

pub(super) use check::*;
pub(super) use cross_ref::*;
pub(super) use dupes::*;
pub(super) use health::*;
pub(super) use perf::*;
pub(super) use traces::*;

use std::io::IsTerminal;
use std::path::Path;

use colored::Colorize;

use super::{Level, plural, relative_path, split_dir_filename};

/// Maximum items shown per flat section (unused files, deps, etc.).
pub(super) const MAX_FLAT_ITEMS: usize = 10;

/// Format a path with dimmed directory and bold filename.
pub(super) fn format_path(path_str: &str) -> String {
    let (dir, filename) = split_dir_filename(path_str);
    format!("{}{}", dir.dimmed(), filename.bold())
}

/// Format a number with thousands separators (e.g., 5433 → "5,433").
pub(super) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

pub(super) fn print_explain_tip_if_tty(has_findings: bool, quiet: bool) {
    if has_findings && !quiet && std::io::stdout().is_terminal() {
        println!(
            "{}",
            "Tip: run `fallow explain <issue-type>` for any finding below.".dimmed()
        );
        println!();
    }
}

/// Build a colored section header with bullet, title, and count.
pub(super) fn build_section_header(title: &str, count: usize, level: Level) -> String {
    let label = format!("{title} ({count})");
    match level {
        Level::Warn => format!("{} {}", "\u{25cf}".yellow(), label.yellow().bold()),
        Level::Info => format!("{} {}", "\u{25cf}".cyan(), label.cyan().bold()),
        Level::Error => format!("{} {}", "\u{25cf}".red(), label.red().bold()),
    }
}

/// Section footer: description + docs URL (with anchor to specific section).
fn section_footer_text(title: &str) -> Option<(&'static str, &'static str)> {
    match title {
        "Unused files" => Some((
            "Files not reachable from any entry point",
            "https://docs.fallow.tools/explanations/dead-code#unused-files",
        )),
        "Unused exports" => Some((
            "Exported symbols with no known consumers",
            "https://docs.fallow.tools/explanations/dead-code#unused-exports",
        )),
        "Unused type exports" => Some((
            "Type exports with no known consumers",
            "https://docs.fallow.tools/explanations/dead-code#unused-types",
        )),
        "Private type leaks" => Some((
            "Exported signatures that reference same-file private types",
            "https://docs.fallow.tools/explanations/dead-code#private-type-leaks",
        )),
        "Unused dependencies" => Some((
            "Listed in dependencies but never imported",
            "https://docs.fallow.tools/explanations/dead-code#unused-dependencies",
        )),
        "Unused devDependencies" => Some((
            "Listed in devDependencies but never imported or referenced",
            "https://docs.fallow.tools/explanations/dead-code#unused-dependencies",
        )),
        "Unused optionalDependencies" => Some((
            "Listed in optionalDependencies but never imported",
            "https://docs.fallow.tools/explanations/dead-code#unused-dependencies",
        )),
        "Unused enum members" => Some((
            "Enum members never referenced outside their declaration",
            "https://docs.fallow.tools/explanations/dead-code#unused-enum-members",
        )),
        "Unused class members" => Some((
            "Class methods or properties never referenced outside their class",
            "https://docs.fallow.tools/explanations/dead-code#unused-class-members",
        )),
        "Unresolved imports" => Some((
            "Import paths that could not be resolved \u{2014} check for missing packages or broken paths. Framework-specific imports may need a plugin: https://docs.fallow.tools/plugins",
            "https://docs.fallow.tools/explanations/dead-code#unresolved-imports",
        )),
        "Unlisted dependencies" => Some((
            "Packages imported in code but missing from package.json",
            "https://docs.fallow.tools/explanations/dead-code#unlisted-dependencies",
        )),
        "Duplicate exports" => Some((
            "Same export name defined in multiple files; barrel re-exports may resolve ambiguously",
            "https://docs.fallow.tools/explanations/dead-code#duplicate-exports",
        )),
        "Circular dependencies" => Some((
            "Import cycles that can cause initialization failures and prevent tree-shaking",
            "https://docs.fallow.tools/explanations/dead-code#circular-dependencies",
        )),
        "Boundary violations" => Some((
            "Imports that cross defined architecture zone boundaries",
            "https://docs.fallow.tools/explanations/dead-code#boundary-violations",
        )),
        "Stale suppressions" => Some((
            "Suppression comments or JSDoc tags that no longer match any issue",
            "https://docs.fallow.tools/explanations/dead-code#stale-suppressions",
        )),
        "Unused catalog entries" => Some((
            "pnpm-workspace.yaml catalog entries not referenced by any workspace package via the `catalog:` protocol",
            "https://docs.fallow.tools/explanations/dead-code#unused-catalog-entries",
        )),
        "Unresolved catalog references" => Some((
            "package.json `catalog:` / `catalog:<name>` references whose catalog does not declare the package (pnpm install will error)",
            "https://docs.fallow.tools/explanations/dead-code#unresolved-catalog-references",
        )),
        t if t.starts_with("Type-only") => Some((
            "Dependencies only used for type imports \u{2014} consider moving to devDependencies",
            "https://docs.fallow.tools/explanations/dead-code#type-only-dependencies",
        )),
        _ => None,
    }
}

/// Map section title to the corresponding fallow-ignore rule name.
fn section_suppress_rule(title: &str) -> Option<&'static str> {
    match title {
        "Unused files" => Some("unused-files"),
        "Unused exports" => Some("unused-exports"),
        "Unused type exports" => Some("unused-types"),
        "Private type leaks" => Some("private-type-leak"),
        "Unused dependencies" | "Unused devDependencies" | "Unused optionalDependencies" => {
            Some("unused-dependencies")
        }
        "Unused enum members" => Some("unused-enum-members"),
        "Unused class members" => Some("unused-class-members"),
        "Unresolved imports" => Some("unresolved-imports"),
        "Unlisted dependencies" => Some("unlisted-dependencies"),
        "Duplicate exports" => Some("duplicate-exports"),
        "Circular dependencies" => Some("circular-dependencies"),
        "Boundary violations" => Some("boundary-violation"),
        "Unused catalog entries" => Some("unused-catalog-entry"),
        "Unresolved catalog references" => Some("unresolved-catalog-reference"),
        _ => None,
    }
}

/// Rules that only support file-level suppression (not next-line).
fn is_file_level_only(rule: &str) -> bool {
    matches!(rule, "circular-dependencies" | "boundary-violation")
}

/// Rules whose findings live in YAML files (so the suppression comment must
/// use `#` rather than `//`).
fn is_yaml_comment_only(rule: &str) -> bool {
    matches!(rule, "unused-catalog-entry")
}

/// Rules whose findings live in a file format that does not support comments
/// at all (e.g., `unresolved-catalog-reference` lives in `package.json`).
/// Suppression for these MUST go through a fallow config entry.
fn is_config_only_suppression(rule: &str) -> bool {
    matches!(rule, "unresolved-catalog-reference")
}

/// Render the config-only suppression hint for a rule that has no inline
/// suppression path.
fn config_only_suppression_hint(rule: &str) -> &'static str {
    match rule {
        "unresolved-catalog-reference" => {
            "To suppress: add an entry to ignoreCatalogReferences in your fallow config"
        }
        _ => "To suppress: add an override in your fallow config",
    }
}

/// Categories that support `fallow fix --dry-run` auto-fix.
fn is_auto_fixable(title: &str) -> bool {
    matches!(
        title,
        "Unused exports" | "Unused type exports" | "Unused enum members"
    )
}

/// Push a dimmed section footer line: description — docs_url, plus suppression hint.
///
/// The `item_count` controls whether the suppress hint is shown (only for sections
/// with 3+ items, to reduce noise for power users scanning many small sections).
pub(super) fn push_section_footer_with_count(
    lines: &mut Vec<String>,
    title: &str,
    item_count: usize,
) {
    push_section_footer_impl(lines, title, item_count, false);
}

/// Push section footer for directory-rollup sections (suggests ignorePatterns config).
pub(super) fn push_section_footer_rollup(lines: &mut Vec<String>, title: &str, item_count: usize) {
    push_section_footer_impl(lines, title, item_count, true);
}

fn push_section_footer_impl(lines: &mut Vec<String>, title: &str, item_count: usize, rollup: bool) {
    if let Some((desc, url)) = section_footer_text(title) {
        lines.push(format!("  {}", format!("{desc} \u{2014} {url}").dimmed()));
    }
    // Only show suppress/fix hints for sections with 3+ items to reduce noise
    if item_count >= 3 {
        // Auto-fix hint for fixable categories
        if is_auto_fixable(title) {
            lines.push(format!(
                "  {}",
                "To auto-fix: fallow fix --dry-run".dimmed()
            ));
        }
        // Suppress hint: config-level for rollup, inline for individual items
        if let Some(rule) = section_suppress_rule(title) {
            let comment = if rollup {
                "To suppress a directory: add to ignorePatterns in .fallowrc.json".to_string()
            } else if is_file_level_only(rule) {
                format!("To suppress: // fallow-ignore-file {rule}")
            } else if is_yaml_comment_only(rule) {
                format!("To suppress: # fallow-ignore-next-line {rule}")
            } else if is_config_only_suppression(rule) {
                config_only_suppression_hint(rule).to_string()
            } else {
                format!("To suppress: // fallow-ignore-next-line {rule}")
            };
            lines.push(format!("  {}", comment.dimmed()));
        }
    }
}

/// Build items grouped by file path, sorted by count descending, with truncation.
pub(super) fn build_grouped_by_file<'a, T>(
    lines: &mut Vec<String>,
    items: &'a [T],
    root: &Path,
    get_path: impl Fn(&'a T) -> &'a Path,
    format_detail: &impl Fn(&T) -> String,
    max_files: usize,
    max_items_per_file: usize,
) {
    // Group items by file path, preserving indices
    let mut file_groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut file_map: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();

    for (i, item) in items.iter().enumerate() {
        let file_str = relative_path(get_path(item), root).display().to_string();
        if let Some(&group_idx) = file_map.get(&file_str) {
            file_groups[group_idx].1.push(i);
        } else {
            file_map.insert(file_str.clone(), file_groups.len());
            file_groups.push((file_str, vec![i]));
        }
    }

    // Sort files by item count descending, alphabetical tiebreaker
    file_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    let total_files = file_groups.len();
    let shown_files = total_files.min(max_files);

    for (file_str, indices) in &file_groups[..shown_files] {
        let count_tag = if indices.len() > 1 {
            format!(" ({})", indices.len()).dimmed().to_string()
        } else {
            String::new()
        };
        lines.push(format!("  {}{}", format_path(file_str), count_tag));

        let shown_items = indices.len().min(max_items_per_file);
        for &i in &indices[..shown_items] {
            lines.push(format!("    {}", format_detail(&items[i])));
        }
        if indices.len() > max_items_per_file {
            lines.push(format!(
                "    {}",
                format!(
                    "... and {} more (--format json for full list)",
                    indices.len() - max_items_per_file
                )
                .dimmed()
            ));
        }
    }

    if total_files > max_files {
        let hidden_files = total_files - max_files;
        let hidden_items: usize = file_groups[max_files..]
            .iter()
            .map(|(_, indices)| indices.len())
            .sum();
        lines.push(format!(
            "  {}",
            format!(
                "... and {} more in {} file{} (--format json for full list)",
                hidden_items,
                hidden_files,
                plural(hidden_files)
            )
            .dimmed()
        ));
    }
}

/// Strip ANSI escape sequences from a string, leaving only the printable text.
#[cfg(test)]
pub(super) fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until 'm' (end of SGR sequence)
            for inner in chars.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Join report lines into a single string with ANSI codes stripped.
#[cfg(test)]
pub(super) fn plain(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| strip_ansi(l))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Utility function tests ──

    #[test]
    fn thousands_zero() {
        assert_eq!(thousands(0), "0");
    }

    #[test]
    fn thousands_small() {
        assert_eq!(thousands(999), "999");
    }

    #[test]
    fn thousands_boundary() {
        assert_eq!(thousands(1000), "1,000");
    }

    #[test]
    fn thousands_large() {
        assert_eq!(thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn thousands_irregular() {
        assert_eq!(thousands(12345), "12,345");
    }

    #[test]
    fn format_path_with_directory() {
        let result = strip_ansi(&format_path("src/components/Button.tsx"));
        assert!(result.ends_with("Button.tsx"));
        assert!(result.contains("src/components/"));
    }

    #[test]
    fn format_path_no_directory() {
        let result = strip_ansi(&format_path("index.ts"));
        assert_eq!(result, "index.ts");
    }

    // ── strip_ansi utility ──

    #[test]
    fn strip_ansi_removes_color_codes() {
        let colored_str = "hello".red().bold().to_string();
        assert_eq!(strip_ansi(&colored_str), "hello");
    }

    #[test]
    fn strip_ansi_preserves_plain_text() {
        assert_eq!(strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn strip_ansi_handles_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    // ── Section header tests ──

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
}
