mod boundary;
pub mod feature_flags;
mod package_json_utils;
mod predicates;
mod unused_catalog;
mod unused_deps;
mod unused_exports;
mod unused_files;
mod unused_members;
mod unused_overrides;

use rustc_hash::FxHashMap;

use fallow_config::{PackageJson, ResolvedConfig, Severity};

use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::{AnalysisResults, CircularDependency};
use crate::suppress::IssueKind;

use unused_catalog::{
    find_unresolved_catalog_references, find_unused_catalog_entries, gather_pnpm_catalog_state,
};
use unused_deps::{
    find_test_only_dependencies, find_type_only_dependencies, find_unlisted_dependencies,
    find_unresolved_imports, find_unused_dependencies,
};
use unused_exports::{
    collect_export_usages, find_duplicate_exports, find_private_type_leaks, find_unused_exports,
    suppress_signature_backing_types,
};
use unused_files::find_unused_files;
use unused_members::find_unused_members;
use unused_overrides::{
    find_misconfigured_dependency_overrides, find_unused_dependency_overrides,
    gather_pnpm_override_state,
};

/// Pre-computed line offset tables indexed by `FileId`, built during parse and
/// carried through the cache. Eliminates redundant file reads during analysis.
pub type LineOffsetsMap<'a> = FxHashMap<FileId, &'a [u32]>;

/// Convert a byte offset to (line, col) using pre-computed line offsets.
/// Falls back to `(1, byte_offset)` when no line table is available.
pub fn byte_offset_to_line_col(
    line_offsets_map: &LineOffsetsMap<'_>,
    file_id: FileId,
    byte_offset: u32,
) -> (u32, u32) {
    line_offsets_map
        .get(&file_id)
        .map_or((1, byte_offset), |offsets| {
            fallow_types::extract::byte_offset_to_line_col(offsets, byte_offset)
        })
}

fn cycle_edge_line_col(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    cycle: &[FileId],
    edge_index: usize,
) -> Option<(u32, u32)> {
    if cycle.is_empty() {
        return None;
    }

    let from = cycle[edge_index];
    let to = cycle[(edge_index + 1) % cycle.len()];
    graph
        .find_import_span_start(from, to)
        .map(|span_start| byte_offset_to_line_col(line_offsets_map, from, span_start))
}

fn is_circular_dependency_suppressed(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    cycle: &[FileId],
) -> bool {
    if cycle
        .iter()
        .any(|&id| suppressions.is_file_suppressed(id, IssueKind::CircularDependency))
    {
        return true;
    }

    let mut line_suppressed = false;
    for edge_index in 0..cycle.len() {
        let from = cycle[edge_index];
        if let Some((line, _)) = cycle_edge_line_col(graph, line_offsets_map, cycle, edge_index)
            && suppressions.is_suppressed(from, line, IssueKind::CircularDependency)
        {
            line_suppressed = true;
        }
    }
    line_suppressed
}

/// Read source content from disk, returning empty string on failure.
/// Only used for LSP Code Lens reference resolution where the referencing
/// file may not be in the line offsets map.
fn read_source(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Check whether any two files in a cycle belong to different workspace packages.
/// Uses longest-prefix-match to assign each file to a workspace root.
/// Files outside all workspace roots (e.g., root-level shared code) are ignored —
/// only cycles between two distinct named workspaces are flagged.
fn is_cross_package_cycle(
    files: &[std::path::PathBuf],
    workspaces: &[fallow_config::WorkspaceInfo],
) -> bool {
    let find_workspace = |path: &std::path::Path| -> Option<&std::path::Path> {
        workspaces
            .iter()
            .map(|w| w.root.as_path())
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
    };

    let mut seen_workspace: Option<&std::path::Path> = None;
    for file in files {
        if let Some(ws) = find_workspace(file) {
            match &seen_workspace {
                None => seen_workspace = Some(ws),
                Some(prev) if *prev != ws => return true,
                _ => {}
            }
        }
    }
    false
}

fn public_workspace_roots<'a>(
    public_packages: &[String],
    workspaces: &'a [fallow_config::WorkspaceInfo],
) -> Vec<&'a std::path::Path> {
    if public_packages.is_empty() || workspaces.is_empty() {
        return Vec::new();
    }

    workspaces
        .iter()
        .filter(|ws| {
            public_packages.iter().any(|pattern| {
                ws.name == *pattern
                    || globset::Glob::new(pattern)
                        .ok()
                        .is_some_and(|g| g.compile_matcher().is_match(&ws.name))
            })
        })
        .map(|ws| ws.root.as_path())
        .collect()
}

