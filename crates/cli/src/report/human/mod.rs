pub(super) mod check;
mod cross_ref;
pub(super) mod dupes;
pub(super) mod health;
mod health_hotspots;
mod health_runtime;
mod health_targets;
mod perf;
mod traces;
pub(super) mod walkthrough;

pub(super) use check::*;
pub(super) use cross_ref::*;
pub(super) use dupes::*;
pub(super) use health::*;
pub(super) use perf::*;
pub(super) use traces::*;

use std::io::IsTerminal;
use std::path::Path;

use colored::Colorize;
use fallow_types::issue_meta::{issue_meta_by_kind, issue_meta_for_contract_token};
use fallow_types::suppress::{IssueKind, issue_kind_to_kebab};

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
    if has_findings
        && !quiet
        && std::io::stdout().is_terminal()
        && !crate::report::sink::is_redirected()
    {
        println!(
            "{}",
            "Tip: run `fallow explain <issue label>`; spaces and hyphens both work, e.g. `fallow explain unused files`."
                .dimmed()
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
    section_dead_code_footer_text(title)
        .or_else(|| section_dependency_footer_text(title))
        .or_else(|| section_framework_footer_text(title))
        .or_else(|| section_component_footer_text(title))
}

fn section_dead_code_footer_text(title: &str) -> Option<(&'static str, &'static str)> {
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
        "Unused store members" => Some((
            "Store state or actions never accessed by any consumer",
            "https://docs.fallow.tools/explanations/dead-code#unused-store-members",
        )),
        "Unresolved imports" => Some((
            "Import paths that could not be resolved, check for missing packages or broken paths. Framework-specific imports may need a plugin: https://docs.fallow.tools/plugins",
            "https://docs.fallow.tools/explanations/dead-code#unresolved-imports",
        )),
        _ => None,
    }
}

fn section_dependency_footer_text(title: &str) -> Option<(&'static str, &'static str)> {
    match title {
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
        "Unused dependency overrides" => Some((
            "pnpm `overrides:` entries whose target package is not declared by any workspace package or resolved in pnpm-lock.yaml",
            "https://docs.fallow.tools/explanations/dead-code#unused-dependency-overrides",
        )),
        "Misconfigured dependency overrides" => Some((
            "pnpm `overrides:` entries with an unparsable key or empty value (pnpm install will error)",
            "https://docs.fallow.tools/explanations/dead-code#misconfigured-dependency-overrides",
        )),
        t if t.starts_with("Type-only") => Some((
            "Dependencies only used for type imports; consider moving to devDependencies",
            "https://docs.fallow.tools/explanations/dead-code#type-only-dependencies",
        )),
        _ => None,
    }
}

fn section_framework_footer_text(title: &str) -> Option<(&'static str, &'static str)> {
    match title {
        "Invalid client exports" => Some((
            "Server-only or route-config exports in a \"use client\" file (Next.js rejects this at build time)",
            "https://docs.fallow.tools/explanations/dead-code#invalid-client-exports",
        )),
        "Mixed client/server barrels" => Some((
            "Barrel re-exports both a \"use client\" module and a server-only module (one import drags the other's directive across the boundary)",
            "https://docs.fallow.tools/explanations/dead-code#mixed-client-server-barrels",
        )),
        "Misplaced directives" => Some((
            "A \"use client\" / \"use server\" directive sits below an import, so the RSC bundler ignores it (move it above every import)",
            "https://docs.fallow.tools/explanations/dead-code#misplaced-directives",
        )),
        "Unprovided injects" => Some((
            "A Vue inject / Svelte getContext whose key is provided nowhere in the project, so at runtime it returns undefined",
            "https://docs.fallow.tools/explanations/dead-code#unprovided-injects",
        )),
        _ => None,
    }
}

