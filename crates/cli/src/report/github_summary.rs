//! `--format github-summary`: GitHub Actions job-summary markdown, written
//! by workflows as `fallow ... --format github-summary >> "$GITHUB_STEP_SUMMARY"`.
//!
//! Sections, ordering, and truncation caps are ported from the bundled
//! action's jq renderers (`action/jq/summary-{check,dupes,health,audit,
//! security,fix,combined}.jq`). Deviations from the jq layer: em dashes in
//! the jq templates render as plain hyphens (repo style rule), and the
//! combined view's dupes file links read `GH_REPO` / `GITHUB_REPOSITORY` and
//! `PR_HEAD_SHA` / `GITHUB_SHA` (the jq layer read only the action-set
//! `GH_REPO` / `PR_HEAD_SHA`), with the link path prefix coming from the
//! path-rebase resolution instead of the action-set `PREFIX` env var.
//!
//! Like the annotations renderer, this is value-driven over the `--format
//! json` envelope, which keeps `fallow report --from` output byte-identical
//! to the direct format run.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde_json::Value;

use super::github::{PathRebase, arr, b, fmt_num, num, resolve_render_options, s, u};
use super::github_annotations::EnvelopeKind;
use crate::report::sink::outln;

const DEAD_CODE_DOCS: &str = "https://docs.fallow.tools/explanations/dead-code";
const HEALTH_DOCS: &str = "https://docs.fallow.tools/explanations/health";
const DUPES_DOCS: &str = "https://docs.fallow.tools/explanations/duplication";
const SUPPRESSION_DOCS: &str = "https://docs.fallow.tools/configuration/suppression";

/// Environment-derived context for the combined view's dupes file links.
#[derive(Debug, Default, Clone)]
pub struct LinkContext {
    /// Repo-root path prefix for link targets (empty or `dir/` with a
    /// trailing slash).
    pub prefix: String,
    /// `owner/repo`; links render as plain code when empty.
    pub repo: String,
    /// Head commit SHA; links render as plain code when empty.
    pub sha: String,
}

impl LinkContext {
    /// Resolve from the workflow environment plus the path-rebase offset.
    #[must_use]
    pub fn from_env(rebase: &PathRebase) -> Self {
        let env = |primary: &str, fallback: &str| {
            std::env::var(primary)
                .or_else(|_| std::env::var(fallback))
                .unwrap_or_default()
        };
        let prefix = match rebase {
            PathRebase::None => String::new(),
            PathRebase::Prefix(prefix) => format!("{prefix}/"),
        };
        Self {
            prefix,
            repo: env("GH_REPO", "GITHUB_REPOSITORY"),
            sha: env("PR_HEAD_SHA", "GITHUB_SHA"),
        }
    }
}

/// Render and print the job-summary markdown for one envelope.
pub fn print_summary(kind: EnvelopeKind, envelope: &Value, root: &Path) -> ExitCode {
    let options = resolve_render_options(root);
    let links = LinkContext::from_env(&options.rebase);
    outln!("{}", render_summary(kind, envelope, &links));
    ExitCode::SUCCESS
}

/// Render the fix envelope's job summary directly from `fallow fix`. The fix
/// envelope has no `kind` field; `fallow report --from` reaches the same
/// renderer via [`EnvelopeKind::Fix`] (resolved by field detection), while the
/// live `fallow fix` command calls this entry point.
pub fn print_fix_summary(envelope: &Value) -> ExitCode {
    outln!("{}", render_fix_summary(envelope));
    ExitCode::SUCCESS
}

/// Pure renderer, dispatching on the envelope family.
#[must_use]
pub fn render_summary(kind: EnvelopeKind, envelope: &Value, links: &LinkContext) -> String {
    match kind {
        EnvelopeKind::DeadCode => render_check_summary(envelope),
        EnvelopeKind::Dupes => render_dupes_summary(envelope),
        EnvelopeKind::Health => render_health_summary(envelope),
        EnvelopeKind::Audit => render_audit_summary(envelope),
        EnvelopeKind::Security => render_security_summary(envelope),
        EnvelopeKind::Combined => render_combined_summary(envelope, links),
        EnvelopeKind::Fix => render_fix_summary(envelope),
    }
}

// ---------------------------------------------------------------------------
// Shared numeric / path helpers (jq `pct`, `signed`, `rel_path`).
// ---------------------------------------------------------------------------

/// jq `pct`: `. * 10 | round / 10`, interpolated without a trailing `.0`.
fn pct(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    fmt_num(&serde_json::json!(rounded))
}

/// jq `signed`: `+x` for positive, plain for negative, `0.0` for zero.
fn signed(value: f64) -> String {
    if value > 0.0 {
        format!("+{}", pct(value))
    } else if value < 0.0 {
        pct(value)
    } else {
        "0.0".to_owned()
    }
}

fn opt_f(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn f_or_zero(value: &Value, key: &str) -> f64 {
    opt_f(value, key).unwrap_or_default()
}

/// jq `rel_path` (audit/security flavor): shorten only absolute paths to
/// their last three segments.
fn rel_path_absolute_only(path: &str) -> String {
    if path.starts_with('/') {
        last_three_segments(path)
    } else {
        path.to_owned()
    }
}

/// jq `rel_path` (combined flavor): always shorten to the last three
/// segments.
fn last_three_segments(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > 3 {
        segments[segments.len() - 3..].join("/")
    } else {
        segments.join("/")
    }
}

/// jq `plural(n; word)`.
fn plural_n(n: usize, word: &str) -> String {
    let suffix = if n == 1 { "" } else { "s" };
    format!("{n} {word}{suffix}")
}

fn str_or<'v>(value: &'v Value, key: &str, default: &'v str) -> &'v str {
    value.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// `` `path` `` / `` `path:line` `` with the jq truthiness gate: the `:line`
/// suffix renders whenever `line` is present and non-null (0 included).
fn path_line(item: &Value) -> String {
    let path = rel_path_absolute_only(s(item, "path"));
    match item.get("line").filter(|line| !line.is_null()) {
        Some(line) => format!("`{path}:{}`", fmt_num(line)),
        None => format!("`{path}`"),
    }
}

fn backtick_join(item: &Value, key: &str) -> String {
    arr(item, key)
        .filter_map(Value::as_str)
        .map(|entry| format!("`{entry}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ---------------------------------------------------------------------------
// Dead-code category table (shared by the check summary and the combined
// code-issues breakdown; labels and docs anchors from the jq layer).
// ---------------------------------------------------------------------------

const DEAD_CODE_CATEGORIES: &[(&str, &str, &str)] = &[
    ("Unused files", "unused_files", "unused-files"),
    ("Unused exports", "unused_exports", "unused-exports"),
    ("Unused types", "unused_types", "unused-types"),
    (
        "Private type leaks",
        "private_type_leaks",
        "private-type-leaks",
    ),
    (
        "Unused dependencies",
        "unused_dependencies",
        "unused-dependencies",
    ),
    (
        "Unused devDependencies",
        "unused_dev_dependencies",
        "unused-dependencies",
    ),
    (
        "Unused optionalDependencies",
        "unused_optional_dependencies",
        "unused-dependencies",
    ),
    (
        "Unused enum members",
        "unused_enum_members",
        "unused-enum-members",
    ),
    (
        "Unused class members",
        "unused_class_members",
        "unused-class-members",
    ),
    (
        "Unused store members",
        "unused_store_members",
        "unused-store-members",
    ),
    (
        "Unresolved imports",
        "unresolved_imports",
        "unresolved-imports",
    ),
    (
        "Unlisted dependencies",
        "unlisted_dependencies",
        "unlisted-dependencies",
    ),
    (
        "Duplicate exports",
        "duplicate_exports",
        "duplicate-exports",
    ),
    (
        "Circular dependencies",
        "circular_dependencies",
        "circular-dependencies",
    ),
    ("Re-export cycles", "re_export_cycles", "re-export-cycles"),
    (
        "Boundary violations",
        "boundary_violations",
        "boundary-violations",
    ),
    (
        "Boundary coverage",
        "boundary_coverage_violations",
        "boundary-violations",
    ),
    (
        "Boundary calls",
        "boundary_call_violations",
        "boundary-violations",
    ),
    (
        "Policy violations",
        "policy_violations",
        "policy-violations",
    ),
    (
        "Invalid client exports",
        "invalid_client_exports",
        "invalid-client-exports",
    ),
    (
        "Mixed client/server barrels",
        "mixed_client_server_barrels",
        "mixed-client-server-barrels",
    ),
    (
        "Misplaced directives",
        "misplaced_directives",
        "misplaced-directives",
    ),
    (
        "Unused server actions",
        "unused_server_actions",
        "unused-server-action",
    ),
    ("Route collisions", "route_collisions", "route-collisions"),
    (
        "Dynamic segment conflicts",
        "dynamic_segment_name_conflicts",
        "dynamic-segment-name-conflicts",
    ),
    (
        "Unrendered components",
        "unrendered_components",
        "unrendered-component",
    ),
    (
        "Unused component props",
        "unused_component_props",
        "unused-component-prop",
    ),
    (
        "Unused component emits",
        "unused_component_emits",
        "unused-component-emit",
    ),
    (
        "Unused component inputs",
        "unused_component_inputs",
        "unused-component-input",
    ),
    (
        "Unused component outputs",
        "unused_component_outputs",
        "unused-component-output",
    ),
    (
        "Unused Svelte events",
        "unused_svelte_events",
        "unused-svelte-event",
    ),
    (
        "Unprovided injects",
        "unprovided_injects",
        "unprovided-inject",
    ),
    (
        "Unused load data keys",
        "unused_load_data_keys",
        "unused-load-data-key",
    ),
    (
        "Type-only dependencies",
        "type_only_dependencies",
        "type-only-dependencies",
    ),
    (
        "Test-only dependencies",
        "test_only_dependencies",
        "test-only-dependencies",
    ),
    (
        "Dev dependencies used in production",
        "dev_dependencies_in_production",
        "dev-dependencies-in-production",
    ),
    (
        "Stale suppressions",
        "stale_suppressions",
        "stale-suppressions",
    ),
    (
        "Unused catalog entries",
        "unused_catalog_entries",
        "unused-catalog-entries",
    ),
    (
        "Empty catalog groups",
        "empty_catalog_groups",
        "empty-catalog-groups",
    ),
    (
        "Unresolved catalog references",
        "unresolved_catalog_references",
        "unresolved-catalog-references",
    ),
    (
        "Unused dependency overrides",
        "unused_dependency_overrides",
        "unused-dependency-overrides",
    ),
    (
        "Misconfigured dependency overrides",
        "misconfigured_dependency_overrides",
        "misconfigured-dependency-overrides",
    ),
];

fn dead_code_docs(anchor: &str) -> String {
    format!("{DEAD_CODE_DOCS}#{anchor}")
}

fn dead_code_category_table(env: &Value) -> String {
    DEAD_CODE_CATEGORIES
        .iter()
        .filter_map(|(name, key, anchor)| {
            let n = arr(env, key).count();
            (n > 0).then(|| format!("| [{name}]({}) | {n} |", dead_code_docs(anchor)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// summary-check.jq
// ---------------------------------------------------------------------------

struct SectionSpec {
    name: &'static str,
    key: &'static str,
    header: &'static str,
    row: fn(&Value) -> String,
}

fn render_check_section(env: &Value, spec: &SectionSpec) -> String {
    let items: Vec<&Value> = arr(env, spec.key).collect();
    let n = items.len();
    if n == 0 {
        return String::new();
    }
    let rows = items
        .iter()
        .take(25)
        .map(|item| (spec.row)(item))
        .collect::<Vec<_>>()
        .join("\n");
    let tail = if n > 25 {
        format!(
            "\n\n> {} more - run `fallow` locally for the full list",
            n - 25
        )
    } else {
        String::new()
    };
    format!(
        "\n<details><summary><strong>{} ({n})</strong></summary>\n\n{}{rows}{tail}\n\n</details>\n",
        spec.name, spec.header,
    )
}

fn check_workspace_context(item: &Value) -> String {
    backtick_join(item, "used_in_workspaces")
}

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
fn check_sections_core() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unused files",
            key: "unused_files",
            header: "Files not reachable from any entry point.\n\n| File |\n|------|\n",
            row: |it| format!("| `{}` |", s(it, "path")),
        },
        SectionSpec {
            name: "Unused exports",
            key: "unused_exports",
            header: "Exported symbols with no known consumers.\n\n| File | Line | Export |\n|------|-----:|--------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} | `{}`{} |",
                    s(it, "path"),
                    num(it, "line"),
                    s(it, "export_name"),
                    if b(it, "is_re_export") {
                        " *(re-export)*"
                    } else {
                        ""
                    },
                )
            },
        },
        SectionSpec {
            name: "Unused types",
            key: "unused_types",
            header: "Type exports with no known consumers.\n\n| File | Line | Type |\n|------|-----:|------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} | `{}` |",
                    s(it, "path"),
                    num(it, "line"),
                    s(it, "export_name"),
                )
            },
        },
        SectionSpec {
            name: "Private type leaks",
            key: "private_type_leaks",
            header: "Exported signatures that reference same-file private types.\n\n| File | Line | Export | Private type |\n|------|-----:|--------|--------------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} | `{}` | `{}` |",
                    s(it, "path"),
                    num(it, "line"),
                    s(it, "export_name"),
                    s(it, "type_name"),
                )
            },
        },
        SectionSpec {
            name: "Unused dependencies",
            key: "unused_dependencies",
            header: "Listed in `dependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} |",
                    s(it, "package_name"),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused devDependencies",
            key: "unused_dev_dependencies",
            header: "Listed in `devDependencies` but never imported or referenced by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} |",
                    s(it, "package_name"),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused optionalDependencies",
            key: "unused_optional_dependencies",
            header: "Listed in `optionalDependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} |",
                    s(it, "package_name"),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused enum members",
            key: "unused_enum_members",
            header: "Enum members never referenced outside their declaration.\n\n| File | Line | Enum | Member |\n|------|-----:|------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unused class members",
            key: "unused_class_members",
            header: "Class methods or properties never referenced outside their class.\n\n| File | Line | Class | Member |\n|------|-----:|-------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unused store members",
            key: "unused_store_members",
            header: "Pinia store members (state, getter, action) never accessed by any consumer.\n\n| File | Line | Store | Member |\n|------|-----:|-------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unresolved imports",
            key: "unresolved_imports",
            header: "Import paths that could not be resolved - check for missing packages or broken paths.\n\n| File | Line | Import |\n|------|-----:|--------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} | `{}` |",
                    s(it, "path"),
                    num(it, "line"),
                    s(it, "specifier"),
                )
            },
        },
        SectionSpec {
            name: "Unlisted dependencies",
            key: "unlisted_dependencies",
            header: "Packages imported in code but missing from `package.json`.\n\n| Package | Used in |\n|---------|--------|\n",
            row: |it| {
                let sites: Vec<&Value> = arr(it, "imported_from").collect();
                let cell = if sites.is_empty() {
                    String::new()
                } else {
                    let shown = sites
                        .iter()
                        .take(3)
                        .map(|site| format!("`{}:{}`", s(site, "path"), num(site, "line")))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = if sites.len() > 3 {
                        format!(" *+{} more*", sites.len() - 3)
                    } else {
                        String::new()
                    };
                    format!("{shown}{more}")
                };
                format!("| `{}` | {cell} |", s(it, "package_name"))
            },
        },
        SectionSpec {
            name: "Duplicate exports",
            key: "duplicate_exports",
            header: "Same export name defined in multiple files - barrel re-exports may resolve ambiguously.\n\n| Export | Locations |\n|--------|-----------|\n",
            row: |it| {
                let locations: Vec<&Value> = arr(it, "locations").collect();
                let shown = locations
                    .iter()
                    .take(3)
                    .map(|location| format!("`{}:{}`", s(location, "path"), num(location, "line")))
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = if locations.len() > 3 {
                    format!(" *+{} more*", locations.len() - 3)
                } else {
                    String::new()
                };
                format!("| `{}` | {shown}{more} |", s(it, "export_name"))
            },
        },
        SectionSpec {
            name: "Circular dependencies",
            key: "circular_dependencies",
            header: "Import cycles that can cause initialization failures and prevent tree-shaking.\n\n| Cycle | Length |\n|-------|-------:|\n",
            row: |it| {
                format!(
                    "| {} | {} |",
                    plain_join(it, "files", " \u{2192} "),
                    num(it, "length"),
                )
            },
        },
    ]
}