fn find_circular_dependencies(
    graph: &ModuleGraph,
    line_offsets_map: &LineOffsetsMap<'_>,
    suppressions: &crate::suppress::SuppressionContext<'_>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<CircularDependency> {
    let cycles = graph.find_cycles();
    let mut dependencies: Vec<CircularDependency> = cycles
        .into_iter()
        .filter_map(|cycle| {
            if is_circular_dependency_suppressed(graph, line_offsets_map, suppressions, &cycle) {
                return None;
            }

            let files: Vec<std::path::PathBuf> = cycle
                .iter()
                .map(|&id| graph.modules[id.0 as usize].path.clone())
                .collect();
            let length = files.len();
            // Look up the import span from cycle[0] -> cycle[1] for precise location.
            let (line, col) =
                cycle_edge_line_col(graph, line_offsets_map, &cycle, 0).unwrap_or((1, 0));
            Some(CircularDependency {
                files,
                length,
                line,
                col,
                is_cross_package: false,
            })
        })
        .collect();

    // Mark cycles that cross workspace package boundaries.
    if !workspaces.is_empty() {
        for dep in &mut dependencies {
            dep.is_cross_package = is_cross_package_cycle(&dep.files, workspaces);
        }
    }

    dependencies
}

/// Find all dead code, with optional resolved module data, plugin context, and workspace info.
#[expect(
    clippy::too_many_lines,
    reason = "orchestration function calling all detectors; each call is one-line and the sequence is easier to follow in one place"
)]
pub fn find_dead_code_full(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    resolved_modules: &[ResolvedModule],
    plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    workspaces: &[fallow_config::WorkspaceInfo],
    modules: &[ModuleInfo],
    collect_usages: bool,
) -> AnalysisResults {
    let _span = tracing::info_span!("find_dead_code").entered();

    // Build suppression context: tracks which suppressions are consumed by detectors
    let suppressions = crate::suppress::SuppressionContext::new(modules);

    // Build line offset index: FileId -> pre-computed line start offsets.
    // Eliminates redundant file reads for byte-to-line/col conversion.
    let line_offsets_by_file: LineOffsetsMap<'_> = modules
        .iter()
        .filter(|m| !m.line_offsets.is_empty())
        .map(|m| (m.file_id, m.line_offsets.as_slice()))
        .collect();

    // Build merged dependency set from root + all workspace package.json files
    let pkg_path = config.root.join("package.json");
    let pkg = PackageJson::load(&pkg_path).ok();

    // Merge the top-level config rules with any plugin-contributed rules.
    // Plain string entries behave like the old global allowlist; scoped object
    // entries only apply to classes that match `extends` / `implements`.
    let mut user_class_members = config.used_class_members.clone();
    if let Some(plugin_result) = plugin_result {
        user_class_members.extend(plugin_result.used_class_members.iter().cloned());
    }

    let virtual_prefixes: Vec<&str> = plugin_result
        .map(|pr| {
            pr.virtual_module_prefixes
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let generated_patterns: Vec<&str> = plugin_result
        .map(|pr| {
            pr.generated_import_patterns
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();

    let (
        (unused_files, export_results),
        (
            (member_results, dependency_results),
            (
                (unresolved_imports, duplicate_exports),
                (boundary_violations, (circular_dependencies, export_usages)),
            ),
        ),
    ) = rayon::join(
        || {
            rayon::join(
                || {
                    if config.rules.unused_files != Severity::Off {
                        find_unused_files(graph, &suppressions)
                    } else {
                        Vec::new()
                    }
                },
                || {
                    let mut results = AnalysisResults::default();
                    if config.rules.unused_exports != Severity::Off
                        || config.rules.unused_types != Severity::Off
                        || config.rules.private_type_leaks != Severity::Off
                    {
                        let (exports, types, stale_expected) = find_unused_exports(
                            graph,
                            modules,
                            config,
                            plugin_result,
                            &suppressions,
                            &line_offsets_by_file,
                        );
                        if config.rules.unused_exports != Severity::Off {
                            results.unused_exports = exports;
                        }
                        if config.rules.unused_types != Severity::Off {
                            results.unused_types = types;
                            suppress_signature_backing_types(
                                &mut results.unused_types,
                                graph,
                                modules,
                            );
                        }
                        if config.rules.private_type_leaks != Severity::Off {
                            results.private_type_leaks = find_private_type_leaks(
                                graph,
                                modules,
                                config,
                                &suppressions,
                                &line_offsets_by_file,
                            );
                        }
                        // @expected-unused tags that became stale (export is now used).
                        if config.rules.stale_suppressions != Severity::Off {
                            results.stale_suppressions.extend(stale_expected);
                        }
                    }
                    results
                },
            )
        },
        || {
            rayon::join(
                || {
                    rayon::join(
                        || {
                            let mut results = AnalysisResults::default();
                            if config.rules.unused_enum_members != Severity::Off
                                || config.rules.unused_class_members != Severity::Off
                            {
                                let (enum_members, class_members) = find_unused_members(
                                    graph,
                                    resolved_modules,
                                    modules,
                                    &suppressions,
                                    &line_offsets_by_file,
                                    &user_class_members,
                                );
                                if config.rules.unused_enum_members != Severity::Off {
                                    results.unused_enum_members = enum_members;
                                }
                                if config.rules.unused_class_members != Severity::Off {
                                    results.unused_class_members = class_members;
                                }
                            }
                            results
                        },
                        || {
                            let mut results = AnalysisResults::default();
                            if let Some(ref pkg) = pkg {
                                if config.rules.unused_dependencies != Severity::Off
                                    || config.rules.unused_dev_dependencies != Severity::Off
                                    || config.rules.unused_optional_dependencies != Severity::Off
                                {
                                    let (deps, dev_deps, optional_deps) = find_unused_dependencies(
                                        graph,
                                        pkg,
                                        config,
                                        plugin_result,
                                        workspaces,
                                    );
                                    if config.rules.unused_dependencies != Severity::Off {
                                        results.unused_dependencies = deps;
                                    }
                                    if config.rules.unused_dev_dependencies != Severity::Off {
                                        results.unused_dev_dependencies = dev_deps;
                                    }
                                    if config.rules.unused_optional_dependencies != Severity::Off {
                                        results.unused_optional_dependencies = optional_deps;
                                    }
                                }

                                if config.rules.unlisted_dependencies != Severity::Off {
                                    results.unlisted_dependencies = find_unlisted_dependencies(
                                        graph,
                                        pkg,
                                        config,
                                        workspaces,
                                        plugin_result,
                                        resolved_modules,
                                        &line_offsets_by_file,
                                    );
                                }

                                // In production mode, detect dependencies that are only used via
                                // type-only imports.
                                if config.production {
                                    results.type_only_dependencies =
                                        find_type_only_dependencies(graph, pkg, config, workspaces);
                                }

                                // In non-production mode, detect production deps only imported by
                                // test/dev files.
                                if !config.production
                                    && config.rules.test_only_dependencies != Severity::Off
                                {
                                    results.test_only_dependencies =
                                        find_test_only_dependencies(graph, pkg, config, workspaces);
                                }
                            }
                            results
                        },
                    )
                },
                || {
                    rayon::join(
                        || {
                            rayon::join(
                                || {
                                    if config.rules.unresolved_imports != Severity::Off
                                        && !resolved_modules.is_empty()
                                    {
                                        find_unresolved_imports(
                                            resolved_modules,
                                            config,
                                            &suppressions,
                                            &virtual_prefixes,
                                            &generated_patterns,
                                            &line_offsets_by_file,
                                        )
                                    } else {
                                        Vec::new()
                                    }
                                },
                                || {
                                    if config.rules.duplicate_exports != Severity::Off {
                                        find_duplicate_exports(
                                            graph,
                                            config,
                                            &suppressions,
                                            &line_offsets_by_file,
                                            resolved_modules,
                                        )
                                    } else {
                                        Vec::new()
                                    }
                                },
                            )
                        },
                        || {
                            rayon::join(
                                || {
                                    if config.rules.boundary_violation != Severity::Off
                                        && !config.boundaries.is_empty()
                                    {
                                        boundary::find_boundary_violations(
                                            graph,
                                            config,
                                            &suppressions,
                                            &line_offsets_by_file,
                                        )
                                    } else {
                                        Vec::new()
                                    }
                                },
                                || {
                                    rayon::join(
                                        || {
                                            if config.rules.circular_dependencies != Severity::Off {
                                                find_circular_dependencies(
                                                    graph,
                                                    &line_offsets_by_file,
                                                    &suppressions,
                                                    workspaces,
                                                )
                                            } else {
                                                Vec::new()
                                            }
                                        },
                                        || {
                                            // Collect export usage counts for Code Lens (LSP
                                            // feature). Skipped in CLI mode since the field is
                                            // #[serde(skip)] in all output formats.
                                            if collect_usages {
                                                collect_export_usages(graph, &line_offsets_by_file)
                                            } else {
                                                Vec::new()
                                            }
                                        },
                                    )
                                },
                            )
                        },
                    )
                },
            )
        },
    );

    let mut results = AnalysisResults {
        unused_files,
        unused_exports: export_results.unused_exports,
        unused_types: export_results.unused_types,
        private_type_leaks: export_results.private_type_leaks,
        stale_suppressions: export_results.stale_suppressions,
        unused_enum_members: member_results.unused_enum_members,
        unused_class_members: member_results.unused_class_members,
        unused_dependencies: dependency_results.unused_dependencies,
        unused_dev_dependencies: dependency_results.unused_dev_dependencies,
        unused_optional_dependencies: dependency_results.unused_optional_dependencies,
        unlisted_dependencies: dependency_results.unlisted_dependencies,
        type_only_dependencies: dependency_results.type_only_dependencies,
        test_only_dependencies: dependency_results.test_only_dependencies,
        unresolved_imports,
        duplicate_exports,
        boundary_violations,
        circular_dependencies,
        export_usages,
        ..AnalysisResults::default()
    };

    // Filter out exported API surface from public packages.
    // Public packages are workspace packages whose exports are intended for external consumers.
    let public_roots = public_workspace_roots(&config.public_packages, workspaces);
    if !public_roots.is_empty() {
        results
            .unused_exports
            .retain(|e| !public_roots.iter().any(|root| e.path.starts_with(root)));
        results
            .unused_types
            .retain(|e| !public_roots.iter().any(|root| e.path.starts_with(root)));
        results
            .unused_enum_members
            .retain(|e| !public_roots.iter().any(|root| e.path.starts_with(root)));
        results
            .unused_class_members
            .retain(|e| !public_roots.iter().any(|root| e.path.starts_with(root)));
    }

    // Detect stale suppression comments (must run after all detectors)
    if config.rules.stale_suppressions != Severity::Off {
        results
            .stale_suppressions
            .extend(suppressions.find_stale(graph));
    }
    results.suppression_count = suppressions.used_count();

    // Detect pnpm catalog issues (purely off package.json + pnpm-workspace.yaml).
    // Both unused-catalog-entries and unresolved-catalog-references share the
    // YAML parse and consumer walk; gather state once and run each detector
    // gated on its own rule severity.
    let need_unused_catalogs = config.rules.unused_catalog_entries != Severity::Off;
    let need_unresolved_refs = config.rules.unresolved_catalog_references != Severity::Off;
    if (need_unused_catalogs || need_unresolved_refs)
        && let Some(state) = gather_pnpm_catalog_state(config, workspaces)
    {
        if need_unused_catalogs {
            results.unused_catalog_entries = find_unused_catalog_entries(&state);
        }
        if need_unresolved_refs {
            results.unresolved_catalog_references = find_unresolved_catalog_references(
                &state,
                &config.compiled_ignore_catalog_references,
                &config.root,
            );
        }
    }

    // Detect pnpm dependency-override issues (off pnpm-workspace.yaml +
    // root package.json's pnpm.overrides). Mirrors the catalog detector: one
    // parse + workspace walk feeds both unused-dependency-overrides and
    // misconfigured-dependency-overrides; each detector gated on its own
    // rule severity.
    let need_unused_overrides = config.rules.unused_dependency_overrides != Severity::Off;
    let need_misconfigured_overrides =
        config.rules.misconfigured_dependency_overrides != Severity::Off;
    if (need_unused_overrides || need_misconfigured_overrides)
        && let Some(state) = gather_pnpm_override_state(config, workspaces)
    {
        if need_unused_overrides {
            results.unused_dependency_overrides = find_unused_dependency_overrides(&state, config);
        }
        if need_misconfigured_overrides {
            results.misconfigured_dependency_overrides =
                find_misconfigured_dependency_overrides(&state, config);
        }
    }

    // Sort all result arrays for deterministic output ordering.
    // Parallel collection and FxHashMap iteration don't guarantee order,
    // so without sorting the same project can produce different orderings.
    results.sort();

    results
}

#[cfg(test)]
mod tests {
    use fallow_types::extract::{byte_offset_to_line_col, compute_line_offsets};

    // Helper: compute line offsets from source and convert byte offset
    fn line_col(source: &str, byte_offset: u32) -> (u32, u32) {
        let offsets = compute_line_offsets(source);
        byte_offset_to_line_col(&offsets, byte_offset)
    }

    // ── compute_line_offsets ─────────────────────────────────────

    #[test]
    fn compute_offsets_empty() {
        assert_eq!(compute_line_offsets(""), vec![0]);
    }

    #[test]
    fn compute_offsets_single_line() {
        assert_eq!(compute_line_offsets("hello"), vec![0]);
    }

    #[test]
    fn compute_offsets_multiline() {
        assert_eq!(compute_line_offsets("abc\ndef\nghi"), vec![0, 4, 8]);
    }

    #[test]
    fn compute_offsets_trailing_newline() {
        assert_eq!(compute_line_offsets("abc\n"), vec![0, 4]);
    }

    #[test]
    fn compute_offsets_crlf() {
        assert_eq!(compute_line_offsets("ab\r\ncd"), vec![0, 4]);
    }

    #[test]
    fn compute_offsets_consecutive_newlines() {
        assert_eq!(compute_line_offsets("\n\n"), vec![0, 1, 2]);
    }

    // ── byte_offset_to_line_col ─────────────────────────────────

    #[test]
    fn byte_offset_empty_source() {
        assert_eq!(line_col("", 0), (1, 0));
    }

    #[test]
    fn byte_offset_single_line_start() {
        assert_eq!(line_col("hello", 0), (1, 0));
    }

    #[test]
    fn byte_offset_single_line_middle() {
        assert_eq!(line_col("hello", 4), (1, 4));
    }

    #[test]
    fn byte_offset_multiline_start_of_line2() {
        assert_eq!(line_col("line1\nline2\nline3", 6), (2, 0));
    }

    #[test]
    fn byte_offset_multiline_middle_of_line3() {
        assert_eq!(line_col("line1\nline2\nline3", 14), (3, 2));
    }

    #[test]
    fn byte_offset_at_newline_boundary() {
        assert_eq!(line_col("line1\nline2", 5), (1, 5));
    }

    #[test]
    fn byte_offset_multibyte_utf8() {
        let source = "hi\n\u{1F600}x";
        assert_eq!(line_col(source, 3), (2, 0));
        assert_eq!(line_col(source, 7), (2, 4));
    }

    #[test]
    fn byte_offset_multibyte_accented_chars() {
        let source = "caf\u{00E9}\nbar";
        assert_eq!(line_col(source, 6), (2, 0));
        assert_eq!(line_col(source, 3), (1, 3));
    }

    #[test]
    fn byte_offset_via_map_fallback() {
        use super::*;
        let map: LineOffsetsMap<'_> = FxHashMap::default();
        assert_eq!(
            super::byte_offset_to_line_col(&map, FileId(99), 42),
            (1, 42)
        );
    }

    #[test]
    fn byte_offset_via_map_lookup() {
        use super::*;
        let offsets = compute_line_offsets("abc\ndef\nghi");
        let mut map: LineOffsetsMap<'_> = FxHashMap::default();
        map.insert(FileId(0), &offsets);
        assert_eq!(super::byte_offset_to_line_col(&map, FileId(0), 5), (2, 1));
    }

    // ── find_dead_code orchestration ──────────────────────────────

    mod orchestration {
        use super::super::*;
        use fallow_config::{FallowConfig, OutputFormat, RulesConfig, Severity};
        use std::path::PathBuf;

        fn find_dead_code(graph: &ModuleGraph, config: &ResolvedConfig) -> AnalysisResults {
            find_dead_code_full(graph, config, &[], None, &[], &[], false)
        }

        fn make_config_with_rules(rules: RulesConfig) -> ResolvedConfig {
            FallowConfig {
                rules,
                ..Default::default()
            }
            .resolve(
                PathBuf::from("/tmp/orchestration-test"),
                OutputFormat::Human,
                1,
                true,
                true,
            )
        }

        #[test]
        fn find_dead_code_all_rules_off_returns_empty() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::graph::ModuleGraph;
            use crate::resolve::ResolvedModule;
            use rustc_hash::FxHashSet;

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                size_bytes: 100,
            }];
            let entry_points = vec![EntryPoint {
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                source: EntryPointSource::ManualEntry,
            }];
            let resolved = vec![ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
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
                namespace_object_aliases: vec![],
            }];
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);

            let rules = RulesConfig {
                unused_files: Severity::Off,
                unused_exports: Severity::Off,
                unused_types: Severity::Off,
                private_type_leaks: Severity::Off,
                unused_dependencies: Severity::Off,
                unused_dev_dependencies: Severity::Off,
                unused_optional_dependencies: Severity::Off,
                unused_enum_members: Severity::Off,
                unused_class_members: Severity::Off,
                unresolved_imports: Severity::Off,
                unlisted_dependencies: Severity::Off,
                duplicate_exports: Severity::Off,
                type_only_dependencies: Severity::Off,
                circular_dependencies: Severity::Off,
                test_only_dependencies: Severity::Off,
                boundary_violation: Severity::Off,
                coverage_gaps: Severity::Off,
                feature_flags: Severity::Off,
                stale_suppressions: Severity::Off,
                unused_catalog_entries: Severity::Off,
                unresolved_catalog_references: Severity::Off,
                unused_dependency_overrides: Severity::Off,
                misconfigured_dependency_overrides: Severity::Off,
            };
            let config = make_config_with_rules(rules);
            let results = find_dead_code(&graph, &config);

            assert!(results.unused_files.is_empty());
            assert!(results.unused_exports.is_empty());
            assert!(results.unused_types.is_empty());
            assert!(results.unused_dependencies.is_empty());
            assert!(results.unused_dev_dependencies.is_empty());
            assert!(results.unused_optional_dependencies.is_empty());
            assert!(results.unused_enum_members.is_empty());
            assert!(results.unused_class_members.is_empty());
            assert!(results.unresolved_imports.is_empty());
            assert!(results.unlisted_dependencies.is_empty());
            assert!(results.duplicate_exports.is_empty());
            assert!(results.circular_dependencies.is_empty());
            assert!(results.export_usages.is_empty());
        }

        #[test]
        fn find_dead_code_full_collect_usages_flag() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::extract::{ExportName, VisibilityTag};
            use crate::graph::{ExportSymbol, ModuleGraph};
            use crate::resolve::ResolvedModule;
            use oxc_span::Span;
            use rustc_hash::FxHashSet;

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                size_bytes: 100,
            }];
            let entry_points = vec![EntryPoint {
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                source: EntryPointSource::ManualEntry,
            }];
            let resolved = vec![ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
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
                namespace_object_aliases: vec![],
            }];
            let mut graph = ModuleGraph::build(&resolved, &entry_points, &files);
            graph.modules[0].exports = vec![ExportSymbol {
                name: ExportName::Named("myExport".to_string()),
                is_type_only: false,
                is_side_effect_used: false,
                visibility: VisibilityTag::None,
                span: Span::new(10, 30),
                references: vec![],
                members: vec![],
            }];

            let rules = RulesConfig::default();
            let config = make_config_with_rules(rules);

            // Without collect_usages
            let results_no_collect = find_dead_code_full(
                &graph,
                &config,
                &[],
                None,
                &[],
                &[],
                false, // collect_usages = false
            );
            assert!(
                results_no_collect.export_usages.is_empty(),
                "export_usages should be empty when collect_usages is false"
            );

            // With collect_usages
            let results_with_collect = find_dead_code_full(
                &graph,
                &config,
                &[],
                None,
                &[],
                &[],
                true, // collect_usages = true
            );
            assert!(
                !results_with_collect.export_usages.is_empty(),
                "export_usages should be populated when collect_usages is true"
            );
            assert_eq!(
                results_with_collect.export_usages[0].export_name,
                "myExport"
            );
        }

        #[test]
        fn find_dead_code_delegates_to_find_dead_code_with_resolved() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::graph::ModuleGraph;
            use crate::resolve::ResolvedModule;
            use rustc_hash::FxHashSet;

            let files = vec![DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                size_bytes: 100,
            }];
            let entry_points = vec![EntryPoint {
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
                source: EntryPointSource::ManualEntry,
            }];
            let resolved = vec![ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/tmp/orchestration-test/src/index.ts"),
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
                namespace_object_aliases: vec![],
            }];
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);
            let config = make_config_with_rules(RulesConfig::default());

            // find_dead_code is a thin wrapper — verify it doesn't panic and returns results
            let results = find_dead_code(&graph, &config);
            // The entry point export analysis is skipped, so these should be empty
            assert!(results.unused_exports.is_empty());
        }

        #[test]
        fn suppressions_built_from_modules() {
            use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
            use crate::extract::ModuleInfo;
            use crate::graph::ModuleGraph;
            use crate::resolve::ResolvedModule;
            use crate::suppress::{IssueKind, Suppression};
            use rustc_hash::FxHashSet;

            let files = vec![
                DiscoveredFile {
                    id: FileId(0),
                    path: PathBuf::from("/tmp/orchestration-test/src/entry.ts"),
                    size_bytes: 100,
                },
                DiscoveredFile {
                    id: FileId(1),
                    path: PathBuf::from("/tmp/orchestration-test/src/utils.ts"),
                    size_bytes: 100,
                },
            ];
            let entry_points = vec![EntryPoint {
                path: PathBuf::from("/tmp/orchestration-test/src/entry.ts"),
                source: EntryPointSource::ManualEntry,
            }];
            let resolved = files
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
                    namespace_object_aliases: vec![],
                })
                .collect::<Vec<_>>();
            let graph = ModuleGraph::build(&resolved, &entry_points, &files);

            // Create module info with a file-level suppression for unused files
            let modules = vec![ModuleInfo {
                file_id: FileId(1),
                exports: vec![],
                imports: vec![],
                re_exports: vec![],
                dynamic_imports: vec![],
                dynamic_import_patterns: vec![],
                require_calls: vec![],
                member_accesses: vec![],
                whole_object_uses: vec![],
                has_cjs_exports: false,
                content_hash: 0,
                suppressions: vec![Suppression {
                    line: 0,
                    comment_line: 1,
                    kind: Some(IssueKind::UnusedFile),
                }],
                unused_import_bindings: vec![],
                type_referenced_import_bindings: vec![],
                value_referenced_import_bindings: vec![],
                line_offsets: vec![],
                complexity: vec![],
                flag_uses: vec![],
                class_heritage: vec![],
                local_type_declarations: Vec::new(),
                public_signature_type_references: Vec::new(),
                namespace_object_aliases: Vec::new(),
            }];

            let rules = RulesConfig {
                unused_files: Severity::Error,
                ..RulesConfig::default()
            };
            let config = make_config_with_rules(rules);

            let results = find_dead_code_full(&graph, &config, &[], None, &[], &modules, false);

            // The suppression should prevent utils.ts from being reported as unused
            // (it would normally be unused since only entry.ts is an entry point).
            // Note: unused_files also checks if the file exists on disk, so it
            // may still be filtered out. The key is the suppression path is exercised.
            assert!(
                !results
                    .unused_files
                    .iter()
                    .any(|f| f.path.to_string_lossy().contains("utils.ts")),
                "suppressed file should not appear in unused_files"
            );
        }
    }
}
