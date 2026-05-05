use std::sync::LazyLock;

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_types::extract::{ExportInfo, ExportName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::results::{
    DuplicateExport, DuplicateLocation, ExportUsage, PrivateTypeLeak, ReferenceLocation,
    StaleSuppression, SuppressionOrigin, UnusedExport,
};
use crate::suppress::{IssueKind, SuppressionContext};

use super::{LineOffsetsMap, byte_offset_to_line_col, read_source};

/// Pre-compiled glob matchers for config ignore_exports rules.
type IgnoreMatchers<'a> = Vec<(globset::GlobMatcher, &'a [String])>;

/// Pre-compiled glob matchers for plugin/framework used_exports rules.
type PluginMatchers<'a> = Vec<CompiledUsedExportRule<'a>>;

/// Compile config ignore_exports rules into glob matchers.
fn compile_ignore_matchers(config: &ResolvedConfig) -> IgnoreMatchers<'_> {
    config
        .ignore_export_rules
        .iter()
        .filter_map(|rule| match globset::Glob::new(&rule.file) {
            Ok(g) => Some((g.compile_matcher(), rule.exports.as_slice())),
            Err(e) => {
                tracing::warn!("invalid ignoreExports pattern '{}': {e}", rule.file);
                None
            }
        })
        .collect()
}

/// Compile plugin-discovered used_exports rules (includes framework preset rules).
fn compile_plugin_matchers(
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
) -> PluginMatchers<'_> {
    let Some(pr) = plugin_result else {
        return Vec::new();
    };
    pr.used_exports
        .iter()
        .filter_map(compile_used_export_rule)
        .collect()
}

struct CompiledUsedExportRule<'a> {
    path: crate::plugins::CompiledPathRule,
    exports: Vec<&'a str>,
}

impl CompiledUsedExportRule<'_> {
    fn matches(&self, path: &str) -> bool {
        self.path.matches(path)
    }
}

fn compile_used_export_rule(
    rule: &crate::plugins::PluginUsedExportRule,
) -> Option<CompiledUsedExportRule<'_>> {
    Some(CompiledUsedExportRule {
        path: crate::plugins::CompiledPathRule::for_used_export_rule(
            &rule.rule.path,
            "used_exports pattern",
        )?,
        exports: rule.rule.exports.iter().map(String::as_str).collect(),
    })
}

/// Check whether a module should be skipped for unused-export analysis.
///
/// Skips entry points that do not have framework/plugin `used_exports` handling,
/// CJS-only modules, Svelte files (whose `export let` declarations are component
/// props), and fully-unreachable modules where every export has zero references
/// (those are already caught by `find_unused_files`). Unreachable modules with
/// *some* referenced exports are NOT skipped — their individually unused exports
/// would otherwise slip through both detectors.
fn should_skip_module(
    module: &ModuleNode,
    has_plugin_used_exports: bool,
    include_entry_exports: bool,
) -> bool {
    if module.is_entry_point() && !has_plugin_used_exports && !include_entry_exports {
        return true;
    }
    if !module.is_reachable() {
        // Completely unreachable with no references at all → caught by find_unused_files
        return module.exports.iter().all(|e| e.references.is_empty());
    }
    // CJS modules with module.exports but no named exports: hard to track individually
    if module.has_cjs_exports() && module.exports.is_empty() {
        return true;
    }
    // Svelte `export let`/`export const` are component props consumed by the runtime;
    // unreachable Svelte files are still caught by `find_unused_files`.
    module.path.extension().is_some_and(|ext| ext == "svelte")
}

/// Check whether an export name is covered by config ignore rules or plugin/framework rules.
fn is_export_ignored(
    export_name: &str,
    matching_ignore: &[&[String]],
    matching_plugin: &[&[&str]],
) -> bool {
    matching_ignore
        .iter()
        .any(|exports| exports.iter().any(|e| e == "*" || e == export_name))
        || matching_plugin
            .iter()
            .any(|exports| exports.contains(&"*") || exports.contains(&export_name))
}

fn local_export_binding_name(export: &ExportInfo) -> Option<&str> {
    export.local_name.as_deref().or(match &export.name {
        ExportName::Named(name) => Some(name.as_str()),
        ExportName::Default => None,
    })
}

fn is_js_like_source(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext,
                "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts"
            )
        })
}

fn collect_exports_used_in_file(module: &ModuleInfo, path: &std::path::Path) -> FxHashSet<String> {
    if module.exports.is_empty() || !is_js_like_source(path) {
        return FxHashSet::default();
    }

    let source = read_source(path);
    if source.is_empty() {
        return FxHashSet::default();
    }

    let source_type = oxc_span::SourceType::from_path(path).unwrap_or_default();
    let allocator = oxc_allocator::Allocator::default();
    let parser_return = oxc_parser::Parser::new(&allocator, &source, source_type).parse();
    let semantic_ret = oxc_semantic::SemanticBuilder::new().build(&parser_return.program);
    let semantic = semantic_ret.semantic;
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let root_scope = scoping.root_scope_id();

    let mut used = FxHashSet::default();
    for export in &module.exports {
        let Some(local_name) = local_export_binding_name(export) else {
            continue;
        };
        let name = oxc_str::Ident::from(local_name);
        let Some(symbol_id) = scoping.get_binding(root_scope, name) else {
            continue;
        };
        let has_real_use = scoping
            .get_resolved_references(symbol_id)
            .any(|reference| !is_inside_export_specifier(nodes, reference.node_id()));
        if has_real_use {
            used.insert(export.name.to_string());
        }
    }

    used
}

/// Walk ancestors of `node_id` and return `true` if the reference originates
/// from inside an `export { foo }` / `export { foo as bar }` specifier or an
/// `export default foo` declaration. Those identifiers are the export site
/// itself, not a same-file *use*, so they must not satisfy
/// `ignoreExportsUsedInFile` (Knip parity).
fn is_inside_export_specifier(
    nodes: &oxc_semantic::AstNodes<'_>,
    node_id: oxc_syntax::node::NodeId,
) -> bool {
    nodes
        .ancestor_kinds(node_id)
        .any(|kind| matches!(kind, oxc_ast::AstKind::ExportSpecifier(_)))
        || matches!(
            nodes.parent_kind(node_id),
            oxc_ast::AstKind::ExportDefaultDeclaration(_)
        )
}