fn member_row(it: &Value) -> String {
    format!(
        "| `{}` | {} | `{}` | `{}` |",
        s(it, "path"),
        num(it, "line"),
        s(it, "parent_name"),
        s(it, "member_name"),
    )
}

fn plain_join(item: &Value, key: &str, separator: &str) -> String {
    arr(item, key)
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(separator)
}

fn path_line_cell(it: &Value) -> String {
    format!("`{}:{}`", s(it, "path"), num(it, "line"))
}

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
fn check_sections_architecture() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Re-export cycles",
            key: "re_export_cycles",
            header: "Barrel files that re-export from each other in a loop. Chain propagation through the loop is a no-op, so imports through any member may silently come up empty.\n\n| Cycle | Kind | Members |\n|-------|------|--------:|\n",
            row: |it| {
                let cycle = arr(it, "files")
                    .filter_map(Value::as_str)
                    .map(|file| format!("`{file}`"))
                    .collect::<Vec<_>>()
                    .join(" <-> ");
                format!(
                    "| {cycle} | {} | {} |",
                    s(it, "kind"),
                    arr(it, "files").count()
                )
            },
        },
        SectionSpec {
            name: "Boundary violations",
            key: "boundary_violations",
            header: "Imports that cross defined architecture zone boundaries.\n\n| From | To | Zones |\n|------|-----|-------|\n",
            row: |it| {
                format!(
                    "| `{}:{}` | `{}` | {} \u{2192} {} |",
                    s(it, "from_path"),
                    num(it, "line"),
                    s(it, "to_path"),
                    s(it, "from_zone"),
                    s(it, "to_zone"),
                )
            },
        },
        SectionSpec {
            name: "Boundary coverage",
            key: "boundary_coverage_violations",
            header: "Files that match no configured architecture boundary zone.\n\n| File |\n|------|\n",
            row: |it| format!("| {} |", path_line_cell(it)),
        },
        SectionSpec {
            name: "Boundary calls",
            key: "boundary_call_violations",
            header: "Calls from zoned files to callees forbidden for that zone.\n\n| File | Callee | Zone | Pattern |\n|------|--------|------|---------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | {} | `{}` |",
                    path_line_cell(it),
                    s(it, "callee"),
                    s(it, "zone"),
                    s(it, "pattern"),
                )
            },
        },
        SectionSpec {
            name: "Policy violations",
            key: "policy_violations",
            header: "Banned calls, imports, and catalogue-derived effects matched by configured rule packs.\n\n| File | Matched | Rule | Severity |\n|------|---------|------|----------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | `{}/{}` | {} |",
                    path_line_cell(it),
                    s(it, "matched"),
                    s(it, "pack"),
                    s(it, "rule_id"),
                    s(it, "severity"),
                )
            },
        },
        SectionSpec {
            name: "Invalid client exports",
            key: "invalid_client_exports",
            header: "`\"use client\"` files exporting a Next.js server-only / route-config name. Next.js rejects this at build time.\n\n| File | Export | Directive |\n|------|--------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | `\"{}\"` |",
                    path_line_cell(it),
                    s(it, "export_name"),
                    s(it, "directive"),
                )
            },
        },
        SectionSpec {
            name: "Mixed client/server barrels",
            key: "mixed_client_server_barrels",
            header: "Barrels re-exporting both a `\"use client\"` module and a server-only module. One import drags the other's directive across the boundary.\n\n| File | Client origin | Server origin |\n|------|---------------|---------------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | `{}` |",
                    path_line_cell(it),
                    s(it, "client_origin"),
                    s(it, "server_origin"),
                )
            },
        },
        SectionSpec {
            name: "Misplaced directives",
            key: "misplaced_directives",
            header: "`\"use client\"` / `\"use server\"` directives written after a non-directive statement, so the RSC bundler ignores them. Move the directive to the top of the file.\n\n| File | Directive |\n|------|-----------|\n",
            row: |it| format!("| {} | `\"{}\"` |", path_line_cell(it), s(it, "directive")),
        },
        SectionSpec {
            name: "Unused server actions",
            key: "unused_server_actions",
            header: "Next.js Server Actions (exports of a `\"use server\"` file) that no project code references. The endpoint stays POST-able, but no code calls it (likely dead).\n\n| File | Action |\n|------|--------|\n",
            row: |it| format!("| {} | `{}` |", path_line_cell(it), s(it, "action_name")),
        },
        SectionSpec {
            name: "Route collisions",
            key: "route_collisions",
            header: "Next.js App Router route files that resolve to the same URL within one app-root. Next.js fails the build because a URL can have only one owner.\n\n| File | URL |\n|------|-----|\n",
            row: |it| format!("| `{}` | `{}` |", s(it, "path"), s(it, "url")),
        },
        SectionSpec {
            name: "Dynamic segment conflicts",
            key: "dynamic_segment_name_conflicts",
            header: "Sibling Next.js dynamic route segments at one position using different slug names. Next.js requires one consistent name per dynamic path.\n\n| File | Position | Segments |\n|------|----------|----------|\n",
            row: |it| {
                format!(
                    "| `{}` | `{}` | `{}` |",
                    s(it, "path"),
                    s(it, "position"),
                    plain_join(it, "conflicting_segments", ", "),
                )
            },
        },
    ]
}

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
fn check_sections_frameworks_and_hygiene() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unrendered components",
            key: "unrendered_components",
            header: "Vue/Svelte components reachable in the module graph but rendered nowhere: no tag, no dynamic binding, no registration. A barrel re-export keeps them alive even though nothing instantiates them.\n\n| File | Component | Framework |\n|------|-----------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | {} |",
                    path_line_cell(it),
                    s(it, "component_name"),
                    s(it, "framework"),
                )
            },
        },
        SectionSpec {
            name: "Unused component props",
            key: "unused_component_props",
            header: "Vue `defineProps` props referenced nowhere inside their own single-file component (neither script nor template).\n\n| File | Component | Prop |\n|------|-----------|------|\n",
            row: |it| component_detail_row(it, "prop_name"),
        },
        SectionSpec {
            name: "Unused component emits",
            key: "unused_component_emits",
            header: "Vue `defineEmits` events emitted nowhere inside their own single-file component (no matching `emit()` call).\n\n| File | Component | Event |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "emit_name"),
        },
        SectionSpec {
            name: "Unused component inputs",
            key: "unused_component_inputs",
            header: "Angular `@Input()` / signal `input()` declarations read nowhere inside their own component (neither class body nor template).\n\n| File | Component | Input |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "input_name"),
        },
        SectionSpec {
            name: "Unused component outputs",
            key: "unused_component_outputs",
            header: "Angular `@Output()` / signal `output()` declarations emitted nowhere inside their own component (no matching `emit()` call).\n\n| File | Component | Output |\n|------|-----------|--------|\n",
            row: |it| component_detail_row(it, "output_name"),
        },
        SectionSpec {
            name: "Unused Svelte events",
            key: "unused_svelte_events",
            header: "Svelte components dispatching a `createEventDispatcher` event listened to nowhere in the project (cross-file dead-output direction).\n\n| File | Component | Event |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "event_name"),
        },
        SectionSpec {
            name: "Unprovided injects",
            key: "unprovided_injects",
            header: "Vue `inject` / Svelte `getContext` calls for a key that no ancestor `provide` / `setContext` supplies.\n\n| File | Key | Framework |\n|------|-----|-----------|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | {} |",
                    path_line_cell(it),
                    s(it, "key_name"),
                    s(it, "framework"),
                )
            },
        },
        SectionSpec {
            name: "Unused load data keys",
            key: "unused_load_data_keys",
            header: "SvelteKit `load()` return-object keys read by no consumer (neither the sibling `+page.svelte` nor `$page.data`). The key runs a real server fetch / DB cost per request for data nothing renders.\n\n| File | Route | Key |\n|------|-------|-----|\n",
            row: |it| {
                format!(
                    "| {} | `{}` | `{}` |",
                    path_line_cell(it),
                    s(it, "route_dir"),
                    s(it, "key_name"),
                )
            },
        },
        SectionSpec {
            name: "Type-only dependencies",
            key: "type_only_dependencies",
            header: "Dependencies only used for type imports - consider moving to `devDependencies`.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Test-only dependencies",
            key: "test_only_dependencies",
            header: "Production dependencies only imported by test files - consider moving to `devDependencies`.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Dev dependencies used in production",
            key: "dev_dependencies_in_production",
            header: "`devDependencies` imported by production code at runtime - consider moving to `dependencies` so a production-only install does not break.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Stale suppressions",
            key: "stale_suppressions",
            header: "Suppression comments or JSDoc tags that no longer match any active issue.\n\n| File | Line | Description |\n|------|-----:|-------------|\n",
            row: |it| {
                format!(
                    "| `{}` | {} | {} |",
                    s(it, "path"),
                    num(it, "line"),
                    stale_suppression_description(it),
                )
            },
        },
    ]
}

