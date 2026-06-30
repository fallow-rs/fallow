use rustc_hash::FxHashMap;

use fallow_config::ResolvedConfig;

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::suppress::{IssueKind, SuppressionContext};
use fallow_types::results::BoundaryViolation;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Detect imports that cross architecture boundary zones without permission.
///
/// For each reachable module, classifies it into a zone and checks all its
/// import targets. If the target is in a different zone that the source zone
/// is not allowed to import from, a `BoundaryViolation` is emitted.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_boundary_violations for typed output; serialize with fallow_api::serialize_boundary_violations_programmatic_json for JSON output. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn find_boundary_violations(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryViolation> {
    let mut violations = Vec::new();
    let mut zone_cache: FxHashMap<FileId, Option<String>> = FxHashMap::default();
    let ctx = BoundaryContext {
        graph,
        config,
        suppressions,
        line_offsets_by_file,
    };

    for node in &graph.modules {
        collect_node_boundary_violations(&mut violations, node, &mut zone_cache, &ctx);
    }

    warn_unmatched_boundary_zones(config, &zone_cache);
    violations
}

struct BoundaryContext<'a> {
    graph: &'a ModuleGraph,
    config: &'a ResolvedConfig,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn classify_boundary_zone(
    file_id: FileId,
    cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> Option<String> {
    if let Some(cached) = cache.get(&file_id) {
        return cached.clone();
    }
    let node = &ctx.graph.modules[file_id.0 as usize];
    let rel_path = node
        .path
        .strip_prefix(&ctx.config.root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let zone = rel_path.and_then(|p| ctx.config.boundaries.classify_zone(&p).map(str::to_owned));
    cache.insert(file_id, zone.clone());
    zone
}

fn collect_node_boundary_violations(
    violations: &mut Vec<BoundaryViolation>,
    node: &ModuleNode,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) {
    if !node.is_reachable() && !node.is_entry_point() {
        return;
    }
    let Some(from_zone) = classify_boundary_zone(node.file_id, zone_cache, ctx) else {
        return;
    };
    if !ctx
        .config
        .boundaries
        .rules
        .iter()
        .any(|r| r.from_zone == from_zone)
    {
        return;
    }
    if ctx
        .suppressions
        .is_file_suppressed(node.file_id, IssueKind::BoundaryViolation)
    {
        return;
    }

    for (target_id, all_type_only, span_start) in ctx.graph.outgoing_edge_summaries(node.file_id) {
        collect_boundary_edge_violation(
            violations,
            node,
            BoundaryEdge {
                target_id,
                all_type_only,
                span_start,
            },
            &from_zone,
            zone_cache,
            ctx,
        );
    }
}

#[derive(Clone, Copy)]
struct BoundaryEdge {
    target_id: FileId,
    all_type_only: bool,
    span_start: Option<u32>,
}

fn collect_boundary_edge_violation(
    violations: &mut Vec<BoundaryViolation>,
    node: &ModuleNode,
    edge: BoundaryEdge,
    from_zone: &str,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) {
    let Some(to_zone) = classify_boundary_zone(edge.target_id, zone_cache, ctx) else {
        return;
    };
    if is_boundary_import_allowed(
        node,
        edge.target_id,
        edge.all_type_only,
        from_zone,
        &to_zone,
        ctx,
    ) {
        return;
    }

    let (line, col) = edge.span_start.map_or((1, 0), |s| {
        byte_offset_to_line_col(ctx.line_offsets_by_file, node.file_id, s)
    });
    if ctx
        .suppressions
        .is_suppressed(node.file_id, line, IssueKind::BoundaryViolation)
    {
        return;
    }

    let target_node = &ctx.graph.modules[edge.target_id.0 as usize];
    violations.push(BoundaryViolation {
        from_path: node.path.clone(),
        to_path: target_node.path.clone(),
        from_zone: from_zone.to_string(),
        to_zone,
        import_specifier: boundary_import_specifier(target_node, ctx.config),
        line,
        col,
    });
}

fn is_boundary_import_allowed(
    node: &ModuleNode,
    target_id: FileId,
    all_type_only: bool,
    from_zone: &str,
    to_zone: &str,
    ctx: &BoundaryContext<'_>,
) -> bool {
    if ctx.config.boundaries.is_import_allowed(from_zone, to_zone) {
        return true;
    }
    if all_type_only
        && ctx
            .config
            .boundaries
            .is_type_only_allowed(from_zone, to_zone)
    {
        tracing::debug!(
            "boundary type-only allowed: '{}' -> '{}' ({} -> {})",
            from_zone,
            to_zone,
            node.path.display(),
            ctx.graph.modules[target_id.0 as usize].path.display()
        );
        return true;
    }
    false
}

fn boundary_import_specifier(target_node: &ModuleNode, config: &ResolvedConfig) -> String {
    target_node.path.strip_prefix(&config.root).map_or_else(
        |_| target_node.path.to_string_lossy().replace('\\', "/"),
        |p| p.to_string_lossy().replace('\\', "/"),
    )
}

fn warn_unmatched_boundary_zones(
    config: &ResolvedConfig,
    zone_cache: &FxHashMap<FileId, Option<String>>,
) {
    if config.boundaries.is_empty() {
        return;
    }

    let classified_zones: rustc_hash::FxHashSet<&str> =
        zone_cache.values().filter_map(|z| z.as_deref()).collect();
    for zone in &config.boundaries.zones {
        if !classified_zones.contains(zone.name.as_str()) {
            tracing::warn!(
                "boundary zone '{}' matched 0 reachable files, check your directory \
                 structure, pattern, or whether these files are all currently unreachable",
                zone.name
            );
        }
    }
}

#[cfg(test)]
#[expect(
    deprecated,
    reason = "ADR-008 keeps direct detector unit tests while the public warning targets external callers"
)]
mod tests {
    use super::*;
    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource};
    use crate::graph::ModuleGraph;
    use crate::resolve::ResolvedModule;
    use crate::suppress::Suppression;
    use fallow_config::{
        BoundaryCallsConfig, BoundaryConfig, BoundaryCoverageConfig, BoundaryRule, BoundaryZone,
        FallowConfig, OutputFormat, ResolvedConfig, RulesConfig, Severity,
    };
    use rustc_hash::FxHashSet;
    use std::path::PathBuf;

    fn make_config(root: PathBuf, boundaries: BoundaryConfig) -> ResolvedConfig {
        FallowConfig {
            rules: RulesConfig {
                boundary_violation: Severity::Error,
                ..RulesConfig::default()
            },
            boundaries,
            ..Default::default()
        }
        .resolve(root, OutputFormat::Human, 1, true, true, None)
    }

    fn resolved_module(file_id: FileId, path: PathBuf) -> ResolvedModule {
        ResolvedModule {
            file_id,
            path,
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
        }
    }

    fn build_graph(
        root: &std::path::Path,
        file_names: &[&str],
        edges: &[(usize, usize, bool)],
    ) -> (Vec<DiscoveredFile>, ModuleGraph) {
        let files: Vec<DiscoveredFile> = file_names
            .iter()
            .enumerate()
            .map(|(i, name)| DiscoveredFile {
                id: FileId(i as u32),
                path: root.join(name),
                size_bytes: 100,
            })
            .collect();

        let entry_points = vec![EntryPoint {
            path: files[0].path.clone(),
            source: EntryPointSource::ManualEntry,
        }];

        let resolved: Vec<ResolvedModule> = files
            .iter()
            .map(|f| {
                let mut rm = resolved_module(f.id, f.path.clone());
                for &(from, to, is_type_only) in edges {
                    if from == f.id.0 as usize {
                        rm.resolved_imports.push(crate::resolve::ResolvedImport {
                            target: crate::resolve::ResolveResult::InternalModule(FileId(
                                to as u32,
                            )),
                            info: fallow_types::extract::ImportInfo {
                                source: format!("./{}", file_names[to]),
                                imported_name: fallow_types::extract::ImportedName::Default,
                                local_name: "x".to_string(),
                                is_type_only,
                                from_style: false,
                                span: oxc_span::Span::new(0, 10),
                                source_span: oxc_span::Span::new(0, 10),
                            },
                        });
                    }
                }
                rm
            })
            .collect();

        let graph = ModuleGraph::build(&resolved, &entry_points, &files);
        (files, graph)
    }

    #[test]
    fn no_boundaries_returns_empty() {
        let root = PathBuf::from("/tmp/boundary-test");
        let config = make_config(root.clone(), BoundaryConfig::default());
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/query.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(violations.is_empty());
    }

    #[test]
    fn allowed_import_no_violation() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/shared/utils.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(violations.is_empty());
    }

    #[test]
    fn disallowed_import_produces_violation() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/query.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].from_zone, "ui");
        assert_eq!(violations[0].to_zone, "db");
    }

    #[test]
    fn self_import_always_allowed() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/ui/helpers.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(violations.is_empty());
    }

    #[test]
    fn unzoned_files_unrestricted() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/utils.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(violations.is_empty());
    }

    #[test]
    fn file_level_suppression_skips_file() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/query.ts"],
            &[(0, 1, false)],
        );

        let supps = vec![Suppression::issue(0, 1, IssueKind::BoundaryViolation)];
        let mut supp_map = FxHashMap::default();
        supp_map.insert(FileId(0), supps.as_slice());
        let suppressions = SuppressionContext::from_map(supp_map);
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(violations.is_empty());
    }

    /// Build a ui->db restricted config with an optional `allowTypeOnly`
    /// list on the `ui` rule. Used by the type-only escape hatch tests.
    fn ui_db_boundaries(allow_type_only: Vec<String>) -> BoundaryConfig {
        BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only,
            }],
        }
    }

    #[test]
    fn type_only_import_allowed_when_zone_listed() {
        let root = PathBuf::from("/tmp/boundary-test");
        let config = make_config(root.clone(), ui_db_boundaries(vec!["db".to_string()]));
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/types.ts"],
            &[(0, 1, true)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(
            violations.is_empty(),
            "type-only import to a zone in allowTypeOnly should not fire"
        );
    }

    #[test]
    fn type_only_import_still_blocked_when_zone_not_listed() {
        let root = PathBuf::from("/tmp/boundary-test");
        let config = make_config(root.clone(), ui_db_boundaries(vec!["other".to_string()]));
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/types.ts"],
            &[(0, 1, true)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert_eq!(
            violations.len(),
            1,
            "type-only import to a zone NOT in allowTypeOnly must still fire"
        );
    }

    #[test]
    fn value_import_blocked_even_when_zone_in_allow_type_only() {
        let root = PathBuf::from("/tmp/boundary-test");
        let config = make_config(root.clone(), ui_db_boundaries(vec!["db".to_string()]));
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/query.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert_eq!(
            violations.len(),
            1,
            "value import must fire regardless of allowTypeOnly"
        );
    }

    #[test]
    fn empty_allow_type_only_preserves_baseline_behavior() {
        let root = PathBuf::from("/tmp/boundary-test");
        let config = make_config(root.clone(), ui_db_boundaries(vec![]));
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/types.ts"],
            &[(0, 1, true)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert_eq!(
            violations.len(),
            1,
            "default empty allowTypeOnly must preserve pre-feature behavior"
        );
    }

    #[test]
    fn allow_type_only_is_independent_of_allow() {
        let root = PathBuf::from("/tmp/boundary-test");
        let boundaries = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/ui/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["db".to_string()],
                allow_type_only: vec!["db".to_string()],
            }],
        };
        let config = make_config(root.clone(), boundaries);
        let (_, graph) = build_graph(
            &root,
            &["src/ui/Button.tsx", "src/db/query.ts"],
            &[(0, 1, false)],
        );
        let suppressions = SuppressionContext::empty();
        let line_offsets = FxHashMap::default();

        let violations = find_boundary_violations(&graph, &config, &suppressions, &line_offsets);
        assert!(
            violations.is_empty(),
            "import already in `allow` must not fire regardless of allowTypeOnly"
        );
    }
}