/// Find exports that are never imported by other files.
pub fn find_unused_exports(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> (Vec<UnusedExport>, Vec<UnusedExport>, Vec<StaleSuppression>) {
    let mut unused_exports = Vec::new();
    let mut unused_types = Vec::new();
    let mut stale_expected_unused = Vec::new();

    let ignore_matchers = compile_ignore_matchers(config);
    let plugin_matchers = compile_plugin_matchers(plugin_result);
    let module_info_by_id: Option<FxHashMap<FileId, &ModuleInfo>> =
        if config.ignore_exports_used_in_file.is_enabled() {
            Some(modules.iter().map(|m| (m.file_id, m)).collect())
        } else {
            None
        };

    // Precompute reachable FileIds so we can distinguish meaningful references
    // (from reachable modules) from unreachable-to-unreachable references.
    let reachable_files: FxHashSet<u32> = graph
        .modules
        .iter()
        .filter(|m| m.is_reachable())
        .map(|m| m.file_id.0)
        .collect();

    let module_results: Vec<(Vec<UnusedExport>, Vec<UnusedExport>, Vec<StaleSuppression>)> = graph
        .modules
        .par_iter()
        .map(|module| {
            let mut unused_exports = Vec::new();
            let mut unused_types = Vec::new();
            let mut stale_expected_unused = Vec::new();

            // Compute relative path once per module for glob matching
            let relative_path = module
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&module.path);
            let file_str = relative_path.to_string_lossy();

            // Collect ignore/plugin matchers that apply to this file
            let matching_ignore: Vec<&[String]> = ignore_matchers
                .iter()
                .filter(|(m, _)| m.is_match(file_str.as_ref()))
                .map(|(_, exports)| *exports)
                .collect();

            let matching_plugin: Vec<&[&str]> = plugin_matchers
                .iter()
                .filter(|rule| rule.matches(file_str.as_ref()))
                .map(|rule| rule.exports.as_slice())
                .collect();

            if should_skip_module(
                module,
                !matching_plugin.is_empty(),
                config.include_entry_exports,
            ) {
                return (unused_exports, unused_types, stale_expected_unused);
            }

            let same_file_used_exports = if let Some(module_info_by_id) = &module_info_by_id {
                module_info_by_id
                    .get(&module.file_id)
                    .map_or_else(FxHashSet::default, |info| {
                        collect_exports_used_in_file(info, &module.path)
                    })
            } else {
                FxHashSet::default()
            };

            // Namespace imports are now handled with member-access narrowing in graph.rs:
            // only specific accessed members get references populated. No blanket skip needed.

            // Pre-compute the set of re-exported names for O(1) is_re_export lookups
            // inside the export loop. Barrel files synthesize one ExportSymbol per
            // ReExportEdge, so the naive iter().any() check would be O(N²).
            let re_export_names: FxHashSet<&str> = module
                .re_exports
                .iter()
                .map(|re| re.exported_name.as_str())
                .collect();

            for export in &module.exports {
                // For unreachable modules, only references from reachable files count —
                // references from other unreachable modules don't save an export.
                let has_cross_file_ref = if module.is_reachable() {
                    !export.references.is_empty()
                } else {
                    export
                        .references
                        .iter()
                        .any(|r| reachable_files.contains(&r.from_file.0))
                };
                // Treat side-effect-registered exports (Lit @customElement,
                // customElements.define) as referenced even when no other file
                // imports them by name. The class is alive at runtime via the
                // registration call inside its own module.
                let is_referenced =
                    has_cross_file_ref || (module.is_reachable() && export.is_side_effect_used);
                // Handle @expected-unused: if the export IS used (has references from
                // reachable modules), report as stale. If it's NOT used, suppress it
                // silently (the tag is working as intended). Note: re-exports through
                // barrel files DO count as references here, since the reference list
                // is already filtered to reachable modules above.
                if matches!(
                    export.visibility,
                    fallow_types::extract::VisibilityTag::ExpectedUnused
                ) {
                    if is_referenced {
                        let (line, col) = byte_offset_to_line_col(
                            line_offsets_by_file,
                            module.file_id,
                            export.span.start,
                        );
                        stale_expected_unused.push(StaleSuppression {
                            path: module.path.clone(),
                            line,
                            col,
                            origin: SuppressionOrigin::JsdocTag {
                                export_name: export.name.to_string(),
                            },
                        });
                    }
                    continue;
                }

                // Other visibility tags (@public, @internal, @alpha, @beta) permanently suppress
                if export.visibility.suppresses_unused() || is_referenced {
                    continue;
                }

                let export_str = export.name.to_string();

                if config
                    .ignore_exports_used_in_file
                    .suppresses(export.is_type_only)
                    && same_file_used_exports.contains(export_str.as_str())
                {
                    continue;
                }

                if is_export_ignored(&export_str, &matching_ignore, &matching_plugin) {
                    continue;
                }

                let (line, col) = byte_offset_to_line_col(
                    line_offsets_by_file,
                    module.file_id,
                    export.span.start,
                );

                // Detect re-exports semantically by looking up the export name in the
                // module's re_exports set, rather than relying on a span sentinel.
                // This catches both synthesized re-exports (which still use Span::default()
                // for narrowing/star cases) and real re-exports (which carry the visitor's
                // span for accurate line-number reporting).
                let is_re_export = re_export_names.contains(export_str.as_str());

                // Check inline suppression
                let issue_kind = if export.is_type_only {
                    IssueKind::UnusedType
                } else {
                    IssueKind::UnusedExport
                };
                if suppressions.is_suppressed(module.file_id, line, issue_kind) {
                    continue;
                }

                let unused = UnusedExport {
                    path: module.path.clone(),
                    export_name: export_str,
                    is_type_only: export.is_type_only,
                    line,
                    col,
                    span_start: export.span.start,
                    is_re_export,
                };

                if export.is_type_only {
                    unused_types.push(unused);
                } else {
                    unused_exports.push(unused);
                }
            }

            (unused_exports, unused_types, stale_expected_unused)
        })
        .collect();

    for (exports, types, stale_expected) in module_results {
        unused_exports.extend(exports);
        unused_types.extend(types);
        stale_expected_unused.extend(stale_expected);
    }

    (unused_exports, unused_types, stale_expected_unused)
}

/// Remove exported type findings when the type is only exported to support
/// another public signature in the same module.
pub fn suppress_signature_backing_types(
    unused_types: &mut Vec<UnusedExport>,
    graph: &ModuleGraph,
    modules: &[fallow_types::extract::ModuleInfo],
) {
    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();
    let backing_types: FxHashSet<(std::path::PathBuf, String)> = modules
        .iter()
        .filter_map(|module| path_by_id.get(&module.file_id).map(|path| (module, *path)))
        .flat_map(|(module, path)| {
            module
                .public_signature_type_references
                .iter()
                .map(move |reference| (path.to_path_buf(), reference.type_name.clone()))
        })
        .collect();

    unused_types.retain(|unused| {
        !backing_types.contains(&(unused.path.clone(), unused.export_name.clone()))
    });
}

/// File-name suffixes that idiomatically declare local helper types
/// (`type Story = StoryObj<typeof Component>`) used by virtually every export.
/// Skipping these in private-type-leak detection keeps Storybook codebases
/// from drowning in true-but-unhelpful findings.
const STORYBOOK_SUFFIXES: &[&str] = &[
    ".stories.ts",
    ".stories.tsx",
    ".stories.js",
    ".stories.jsx",
    ".stories.mts",
    ".stories.cts",
    ".stories.mjs",
    ".stories.cjs",
    ".story.ts",
    ".story.tsx",
    ".story.js",
    ".story.jsx",
    ".story.mts",
    ".story.cts",
    ".story.mjs",
    ".story.cjs",
];