fn component_detail_row(it: &Value, detail_key: &str) -> String {
    format!(
        "| {} | `{}` | `{}` |",
        path_line_cell(it),
        s(it, "component_name"),
        s(it, detail_key),
    )
}

fn package_row(it: &Value) -> String {
    format!("| `{}` |", s(it, "package_name"))
}

fn stale_suppression_description(it: &Value) -> String {
    let origin = it.get("origin").cloned().unwrap_or(Value::Null);
    if s(&origin, "type") == "jsdoc_tag" {
        return format!("`@expected-unused` on `{}`", s(&origin, "export_name"));
    }
    if origin.get("kind_known").and_then(Value::as_bool) == Some(false) {
        return format!("unknown kind `{}`", s(&origin, "issue_kind"));
    }
    match origin.get("issue_kind").and_then(Value::as_str) {
        Some(kind) => format!("`{kind}`"),
        None => "blanket".to_owned(),
    }
}

fn check_sections_catalog() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unused catalog entries",
            key: "unused_catalog_entries",
            header: "pnpm catalog entries not referenced by any workspace package.\n\n| Entry | Catalog | Location | Hardcoded consumers |\n|-------|---------|----------|---------------------|\n",
            row: |it| {
                format!(
                    "| `{}` | `{}` | {} | {} |",
                    s(it, "entry_name"),
                    s(it, "catalog_name"),
                    path_line_cell(it),
                    backtick_join(it, "hardcoded_consumers"),
                )
            },
        },
        SectionSpec {
            name: "Empty catalog groups",
            key: "empty_catalog_groups",
            header: "Named pnpm catalog groups with no entries.\n\n| Catalog | Location |\n|---------|----------|\n",
            row: |it| format!("| `{}` | {} |", s(it, "catalog_name"), path_line_cell(it)),
        },
        SectionSpec {
            name: "Unresolved catalog references",
            key: "unresolved_catalog_references",
            header: "Workspace `package.json` references to catalogs that do not declare the package. `pnpm install` will fail until each entry is added to its named catalog or the reference is switched.\n\n| Entry | Catalog | Location | Available in |\n|-------|---------|----------|--------------|\n",
            row: |it| {
                format!(
                    "| `{}` | `{}` | {} | {} |",
                    s(it, "entry_name"),
                    s(it, "catalog_name"),
                    path_line_cell(it),
                    backtick_join(it, "available_in_catalogs"),
                )
            },
        },
        SectionSpec {
            name: "Unused dependency overrides",
            key: "unused_dependency_overrides",
            header: "`pnpm.overrides` entries forcing a version no workspace package depends on. Some entries may be intentional pins for transitive CVEs; the hint column flags those.\n\n| Override | Forces | Source | Location | Hint |\n|----------|--------|--------|----------|------|\n",
            row: |it| {
                format!(
                    "| `{}` | `{}` -> `{}` | `{}` | {} | {} |",
                    s(it, "raw_key"),
                    s(it, "target_package"),
                    s(it, "version_range"),
                    s(it, "source"),
                    path_line_cell(it),
                    str_or(it, "hint", ""),
                )
            },
        },
        SectionSpec {
            name: "Misconfigured dependency overrides",
            key: "misconfigured_dependency_overrides",
            header: "`pnpm.overrides` entries with an unparsable key or empty value. `pnpm install` will reject these.\n\n| Override | Value | Source | Location | Reason |\n|----------|-------|--------|----------|--------|\n",
            row: |it| {
                format!(
                    "| `{}` | `{}` | `{}` | {} | {} |",
                    str_or(it, "raw_key", ""),
                    str_or(it, "raw_value", ""),
                    s(it, "source"),
                    path_line_cell(it),
                    str_or(it, "reason", "unparsable"),
                )
            },
        },
    ]
}

fn check_tips(env: &Value) -> String {
    let fixable = arr(env, "unused_exports").count()
        + arr(env, "unused_dependencies").count()
        + arr(env, "unused_enum_members").count();
    let mut tips = String::from("\n\n> [!TIP]\n");
    if fixable > 0 {
        tips.push_str("> Run `fallow fix --dry-run` to preview safe auto-fixes.\n");
    }
    if arr(env, "unused_exports").count() > 0 {
        let _ = writeln!(
            tips,
            "> Intentionally public? Add [`/** @public */`]({SUPPRESSION_DOCS}) above exports to preserve them."
        );
    }
    let _ = write!(
        tips,
        "> Add [`// fallow-ignore-next-line`]({SUPPRESSION_DOCS}) above a line to suppress a specific finding."
    );
    tips
}

/// Port of `summary-check.jq`.
#[must_use]
pub fn render_check_summary(env: &Value) -> String {
    let elapsed = num(env, "elapsed_ms");
    let total_issues = u(env, "total_issues");
    if total_issues == 0 {
        return format!(
            "# Fallow Analysis\n\n> [!NOTE]\n> **No issues found** \u{b7} {elapsed}ms\n\nAll exports are used, all dependencies are declared, and no issues were detected."
        );
    }
    let mut sections = String::new();
    for group in [
        check_sections_core(),
        check_sections_architecture(),
        check_sections_frameworks_and_hygiene(),
        check_sections_catalog(),
    ] {
        for spec in &group {
            sections.push_str(&render_check_section(env, spec));
        }
    }
    let issue_noun = if total_issues == 1 { "issue" } else { "issues" };
    format!(
        "# Fallow Analysis\n\n> [!WARNING]\n> **{total_issues} {issue_noun}** found \u{b7} {elapsed}ms\n\n| Category | Count |\n|----------|------:|\n{}\n\n---\n{sections}{}",
        dead_code_category_table(env),
        check_tips(env),
    )
}

// ---------------------------------------------------------------------------
// summary-dupes.jq
// ---------------------------------------------------------------------------

