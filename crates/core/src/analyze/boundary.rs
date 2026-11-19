use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::ResolvedConfig;
use fallow_types::extract::ImportedName;

use crate::discover::FileId;
use crate::graph::{
    EffectiveExportBinding, EffectiveExportResolution, ExportNamespace, ImportedSymbol,
    ModuleGraph, ModuleNode,
};
use crate::suppress::{IssueKind, SuppressionContext};
use fallow_types::results::{BoundaryViolation, BoundaryViolationReason};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Detect imports that cross architecture boundary zones without permission.
///
/// For each analyzed module, classifies it into a zone and checks all its
/// import targets. If the target is in a different zone that the source zone
/// is not allowed to import from, a `BoundaryViolation` is emitted.
///
/// Reachability does not gate the check. A file that no entry point reaches
/// can still run (for example from a task runner that fallow does not read),
/// so its imports get the same rules as a reachable file (issue #2937).
pub fn find_boundary_violations(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryViolation> {
    let mut violations = Vec::new();
    let mut zone_cache: FxHashMap<FileId, Option<String>> = FxHashMap::default();
    let mut ctx = BoundaryContext {
        graph,
        config,
        suppressions,
        line_offsets_by_file,
        public_files: FxHashMap::default(),
        public_file_ids: FxHashSet::default(),
    };
    (ctx.public_files, ctx.public_file_ids) = collect_public_files(&mut zone_cache, &ctx);

    for node in &graph.modules {
        collect_node_boundary_violations(&mut violations, node, &mut zone_cache, &ctx);
    }

    warn_unmatched_boundary_zones(config, &zone_cache);
    warn_zones_without_public_files(config, &ctx.public_files);
    violations
}

struct BoundaryContext<'a> {
    graph: &'a ModuleGraph,
    config: &'a ResolvedConfig,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    /// The public entry files of each zone that declares `publicFiles`, sorted.
    public_files: FxHashMap<String, Vec<std::path::PathBuf>>,
    /// The same files, for lookup by id.
    public_file_ids: FxHashSet<FileId>,
}