fn section_component_footer_text(title: &str) -> Option<(&'static str, &'static str)> {
    match title {
        "Unrendered components" => Some((
            "A Vue / Svelte component reachable through a barrel but rendered nowhere in the project (render it somewhere or remove it)",
            "https://docs.fallow.tools/explanations/dead-code#unrendered-components",
        )),
        "Unused component props" => Some((
            "A Vue, Svelte, or React component prop referenced nowhere inside its own component (remove it or use it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-component-props",
        )),
        "Prop drilling" => Some((
            "A React/Preact prop forwarded unused through two or more intermediate components before a component consumes it (colocate the consumer or lift it to a context); opt-in, off by default",
            "https://docs.fallow.tools/explanations/dead-code#prop-drilling",
        )),
        "Thin wrappers" => Some((
            "A React/Preact component whose whole body forwards props to a single child (return <Child {...props}/>); inline it at call sites or delete it; opt-in, off by default",
            "https://docs.fallow.tools/explanations/dead-code#thin-wrapper",
        )),
        "Duplicate prop shapes" => Some((
            "Three or more React/Preact components across two or more files declaring an identical prop-name set (after stripping common DOM props); extract a shared Props type or base component; opt-in, off by default",
            "https://docs.fallow.tools/explanations/dead-code#duplicate-prop-shape",
        )),
        "Unused component emits" => Some((
            "A Vue <script setup> defineEmits event emitted nowhere inside its own component (remove it or emit it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-component-emits",
        )),
        "Unused component inputs" => Some((
            "An Angular @Input() / signal input() declaration read nowhere inside its own component (remove it or use it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-component-inputs",
        )),
        "Unused component outputs" => Some((
            "An Angular @Output() / signal output() declaration emitted nowhere inside its own component (remove it or emit it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-component-outputs",
        )),
        "Unused Svelte events" => Some((
            "A Svelte component dispatching a createEventDispatcher event whose name is listened to nowhere in the project (remove it or listen for it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-svelte-events",
        )),
        "Unused server actions" => Some((
            "A Next.js Server Action exported from a \"use server\" file that no code in the project references (wire it to a consumer or remove it)",
            "https://docs.fallow.tools/explanations/dead-code#unused-server-actions",
        )),
        "Unused load data keys" => Some((
            "A SvelteKit load() return-object key no consumer reads (sibling +page.svelte data.<key> or project-wide page.data.<key>); delete the key or wire a consumer",
            "https://docs.fallow.tools/explanations/dead-code#unused-load-data-keys",
        )),
        _ => None,
    }
}

/// Map a human-output section title to the issue kind it reports.
///
/// This title-to-kind map is the only hand-maintained association; every
/// suppression token and file-level flag is then derived from the registry in
/// [`fallow_types::issue_meta::ISSUE_KIND_META`], so the printed hint can never
/// drift from what [`fallow_types::suppress::parse_suppression_target`] accepts.
fn section_issue_kind(title: &str) -> Option<IssueKind> {
    Some(match title {
        "Unused files" => IssueKind::UnusedFile,
        "Unused exports" => IssueKind::UnusedExport,
        "Unused type exports" => IssueKind::UnusedType,
        "Private type leaks" => IssueKind::PrivateTypeLeak,
        "Unused dependencies" => IssueKind::UnusedDependency,
        "Unused devDependencies" => IssueKind::UnusedDevDependency,
        // "Unused optionalDependencies" has no backing IssueKind and its findings
        // live in package.json (no inline comment surface), so it maps to nothing
        // and emits no suppress hint.
        "Unused enum members" => IssueKind::UnusedEnumMember,
        "Unused class members" => IssueKind::UnusedClassMember,
        "Unused store members" => IssueKind::UnusedStoreMember,
        "Unresolved imports" => IssueKind::UnresolvedImport,
        "Unlisted dependencies" => IssueKind::UnlistedDependency,
        "Duplicate exports" => IssueKind::DuplicateExport,
        "Circular dependencies" => IssueKind::CircularDependency,
        "Boundary violations" => IssueKind::BoundaryViolation,
        "Unused catalog entries" => IssueKind::PnpmCatalogEntry,
        "Unresolved catalog references" => IssueKind::UnresolvedCatalogReference,
        "Unused dependency overrides" => IssueKind::UnusedDependencyOverride,
        "Misconfigured dependency overrides" => IssueKind::MisconfiguredDependencyOverride,
        "Invalid client exports" => IssueKind::InvalidClientExport,
        "Mixed client/server barrels" => IssueKind::MixedClientServerBarrel,
        "Misplaced directives" => IssueKind::MisplacedDirective,
        "Unprovided injects" => IssueKind::UnprovidedInject,
        "Unrendered components" => IssueKind::UnrenderedComponent,
        "Unused component props" => IssueKind::UnusedComponentProp,
        "Prop drilling" => IssueKind::PropDrilling,
        "Thin wrappers" => IssueKind::ThinWrapper,
        "Duplicate prop shapes" => IssueKind::DuplicatePropShape,
        "Unused component emits" => IssueKind::UnusedComponentEmit,
        "Unused component inputs" => IssueKind::UnusedComponentInput,
        "Unused component outputs" => IssueKind::UnusedComponentOutput,
        "Unused Svelte events" => IssueKind::UnusedSvelteEvent,
        "Unused server actions" => IssueKind::UnusedServerAction,
        "Unused load data keys" => IssueKind::UnusedLoadDataKey,
        _ => return None,
    })
}