fn dupes_family_entry(family: &Value) -> String {
    let files: Vec<&str> = arr(family, "files").filter_map(Value::as_str).collect();
    let shown = files.iter().take(3).copied().collect::<Vec<_>>().join(", ");
    let more = if files.len() > 3 {
        format!(" (+{} more)", files.len() - 3)
    } else {
        String::new()
    };
    let mut entry = format!(
        "- **{shown}{more}** - {} lines, {} groups",
        num(family, "total_duplicated_lines"),
        arr(family, "groups").count(),
    );
    if let Some(first_group) = arr(family, "groups").next()
        && arr(first_group, "instances").next().is_some()
    {
        let locations = arr(first_group, "instances")
            .map(instance_location)
            .collect::<Vec<_>>()
            .join(", ");
        let _ = write!(entry, "\n  - {locations}");
    }
    if arr(family, "suggestions").next().is_some() {
        let suggestions = arr(family, "suggestions")
            .map(|suggestion| {
                format!(
                    "  - {} (~{} lines)",
                    s(suggestion, "description"),
                    num(suggestion, "estimated_savings"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(entry, "\n{suggestions}");
    }
    entry
}

fn instance_location(instance: &Value) -> String {
    format!(
        "`{}:{}-{}`",
        s(instance, "file"),
        num(instance, "start_line"),
        num(instance, "end_line"),
    )
}

/// jq: `sort_by([line_count, token_count]) | reverse` (stable sort then full
/// reverse, so ties land in reverse input order).
fn sorted_clone_groups(env: &Value) -> Vec<&Value> {
    let mut groups: Vec<&Value> = arr(env, "clone_groups").collect();
    groups.sort_by_key(|group| (u(group, "line_count"), u(group, "token_count")));
    groups.reverse();
    groups
}

fn dupes_details(env: &Value) -> String {
    let families: Vec<&Value> = arr(env, "clone_families").collect();
    if families.is_empty() {
        let groups = sorted_clone_groups(env);
        let rows = groups
            .iter()
            .take(20)
            .map(|group| {
                let locations = arr(group, "instances")
                    .map(instance_location)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "- **{} lines, {} tokens**, {locations}",
                    num(group, "line_count"),
                    num(group, "token_count"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let tail = if groups.len() > 20 {
            format!("\n- *... and {} more groups*", groups.len() - 20)
        } else {
            String::new()
        };
        format!("{rows}{tail}")
    } else {
        let entries = families
            .iter()
            .take(15)
            .map(|family| dupes_family_entry(family))
            .collect::<Vec<_>>()
            .join("\n");
        let tail = if families.len() > 15 {
            format!("\n- *... and {} more families*", families.len() - 15)
        } else {
            String::new()
        };
        format!("**Clone Families ({})**\n\n{entries}{tail}", families.len())
    }
}

/// Port of `summary-dupes.jq`.
#[must_use]
pub fn render_dupes_summary(env: &Value) -> String {
    let stats = env.get("stats").cloned().unwrap_or(Value::Null);
    let elapsed = num(env, "elapsed_ms");
    if u(&stats, "clone_groups") == 0 {
        return format!(
            "## Fallow - Code Duplication\n\nNo code duplication found.\n\n*Analyzed {} files in {elapsed}ms*",
            num(&stats, "total_files"),
        );
    }
    format!(
        "## Fallow - Code Duplication\n\nFound **{} clone groups** ({} instances) across {} files in {elapsed}ms\n\n| Metric | Value |\n|--------|-------|\n| Files analyzed | {} |\n| Files with clones | {} |\n| Clone groups | {} |\n| Clone instances | {} |\n| Duplicated lines | {} / {} ({}%) |\n\n<details>\n<summary>View details</summary>\n\n{}\n\n</details>",
        num(&stats, "clone_groups"),
        num(&stats, "clone_instances"),
        num(&stats, "files_with_clones"),
        num(&stats, "total_files"),
        num(&stats, "files_with_clones"),
        num(&stats, "clone_groups"),
        num(&stats, "clone_instances"),
        num(&stats, "duplicated_lines"),
        num(&stats, "total_lines"),
        pct(f_or_zero(&stats, "duplication_percentage")),
        dupes_details(env),
    )
}

// ---------------------------------------------------------------------------
// summary-health.jq
// ---------------------------------------------------------------------------

fn metric_delta<'v>(score_env: &'v Value, name: &str) -> Option<&'v Value> {
    score_env
        .get("health_trend")
        .and_then(|trend| trend.get("metrics"))
        .and_then(Value::as_array)
        .and_then(|metrics| metrics.iter().find(|metric| s(metric, "name") == name))
}

/// The `> **Health: ...**` trend header shared by `summary-health.jq` and
/// `summary-combined.jq`. Empty when no `health_score` block is present.
fn health_score_header(score_env: &Value) -> String {
    let Some(score) = score_env
        .get("health_score")
        .filter(|value| !value.is_null())
    else {
        return String::new();
    };
    let mut header = format!(
        "> **Health: {} ({})**",
        s(score, "grade"),
        pct(f_or_zero(score, "score")),
    );
    if let Some(score_delta) = metric_delta(score_env, "score") {
        let compared = score_env
            .get("health_trend")
            .and_then(|trend| trend.get("compared_to"))
            .cloned()
            .unwrap_or(Value::Null);
        let _ = write!(
            header,
            " \u{b7} {} pts vs previous ({} {})",
            signed(f_or_zero(score_delta, "delta")),
            s(&compared, "grade"),
            pct(f_or_zero(&compared, "score")),
        );
        if let Some(dead_delta) = metric_delta(score_env, "dead_export_pct")
            && f_or_zero(dead_delta, "delta") != 0.0
        {
            let _ = write!(
                header,
                " \u{b7} {} {}% ({}%)",
                s(dead_delta, "label").to_ascii_lowercase(),
                pct(f_or_zero(dead_delta, "current")),
                signed(f_or_zero(dead_delta, "delta")),
            );
            if f_or_zero(dead_delta, "delta") > 0.0 {
                let _ = write!(header, " [suppress?]({SUPPRESSION_DOCS})");
            }
        }
        if let Some(cx_delta) = metric_delta(score_env, "avg_cyclomatic")
            && f_or_zero(cx_delta, "delta") != 0.0
        {
            let _ = write!(
                header,
                " \u{b7} {} {} ({})",
                s(cx_delta, "label").to_ascii_lowercase(),
                pct(f_or_zero(cx_delta, "current")),
                signed(f_or_zero(cx_delta, "delta")),
            );
        }
    } else {
        header.push_str("\n> _Enable `save-snapshot: true` to track score trends over time._");
    }
    header.push_str("\n\n");
    header
}

fn exceeded_marker(it: &Value, needles: &[&str]) -> &'static str {
    let exceeded = s(it, "exceeded");
    if needles.iter().any(|needle| exceeded.contains(needle)) {
        " **!**"
    } else {
        ""
    }
}

fn crap_cell(it: &Value) -> String {
    match it.get("crap").filter(|crap| !crap.is_null()) {
        None => "-".to_owned(),
        Some(crap) => format!("{}{}", fmt_num(crap), exceeded_marker(it, &["crap", "all"])),
    }
}

fn complexity_table_row(it: &Value) -> String {
    format!(
        "| `{}:{}` | `{}` | {} | {}{} | {}{} | {} | {} |",
        s(it, "path"),
        num(it, "line"),
        s(it, "name"),
        str_or(it, "severity", "moderate"),
        num(it, "cyclomatic"),
        exceeded_marker(it, &["cyclomatic", "both", "all"]),
        num(it, "cognitive"),
        exceeded_marker(it, &["cognitive", "both", "all"]),
        crap_cell(it),
        num(it, "line_count"),
    )
}

const COMPLEXITY_TABLE_HEADER: &str = "| File | Function | Severity | Cyclomatic | Cognitive | CRAP | Lines |\n|:-----|:---------|:---------|:-----------|:----------|:-----|:------|\n";

fn health_thresholds_footer(env: &Value) -> String {
    let summary = env.get("summary").cloned().unwrap_or(Value::Null);
    format!(
        "\n\n**{}** files, **{}** functions analyzed (thresholds: cyclomatic > {}, cognitive > {}, CRAP >= {})",
        num(&summary, "files_analyzed"),
        num(&summary, "functions_analyzed"),
        num(&summary, "max_cyclomatic_threshold"),
        num(&summary, "max_cognitive_threshold"),
        threshold_or(&summary, "max_crap_threshold", "30"),
    )
}

fn threshold_or(summary: &Value, key: &str, default: &str) -> String {
    summary
        .get(key)
        .filter(|value| !value.is_null())
        .map_or_else(|| default.to_owned(), fmt_num)
}

fn complexity_rows(findings: &[&Value], cap: usize) -> String {
    findings
        .iter()
        .take(cap)
        .map(|finding| complexity_table_row(finding))
        .collect::<Vec<_>>()
        .join("\n")
}

fn runtime_finding_row(it: &Value) -> String {
    let invocations = it
        .get("invocations")
        .filter(|value| !value.is_null())
        .map_or_else(|| "-".to_owned(), fmt_num);
    format!(
        "| `{}:{}` | `{}` | `{}` | {invocations} | {} |",
        s(it, "path"),
        num(it, "line"),
        s(it, "function"),
        s(it, "verdict"),
        s(it, "confidence"),
    )
}

fn render_health_complexity_only(env: &Value, complex: usize, elapsed: &str) -> String {
    let summary = env.get("summary").cloned().unwrap_or(Value::Null);
    if complex == 0 {
        return format!(
            "## Fallow - Code Complexity\n\n> [!NOTE]\n> **No functions exceed complexity thresholds** \u{b7} {elapsed}ms\n\n{} functions analyzed (max cyclomatic: {}, max cognitive: {}, max CRAP: {})",
            num(&summary, "functions_analyzed"),
            num(&summary, "max_cyclomatic_threshold"),
            num(&summary, "max_cognitive_threshold"),
            threshold_or(&summary, "max_crap_threshold", "30"),
        );
    }
    let above = u(&summary, "functions_above_threshold");
    let findings: Vec<&Value> = arr(env, "findings").collect();
    let tail = if complex > 25 {
        format!(
            "\n\n> {} more - run `fallow health` locally for the full list",
            complex - 25
        )
    } else {
        String::new()
    };
    format!(
        "## Fallow - Code Complexity\n\n> [!WARNING]\n> **{above} function{} exceed{} thresholds** \u{b7} {elapsed}ms\n\n{COMPLEXITY_TABLE_HEADER}{}{tail}{}",
        if above == 1 { "" } else { "s" },
        if above == 1 { "s" } else { "" },
        complexity_rows(&findings, 25),
        health_thresholds_footer(env),
    )
}

fn prod_phrase(complex: usize, prod: usize) -> String {
    let complexity = format!(
        "{complex} complexity finding{}",
        if complex == 1 { "" } else { "s" }
    );
    let runtime = format!(
        "{prod} runtime coverage finding{}",
        if prod == 1 { "" } else { "s" }
    );
    if complex > 0 && prod > 0 {
        format!("{complexity} and {runtime}")
    } else if complex > 0 {
        complexity
    } else {
        runtime
    }
}

fn render_health_with_runtime(env: &Value, complex: usize, elapsed: &str) -> String {
    let runtime = env.get("runtime_coverage").cloned().unwrap_or(Value::Null);
    let prod_findings: Vec<&Value> = arr(&runtime, "findings").collect();
    let hot_paths: Vec<&Value> = arr(&runtime, "hot_paths").collect();
    let prod = prod_findings.len();
    let hot = hot_paths.len();
    let mut out = String::from("## Fallow - Health\n\n");
    if complex == 0 && prod == 0 {
        let _ = write!(
            out,
            "> [!NOTE]\n> **No failing health findings** \u{b7} {elapsed}ms\n\n"
        );
    } else {
        let _ = write!(
            out,
            "> [!WARNING]\n> **{}** \u{b7} {elapsed}ms\n\n",
            prod_phrase(complex, prod),
        );
    }
    if complex > 0 {
        let findings: Vec<&Value> = arr(env, "findings").collect();
        let _ = write!(
            out,
            "### Complexity\n\n{COMPLEXITY_TABLE_HEADER}{}",
            complexity_rows(&findings, 25),
        );
        if complex > 25 {
            let _ = write!(
                out,
                "\n\n> {} more complexity findings - run `fallow health` locally for the full list",
                complex - 25,
            );
        }
    }
    if prod > 0 {
        if complex > 0 {
            out.push_str("\n\n");
        }
        out.push_str("### Runtime Coverage\n\n| File | Function | Verdict | Invocations | Confidence |\n|:-----|:---------|:--------|------------:|:-----------|\n");
        out.push_str(
            &prod_findings
                .iter()
                .take(25)
                .map(|finding| runtime_finding_row(finding))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if prod > 25 {
            let _ = write!(
                out,
                "\n\n> {} more runtime coverage findings - run `fallow health` locally for the full list",
                prod - 25,
            );
        }
    }
    if hot > 0 {
        if complex > 0 || prod > 0 {
            out.push_str("\n\n");
        }
        out.push_str("### Hot Paths\n\n| File | Function | Invocations | Percentile |\n|:-----|:---------|------------:|-----------:|\n");
        out.push_str(
            &hot_paths
                .iter()
                .take(10)
                .map(|path| {
                    format!(
                        "| `{}:{}` | `{}` | {} | {} |",
                        s(path, "path"),
                        num(path, "line"),
                        s(path, "function"),
                        num(path, "invocations"),
                        num(path, "percentile"),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if hot > 10 {
            let _ = write!(out, "\n\n> {} more hot paths in the full report", hot - 10);
        }
    }
    out.push_str(&health_runtime_footer(env, complex, prod, hot, &runtime));
    out
}

fn health_runtime_footer(
    env: &Value,
    complex: usize,
    prod: usize,
    hot: usize,
    runtime: &Value,
) -> String {
    if complex > 0 {
        return health_thresholds_footer(env);
    }
    if prod > 0 {
        let summary = runtime.get("summary").cloned().unwrap_or(Value::Null);
        return format!(
            "\n\n**{}** tracked functions, **{}** hit, **{}** unhit, **{}** untracked",
            num(&summary, "functions_tracked"),
            num(&summary, "functions_hit"),
            num(&summary, "functions_unhit"),
            num(&summary, "functions_untracked"),
        );
    }
    format!(
        "\n\nObserved **{hot}** hot path{} in runtime coverage.",
        if hot == 1 { "" } else { "s" },
    )
}

/// Port of `summary-health.jq`.
#[must_use]
pub fn render_health_summary(env: &Value) -> String {
    let elapsed = num(env, "elapsed_ms");
    let complex = arr(env, "findings").count();
    let runtime = env.get("runtime_coverage").cloned().unwrap_or(Value::Null);
    let prod = arr(&runtime, "findings").count();
    let hot = arr(&runtime, "hot_paths").count();
    let body = if prod == 0 && hot == 0 {
        render_health_complexity_only(env, complex, &elapsed)
    } else {
        render_health_with_runtime(env, complex, &elapsed)
    };
    format!("{}{body}", health_score_header(env))
}

// ---------------------------------------------------------------------------
// summary-audit.jq
// ---------------------------------------------------------------------------

const fn audit_verdict_label(verdict: &str) -> &'static str {
    match verdict.as_bytes() {
        b"fail" => "[!WARNING]\n> **Audit failed**",
        b"warn" => "[!WARNING]\n> **Audit passed with warnings**",
        _ => "[!NOTE]\n> **Audit passed**",
    }
}

fn introduced_label(item: &Value) -> &'static str {
    match item.get("introduced").and_then(Value::as_bool) {
        Some(true) => "new",
        Some(false) => "inherited",
        None => "-",
    }
}

struct AuditRow {
    kind: &'static str,
    location: String,
    item: String,
    status: &'static str,
}

fn audit_row(kind: &'static str, location: String, item: String, finding: &Value) -> AuditRow {
    AuditRow {
        kind,
        location,
        item,
        status: introduced_label(finding),
    }
}

type AuditRowSpec = (&'static str, &'static str, fn(&Value) -> String);

/// jq order positions 2-11: exports, types, leaks, dependencies, members,
/// unresolved imports.
const AUDIT_EXPORT_DEP_ROWS: &[AuditRowSpec] = &[
    ("Unused export", "unused_exports", |it| {
        format!("`{}`", s(it, "export_name"))
    }),
    ("Unused type", "unused_types", |it| {
        format!("`{}`", s(it, "export_name"))
    }),
    ("Private type leak", "private_type_leaks", |it| {
        format!("`{}` -> `{}`", s(it, "export_name"), s(it, "type_name"))
    }),
    ("Unused dependency", "unused_dependencies", |it| {
        format!("`{}`", s(it, "package_name"))
    }),
    ("Unused devDependency", "unused_dev_dependencies", |it| {
        format!("`{}`", s(it, "package_name"))
    }),
    (
        "Unused optionalDependency",
        "unused_optional_dependencies",
        |it| format!("`{}`", s(it, "package_name")),
    ),
    ("Unused enum member", "unused_enum_members", member_item),
    ("Unused class member", "unused_class_members", member_item),
    ("Unused store member", "unused_store_members", member_item),
    ("Unresolved import", "unresolved_imports", |it| {
        format!("`{}`", s(it, "specifier"))
    }),
];

/// jq order positions 26-33: component-model kinds.
const AUDIT_COMPONENT_ROWS: &[AuditRowSpec] = &[
    ("Unrendered component", "unrendered_components", |it| {
        format!("`{}` ({})", s(it, "component_name"), s(it, "framework"))
    }),
    ("Unused component prop", "unused_component_props", |it| {
        format!("`{}.{}`", s(it, "component_name"), s(it, "prop_name"))
    }),
    ("Unused component emit", "unused_component_emits", |it| {
        format!(
            "`{}` emit `{}`",
            s(it, "component_name"),
            s(it, "emit_name")
        )
    }),
    ("Unused component input", "unused_component_inputs", |it| {
        format!("`{}.{}`", s(it, "component_name"), s(it, "input_name"))
    }),
    (
        "Unused component output",
        "unused_component_outputs",
        |it| {
            format!(
                "`{}` output `{}`",
                s(it, "component_name"),
                s(it, "output_name")
            )
        },
    ),
    ("Unused Svelte event", "unused_svelte_events", |it| {
        format!(
            "`{}` event `{}`",
            s(it, "component_name"),
            s(it, "event_name")
        )
    }),
    ("Unprovided inject", "unprovided_injects", |it| {
        format!("`{}` ({})", s(it, "key_name"), s(it, "framework"))
    }),
    ("Unused load data key", "unused_load_data_keys", |it| {
        format!("`{}`", s(it, "key_name"))
    }),
];

/// jq order positions 34-42: dependency hygiene, suppressions, catalog.
const AUDIT_HYGIENE_ROWS: &[AuditRowSpec] = &[
    ("Type-only dependency", "type_only_dependencies", |it| {
        format!("`{}`", s(it, "package_name"))
    }),
    ("Test-only dependency", "test_only_dependencies", |it| {
        format!("`{}`", s(it, "package_name"))
    }),
    (
        "Dev dependency in production",
        "dev_dependencies_in_production",
        |it| format!("`{}`", s(it, "package_name")),
    ),
    ("Stale suppression", "stale_suppressions", |it| {
        str_or(it, "description", "suppression").to_owned()
    }),
    ("Unused catalog entry", "unused_catalog_entries", |it| {
        format!("`{}` (`{}`)", s(it, "entry_name"), s(it, "catalog_name"))
    }),
    ("Empty catalog group", "empty_catalog_groups", |it| {
        format!("`{}`", s(it, "catalog_name"))
    }),
    (
        "Unresolved catalog reference",
        "unresolved_catalog_references",
        |it| format!("`{}` -> `{}`", s(it, "entry_name"), s(it, "catalog_name")),
    ),
    (
        "Unused dependency override",
        "unused_dependency_overrides",
        |it| format!("`{}` (`{}`)", s(it, "raw_key"), s(it, "source")),
    ),
    (
        "Misconfigured dependency override",
        "misconfigured_dependency_overrides",
        |it| format!("`{}` (`{}`)", s(it, "raw_key"), s(it, "source")),
    ),
];

fn audit_rows_from_table(dead_code: &Value, table: &[AuditRowSpec], rows: &mut Vec<AuditRow>) {
    for (kind, key, item_fn) in table {
        for finding in arr(dead_code, key) {
            rows.push(audit_row(
                kind,
                path_line(finding),
                item_fn(finding),
                finding,
            ));
        }
    }
}

fn member_item(it: &Value) -> String {
    format!("`{}.{}`", s(it, "parent_name"), s(it, "member_name"))
}

fn first_import_site(it: &Value) -> String {
    arr(it, "imported_from").next().map_or_else(
        || path_line(it),
        |site| {
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(site, "path")),
                num(site, "line"),
            )
        },
    )
}

/// jq order positions 12-15: unlisted dependencies, duplicate exports,
/// circular dependencies, re-export cycles.
fn audit_rows_graph(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for it in arr(dead_code, "unlisted_dependencies") {
        rows.push(audit_row(
            "Unlisted dependency",
            first_import_site(it),
            format!("`{}`", s(it, "package_name")),
            it,
        ));
    }
    for it in arr(dead_code, "duplicate_exports") {
        let location = arr(it, "locations")
            .take(3)
            .map(|loc| {
                format!(
                    "`{}:{}`",
                    rel_path_absolute_only(s(loc, "path")),
                    num(loc, "line")
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(audit_row(
            "Duplicate export",
            location,
            format!("`{}`", s(it, "export_name")),
            it,
        ));
    }
    for it in arr(dead_code, "circular_dependencies") {
        let location = arr(it, "files")
            .filter_map(Value::as_str)
            .map(|file| format!("`{}`", rel_path_absolute_only(file)))
            .collect::<Vec<_>>()
            .join(" -> ");
        rows.push(audit_row(
            "Circular dependency",
            location,
            "cycle".to_owned(),
            it,
        ));
    }
    for it in arr(dead_code, "re_export_cycles") {
        let location = arr(it, "files")
            .filter_map(Value::as_str)
            .map(|file| format!("`{}`", rel_path_absolute_only(file)))
            .collect::<Vec<_>>()
            .join(" <-> ");
        rows.push(audit_row(
            "Re-export cycle",
            location,
            str_or(it, "kind", "cycle").to_owned(),
            it,
        ));
    }
}

/// jq order positions 16-19: boundary and policy kinds.
fn audit_rows_boundaries(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for it in arr(dead_code, "boundary_violations") {
        rows.push(audit_row(
            "Boundary violation",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "from_path")),
                num(it, "line")
            ),
            format!("{} -> {}", s(it, "from_zone"), s(it, "to_zone")),
            it,
        ));
    }
    for it in arr(dead_code, "boundary_coverage_violations") {
        rows.push(audit_row(
            "Boundary coverage",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            "no matching zone".to_owned(),
            it,
        ));
    }
    for it in arr(dead_code, "boundary_call_violations") {
        rows.push(audit_row(
            "Boundary call",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            format!("`{}` in {}", s(it, "callee"), s(it, "zone")),
            it,
        ));
    }
    for it in arr(dead_code, "policy_violations") {
        rows.push(audit_row(
            "Policy violation",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            format!(
                "`{}` banned by {}/{}",
                s(it, "matched"),
                s(it, "pack"),
                s(it, "rule_id")
            ),
            it,
        ));
    }
}

/// jq order positions 20-25: RSC/Next.js kinds, including unused server
/// actions between misplaced directives and route collisions.
fn audit_rows_frameworks(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for it in arr(dead_code, "invalid_client_exports") {
        rows.push(audit_row(
            "Invalid client export",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            format!("`{}` in `\"{}\"`", s(it, "export_name"), s(it, "directive")),
            it,
        ));
    }
    for it in arr(dead_code, "mixed_client_server_barrels") {
        rows.push(audit_row(
            "Mixed client/server barrel",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            format!(
                "`{}` + `{}`",
                s(it, "client_origin"),
                s(it, "server_origin")
            ),
            it,
        ));
    }
    for it in arr(dead_code, "misplaced_directives") {
        rows.push(audit_row(
            "Misplaced directive",
            format!(
                "`{}:{}`",
                rel_path_absolute_only(s(it, "path")),
                num(it, "line")
            ),
            format!("`\"{}\"`", s(it, "directive")),
            it,
        ));
    }
    for it in arr(dead_code, "unused_server_actions") {
        rows.push(audit_row(
            "Unused server action",
            path_line(it),
            format!("`{}`", s(it, "action_name")),
            it,
        ));
    }
    for it in arr(dead_code, "route_collisions") {
        rows.push(audit_row(
            "Route collision",
            format!("`{}`", rel_path_absolute_only(s(it, "path"))),
            format!("`{}`", s(it, "url")),
            it,
        ));
    }
    for it in arr(dead_code, "dynamic_segment_name_conflicts") {
        rows.push(audit_row(
            "Dynamic segment conflict",
            format!("`{}`", rel_path_absolute_only(s(it, "path"))),
            format!("`{}`", plain_join(it, "conflicting_segments", ", ")),
            it,
        ));
    }
}

/// Rows in `summary-audit.jq`'s `dead_code_rows` declaration order.
fn audit_dead_code_rows(dead_code: &Value) -> Vec<AuditRow> {
    let mut rows: Vec<AuditRow> = Vec::new();
    for it in arr(dead_code, "unused_files") {
        rows.push(audit_row(
            "Unused file",
            format!("`{}`", rel_path_absolute_only(s(it, "path"))),
            "-".to_owned(),
            it,
        ));
    }
    audit_rows_from_table(dead_code, AUDIT_EXPORT_DEP_ROWS, &mut rows);
    audit_rows_graph(dead_code, &mut rows);
    audit_rows_boundaries(dead_code, &mut rows);
    audit_rows_frameworks(dead_code, &mut rows);
    audit_rows_from_table(dead_code, AUDIT_COMPONENT_ROWS, &mut rows);
    audit_rows_from_table(dead_code, AUDIT_HYGIENE_ROWS, &mut rows);
    rows
}

fn audit_complexity_section(env: &Value) -> String {
    let complexity = env.get("complexity").cloned().unwrap_or(Value::Null);
    let findings: Vec<&Value> = arr(&complexity, "findings").collect();
    if findings.is_empty() {
        return String::new();
    }
    let rows = findings
        .iter()
        .take(15)
        .map(|it| {
            format!(
                "| `{}:{}` | `{}` | {} | {} | {} | {} | {} | {} |",
                s(it, "path"),
                num(it, "line"),
                s(it, "name"),
                introduced_label(it),
                str_or(it, "severity", "moderate"),
                num(it, "cyclomatic"),
                num(it, "cognitive"),
                str_or(it, "coverage_tier", "-"),
                it.get("crap")
                    .filter(|crap| !crap.is_null())
                    .map_or_else(|| "-".to_owned(), fmt_num),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tail = if findings.len() > 15 {
        format!(
            "\n\n> {} more complexity findings in the full audit report",
            findings.len() - 15,
        )
    } else {
        String::new()
    };
    format!(
        "### Complexity\n\n| File | Function | Status | Severity | Cyclomatic | Cognitive | Coverage | CRAP |\n|:-----|:---------|:-------|:---------|:-----------|:----------|:---------|:-----|\n{rows}{tail}{}\n\n",
        audit_coverage_model_note(&complexity),
    )
}

fn audit_coverage_model_note(complexity: &Value) -> String {
    let summary = complexity.get("summary").cloned().unwrap_or(Value::Null);
    let model = summary.get("coverage_model").and_then(Value::as_str);
    match model {
        Some("istanbul") => {
            let matched = summary.get("istanbul_matched").and_then(Value::as_u64);
            let total = summary.get("istanbul_total").and_then(Value::as_u64);
            match (matched, total) {
                (Some(matched), Some(total)) if total > 0 => {
                    let suffix = if matched * 100 / total < 50 {
                        ". Low match rate; check `--coverage-root` is correct for this checkout."
                    } else {
                        "."
                    };
                    format!(
                        "\n\n*Coverage model: istanbul. Matched {matched}/{total} functions{suffix}*"
                    )
                }
                _ => "\n\n*Coverage model: istanbul (exact, from `--coverage`).*".to_owned(),
            }
        }
        Some("static_estimated" | "static_binary") => {
            "\n\n*Coverage model: static (estimated). Pair with `--coverage <coverage-final.json>` for measured coverage instead of estimates.*".to_owned()
        }
        _ => String::new(),
    }
}

fn audit_duplication_section(env: &Value) -> String {
    let duplication = env.get("duplication").cloned().unwrap_or(Value::Null);
    let groups: Vec<&Value> = arr(&duplication, "clone_groups").collect();
    if groups.is_empty() {
        return String::new();
    }
    let rows = groups
        .iter()
        .take(10)
        .map(|group| {
            let instances: Vec<&Value> = arr(group, "instances").collect();
            let location = instances.first().map_or_else(
                || "-".to_owned(),
                |first| {
                    let file = s(first, "file");
                    if file.is_empty() {
                        "-".to_owned()
                    } else {
                        let start = first
                            .get("start_line")
                            .filter(|line| !line.is_null())
                            .map_or_else(|| "1".to_owned(), fmt_num);
                        format!("`{}:{start}`", rel_path_absolute_only(file))
                    }
                },
            );
            let mut files: Vec<String> = instances
                .iter()
                .map(|instance| rel_path_absolute_only(s(instance, "file")))
                .collect();
            files.sort();
            files.dedup();
            let files = files.into_iter().take(3).collect::<Vec<_>>().join(", ");
            format!(
                "| {location} | {files} | {} lines / {} tokens | {} | {} |",
                num(group, "line_count"),
                num(group, "token_count"),
                instances.len(),
                introduced_label(group),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tail = if groups.len() > 10 {
        format!(
            "\n\n> {} more clone groups in the full audit report",
            groups.len() - 10
        )
    } else {
        String::new()
    };
    format!(
        "### Duplication\n\n| Location | Files | Size | Instances | Status |\n|:---------|:------|:-----|----------:|:-------|\n{rows}{tail}\n\n"
    )
}

/// Port of `summary-audit.jq`.
#[must_use]
pub fn render_audit_summary(env: &Value) -> String {
    let verdict = str_or(env, "verdict", "pass");
    let summary = env.get("summary").cloned().unwrap_or(Value::Null);
    let attribution = env.get("attribution").cloned().unwrap_or(Value::Null);
    let dead_code = env.get("dead_code").cloned().unwrap_or(Value::Null);
    let dead_rows = audit_dead_code_rows(&dead_code);

    let mut out = format!(
        "## Fallow Audit\n\n> {} \u{b7} {} \u{b7} {}ms\n\n| Category | Findings | Introduced | Inherited |\n|:---------|---------:|-----------:|----------:|\n| Dead code | {} | {} | {} |\n| Complexity | {} | {} | {} |\n| Duplication | {} | {} | {} |\n\n",
        audit_verdict_label(verdict),
        plural_n(u(env, "changed_files_count") as usize, "changed file"),
        num(env, "elapsed_ms"),
        num(&summary, "dead_code_issues"),
        num(&attribution, "dead_code_introduced"),
        num(&attribution, "dead_code_inherited"),
        num(&summary, "complexity_findings"),
        num(&attribution, "complexity_introduced"),
        num(&attribution, "complexity_inherited"),
        num(&summary, "duplication_clone_groups"),
        num(&attribution, "duplication_introduced"),
        num(&attribution, "duplication_inherited"),
    );
    if !dead_rows.is_empty() {
        let rows = dead_rows
            .iter()
            .take(10)
            .map(|row| {
                format!(
                    "| {} | {} | {} | {} |",
                    row.kind, row.location, row.item, row.status
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let tail = if dead_rows.len() > 10 {
            format!(
                "\n\n> {} more dead-code findings in the full audit report",
                dead_rows.len() - 10
            )
        } else {
            String::new()
        };
        let _ = write!(
            out,
            "### Dead Code\n\n| Type | Location | Item | Status |\n|:-----|:---------|:-----|:-------|\n{rows}{tail}\n\n"
        );
    }
    out.push_str(&audit_complexity_section(env));
    out.push_str(&audit_duplication_section(env));
    out.push_str(if s(&attribution, "gate") == "all" {
        "*Audit gate: all. Every finding in changed files affects the verdict.*"
    } else {
        "*Audit gate: new-only. Inherited findings are reported but do not fail the verdict.*"
    });
    out
}

// ---------------------------------------------------------------------------
// summary-security.jq
// ---------------------------------------------------------------------------

/// Port of `summary-security.jq`.
#[must_use]
pub fn render_security_summary(env: &Value) -> String {
    let findings: Vec<&Value> = arr(env, "security_findings").collect();
    let gate = env.get("gate").filter(|gate| !gate.is_null());
    let count = gate.map_or_else(
        || {
            env.get("summary")
                .and_then(|summary| summary.get("security_findings"))
                .and_then(Value::as_u64)
                .unwrap_or(findings.len() as u64) as usize
        },
        |gate| u(gate, "new_count") as usize,
    );
    let mut out = String::from("## Fallow Security\n\n");
    if count == 0 {
        let _ = write!(
            out,
            "> [!NOTE]\n> **No security candidates matched** \u{b7} {}ms",
            num(env, "elapsed_ms"),
        );
    } else {
        let _ = write!(
            out,
            "> [!WARNING]\n> **{} matched** \u{b7} {}ms",
            plural_n(count, "security candidate"),
            num(env, "elapsed_ms"),
        );
    }
    if let Some(gate) = gate {
        let _ = write!(
            out,
            "\n\nSecurity gate: `{}`, verdict: `{}`, matching candidates: **{}**.",
            s(gate, "mode"),
            s(gate, "verdict"),
            num(gate, "new_count"),
        );
    }
    if !findings.is_empty() {
        let rows = findings
            .iter()
            .take(15)
            .map(|finding| {
                format!(
                    "| {} | {} | {} | {} |",
                    path_line(finding),
                    s(finding, "kind"),
                    str_or(finding, "severity", "unknown"),
                    finding
                        .get("candidate")
                        .and_then(|candidate| candidate.get("sink"))
                        .and_then(|sink| sink.get("callee"))
                        .and_then(Value::as_str)
                        .unwrap_or("-"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _ = write!(
            out,
            "\n\n| Location | Kind | Severity | Sink |\n|:---------|:-----|:---------|:-----|\n{rows}"
        );
        if findings.len() > 15 {
            let _ = write!(
                out,
                "\n\n> {} more candidates in the full report",
                findings.len() - 15,
            );
        }
    }
    out.push_str("\n\nTreat these as candidates for verification, not confirmed vulnerabilities.");
    out
}

// ---------------------------------------------------------------------------
// summary-fix.jq
// ---------------------------------------------------------------------------

fn fix_entries<'v>(env: &'v Value, entry_type: &str) -> Vec<&'v Value> {
    arr(env, "fixes")
        .filter(|fix| s(fix, "type") == entry_type)
        .collect()
}

fn fix_detail_block(label: &str, entries: &[&Value], row: impl Fn(&Value) -> String) -> String {
    let rows = entries
        .iter()
        .take(25)
        .map(|entry| row(entry))
        .collect::<Vec<_>>()
        .join("\n");
    let tail = if entries.len() > 25 {
        format!("\n- *... and {} more*", entries.len() - 25)
    } else {
        String::new()
    };
    format!("**{label} ({})**\n{rows}{tail}", entries.len())
}

/// Port of `summary-fix.jq`.
#[must_use]
pub fn render_fix_summary(env: &Value) -> String {
    let exports = fix_entries(env, "remove_export");
    let dependencies = fix_entries(env, "remove_dependency");
    let fix_attempts = arr(env, "fixes").filter(|fix| !b(fix, "skipped")).count();
    let content_changed = u(env, "skipped_content_changed") as usize;
    let mixed_eol = u(env, "skipped_mixed_line_endings") as usize;
    let low_confidence = u(env, "skipped_low_confidence_exports") as usize;
    let dry_run = b(env, "dry_run");

    if fix_attempts == 0 && content_changed == 0 && mixed_eol == 0 && low_confidence == 0 {
        return "## Fallow - Auto-fix\n\nNo fixable issues found.".to_owned();
    }

    let mut out = String::from("## Fallow - Auto-fix\n\n");
    out.push_str(if dry_run {
        "**Dry run**: would apply"
    } else {
        "Applied"
    });
    let _ = write!(out, " **{fix_attempts} fixes**");
    if !dry_run {
        let _ = write!(out, " ({} succeeded)", num(env, "total_fixed"));
    }
    if content_changed > 0 {
        let _ = write!(
            out,
            ", skipped {content_changed} file(s) that changed since analysis"
        );
    }
    if mixed_eol > 0 {
        let _ = write!(out, ", skipped {mixed_eol} file(s) with mixed line endings");
    }
    if low_confidence > 0 {
        let _ = write!(
            out,
            ", kept exports in {low_confidence} file(s) where consumers may be hidden from static analysis"
        );
    }
    out.push_str("\n\n| Type | Count |\n|------|-------|\n");
    if !exports.is_empty() {
        let _ = writeln!(out, "| Export removals | {} |", exports.len());
    }
    if !dependencies.is_empty() {
        let _ = writeln!(out, "| Dependency removals | {} |", dependencies.len());
    }
    out.push_str("\n<details>\n<summary>View details</summary>\n\n");
    if !exports.is_empty() {
        out.push_str(&fix_detail_block("Export removals", &exports, |it| {
            format!(
                "- `{}:{}` - `{}`",
                s(it, "path"),
                num(it, "line"),
                s(it, "name")
            )
        }));
        out.push_str("\n\n");
    }
    if !dependencies.is_empty() {
        out.push_str(&fix_detail_block(
            "Dependency removals",
            &dependencies,
            |it| {
                format!(
                    "- `{}` from {} in `{}`",
                    s(it, "package"),
                    s(it, "location"),
                    s(it, "file"),
                )
            },
        ));
        out.push('\n');
    }
    out.push_str("\n\n</details>");
    out
}

// ---------------------------------------------------------------------------
// summary-combined.jq
// ---------------------------------------------------------------------------

fn file_link(links: &LinkContext, path: &str, start: &str, end: &str) -> String {
    let display = last_three_segments(path);
    if links.repo.is_empty() || links.sha.is_empty() {
        format!("`{display}:{start}-{end}`")
    } else {
        format!(
            "[`{display}:{start}-{end}`](https://github.com/{}/blob/{}/{}{path}#L{start}-L{end})",
            links.repo, links.sha, links.prefix,
        )
    }
}

fn exceeded_priority(it: &Value) -> u8 {
    match s(it, "exceeded") {
        "all" => 5,
        "cyclomatic_crap" | "cognitive_crap" => 4,
        "crap" => 3,
        "both" => 2,
        "cyclomatic" | "cognitive" => 1,
        _ => 0,
    }
}

fn severity_priority(it: &Value) -> u8 {
    match s(it, "severity") {
        "critical" => 3,
        "high" => 2,
        "moderate" => 1,
        _ => 0,
    }
}

/// jq `ranked_health_findings`: stable ascending sort by the priority tuple,
/// then a full reverse (ties land in reverse input order).
fn ranked_health_findings(health: &Value) -> Vec<&Value> {
    let mut findings: Vec<&Value> = arr(health, "findings").collect();
    findings.sort_by_key(|it| {
        (
            exceeded_priority(it),
            severity_priority(it),
            it.get("crap").is_some_and(|crap| !crap.is_null()),
            u(it, "cyclomatic"),
            u(it, "cognitive"),
            u(it, "line_count"),
        )
    });
    findings.reverse();
    findings
}

const PROD_FAILING_VERDICTS: &[&str] = &["safe_to_delete", "review_required", "low_traffic"];

struct CombinedCounts {
    check: usize,
    dupes: usize,
    complex: usize,
    prod_failing: usize,
    prod_advisory: usize,
    hot_paths: usize,
}

impl CombinedCounts {
    fn health(&self) -> usize {
        self.complex + self.prod_failing
    }

    fn total(&self) -> usize {
        self.check + self.dupes + self.health()
    }
}

fn combined_counts(env: &Value) -> CombinedCounts {
    let check = env
        .get("check")
        .map_or(0, |check| u(check, "total_issues") as usize);
    let dupes = env
        .get("dupes")
        .map_or(0, |dupes| arr(dupes, "clone_groups").count());
    let health = env.get("health").cloned().unwrap_or(Value::Null);
    let complex = health.get("summary").map_or(0, |summary| {
        u(summary, "functions_above_threshold") as usize
    });
    let runtime = health
        .get("runtime_coverage")
        .cloned()
        .unwrap_or(Value::Null);
    let prod_failing = arr(&runtime, "findings")
        .filter(|finding| PROD_FAILING_VERDICTS.contains(&s(finding, "verdict")))
        .count();
    let prod_advisory = arr(&runtime, "findings")
        .filter(|finding| !PROD_FAILING_VERDICTS.contains(&s(finding, "verdict")))
        .count();
    let hot_paths = arr(&runtime, "hot_paths").count();
    CombinedCounts {
        check,
        dupes,
        complex,
        prod_failing,
        prod_advisory,
        hot_paths,
    }
}

fn hot_path_label(env: &Value, n: usize) -> String {
    let touched = env
        .get("health")
        .and_then(|health| health.get("runtime_coverage"))
        .is_some_and(|runtime| s(runtime, "verdict") == "hot-path-touched");
    let plural = if n == 1 { "" } else { "s" };
    if touched {
        format!("hot path{plural} touched")
    } else {
        format!("hot path{plural}")
    }
}

fn combined_zero_case(env: &Value, counts: &CombinedCounts) -> String {
    let vitals = env
        .get("health")
        .and_then(|health| health.get("vital_signs"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut out = String::from("# \u{1F33F} Fallow\n\n");
    if counts.prod_advisory > 0 || counts.hot_paths > 0 {
        out.push_str(
            "> [!NOTE]\n> **Quality gate passed**\n\n:white_check_mark: No code issues \u{b7} :white_check_mark: No duplication \u{b7} :white_check_mark: No blocking health findings",
        );
        if counts.prod_advisory > 0 {
            let _ = write!(
                out,
                " \u{b7} :information_source: **{}** runtime coverage advisory finding{}",
                counts.prod_advisory,
                if counts.prod_advisory == 1 { "" } else { "s" },
            );
        }
        if counts.hot_paths > 0 {
            let _ = write!(
                out,
                " \u{b7} :eyes: **{}** {}",
                counts.hot_paths,
                hot_path_label(env, counts.hot_paths),
            );
        }
    } else {
        out.push_str(
            "> [!NOTE]\n> **Quality gate passed**\n\n:white_check_mark: No code issues \u{b7} :white_check_mark: No duplication \u{b7} :white_check_mark: No complex functions",
        );
    }
    if let Some(maintainability) = opt_f(&vitals, "maintainability_avg") {
        let _ = write!(
            out,
            "\n\n| Metric | Value |\n|:-------|------:|\n| [Maintainability]({HEALTH_DOCS}#maintainability-index-mi) | **{}** / 100 |\n",
            pct(maintainability),
        );
    }
    out
}

fn combined_status_line(env: &Value, counts: &CombinedCounts) -> String {
    let mut out = String::new();
    if counts.check > 0 {
        let _ = write!(
            out,
            ":warning: **{}** code {}",
            counts.check,
            if counts.check == 1 { "issue" } else { "issues" },
        );
    } else {
        out.push_str(":white_check_mark: No code issues");
    }
    out.push_str(" \u{b7} ");
    if counts.dupes > 0 {
        let _ = write!(
            out,
            ":warning: **{}** clone {}",
            counts.dupes,
            if counts.dupes == 1 { "group" } else { "groups" },
        );
    } else {
        out.push_str(":white_check_mark: No duplication");
    }
    out.push_str(" \u{b7} ");
    let health = counts.health();
    if health > 0 {
        let _ = write!(
            out,
            ":warning: **{health}** health {}",
            if health == 1 { "finding" } else { "findings" },
        );
    } else {
        out.push_str(":white_check_mark: No blocking health findings");
    }
    if counts.prod_advisory > 0 {
        let _ = write!(
            out,
            " \u{b7} :information_source: **{}** coverage advisory finding{}",
            counts.prod_advisory,
            if counts.prod_advisory == 1 { "" } else { "s" },
        );
    }
    if counts.hot_paths > 0 {
        let _ = write!(
            out,
            " \u{b7} :eyes: **{}** {}",
            counts.hot_paths,
            hot_path_label(env, counts.hot_paths),
        );
    }
    out.push_str("\n\n");
    out
}

fn combined_check_breakdown(env: &Value, counts: &CombinedCounts) -> String {
    if counts.check == 0 {
        return String::new();
    }
    let check = env.get("check").cloned().unwrap_or(Value::Null);
    format!(
        "<details>\n<summary><strong><a href=\"{DEAD_CODE_DOCS}\">Code issues</a> ({})</strong></summary>\n\n| Category | Count |\n|:---------|------:|\n{}\n\n</details>\n\n",
        counts.check,
        dead_code_category_table(&check),
    )
}

fn combined_dupes_breakdown(env: &Value, counts: &CombinedCounts, links: &LinkContext) -> String {
    if counts.dupes == 0 {
        return String::new();
    }
    let dupes = env.get("dupes").cloned().unwrap_or(Value::Null);
    let stats = dupes.get("stats").cloned().unwrap_or(Value::Null);
    let groups = sorted_clone_groups(&dupes);
    let files_with_clones = u(&stats, "files_with_clones") as usize;
    let rows = groups
        .iter()
        .take(5)
        .map(|group| {
            let locations = arr(group, "instances")
                .map(|instance| {
                    file_link(
                        links,
                        s(instance, "file"),
                        &num(instance, "start_line"),
                        &num(instance, "end_line"),
                    )
                })
                .collect::<Vec<_>>()
                .join("<br>");
            format!(
                "| {locations} | {} | {} |",
                num(group, "line_count"),
                num(group, "token_count"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let tail = if counts.dupes > 5 {
        format!("\n\n*\u{2026} and {} more groups.*", counts.dupes - 5)
    } else {
        String::new()
    };
    format!(
        "<details>\n<summary><strong><a href=\"{DUPES_DOCS}\">Duplication</a> ({} {} \u{b7} {} lines \u{b7} {}%)</strong></summary>\n\n| Locations | Lines | Tokens |\n|:----------|------:|-------:|\n{rows}{tail}\n\nAcross {files_with_clones} {}.\n\n</details>\n\n",
        counts.dupes,
        if counts.dupes == 1 { "group" } else { "groups" },
        num(&stats, "duplicated_lines"),
        pct(f_or_zero(&stats, "duplication_percentage")),
        if files_with_clones == 1 {
            "file"
        } else {
            "files"
        },
    )
}

fn combined_complexity_breakdown(env: &Value, counts: &CombinedCounts) -> String {
    if counts.complex == 0 {
        return String::new();
    }
    let health = env.get("health").cloned().unwrap_or(Value::Null);
    let summary = health.get("summary").cloned().unwrap_or(Value::Null);
    let findings = ranked_health_findings(&health);
    let show_crap = summary
        .get("max_crap_threshold")
        .is_some_and(|threshold| !threshold.is_null())
        || findings
            .iter()
            .any(|finding| finding.get("crap").is_some_and(|crap| !crap.is_null()));
    let cyc_t = threshold_or(&summary, "max_cyclomatic_threshold", "default");
    let cog_t = threshold_or(&summary, "max_cognitive_threshold", "default");
    let crap_t = threshold_or(&summary, "max_crap_threshold", "default");
    let crap_header = if show_crap {
        format!(" | [CRAP]({HEALTH_DOCS}#crap-score)")
    } else {
        String::new()
    };
    let crap_separator = if show_crap { "|-----:" } else { "" };
    let rows = findings
        .iter()
        .take(5)
        .map(|it| {
            let crap_column = if show_crap {
                format!(" | {}", crap_cell(it))
            } else {
                String::new()
            };
            format!(
                "| `{}:{}` | `{}` | {} | {}{} | {}{}{crap_column} | {} |",
                last_three_segments(s(it, "path")),
                num(it, "line"),
                s(it, "name"),
                str_or(it, "severity", "moderate"),
                num(it, "cyclomatic"),
                exceeded_marker(it, &["cyclomatic", "both", "all"]),
                num(it, "cognitive"),
                exceeded_marker(it, &["cognitive", "both", "all"]),
                num(it, "line_count"),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let crap_footer = if show_crap {
        format!(", CRAP >= {crap_t}")
    } else {
        String::new()
    };
    format!(
        "<details>\n<summary><strong><a href=\"{HEALTH_DOCS}#complexity-metrics\">Complexity</a> ({} {} above threshold)</strong></summary>\n\n| File | Function | Severity | [Cyclomatic]({HEALTH_DOCS}#cyclomatic-complexity) | [Cognitive]({HEALTH_DOCS}#cognitive-complexity){crap_header} | Lines |\n|:-----|:---------|:---------|----------:|---------:{crap_separator}|------:|\n{rows}\n\n**{}** files, **{}** functions analyzed (thresholds: cyclomatic > {cyc_t}, cognitive > {cog_t}{crap_footer})\n\n</details>\n\n",
        counts.complex,
        if counts.complex == 1 {
            "function"
        } else {
            "functions"
        },
        threshold_or(&summary, "files_analyzed", "unknown"),
        threshold_or(&summary, "functions_analyzed", "unknown"),
    )
}

fn combined_runtime_breakdown(env: &Value, counts: &CombinedCounts) -> String {
    let prod_total = counts.prod_failing + counts.prod_advisory;
    if prod_total == 0 && counts.hot_paths == 0 {
        return String::new();
    }
    let runtime = env
        .get("health")
        .and_then(|health| health.get("runtime_coverage"))
        .cloned()
        .unwrap_or(Value::Null);
    let hot_suffix = if counts.hot_paths > 0 {
        format!(
            ", {} {}",
            counts.hot_paths,
            hot_path_label(env, counts.hot_paths)
        )
    } else {
        String::new()
    };
    let mut out = format!(
        "<details>\n<summary><strong><a href=\"{HEALTH_DOCS}#runtime-coverage\">Runtime coverage</a> ({prod_total} finding{}{hot_suffix})</strong></summary>\n\n",
        if prod_total == 1 { "" } else { "s" },
    );
    if prod_total > 0 {
        out.push_str("| File | Function | Verdict | Invocations | Confidence |\n|:-----|:---------|:--------|------------:|:-----------|\n");
        out.push_str(
            &arr(&runtime, "findings")
                .take(5)
                .map(|it| {
                    let invocations = it
                        .get("invocations")
                        .filter(|value| !value.is_null())
                        .map_or_else(|| "-".to_owned(), fmt_num);
                    format!(
                        "| `{}:{}` | `{}` | `{}` | {invocations} | {} |",
                        last_three_segments(s(it, "path")),
                        num(it, "line"),
                        s(it, "function"),
                        s(it, "verdict"),
                        s(it, "confidence"),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        if counts.hot_paths > 0 {
            out.push_str("\n\n");
        }
    }
    if counts.hot_paths > 0 {
        out.push_str("| File | Function | Invocations | Percentile |\n|:-----|:---------|------------:|-----------:|\n");
        out.push_str(
            &arr(&runtime, "hot_paths")
                .take(5)
                .map(|it| {
                    format!(
                        "| `{}:{}` | `{}` | {} | {} |",
                        last_three_segments(s(it, "path")),
                        num(it, "line"),
                        s(it, "function"),
                        num(it, "invocations"),
                        num(it, "percentile"),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        );
        out.push_str("\n\n");
    }
    out.push_str("</details>\n\n");
    out
}

fn combined_vitals(env: &Value) -> String {
    let health = env.get("health").cloned().unwrap_or(Value::Null);
    let vitals = health.get("vital_signs").cloned().unwrap_or(Value::Null);
    let has_vitals = vitals.as_object().is_some_and(|vitals| !vitals.is_empty());
    if !has_vitals {
        return String::new();
    }
    let scores: Vec<f64> = arr(&health, "file_scores")
        .filter_map(|score| opt_f(score, "maintainability_index"))
        .collect();
    let scoped_maintainability = if scores.is_empty() {
        None
    } else {
        let avg = scores.iter().sum::<f64>() / scores.len() as f64;
        Some((avg * 10.0).round() / 10.0)
    };
    let mut out = format!(
        "#### [Codebase health]({HEALTH_DOCS})\n\n| Metric | Value |\n|:-------|------:|\n"
    );
    let maintainability = opt_f(&vitals, "maintainability_avg");
    if let Some(avg) = maintainability {
        let _ = writeln!(
            out,
            "| [Maintainability]({HEALTH_DOCS}#maintainability-index-mi) | **{}** / 100 |",
            pct(avg),
        );
    }
    if let Some(scoped) = scoped_maintainability {
        let rounded_avg = (maintainability.unwrap_or_default() * 10.0).round() / 10.0;
        if (scoped - rounded_avg).abs() > f64::EPSILON {
            let _ = writeln!(
                out,
                "| [Maintainability]({HEALTH_DOCS}#maintainability-index-mi) (changed files) | **{}** / 100 |",
                fmt_num(&serde_json::json!(scoped)),
            );
        }
    }
    if let Some(avg_cyclomatic) = opt_f(&vitals, "avg_cyclomatic") {
        let _ = writeln!(
            out,
            "| [Avg complexity]({HEALTH_DOCS}#cyclomatic-complexity) | {} |",
            pct(avg_cyclomatic),
        );
    }
    out.push('\n');
    out
}

fn combined_tips(env: &Value) -> String {
    let check = env.get("check").cloned().unwrap_or(Value::Null);
    let fixable = arr(&check, "unused_exports").count()
        + arr(&check, "unused_dependencies").count()
        + arr(&check, "unused_enum_members").count();
    if fixable == 0 {
        return String::new();
    }
    let mut out = String::from("> [!TIP]\n> Run `fallow fix --dry-run` to preview auto-fixes.\n");
    if arr(&check, "unused_exports").count() > 0 {
        let _ = writeln!(
            out,
            "> Add [`/** @public */`]({SUPPRESSION_DOCS}) above exports to preserve them."
        );
    }
    out
}

/// Port of `summary-combined.jq`.
#[must_use]
pub fn render_combined_summary(env: &Value, links: &LinkContext) -> String {
    let counts = combined_counts(env);
    let header = health_score_header(&env.get("health").cloned().unwrap_or(Value::Null));
    if counts.total() == 0 {
        return format!("{header}{}", combined_zero_case(env, &counts));
    }
    let pointer = if counts.check > 0 || counts.dupes > 0 || counts.health() > 0 {
        "See inline review comments for per-finding details.\n\n"
    } else {
        ""
    };
    format!(
        "{header}# \u{1F33F} Fallow\n\n> [!WARNING]\n> **Review needed**\n\n{}{pointer}{}{}{}{}{}{}",
        combined_status_line(env, &counts),
        combined_check_breakdown(env, &counts),
        combined_dupes_breakdown(env, &counts, links),
        combined_complexity_breakdown(env, &counts),
        combined_runtime_breakdown(env, &counts),
        combined_vitals(env),
        combined_tips(env),
    )
}