/// The public entry files of each zone that declares `publicFiles`: the files
/// that the zone owns and that match one of its globs.
fn collect_public_files(
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> (
    FxHashMap<String, Vec<std::path::PathBuf>>,
    FxHashSet<FileId>,
) {
    let boundaries = &ctx.config.boundaries;
    let mut paths: FxHashMap<String, Vec<std::path::PathBuf>> = boundaries
        .zones
        .iter()
        .filter(|zone| !zone.public_file_matchers.is_empty())
        .map(|zone| (zone.name.clone(), Vec::new()))
        .collect();
    let mut files = FxHashSet::default();
    if paths.is_empty() {
        return (paths, files);
    }
    for node in &ctx.graph.modules {
        if let Some(zone) = classify_boundary_zone(node.file_id, zone_cache, ctx)
            && let Some(zone_paths) = paths.get_mut(&zone)
            && relative_path(node.file_id, ctx)
                .is_some_and(|path| boundaries.is_public_file(&zone, &path))
        {
            zone_paths.push(node.path.clone());
            files.insert(node.file_id);
        }
    }
    for zone_paths in paths.values_mut() {
        zone_paths.sort();
    }
    (paths, files)
}

fn relative_path(file_id: FileId, ctx: &BoundaryContext<'_>) -> Option<String> {
    ctx.graph.modules[file_id.0 as usize]
        .path
        .strip_prefix(&ctx.config.root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

fn classify_boundary_zone(
    file_id: FileId,
    cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> Option<String> {
    if let Some(cached) = cache.get(&file_id) {
        return cached.clone();
    }
    let zone = relative_path(file_id, ctx)
        .and_then(|p| ctx.config.boundaries.classify_zone(&p).map(str::to_owned));
    cache.insert(file_id, zone.clone());
    zone
}

fn collect_node_boundary_violations(
    violations: &mut Vec<BoundaryViolation>,
    node: &ModuleNode,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) {
    let Some(from_zone) = classify_boundary_zone(node.file_id, zone_cache, ctx) else {
        return;
    };
    // A zone without a rule may import any zone, but it must still respect
    // the public entry files of other zones.
    if ctx.public_files.is_empty()
        && !ctx
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

    let mut judged_targets: FxHashSet<FileId> = FxHashSet::default();
    let mut barrel_edges: Vec<(FileId, &[ImportedSymbol])> = Vec::new();
    // Only edges that run their target cross a boundary. A
    // `require.resolve('./x')` path or an asset loader request loads nothing.
    for summary in ctx.graph.outgoing_edge_summaries(node.file_id) {
        let edge = BoundaryEdge {
            target_id: summary.target,
            all_type_only: summary.all_type_only,
            span_start: summary.span_start,
            via: None,
        };
        if let Some(reason) = edge_violation(node.file_id, edge, zone_cache, ctx) {
            judged_targets.insert(summary.target);
            push_boundary_violation(violations, node, edge, reason, zone_cache, ctx);
        } else {
            barrel_edges.push((summary.target, summary.symbols));
        }
    }

    for edge in chain_origin_edges(&barrel_edges, zone_cache, ctx) {
        if !judged_targets.insert(edge.target_id) {
            continue;
        }
        if let Some(reason) = edge_violation(node.file_id, edge, zone_cache, ctx) {
            push_boundary_violation(violations, node, edge, reason, zone_cache, ctx);
        }
    }
}

/// One import judged against the boundary rules. `target_id` is the module
/// that the import reaches; `via` is the barrel that the importer names when
/// the target was found through a re-export chain.
#[derive(Clone, Copy)]
struct BoundaryEdge {
    target_id: FileId,
    all_type_only: bool,
    span_start: Option<u32>,
    via: Option<FileId>,
}

/// Group the named and default symbols of barrel edges by the module that
/// declares them (issue #2939).
///
/// Namespace and side-effect imports stay judged by their direct target. A
/// symbol whose chain holds a hop that is itself a boundary violation is
/// dropped, because that hop already gets its own finding.
fn chain_origin_edges(
    barrel_edges: &[(FileId, &[ImportedSymbol])],
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> Vec<BoundaryEdge> {
    let mut origins: Vec<BoundaryEdge> = Vec::new();
    for &(barrel_id, symbols) in barrel_edges {
        for symbol in symbols.iter().filter(|symbol| symbol.loads_target()) {
            let name = match &symbol.imported_name {
                ImportedName::Named(name) => name.as_str(),
                ImportedName::Default => "default",
                ImportedName::Namespace | ImportedName::SideEffect => continue,
            };
            let Some(origin) =
                resolve_chain_origin(barrel_id, name, symbol.is_type_only, zone_cache, ctx)
            else {
                continue;
            };
            if origin.file_id == barrel_id {
                continue;
            }
            let span_start = Some(symbol.import_span.start);
            if let Some(existing) = origins.iter_mut().find(|e| e.target_id == origin.file_id) {
                if existing.all_type_only && !origin.type_only {
                    existing.span_start = span_start;
                    existing.via = Some(barrel_id);
                }
                existing.all_type_only &= origin.type_only;
                continue;
            }
            origins.push(BoundaryEdge {
                target_id: origin.file_id,
                all_type_only: origin.type_only,
                span_start,
                via: Some(barrel_id),
            });
        }
    }
    origins
}

struct ChainOrigin {
    file_id: FileId,
    type_only: bool,
}

/// Follow one imported name from a barrel to the module that declares it.
///
/// The walk stops at the first public entry file (`publicFiles`) and returns
/// that file, so an import through a public entry file is judged by the entry
/// file (issue #4760). A barrel that is itself public returns itself, and the
/// caller then keeps the verdict of the direct edge.
///
/// Returns `None` when the name does not resolve to one declaration, when the
/// chain has a cycle, or when a hop in the chain breaks the rules of the
/// zone that owns the re-exporting file.
fn resolve_chain_origin(
    barrel_id: FileId,
    name: &str,
    import_type_only: bool,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> Option<ChainOrigin> {
    let lanes = if import_type_only {
        [ExportNamespace::Type, ExportNamespace::Value]
    } else {
        [ExportNamespace::Value, ExportNamespace::Type]
    };
    let (namespace, binding) = lanes.into_iter().find_map(|namespace| {
        match ctx.graph.resolve_export(barrel_id, name, namespace) {
            EffectiveExportResolution::Unique(binding) => Some((namespace, binding)),
            EffectiveExportResolution::Missing | EffectiveExportResolution::Ambiguous => None,
        }
    })?;

    let mut current = barrel_id;
    let mut current_name = name.to_owned();
    let mut type_only = import_type_only;
    let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
    loop {
        if !visited.insert((current, current_name.clone())) {
            return None;
        }
        if is_public_file(current, ctx) {
            return Some(ChainOrigin {
                file_id: current,
                type_only,
            });
        }
        if binding.origin_file() == current {
            let Some(source) = binding.namespace_source() else {
                return Some(ChainOrigin {
                    file_id: current,
                    type_only,
                });
            };
            type_only |= ctx.graph.modules[current.0 as usize]
                .re_exports
                .iter()
                .any(|re| {
                    re.exported_name == current_name
                        && re.imported_name == "*"
                        && re.source_file == source
                        && re.is_type_only
                });
            return (!is_hop_violation(current, source, type_only, zone_cache, ctx)).then_some(
                ChainOrigin {
                    file_id: source,
                    type_only,
                },
            );
        }
        let hop = next_chain_hop(current, &current_name, namespace, binding, ctx)?;
        type_only |= hop.type_only;
        if is_hop_violation(current, hop.file_id, type_only, zone_cache, ctx) {
            return None;
        }
        current = hop.file_id;
        current_name = hop.name;
    }
}

/// Whether the file is a public entry file of the zone that owns it.
fn is_public_file(file_id: FileId, ctx: &BoundaryContext<'_>) -> bool {
    ctx.public_file_ids.contains(&file_id)
}

struct ChainHop {
    file_id: FileId,
    name: String,
    type_only: bool,
}

/// The re-export on `current` that forwards `name` to the same declaration.
fn next_chain_hop(
    current: FileId,
    name: &str,
    namespace: ExportNamespace,
    binding: EffectiveExportBinding,
    ctx: &BoundaryContext<'_>,
) -> Option<ChainHop> {
    let expected = EffectiveExportResolution::Unique(binding);
    ctx.graph.modules[current.0 as usize]
        .re_exports
        .iter()
        .filter(|re| namespace == ExportNamespace::Type || !re.is_type_only)
        .find_map(|re| {
            let source_name = if re.exported_name == "*" {
                (name != "default").then_some(name)?
            } else if re.exported_name == name && re.imported_name != "*" {
                re.imported_name.as_str()
            } else {
                return None;
            };
            (ctx.graph
                .resolve_export(re.source_file, source_name, namespace)
                == expected)
                .then(|| ChainHop {
                    file_id: re.source_file,
                    name: source_name.to_owned(),
                    type_only: re.is_type_only,
                })
        })
}

/// Whether the re-export hop `from -> to` breaks the rules of the zone that
/// owns `from`. That hop is reported on `from` itself, so the files that
/// consume the barrel do not get a second finding for it.
fn is_hop_violation(
    from: FileId,
    to: FileId,
    type_only: bool,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> bool {
    let edge = BoundaryEdge {
        target_id: to,
        all_type_only: type_only,
        span_start: None,
        via: None,
    };
    edge_violation(from, edge, zone_cache, ctx).is_some()
}

/// The rule that the import `from -> edge.target_id` breaks, if any. A zone
/// rule comes first, so a forbidden deep import gets one finding.
fn edge_violation(
    from: FileId,
    edge: BoundaryEdge,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) -> Option<BoundaryViolationReason> {
    let from_zone = classify_boundary_zone(from, zone_cache, ctx)?;
    let to_zone = classify_boundary_zone(edge.target_id, zone_cache, ctx)?;
    if from_zone == to_zone {
        return None;
    }
    if !is_boundary_import_allowed(
        from,
        edge.target_id,
        edge.all_type_only,
        &from_zone,
        &to_zone,
        ctx,
    ) {
        return Some(BoundaryViolationReason::DisallowedZone);
    }
    (ctx.public_files.contains_key(&to_zone) && !is_public_file(edge.target_id, ctx))
        .then_some(BoundaryViolationReason::InternalFile)
}

fn push_boundary_violation(
    violations: &mut Vec<BoundaryViolation>,
    node: &ModuleNode,
    edge: BoundaryEdge,
    reason: BoundaryViolationReason,
    zone_cache: &mut FxHashMap<FileId, Option<String>>,
    ctx: &BoundaryContext<'_>,
) {
    let (Some(from_zone), Some(to_zone)) = (
        classify_boundary_zone(node.file_id, zone_cache, ctx),
        classify_boundary_zone(edge.target_id, zone_cache, ctx),
    ) else {
        return;
    };
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
    let public_files = match reason {
        BoundaryViolationReason::InternalFile => {
            ctx.public_files.get(&to_zone).cloned().unwrap_or_default()
        }
        BoundaryViolationReason::DisallowedZone => Vec::new(),
    };
    violations.push(BoundaryViolation {
        from_path: node.path.clone(),
        to_path: target_node.path.clone(),
        from_zone,
        to_zone,
        import_specifier: boundary_import_specifier(target_node, ctx.config),
        line,
        col,
        via_path: edge
            .via
            .map(|via| ctx.graph.modules[via.0 as usize].path.clone()),
        reason,
        public_files,
    });
}

fn is_boundary_import_allowed(
    from: FileId,
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
            ctx.graph.modules[from.0 as usize].path.display(),
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
            tracing::warn!("{}", unmatched_zone_warning(&zone.name, config));
        }
    }
}

/// Warn for each zone whose `publicFiles` globs match no file of the zone,
/// because every import into that zone from another zone is then a finding.
fn warn_zones_without_public_files(
    config: &ResolvedConfig,
    public_files: &FxHashMap<String, Vec<std::path::PathBuf>>,
) {
    let mut empty: Vec<&str> = public_files
        .iter()
        .filter(|(_, paths)| paths.is_empty())
        .map(|(zone, _)| zone.as_str())
        .collect();
    empty.sort_unstable();
    for zone in empty {
        tracing::warn!("{}", no_public_files_warning(zone, config));
    }
}

fn no_public_files_warning(zone: &str, config: &ResolvedConfig) -> String {
    let scope = if config.analysis_snapshot.is_base() {
        "base revision snapshot (audit --base): "
    } else {
        ""
    };
    format!(
        "{scope}boundary zone '{zone}' sets publicFiles, but no file of the zone matches its \
         globs, so every import into the zone from another zone is reported"
    )
}

/// Message for a boundary zone that classified no analyzed file.
///
/// The `audit --base` pass analyzes the base revision, where a zone can be
/// legitimately empty while the working tree matches it. That pass is labelled
/// explicitly so the warning is not read as a defect in the current
/// configuration (issue #2013). The working-tree wording is unchanged.
fn unmatched_zone_warning(zone: &str, config: &ResolvedConfig) -> String {
    if config.analysis_snapshot.is_base() {
        return format!(
            "base revision snapshot (audit --base): boundary zone '{zone}' matched 0 files \
             in the base revision, this is about the base revision only and not about your \
             current configuration"
        );
    }
    format!(
        "boundary zone '{zone}' matched 0 files, check your directory structure \
         or pattern"
    )
}

#[cfg(test)]
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
            exports: vec![].into(),
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![].into(),
            semantic_facts: std::sync::Arc::default(),
            whole_object_uses: std::sync::Arc::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: std::sync::Arc::default(),
            exported_factory_return_object_shapes: std::sync::Arc::default(),
            type_member_types: std::sync::Arc::default(),
            missing_export_targets: vec![],
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
                                is_type_only_star: false,
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
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
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
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
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
                public_files: vec![],
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
                public_files: vec![],
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
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
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
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
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
                    public_files: vec![],
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                    public_files: vec![],
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

    #[test]
    fn unmatched_zone_warning_keeps_current_revision_wording() {
        let config = make_config(PathBuf::from("/project"), BoundaryConfig::default());

        assert_eq!(
            unmatched_zone_warning("ui", &config),
            "boundary zone 'ui' matched 0 files, check your directory structure or pattern"
        );
    }

    #[test]
    fn zone_without_public_files_warning_names_the_zone() {
        let config = make_config(PathBuf::from("/project"), BoundaryConfig::default());

        let warning = no_public_files_warning("billing", &config);
        assert!(
            warning.starts_with("boundary zone 'billing' sets publicFiles, but no file"),
            "got {warning}"
        );
    }

    #[test]
    fn unmatched_zone_warning_labels_the_base_revision_snapshot() {
        let mut config = make_config(PathBuf::from("/project"), BoundaryConfig::default());
        config.analysis_snapshot = fallow_config::AnalysisSnapshot::Base;

        let message = unmatched_zone_warning("ui", &config);

        assert!(
            message.starts_with("base revision snapshot (audit --base): "),
            "base-revision warning must be labelled, got: {message}"
        );
        assert_ne!(
            message,
            unmatched_zone_warning(
                "ui",
                &make_config(PathBuf::from("/project"), BoundaryConfig::default())
            ),
            "base-revision warning must differ from the current-revision warning"
        );
    }
}