fn is_storybook_file(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            STORYBOOK_SUFFIXES
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

/// File-path globs for framework routing conventions where authors share a
/// per-file private type across multiple exports (e.g., `Page` + `generateMetadata`
/// both annotated `Props`). Skipping these mirrors the Storybook treatment:
/// the type IS technically leaked, but the framework convention forces it,
/// so flagging every route file produces noise without actionable findings.
///
/// Patterns match against the project-relative path. Covers Next.js App
/// Router and Pages Router (with `src/` variants), Remix, TanStack Router,
/// Gatsby, and Expo Router special files. Vue and Svelte SFCs are silently
/// below the rule's reach already (the visitor doesn't surface their
/// `defineProps<T>()` macros as TS type references), so SvelteKit and Nuxt
/// do not need entries here.
const ROUTE_CONVENTION_PATTERNS: &[&str] = &[
    // Next.js App Router (and `src/app` variant). Patterns prefixed with `**/`
    // so they match in monorepo subpackages too (e.g. `packages/web/src/app/...`).
    "**/app/**/page.{ts,tsx,js,jsx}",
    "**/app/**/layout.{ts,tsx,js,jsx}",
    "**/app/**/template.{ts,tsx,js,jsx}",
    "**/app/**/loading.{ts,tsx,js,jsx}",
    "**/app/**/error.{ts,tsx,js,jsx}",
    "**/app/**/not-found.{ts,tsx,js,jsx}",
    "**/app/**/route.{ts,tsx,js,jsx}",
    "**/app/**/default.{ts,tsx,js,jsx}",
    "**/app/**/global-error.{ts,tsx,js,jsx}",
    "**/app/**/forbidden.{ts,tsx,js,jsx}",
    "**/app/**/unauthorized.{ts,tsx,js,jsx}",
    "**/app/global-not-found.{ts,tsx,js,jsx}",
    // Next.js App Router top-level convention files (no nested segments).
    "**/app/page.{ts,tsx,js,jsx}",
    "**/app/layout.{ts,tsx,js,jsx}",
    "**/app/template.{ts,tsx,js,jsx}",
    "**/app/loading.{ts,tsx,js,jsx}",
    "**/app/error.{ts,tsx,js,jsx}",
    "**/app/not-found.{ts,tsx,js,jsx}",
    "**/app/route.{ts,tsx,js,jsx}",
    "**/app/default.{ts,tsx,js,jsx}",
    "**/app/global-error.{ts,tsx,js,jsx}",
    // Next.js App Router metadata files.
    "**/app/**/opengraph-image.{ts,tsx,js,jsx}",
    "**/app/**/twitter-image.{ts,tsx,js,jsx}",
    "**/app/**/icon.{ts,tsx,js,jsx}",
    "**/app/**/apple-icon.{ts,tsx,js,jsx}",
    "**/app/**/manifest.{ts,tsx,js,jsx}",
    "**/app/**/sitemap.{ts,tsx,js,jsx}",
    "**/app/**/robots.{ts,tsx,js,jsx}",
    // Next.js Pages Router (every file is a route by definition). The `**/`
    // prefix lets monorepo subpackages match (e.g., `apps/web/pages/about.tsx`),
    // which over-skips any non-framework directory named `pages/`
    // (e.g., `components/pages/Home.tsx`). Acceptable tradeoff: the leak rule's
    // false-positive rate on Next.js routes (~62 per real project) is the
    // dominant signal; component directories named `pages/` are uncommon and
    // their leaks are still caught by other rules.
    "**/pages/**/*.{ts,tsx,js,jsx}",
    // Gatsby templates (Gatsby uses `src/templates/`; pages are already covered
    // by `**/pages/`). Scoped under `src/` so generic `templates/` directories
    // (email, code-gen, Handlebars partials) keep `private-type-leak` coverage.
    "**/src/templates/**/*.{ts,tsx,js,jsx}",
    // Remix v2 flat routes and folder-route entries. Subtrees under
    // `app/routes/<segment>/` are NOT skipped wholesale: Remix users commonly
    // co-locate non-route helpers there where private type leaks are still
    // actionable.
    "**/routes/*.{ts,tsx,js,jsx}",
    "**/routes/**/route.{ts,tsx,js,jsx}",
    "**/routes/**/_layout.{ts,tsx,js,jsx}",
    "**/routes/**/_index.{ts,tsx,js,jsx}",
    "**/routes/**/index.{ts,tsx,js,jsx}",
    // TanStack Router root file.
    "**/routes/**/__root.{ts,tsx,js,jsx}",
    // Expo Router special files. Regular Expo route files reuse Next.js
    // convention names (`page`, `layout`) and are already covered above.
    "**/app/**/_layout.{ts,tsx,js,jsx}",
    "**/app/**/+*.{ts,tsx,js,jsx}",
];

fn build_route_convention_globset() -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in ROUTE_CONVENTION_PATTERNS {
        // `literal_separator(true)` matches the project-wide convention so a
        // single `*` segment cannot cross `/`. Without this,
        // `**/routes/*.{ts,tsx,...}` would also match `app/routes/utils/format.ts`,
        // breaking the "subtrees not skipped wholesale" guarantee for Remix
        // co-located helpers.
        let glob = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .expect("static route-convention glob pattern must be valid");
        builder.add(glob);
    }
    builder
        .build()
        .expect("static route-convention globset must build")
}

fn is_route_convention_file(absolute_path: &std::path::Path, root: &std::path::Path) -> bool {
    let relative = absolute_path.strip_prefix(root).unwrap_or(absolute_path);
    // Normalize separators so the forward-slash patterns match on Windows where
    // `Path::strip_prefix` preserves backslashes.
    let normalized = relative.to_string_lossy().replace('\\', "/");
    ROUTE_CONVENTION_GLOBSET.is_match(normalized.as_str())
}

static ROUTE_CONVENTION_GLOBSET: LazyLock<globset::GlobSet> =
    LazyLock::new(build_route_convention_globset);