/// Map a section title to the fallow-ignore suppression token to print, derived
/// from the issue registry so it always parses back to the same kind.
///
/// Returns `None` when the section's findings have no inline suppression surface:
/// a kind with no dedicated `suppress_token` whose findings live in package.json
/// (the dependency sections) would otherwise print a token that parses but points
/// at a comment the user cannot place. Catalog entries (YAML comment surface) and
/// catalog references / dependency overrides (config-entry surface) keep a hint,
/// routed through [`is_yaml_comment_only`] / [`is_config_only_suppression`].
fn section_suppress_rule(title: &str) -> Option<&'static str> {
    let kind = section_issue_kind(title)?;
    let meta = issue_meta_by_kind(kind)?;
    let token = issue_kind_to_kebab(kind);
    if meta.suppress_token.is_none()
        && !is_yaml_comment_only(token)
        && !is_config_only_suppression(token)
    {
        return None;
    }
    Some(token)
}

/// Rules that only support file-level suppression (not next-line), derived from
/// the issue registry's `suppress_file_level` flag so the printed hint form
/// matches how each kind's detector actually consumes suppressions.
fn is_file_level_only(rule: &str) -> bool {
    issue_meta_for_contract_token(rule).is_some_and(|meta| meta.suppress_file_level)
}

/// Rules whose findings live in YAML files (so the suppression comment must
/// use `#` rather than `//`).
fn is_yaml_comment_only(rule: &str) -> bool {
    matches!(rule, "unused-catalog-entry")
}

/// Rules whose findings live in a file format that does not support comments
/// at all (e.g., `unresolved-catalog-reference` lives in `package.json`), or
/// whose findings can live in either YAML or JSON (`*-dependency-override`),
/// so an inline suppression mechanism would be format-dependent. Suppression
/// for these MUST go through a fallow config entry.
fn is_config_only_suppression(rule: &str) -> bool {
    matches!(
        rule,
        "unresolved-catalog-reference"
            | "unused-dependency-override"
            | "misconfigured-dependency-override"
    )
}

/// Render the config-only suppression hint for a rule that has no inline
/// suppression path.
fn config_only_suppression_hint(rule: &str) -> &'static str {
    match rule {
        "unresolved-catalog-reference" => {
            "To suppress: add an entry to ignoreCatalogReferences in your fallow config"
        }
        "unused-dependency-override" | "misconfigured-dependency-override" => {
            "To suppress: add an entry to ignoreDependencyOverrides in your fallow config"
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
    if item_count >= 3 {
        if is_auto_fixable(title) {
            lines.push(format!(
                "  {}",
                "To auto-fix: fallow fix --dry-run".dimmed()
            ));
        }
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
pub(super) struct GroupedByFileInput<'out, 'items, T, P, F>
where
    P: Fn(&'items T) -> &'items Path,
    F: Fn(&T) -> String,
{
    pub(super) lines: &'out mut Vec<String>,
    pub(super) items: &'items [T],
    pub(super) root: &'out Path,
    pub(super) get_path: P,
    pub(super) format_detail: &'out F,
    pub(super) max_files: usize,
    pub(super) max_items_per_file: usize,
}

pub(super) fn build_grouped_by_file<'out, 'items, T, P, F>(
    input: GroupedByFileInput<'out, 'items, T, P, F>,
) where
    P: Fn(&'items T) -> &'items Path,
    F: Fn(&T) -> String,
{
    let GroupedByFileInput {
        lines,
        items,
        root,
        get_path,
        format_detail,
        max_files,
        max_items_per_file,
    } = input;

    let mut file_groups = group_item_indices_by_file(items, root, get_path);
    file_groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));

    let total_files = file_groups.len();
    let shown_files = total_files.min(max_files);

    for (file_str, indices) in &file_groups[..shown_files] {
        push_grouped_file_lines(
            lines,
            file_str,
            indices,
            items,
            format_detail,
            max_items_per_file,
        );
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

/// Group item indices by their relative file path, preserving first-seen order.
fn group_item_indices_by_file<'items, T, P>(
    items: &'items [T],
    root: &Path,
    get_path: P,
) -> Vec<(String, Vec<usize>)>
where
    P: Fn(&'items T) -> &'items Path,
{
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
    file_groups
}

/// Render one file's header and its (truncated) per-item detail lines.
fn push_grouped_file_lines<T, F>(
    lines: &mut Vec<String>,
    file_str: &str,
    indices: &[usize],
    items: &[T],
    format_detail: &F,
    max_items_per_file: usize,
) where
    F: Fn(&T) -> String,
{
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

/// Strip ANSI escape sequences from a string, leaving only the printable text.
#[cfg(test)]
pub(super) fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
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

    /// Every human-output footer section title. Drives the suppression-token
    /// guards below over the whole surface. A section added to
    /// `section_footer_text` without an entry here leaves its hint untested; an
    /// entry here that names no real footer trips `section_footer_text`.
    const ALL_FOOTER_SECTION_TITLES: &[&str] = &[
        "Unused files",
        "Unused exports",
        "Unused type exports",
        "Private type leaks",
        "Unused dependencies",
        "Unused devDependencies",
        "Unused optionalDependencies",
        "Unused enum members",
        "Unused class members",
        "Unused store members",
        "Unresolved imports",
        "Unlisted dependencies",
        "Duplicate exports",
        "Circular dependencies",
        "Boundary violations",
        "Stale suppressions",
        "Unused catalog entries",
        "Unresolved catalog references",
        "Unused dependency overrides",
        "Misconfigured dependency overrides",
        "Type-only dependencies",
        "Invalid client exports",
        "Mixed client/server barrels",
        "Misplaced directives",
        "Unprovided injects",
        "Unrendered components",
        "Unused component props",
        "Prop drilling",
        "Thin wrappers",
        "Duplicate prop shapes",
        "Unused component emits",
        "Unused component inputs",
        "Unused component outputs",
        "Unused Svelte events",
        "Unused server actions",
        "Unused load data keys",
    ];

    /// The core issue #1828 guard: every token `section_suppress_rule` can emit
    /// must parse via `parse_suppression_target`, and its file-level flag must
    /// match the kind's registry entry. Adding a section whose token does not
    /// parse (or whose file-level form disagrees with the registry) fails here.
    #[test]
    fn every_section_suppress_token_parses_and_matches_registry_file_level() {
        use fallow_types::suppress::parse_suppression_target;

        for &title in ALL_FOOTER_SECTION_TITLES {
            assert!(
                section_footer_text(title).is_some(),
                "{title:?} is listed as a footer section but section_footer_text does not recognize it",
            );

            let Some(rule) = section_suppress_rule(title) else {
                // Sections with no inline suppression surface (dependency sections
                // whose findings live in package.json, plus stale-suppression and
                // type-only-dependency, which carry no suppress token) print no hint.
                continue;
            };

            assert!(
                parse_suppression_target(rule).is_some(),
                "{title:?} prints suppress token {rule:?}, which parse_suppression_target rejects",
            );

            let kind = section_issue_kind(title)
                .expect("a section with a suppress rule has a mapped issue kind");
            let registry_file_level = issue_meta_by_kind(kind)
                .expect("mapped issue kind has a registry row")
                .suppress_file_level;
            assert_eq!(
                is_file_level_only(rule),
                registry_file_level,
                "{title:?} token {rule:?}: is_file_level_only disagrees with registry suppress_file_level",
            );
        }
    }

    /// The rendered footer hint must embed the parseable token and use the
    /// file-level comment form exactly when the kind is file-level-only.
    #[test]
    fn printed_footer_hint_uses_registry_file_level_form() {
        use fallow_types::suppress::parse_suppression_target;

        for &title in ALL_FOOTER_SECTION_TITLES {
            let mut lines = Vec::new();
            // 3 items clears the >= 3 threshold that gates the suppress hint.
            push_section_footer_with_count(&mut lines, title, 3);
            let rendered = lines
                .iter()
                .map(|l| strip_ansi(l))
                .collect::<Vec<_>>()
                .join("\n");

            let Some(rule) = section_suppress_rule(title) else {
                assert!(
                    !rendered.contains("fallow-ignore"),
                    "{title:?} has no suppress rule but printed a fallow-ignore hint: {rendered:?}",
                );
                continue;
            };

            // Config-only sections print a config-entry hint, not a comment token.
            if is_config_only_suppression(rule) {
                assert!(
                    !rendered.contains("fallow-ignore"),
                    "{title:?} is config-only but printed a fallow-ignore hint: {rendered:?}",
                );
                continue;
            }

            let hint_line = rendered
                .lines()
                .find(|l| l.contains("fallow-ignore"))
                .unwrap_or_else(|| {
                    panic!("{title:?} has suppress token {rule:?} but no fallow-ignore hint: {rendered:?}")
                });

            let printed_token = hint_line
                .rsplit(' ')
                .next()
                .expect("hint line ends with the token");
            assert_eq!(
                printed_token, rule,
                "{title:?} printed token differs from section_suppress_rule",
            );
            assert!(
                parse_suppression_target(printed_token).is_some(),
                "{title:?} printed token {printed_token:?} does not parse",
            );

            let uses_file_form = hint_line.contains("fallow-ignore-file");
            assert_eq!(
                uses_file_form,
                is_file_level_only(rule),
                "{title:?} hint {hint_line:?}: file-level form does not match is_file_level_only",
            );
        }
    }

    /// Regression for the eight sections issue #1828 verified broken: the six with
    /// an inline comment surface now emit the singular registered token, and the
    /// two dependency sections emit no hint at all.
    #[test]
    fn previously_unparseable_sections_are_fixed() {
        use fallow_types::suppress::parse_suppression_target;

        let fixed = [
            ("Unused exports", "unused-export"),
            ("Unused type exports", "unused-type"),
            ("Unused enum members", "unused-enum-member"),
            ("Unused class members", "unused-class-member"),
            ("Unresolved imports", "unresolved-import"),
            ("Duplicate exports", "duplicate-export"),
        ];
        for (title, expected) in fixed {
            let rule = section_suppress_rule(title)
                .unwrap_or_else(|| panic!("{title:?} should still print a suppress token"));
            assert_eq!(rule, expected, "{title:?} token drifted");
            assert!(
                parse_suppression_target(rule).is_some(),
                "{title:?} token {rule:?} still does not parse",
            );
        }

        for title in ["Unused dependencies", "Unlisted dependencies"] {
            assert!(
                section_suppress_rule(title).is_none(),
                "{title:?} should emit no suppress hint (package.json finding)",
            );
        }
    }

    /// Documents the `is_file_level_only` audit against the registry: the
    /// file-level-only kinds among footer sections, and the multi-file kinds that
    /// still honor next-line suppression.
    #[test]
    fn is_file_level_only_matches_registry_for_footer_kinds() {
        assert!(is_file_level_only(
            section_suppress_rule("Unused files").unwrap()
        ));
        // duplicate-export is file-level-only (issue #1820 review note): its
        // detector consumes only file-level suppression.
        assert!(is_file_level_only(
            section_suppress_rule("Duplicate exports").unwrap()
        ));
        // circular-dependency and boundary-violation honor next-line suppression,
        // so they are NOT file-level-only despite spanning multiple files.
        assert!(!is_file_level_only(
            section_suppress_rule("Circular dependencies").unwrap()
        ));
        assert!(!is_file_level_only(
            section_suppress_rule("Boundary violations").unwrap()
        ));
    }
}