/// Find exported signatures that reference same-file type declarations that
/// are not exported by that same name.
pub fn find_private_type_leaks(
    graph: &ModuleGraph,
    modules: &[fallow_types::extract::ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<PrivateTypeLeak> {
    let module_by_id: FxHashMap<FileId, &ModuleNode> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    let mut leaks = Vec::new();
    for module_info in modules {
        if module_info.public_signature_type_references.is_empty()
            || module_info.local_type_declarations.is_empty()
        {
            continue;
        }
        let Some(module) = module_by_id.get(&module_info.file_id) else {
            continue;
        };
        if is_storybook_file(&module.path) || is_route_convention_file(&module.path, &config.root) {
            continue;
        }
        let local_types: FxHashSet<&str> = module_info
            .local_type_declarations
            .iter()
            .map(|decl| decl.name.as_str())
            .collect();
        let exported_names: FxHashSet<String> = module
            .exports
            .iter()
            .map(|export| export.name.to_string())
            .collect();

        let mut seen: FxHashSet<(String, String)> = FxHashSet::default();
        for reference in &module_info.public_signature_type_references {
            if !local_types.contains(reference.type_name.as_str())
                || exported_names.contains(&reference.type_name)
            {
                continue;
            }
            if !seen.insert((reference.export_name.clone(), reference.type_name.clone())) {
                continue;
            }
            let (line, col) = byte_offset_to_line_col(
                line_offsets_by_file,
                module_info.file_id,
                reference.span.start,
            );
            if suppressions.is_suppressed(module_info.file_id, line, IssueKind::PrivateTypeLeak) {
                continue;
            }
            leaks.push(PrivateTypeLeak {
                path: module.path.clone(),
                export_name: reference.export_name.clone(),
                type_name: reference.type_name.clone(),
                line,
                col,
                span_start: reference.span.start,
            });
        }
    }

    leaks
}

/// Add dynamic-import edges that act as re-exports to the existing
/// `re_export_sources` map. Caller has already populated it from static
/// `re_exports`. A dynamic import counts as a re-export only when the wrapper
/// module also exports the same name, mirroring the static `export { X } from`
/// shape.
fn collect_dynamic_reexport_sources(
    resolved_modules: &[crate::resolve::ResolvedModule],
    graph: &ModuleGraph,
    module_idx_by_file_id: &FxHashMap<FileId, usize>,
    re_export_sources: &mut FxHashMap<usize, FxHashSet<usize>>,
) {
    use crate::extract::ExportName;
    use fallow_types::extract::ImportedName;

    for resolved in resolved_modules {
        let Some(&wrapper_idx) = module_idx_by_file_id.get(&resolved.file_id) else {
            continue;
        };
        let wrapper_exports = &graph.modules[wrapper_idx].exports;

        for dynamic_import in &resolved.resolved_dynamic_imports {
            let crate::resolve::ResolveResult::InternalModule(source_file_id) =
                &dynamic_import.target
            else {
                continue;
            };

            // Only count as a re-export when the wrapper exports the same shape.
            let matches_export = match &dynamic_import.info.imported_name {
                ImportedName::Named(name) => wrapper_exports
                    .iter()
                    .any(|e| matches!(&e.name, ExportName::Named(n) if n == name)),
                ImportedName::Default => wrapper_exports
                    .iter()
                    .any(|e| matches!(&e.name, ExportName::Default)),
                ImportedName::Namespace | ImportedName::SideEffect => false,
            };
            if !matches_export {
                continue;
            }

            let Some(&source_idx) = module_idx_by_file_id.get(source_file_id) else {
                continue;
            };
            re_export_sources
                .entry(wrapper_idx)
                .or_default()
                .insert(source_idx);
        }
    }
}

/// Find exports that appear with the same name in multiple files (potential duplicates).
///
/// Barrel re-exports (files that only re-export from other modules via `export { X } from './source'`)
/// are excluded — having an index.ts re-export the same name as the source module is the normal
/// barrel file pattern, not a true duplicate.
///
/// `resolved_modules` is the set of modules with their resolved dynamic imports.
/// Pass `&[]` to opt out of dynamic-import re-export detection (existing static
/// `export { X } from '...'` re-exports are still recognized).
pub fn find_duplicate_exports(
    graph: &ModuleGraph,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    resolved_modules: &[crate::resolve::ResolvedModule],
) -> Vec<DuplicateExport> {
    // Build a map from FileId to module index for dynamic re-export source lookup.
    let module_idx_by_file_id: FxHashMap<FileId, usize> = graph
        .modules
        .iter()
        .enumerate()
        .map(|(idx, m)| (m.file_id, idx))
        .collect();

    // Build a set of re-export relationships: (re-exporting module idx) -> set of (source module idx)
    let mut re_export_sources: FxHashMap<usize, FxHashSet<usize>> = FxHashMap::default();
    for (idx, module) in graph.modules.iter().enumerate() {
        for re in &module.re_exports {
            re_export_sources
                .entry(idx)
                .or_default()
                .insert(re.source_file.0 as usize);
        }
    }

    // Extend re_export_sources with dynamic imports that act as re-exports.
    //
    // The Next.js `dynamic(() => import('./Foo').then(m => m.Foo))` idiom is
    // semantically equivalent to `export { Foo } from './Foo'`. We treat module
    // A as dynamically re-exporting from module S when A has a resolved dynamic
    // import targeting `InternalModule(S)` AND A also exports the name being
    // imported. The export-side check guards against false negatives where a
    // module dynamically imports something but does not actually re-export it.
    collect_dynamic_reexport_sources(
        resolved_modules,
        graph,
        &module_idx_by_file_id,
        &mut re_export_sources,
    );

    struct ExportEntry {
        module_idx: usize,
        path: std::path::PathBuf,
        file_id: FileId,
        span_start: u32,
        is_type_only: bool,
    }

    let mut export_locations: FxHashMap<String, Vec<ExportEntry>> = FxHashMap::default();
    let file_id_by_path: FxHashMap<&std::path::Path, FileId> = graph
        .modules
        .iter()
        .map(|module| (module.path.as_path(), module.file_id))
        .collect();

    for (idx, module) in graph.modules.iter().enumerate() {
        if !module.is_reachable() || module.is_entry_point() {
            continue;
        }

        // Skip files with file-wide duplicate-export suppression
        if suppressions.is_file_suppressed(module.file_id, IssueKind::DuplicateExport) {
            continue;
        }

        for export in &module.exports {
            if matches!(export.name, crate::extract::ExportName::Default) {
                continue; // Skip default exports
            }
            // Skip synthetic re-export entries (span 0..0) — these are generated by
            // graph construction for re-exports, not real local declarations
            if export.span.start == 0 && export.span.end == 0 {
                continue;
            }
            let name = export.name.to_string();
            export_locations.entry(name).or_default().push(ExportEntry {
                module_idx: idx,
                path: module.path.clone(),
                file_id: module.file_id,
                span_start: export.span.start,
                is_type_only: export.is_type_only,
            });
        }
    }

    // Filter: only keep truly independent duplicates (not re-export chains)
    // Sort by export name for deterministic output order
    let mut sorted_locations: Vec<_> = export_locations.into_iter().collect();
    sorted_locations.sort_by(|a, b| a.0.cmp(&b.0));

    sorted_locations
        .into_iter()
        .filter_map(|(name, locations)| {
            if locations.len() <= 1 {
                return None;
            }

            // TypeScript declaration merging: a value export (`export const X`) and
            // a type export (`export type X`) sharing the same name are distinct in
            // TS's value/type namespace split. This is idiomatic with Zod, Prisma,
            // class+interface merging, etc. Skip groups that mix value and type exports.
            let has_value = locations.iter().any(|e| !e.is_type_only);
            let has_type = locations.iter().any(|e| e.is_type_only);
            if has_value && has_type {
                // Deduplicate within each namespace: keep only value-only or type-only
                // entries and check if either namespace alone has duplicates.
                let value_modules: FxHashSet<usize> = locations
                    .iter()
                    .filter(|e| !e.is_type_only)
                    .map(|e| e.module_idx)
                    .collect();
                let type_modules: FxHashSet<usize> = locations
                    .iter()
                    .filter(|e| e.is_type_only)
                    .map(|e| e.module_idx)
                    .collect();
                // If neither namespace alone has cross-file duplicates, skip entirely
                if value_modules.len() <= 1 && type_modules.len() <= 1 {
                    return None;
                }
            }

            // Remove entries where one module re-exports from another in the set.
            // For each pair (A, B), if A re-exports from B or B re-exports from A,
            // they are part of the same export chain, not true duplicates.
            let module_indices: FxHashSet<usize> = locations.iter().map(|e| e.module_idx).collect();
            let independent: Vec<DuplicateLocation> = locations
                .into_iter()
                .filter(|e| {
                    let sources = re_export_sources.get(&e.module_idx);
                    let has_source_in_set =
                        sources.is_some_and(|s| s.iter().any(|src| module_indices.contains(src)));
                    !has_source_in_set
                })
                .map(|e| {
                    let (line, col) =
                        byte_offset_to_line_col(line_offsets_by_file, e.file_id, e.span_start);
                    DuplicateLocation {
                        path: e.path,
                        line,
                        col,
                    }
                })
                .collect();

            if independent.len() <= 1 {
                return None;
            }

            // Filter: only report duplicates where at least two files share a common
            // importer in the module graph. Unrelated leaf files (e.g., SvelteKit route
            // modules in different directories) that happen to export the same name
            // are not actionable duplicates since they can never be confused at an
            // import site.
            let has_shared_importer = has_common_importer(&independent, graph, &file_id_by_path);
            if has_shared_importer {
                Some(DuplicateExport {
                    export_name: name,
                    locations: independent,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Check if any two files in the duplicate set share a common importer.
///
/// Two files "share a common importer" if there exists a third file that imports
/// from both. This filters out false positives from unrelated leaf modules (e.g.,
/// SvelteKit route files in different directories) that coincidentally export the
/// same name but are never imported together.
fn has_common_importer(
    locations: &[DuplicateLocation],
    graph: &ModuleGraph,
    file_id_by_path: &FxHashMap<&std::path::Path, FileId>,
) -> bool {
    if locations.len() <= 1 {
        return false;
    }

    // Collect FileIds for the duplicate locations through a pre-built path index.
    let file_ids: Vec<FileId> = locations
        .iter()
        .filter_map(|loc| file_id_by_path.get(loc.path.as_path()).copied())
        .collect();

    if file_ids.len() <= 1 {
        return false;
    }

    let duplicate_files: FxHashSet<FileId> = file_ids.iter().copied().collect();
    let mut importer_owner: FxHashMap<FileId, FileId> = FxHashMap::default();

    for file_id in file_ids {
        let idx = file_id.0 as usize;
        if idx >= graph.reverse_deps.len() {
            continue;
        }

        for &importer in &graph.reverse_deps[idx] {
            // One duplicate file importing another is also actionable.
            if duplicate_files.contains(&importer) {
                return true;
            }
            if let Some(previous_file) = importer_owner.insert(importer, file_id)
                && previous_file != file_id
            {
                return true;
            }
        }
    }

    false
}

/// Collect usage counts for all exports in the module graph.
///
/// Iterates every module and every export, producing an `ExportUsage` entry with the
/// reference count and reference locations. This data is used by the LSP server to show
/// Code Lens annotations (e.g., "3 references") above export declarations, with
/// click-to-navigate support via `editor.action.showReferences`.
pub fn collect_export_usages(
    graph: &ModuleGraph,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<ExportUsage> {
    let mut usages = Vec::new();

    // Build FileId -> path index for resolving reference locations
    let file_paths: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|m| (m.file_id, m.path.as_path()))
        .collect();

    // Fallback source + line-offset cache for reference locations not in the line offsets map.
    // Only populated when a referencing file's line offsets are unavailable.
    // Caches both source and computed offsets to avoid redundant recomputation.
    let mut source_cache: FxHashMap<FileId, (String, Vec<u32>)> = FxHashMap::default();

    for module in &graph.modules {
        // Skip unreachable modules — no point showing Code Lens for files
        // that aren't reachable from any entry point
        if !module.is_reachable() {
            continue;
        }

        for export in &module.exports {
            // Skip synthetic re-export entries (span 0..0) — these are generated
            // by graph construction, not real local declarations in the source
            if export.span.start == 0 && export.span.end == 0 {
                continue;
            }

            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, export.span.start);

            // Resolve reference locations for Code Lens navigation
            let reference_locations: Vec<ReferenceLocation> = export
                .references
                .iter()
                .filter_map(|r| {
                    // Skip references with no span (e.g. from dynamic import patterns)
                    if r.import_span.start == 0 && r.import_span.end == 0 {
                        return None;
                    }
                    let ref_path = file_paths.get(&r.from_file)?;
                    // Use pre-computed line offsets when available, fall back to disk read
                    let (ref_line, ref_col) = if line_offsets_by_file.contains_key(&r.from_file) {
                        byte_offset_to_line_col(
                            line_offsets_by_file,
                            r.from_file,
                            r.import_span.start,
                        )
                    } else {
                        let (_, offsets) = source_cache.entry(r.from_file).or_insert_with(|| {
                            let src = read_source(ref_path);
                            let ofs = fallow_types::extract::compute_line_offsets(&src);
                            (src, ofs)
                        });
                        fallow_types::extract::byte_offset_to_line_col(offsets, r.import_span.start)
                    };
                    Some(ReferenceLocation {
                        path: ref_path.to_path_buf(),
                        line: ref_line,
                        col: ref_col,
                    })
                })
                .collect();

            usages.push(ExportUsage {
                path: module.path.clone(),
                export_name: export.name.to_string(),
                line,
                col,
                reference_count: export.references.len(),
                reference_locations,
            });
        }
    }

    usages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use crate::extract::{ExportName, VisibilityTag};
    use crate::graph::{ExportSymbol, ModuleGraph, ReExportEdge, SymbolReference};
    use crate::resolve::ResolvedModule;
    use crate::suppress::Suppression;
    use oxc_span::Span;
    use std::path::PathBuf;

    /// Build a minimal ModuleGraph via the build() constructor.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_graph(file_specs: &[(&str, bool)]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = file_specs
            .iter()
            .enumerate()
            .map(|(i, (path, _))| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(path),
                size_bytes: 0,
            })
            .collect();

        let entry_points: Vec<EntryPoint> = file_specs
            .iter()
            .filter(|(_, is_entry)| *is_entry)
            .map(|(path, _)| EntryPoint {
                path: PathBuf::from(path),
                source: EntryPointSource::ManualEntry,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                exports: vec![],
                re_exports: vec![],
                resolved_imports: vec![],
                resolved_dynamic_imports: vec![],
                resolved_dynamic_patterns: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                unused_import_bindings: FxHashSet::default(),
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
            })
            .collect();

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    /// Build a default ResolvedConfig for tests.
    fn test_config() -> ResolvedConfig {
        fallow_config::FallowConfig::default().resolve(
            PathBuf::from("/tmp/test"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
        )
    }

    fn make_export(name: &str, span_start: u32, span_end: u32) -> ExportSymbol {
        ExportSymbol {
            name: ExportName::Named(name.to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(span_start, span_end),
            references: vec![],
            members: vec![],
        }
    }

    fn make_referenced_export(
        name: &str,
        span_start: u32,
        span_end: u32,
        from: u32,
    ) -> ExportSymbol {
        ExportSymbol {
            name: ExportName::Named(name.to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(span_start, span_end),
            references: vec![SymbolReference {
                from_file: FileId(from),
                kind: crate::graph::ReferenceKind::NamedImport,
                import_span: Span::new(0, 10),
            }],
            members: vec![],
        }
    }

    // ---- find_duplicate_exports tests ----

    #[test]
    fn duplicate_exports_empty_graph() {
        let graph = build_graph(&[]);
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_no_duplicates_single_module() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/utils.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20), make_export("bar", 30, 40)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_detects_same_name_in_two_modules() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        // entry.ts imports both a.ts and b.ts — they share a common importer
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].export_name, "helper");
        assert_eq!(result[0].locations.len(), 2);
    }

    #[test]
    fn duplicate_exports_skips_default_exports() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(10, 20),
            references: vec![],
            members: vec![],
        }];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(10, 20),
            references: vec![],
            members: vec![],
        }];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_skips_synthetic_re_export_entries() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 0, 0)]; // synthetic
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)]; // real
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_skips_unreachable_modules() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        // Module 2 stays unreachable
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_skips_entry_points() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/b.ts", false)]);
        graph.modules[0].exports = vec![make_export("helper", 10, 20)];
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_filters_re_export_chains() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/index.ts", false),
            ("/src/helper.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(2),
            imported_name: "helper".to_string(),
            exported_name: "helper".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 5, 15)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_suppressed_file_wide() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];

        let supp = vec![Suppression {
            line: 0,
            comment_line: 1,
            kind: Some(IssueKind::DuplicateExport),
        }];
        let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
        supp_map.insert(FileId(2), &supp);
        let suppressions = SuppressionContext::from_map(supp_map);

        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_three_modules_same_name() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
            ("/src/c.ts", false),
        ]);
        for i in 1..=3 {
            graph.modules[i].set_reachable(true);
            graph.modules[i].exports = vec![make_export("sharedFn", 10, 20)];
        }
        // entry.ts imports all three — they share a common importer
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];
        graph.reverse_deps[3] = vec![FileId(0)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].export_name, "sharedFn");
        assert_eq!(result[0].locations.len(), 3);
    }

    #[test]
    fn duplicate_exports_unrelated_leaf_files_not_flagged() {
        // Two route files exporting the same name but with no common importer
        // (e.g., SvelteKit routes in different directories)
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/routes/foo/page.ts", false),
            ("/src/routes/bar/page.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("Area", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("Area", 10, 20)];
        // No shared importer: each is imported by a different parent
        // (or not imported at all — just reachable via framework routing)
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(
            result.is_empty(),
            "unrelated leaf files should not be flagged as duplicates"
        );
    }

    #[test]
    fn duplicate_exports_direct_import_still_flagged() {
        // Two files where one imports the other — they are connected
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        // a.ts imports b.ts directly
        graph.reverse_deps[2] = vec![FileId(1)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert_eq!(
            result.len(),
            1,
            "directly connected files should still be flagged"
        );
    }

    #[test]
    fn duplicate_exports_different_names_not_duplicated() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("bar", 10, 20)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn duplicate_exports_value_type_merging_not_flagged() {
        // `export const Status = z.enum([...])` + `export type Status = z.infer<typeof Status>`
        // in the same file is TypeScript declaration merging, not a duplicate.
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/schema.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export("Status", 10, 20),      // value export
            make_type_export("Status", 50, 60), // type export
        ];
        graph.reverse_deps[1] = vec![FileId(0)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(
            result.is_empty(),
            "value+type merging should not be flagged as duplicate"
        );
    }

    #[test]
    fn duplicate_exports_value_type_cross_file_not_flagged() {
        // File A exports `const Status` and file B exports `type Status`.
        // These are in different TS namespaces and should not be flagged.
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("Status", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_type_export("Status", 10, 20)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert!(
            result.is_empty(),
            "cross-file value+type should not be flagged"
        );
    }

    #[test]
    fn duplicate_exports_same_namespace_still_flagged() {
        // Two files both export `const helper` (both value exports) — this IS a duplicate.
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(&graph, &suppressions, &FxHashMap::default(), &[]);
        assert_eq!(
            result.len(),
            1,
            "same-namespace duplicates should still be flagged"
        );
    }

    // ---- Next.js dynamic() re-export tests ----

    /// Build a `ResolvedModule` shell for use in dynamic-import test fixtures.
    /// All side-channel fields default to empty.
    fn make_resolved_module(file_id: u32, path: &str) -> crate::resolve::ResolvedModule {
        crate::resolve::ResolvedModule {
            file_id: FileId(file_id),
            path: std::path::PathBuf::from(path),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
        }
    }

    /// Build a resolved dynamic import `target_id` of the given import shape.
    fn dynamic_import_to(
        target_id: u32,
        imported_name: crate::extract::ImportedName,
    ) -> crate::resolve::ResolvedImport {
        crate::resolve::ResolvedImport {
            info: fallow_types::extract::ImportInfo {
                source: "./target".to_string(),
                imported_name,
                local_name: "_local".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 1),
                source_span: oxc_span::Span::default(),
            },
            target: crate::resolve::ResolveResult::InternalModule(FileId(target_id)),
        }
    }

    #[test]
    fn dynamic_import_then_member_not_flagged_as_duplicate() {
        // Foo-lazy.tsx exports `Foo` via `dynamic(() => import('./Foo').then(m => m.Foo))`.
        // The lazy wrapper is semantically a re-export — must NOT be flagged.
        use crate::extract::ImportedName;

        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/Foo.tsx", false),
            ("/src/Foo-lazy.tsx", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("Foo", 10, 30)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("Foo", 10, 30)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];

        let mut foo_lazy = make_resolved_module(2, "/src/Foo-lazy.tsx");
        foo_lazy.resolved_dynamic_imports =
            vec![dynamic_import_to(1, ImportedName::Named("Foo".to_string()))];

        let resolved_modules = vec![make_resolved_module(1, "/src/Foo.tsx"), foo_lazy];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert!(
            result.is_empty(),
            "dynamic(import().then(m=>m.Foo)) wrapper must not be flagged as duplicate-export"
        );
    }

    #[test]
    fn dynamic_import_without_then_default_not_flagged_as_duplicate() {
        // `dynamic(() => import('./Foo'))` (default-import variant).
        // Both modules have a default export so the wrapper is a real re-export.
        use crate::extract::{ExportName, ImportedName, VisibilityTag};
        use crate::graph::ExportSymbol;

        let make_default_export = || ExportSymbol {
            name: ExportName::Default,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: oxc_span::Span::new(0, 10),
            references: vec![],
            members: vec![],
        };

        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/Foo.tsx", false),
            ("/src/Foo-lazy.tsx", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_default_export()];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_default_export()];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];

        let mut foo_lazy = make_resolved_module(2, "/src/Foo-lazy.tsx");
        foo_lazy.resolved_dynamic_imports = vec![dynamic_import_to(1, ImportedName::Default)];

        let resolved_modules = vec![make_resolved_module(1, "/src/Foo.tsx"), foo_lazy];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert!(
            result.is_empty(),
            "dynamic(import('./Foo')) wrapper with matching default export must not be flagged"
        );
    }

    #[test]
    fn dynamic_import_named_without_matching_export_still_flagged() {
        // Wrapper has a Named dynamic import of "Foo" but does NOT export "Foo".
        // It exports "Bar". The duplicate-export detector should still flag the
        // unrelated "Foo" duplication between the source and a third module.
        use crate::extract::ImportedName;

        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/Foo.tsx", false),
            ("/src/other-foo.tsx", false),
            ("/src/wrapper.tsx", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("Foo", 10, 30)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("Foo", 10, 30)];
        graph.modules[3].set_reachable(true);
        graph.modules[3].exports = vec![make_export("Bar", 10, 30)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];
        graph.reverse_deps[3] = vec![FileId(0)];

        // Wrapper dynamically imports "Foo" from Foo.tsx but exports only "Bar".
        let mut wrapper = make_resolved_module(3, "/src/wrapper.tsx");
        wrapper.resolved_dynamic_imports =
            vec![dynamic_import_to(1, ImportedName::Named("Foo".to_string()))];

        let resolved_modules = vec![
            make_resolved_module(1, "/src/Foo.tsx"),
            make_resolved_module(2, "/src/other-foo.tsx"),
            wrapper,
        ];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert_eq!(
            result.len(),
            1,
            "Foo duplication between unrelated modules must be flagged"
        );
        assert_eq!(result[0].export_name, "Foo");
    }

    #[test]
    fn dynamic_import_default_without_default_export_still_flagged() {
        // Wrapper has a Default dynamic import targeting source, but does not
        // have a Default export of its own — only a Named "helper". The
        // dynamic import is for some other purpose, not re-export. Both modules
        // exporting "helper" must still be flagged as a duplicate.
        use crate::extract::ImportedName;

        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/source.ts", false),
            ("/src/wrapper.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];

        // Wrapper dynamically imports source.ts as Default, but exports only
        // a Named "helper" — the dynamic import is not re-exporting anything.
        let mut wrapper = make_resolved_module(2, "/src/wrapper.ts");
        wrapper.resolved_dynamic_imports = vec![dynamic_import_to(1, ImportedName::Default)];

        let resolved_modules = vec![make_resolved_module(1, "/src/source.ts"), wrapper];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert_eq!(
            result.len(),
            1,
            "Default dynamic import without matching Default export must not suppress duplicate-export"
        );
        assert_eq!(result[0].export_name, "helper");
    }

    #[test]
    fn duplicate_without_dynamic_link_still_flagged() {
        // Two modules both export "helper" with no dynamic-import link between
        // them. Pre-existing duplicate-export behaviour must hold even when
        // resolved_modules is supplied.
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/a.ts", false),
            ("/src/b.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("helper", 10, 20)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];

        let resolved_modules = vec![
            make_resolved_module(1, "/src/a.ts"),
            make_resolved_module(2, "/src/b.ts"),
        ];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].export_name, "helper");
    }

    #[test]
    fn dynamic_import_named_mismatched_with_wrapper_export_still_flagged() {
        // Wrapper(2) exports `Foo` AND dynamically imports `Bar` from source(1).
        // Source(1) also exports `Foo`. The wrapper's dynamic import name `Bar`
        // does NOT match its own export `Foo`, so the matches_export check must
        // reject the edge: otherwise the (source, wrapper) `Foo` duplicate
        // would be silently suppressed.
        //
        // Regression-strength for the Named branch of matches_export: removing
        // the matches_export check makes this test fail.
        use crate::extract::ImportedName;

        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/source.ts", false),
            ("/src/wrapper.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("Foo", 10, 30)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("Foo", 10, 30)];
        graph.reverse_deps[1] = vec![FileId(0)];
        graph.reverse_deps[2] = vec![FileId(0)];

        let mut wrapper = make_resolved_module(2, "/src/wrapper.ts");
        wrapper.resolved_dynamic_imports =
            vec![dynamic_import_to(1, ImportedName::Named("Bar".to_string()))];

        let resolved_modules = vec![make_resolved_module(1, "/src/source.ts"), wrapper];
        let suppressions = SuppressionContext::empty();
        let result = find_duplicate_exports(
            &graph,
            &suppressions,
            &FxHashMap::default(),
            &resolved_modules,
        );
        assert_eq!(
            result.len(),
            1,
            "Named dynamic import whose name does not match the wrapper's own export must not suppress the duplicate"
        );
        assert_eq!(result[0].export_name, "Foo");
    }

    // ---- find_unused_exports tests (exercises compile_ignore_matchers, compile_plugin_matchers,
    //       should_skip_module, is_export_ignored) ----

    /// Helper: build a config with ignore_exports rules.
    fn test_config_with_ignore_exports(
        rules: Vec<fallow_config::IgnoreExportRule>,
    ) -> ResolvedConfig {
        fallow_config::FallowConfig {
            ignore_exports: rules,
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            ..Default::default()
        }
        .resolve(
            PathBuf::from("/tmp/test"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
        )
    }

    /// Helper: build a minimal AggregatedPluginResult with used_exports.
    fn make_plugin_result(
        used_exports: Vec<(String, Vec<String>)>,
    ) -> crate::plugins::AggregatedPluginResult {
        crate::plugins::AggregatedPluginResult {
            entry_patterns: vec![],
            config_patterns: vec![],
            always_used: vec![],
            used_exports: used_exports
                .into_iter()
                .map(|(pattern, exports)| {
                    crate::plugins::PluginUsedExportRule::new(
                        "test-plugin",
                        crate::plugins::UsedExportRule::new(pattern, exports),
                    )
                })
                .collect(),
            used_class_members: vec![],
            scss_include_paths: vec![],
            entry_point_roles: FxHashMap::default(),
            referenced_dependencies: vec![],
            discovered_always_used: vec![],
            setup_files: vec![],
            tooling_dependencies: vec![],
            script_used_packages: FxHashSet::default(),
            virtual_module_prefixes: vec![],
            virtual_package_suffixes: vec![],
            generated_import_patterns: vec![],
            path_aliases: vec![],
            active_plugins: vec![],
            fixture_patterns: vec![],
        }
    }

    fn make_type_export(name: &str, span_start: u32, span_end: u32) -> ExportSymbol {
        ExportSymbol {
            name: ExportName::Named(name.to_string()),
            is_type_only: true,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(span_start, span_end),
            references: vec![],
            members: vec![],
        }
    }

    // -- find_unused_exports: basic behavior --

    #[test]
    fn unused_exports_empty_graph() {
        let graph = build_graph(&[]);
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_detects_unreferenced_export() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "helper");
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_skips_referenced_export() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_referenced_export("helper", 10, 20, 0)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_skips_public_export() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("publicFn".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Public,
            span: Span::new(10, 20),
            references: vec![],
            members: vec![],
        }];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_separates_types_from_values() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export("valueFn", 10, 20),
            make_type_export("MyType", 30, 40),
        ];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "valueFn");
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].export_name, "MyType");
    }

    // -- should_skip_module: unreachable --

    #[test]
    fn unused_exports_skips_unreachable_module() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/dead.ts", false),
        ]);
        // Module stays unreachable (default)
        graph.modules[1].exports = vec![make_export("orphan", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    // -- should_skip_module: entry point --

    #[test]
    fn unused_exports_skips_entry_point() {
        let mut graph = build_graph(&[("/tmp/test/src/entry.ts", true)]);
        graph.modules[0].exports = vec![make_export("main", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_reports_non_framework_exports_in_entry_point_with_plugin_rules() {
        let mut graph = build_graph(&[("/tmp/test/src/app/page.tsx", true)]);
        graph.modules[0].set_reachable(true);
        graph.modules[0].exports = vec![
            make_export("default", 10, 20),
            make_export("generateMetadata", 30, 40),
            make_export("helper", 50, 60),
        ];

        let plugin = make_plugin_result(vec![(
            "src/app/**/page.{ts,tsx,js,jsx}".to_string(),
            vec!["default".to_string(), "generateMetadata".to_string()],
        )]);
        let config = test_config();
        let suppressions = SuppressionContext::empty();

        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            Some(&plugin),
            &suppressions,
            &FxHashMap::default(),
        );

        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "helper");
        assert!(types.is_empty());
    }

    // -- should_skip_module: CJS-only --

    #[test]
    fn unused_exports_skips_cjs_only_module() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/legacy.js", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].set_cjs_exports(true);
        // No named exports, only module.exports
        graph.modules[1].exports = vec![];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_does_not_skip_cjs_module_with_named_exports() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/mixed.js", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].set_cjs_exports(true);
        graph.modules[1].exports = vec![make_export("namedFn", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "namedFn");
    }

    // -- should_skip_module: Svelte files --

    #[test]
    fn unused_exports_skips_svelte_files() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/Component.svelte", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("count", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(exports.is_empty());
        assert!(types.is_empty());
    }

    // -- should_skip_module: module passes all checks --

    #[test]
    fn unused_exports_reports_reachable_non_entry_non_cjs_non_svelte() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].set_cjs_exports(false);
        graph.modules[1].exports = vec![make_export("helper", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "helper");
    }

    // -- compile_ignore_matchers: empty config --

    #[test]
    fn unused_exports_empty_ignore_config() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];
        let config = test_config(); // no ignore_exports rules
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports.len(),
            1,
            "no ignore rules, export should be reported"
        );
    }

    // -- compile_ignore_matchers: multiple patterns --

    #[test]
    fn unused_exports_ignore_multiple_patterns() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/types.ts", false),
            ("/tmp/test/src/constants.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("MyType", 10, 20)];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export("MY_CONST", 10, 20)];

        let config = test_config_with_ignore_exports(vec![
            fallow_config::IgnoreExportRule {
                file: "src/types.ts".to_string(),
                exports: vec!["*".to_string()],
            },
            fallow_config::IgnoreExportRule {
                file: "src/constants.ts".to_string(),
                exports: vec!["MY_CONST".to_string()],
            },
        ]);
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "both exports should be ignored by config rules"
        );
    }

    // -- compile_ignore_matchers: invalid glob handled gracefully --

    #[test]
    fn unused_exports_invalid_ignore_glob_skipped() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];

        // Invalid glob pattern with unclosed bracket
        let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
            file: "[invalid".to_string(),
            exports: vec!["*".to_string()],
        }]);
        let suppressions = SuppressionContext::empty();
        // Should not panic — invalid globs are silently skipped
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports.len(),
            1,
            "invalid glob should be skipped, export still reported"
        );
    }

    // -- is_export_ignored: config wildcard match --

    #[test]
    fn unused_exports_ignore_wildcard_matches_all() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("TypeA", 10, 20), make_export("TypeB", 30, 40)];

        let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
            file: "src/types.ts".to_string(),
            exports: vec!["*".to_string()],
        }]);
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "wildcard * should ignore all exports in matching file"
        );
    }

    // -- is_export_ignored: config specific name match --

    #[test]
    fn unused_exports_ignore_specific_name_only() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export("ignored", 10, 20),
            make_export("reported", 30, 40),
        ];

        let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
            file: "src/utils.ts".to_string(),
            exports: vec!["ignored".to_string()],
        }]);
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "reported");
    }

    // -- is_export_ignored: no match --

    #[test]
    fn unused_exports_ignore_rule_wrong_file_no_effect() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];

        let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
            file: "src/other.ts".to_string(),
            exports: vec!["*".to_string()],
        }]);
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports.len(),
            1,
            "ignore rule for different file should not suppress"
        );
    }

    // -- compile_plugin_matchers: no plugin result --

    #[test]
    fn unused_exports_no_plugin_result() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports.len(),
            1,
            "None plugin_result means no plugin matchers"
        );
    }

    // -- compile_plugin_matchers: plugin with empty used_exports --

    #[test]
    fn unused_exports_plugin_no_used_exports() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let pr = make_plugin_result(vec![]);
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            Some(&pr),
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports.len(),
            1,
            "plugin with no used_exports should not suppress"
        );
    }

    // -- compile_plugin_matchers / is_export_ignored: plugin used_exports match --

    #[test]
    fn unused_exports_plugin_used_exports_suppresses() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/pages/index.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export("getStaticProps", 10, 20),
            make_export("unusedHelper", 30, 40),
        ];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let pr = make_plugin_result(vec![(
            "src/pages/**".to_string(),
            vec!["getStaticProps".to_string()],
        )]);
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            Some(&pr),
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].export_name, "unusedHelper");
    }

    // -- is_export_ignored: matching both config and plugin --

    #[test]
    fn unused_exports_both_config_and_plugin_ignore() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/api/handler.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("handler", 10, 20)];

        let config = test_config_with_ignore_exports(vec![fallow_config::IgnoreExportRule {
            file: "src/api/*.ts".to_string(),
            exports: vec!["handler".to_string()],
        }]);
        let suppressions = SuppressionContext::empty();
        let pr = make_plugin_result(vec![(
            "src/api/**".to_string(),
            vec!["handler".to_string()],
        )]);
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            Some(&pr),
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "export matching both config and plugin should be ignored"
        );
    }

    // -- compile_plugin_matchers: invalid plugin glob handled gracefully --

    #[test]
    fn unused_exports_invalid_plugin_glob_skipped() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("foo", 10, 20)];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let pr = make_plugin_result(vec![("[invalid".to_string(), vec!["foo".to_string()])]);
        // Should not panic
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            Some(&pr),
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1, "invalid plugin glob should be skipped");
    }

    // -- find_unused_exports: re-export semantic detection --

    #[test]
    fn unused_exports_marks_re_export_semantically() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/barrel.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export("reexported", 100, 120)];
        // The export must have a matching ReExportEdge for the unused-export
        // detector to classify it as a re-export. This mirrors how the graph
        // builder synthesizes ExportSymbol entries from ReExportInfo.
        graph.modules[1].re_exports = vec![ReExportEdge {
            source_file: FileId(0),
            imported_name: "reexported".to_string(),
            exported_name: "reexported".to_string(),
            is_type_only: false,
            span: oxc_span::Span::default(),
        }];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(exports.len(), 1);
        assert!(
            exports[0].is_re_export,
            "export with matching ReExportEdge should be flagged as re-export"
        );
        // span_start carries the original byte offset (100), not the (0,0) sentinel
        // — confirms that the re-export reporting uses the visitor's real span.
        assert_eq!(exports[0].span_start, 100);
    }

    // ---- collect_export_usages tests ----

    #[test]
    fn collect_usages_empty_graph() {
        let graph = build_graph(&[]);
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert!(result.is_empty());
    }

    #[test]
    fn collect_usages_skips_unreachable_modules() {
        let mut graph = build_graph(&[("/src/dead.ts", false)]);
        graph.modules[0].exports = vec![make_export("unused", 10, 20)];
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert!(result.is_empty());
    }

    #[test]
    fn collect_usages_skips_synthetic_exports() {
        let mut graph = build_graph(&[("/src/barrel.ts", true)]);
        graph.modules[0].exports = vec![make_export("reexported", 0, 0)];
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert!(result.is_empty());
    }

    #[test]
    fn collect_usages_counts_references() {
        let mut graph = build_graph(&[("/src/utils.ts", true), ("/src/app.ts", false)]);
        graph.modules[0].exports = vec![make_referenced_export("helper", 10, 20, 1)];
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].export_name, "helper");
        assert_eq!(result[0].reference_count, 1);
    }

    #[test]
    fn collect_usages_zero_references_still_reported() {
        let mut graph = build_graph(&[("/src/utils.ts", true)]);
        graph.modules[0].exports = vec![make_export("unused", 10, 20)];
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].export_name, "unused");
        assert_eq!(result[0].reference_count, 0);
        assert!(result[0].reference_locations.is_empty());
    }

    #[test]
    fn collect_usages_multiple_exports_same_module() {
        let mut graph = build_graph(&[("/src/utils.ts", true)]);
        graph.modules[0].exports = vec![make_export("alpha", 10, 20), make_export("beta", 30, 40)];
        let result = collect_export_usages(&graph, &FxHashMap::default());
        assert_eq!(result.len(), 2);
        let names: FxHashSet<&str> = result.iter().map(|u| u.export_name.as_str()).collect();
        assert!(names.contains("alpha"));
        assert!(names.contains("beta"));
    }

    // -- unreachable module with mixed references (blindspot fix) --

    #[test]
    fn unused_exports_checks_unreachable_module_with_mixed_references() {
        // Unreachable module with 2 exports:
        // - "usedByUnreachable" referenced by another unreachable module (should still be flagged)
        // - "totallyUnused" referenced by nobody (should be flagged)
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/helpers.ts", false),
            ("/tmp/test/src/setup.ts", false),
        ]);
        // helpers.ts is unreachable, has one export referenced by setup.ts (also unreachable)
        graph.modules[1].exports = vec![
            ExportSymbol {
                name: ExportName::Named("usedByUnreachable".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: Span::new(10, 30),
                references: vec![SymbolReference {
                    from_file: FileId(2), // setup.ts — also unreachable
                    kind: crate::graph::ReferenceKind::NamedImport,
                    import_span: Span::new(0, 10),
                }],
                members: vec![],
            },
            make_export("totallyUnused", 40, 55),
        ];
        // setup.ts is also unreachable
        graph.modules[2].exports = vec![];

        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        // Both exports should be flagged: the unreachable-to-unreachable reference doesn't count
        let names: FxHashSet<&str> = exports.iter().map(|e| e.export_name.as_str()).collect();
        assert!(
            names.contains("usedByUnreachable"),
            "reference from unreachable module should not save an export"
        );
        assert!(
            names.contains("totallyUnused"),
            "completely unreferenced export should be flagged"
        );
        assert_eq!(exports.len(), 2);
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_skips_export_referenced_by_reachable() {
        // Unreachable module with 1 export referenced by a REACHABLE module.
        // The export should NOT be flagged as unused.
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/helpers.ts", false),
        ]);
        // helpers.ts is unreachable but has an export referenced by entry.ts (reachable)
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("usedByReachable".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(10, 28),
            references: vec![SymbolReference {
                from_file: FileId(0), // entry.ts — reachable (entry point)
                kind: crate::graph::ReferenceKind::NamedImport,
                import_span: Span::new(0, 10),
            }],
            members: vec![],
        }];

        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "export referenced by reachable module should not be flagged"
        );
        assert!(types.is_empty());
    }

    // -- VisibilityTag suppression --

    #[test]
    fn unused_exports_skips_internal_visibility() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("internalHelper".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Internal,
            span: Span::new(10, 30),
            references: vec![],
            members: vec![],
        }];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "@internal export should not be flagged as unused"
        );
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_skips_beta_visibility() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("betaFeature".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Beta,
            span: Span::new(10, 30),
            references: vec![],
            members: vec![],
        }];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "@beta export should not be flagged as unused"
        );
        assert!(types.is_empty());
    }

    #[test]
    fn unused_exports_skips_alpha_visibility() {
        let mut graph = build_graph(&[
            ("/tmp/test/src/entry.ts", true),
            ("/tmp/test/src/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![ExportSymbol {
            name: ExportName::Named("alphaFeature".to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::Alpha,
            span: Span::new(10, 30),
            references: vec![],
            members: vec![],
        }];
        let config = test_config();
        let suppressions = SuppressionContext::empty();
        let (exports, types, _stale) = find_unused_exports(
            &graph,
            &[],
            &config,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports.is_empty(),
            "@alpha export should not be flagged as unused"
        );
        assert!(types.is_empty());
    }

    // -- include_entry_exports --

    #[test]
    fn unused_exports_include_entry_exports_flag() {
        // With include_entry_exports = false (default), entry point exports are skipped
        let mut graph = build_graph(&[("/tmp/test/src/entry.ts", true)]);
        graph.modules[0].exports = vec![make_export("main", 10, 20)];

        let config_off = test_config();
        assert!(!config_off.include_entry_exports);
        let suppressions = SuppressionContext::empty();
        let (exports_off, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config_off,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert!(
            exports_off.is_empty(),
            "entry export should be skipped when include_entry_exports is false"
        );

        // With include_entry_exports = true, entry point exports ARE checked
        let mut config_on = test_config();
        config_on.include_entry_exports = true;
        let (exports_on, _, _stale) = find_unused_exports(
            &graph,
            &[],
            &config_on,
            None,
            &suppressions,
            &FxHashMap::default(),
        );
        assert_eq!(
            exports_on.len(),
            1,
            "entry export should be flagged when include_entry_exports is true"
        );
        assert_eq!(exports_on[0].export_name, "main");
    }
}
