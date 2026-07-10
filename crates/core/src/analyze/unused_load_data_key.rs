//! Detection of dead SvelteKit `load()` return-object keys.
//!
//! A SvelteKit route's `load()` (in `+page.ts` / `+page.server.ts` and the
//! `.js` variants) returns an object whose keys become the route's `data` prop.
//! A returned key that NO consumer reads is dead: it runs a real server-side
//! fetch / DB cost on every request for data nothing renders. `svelte-check`
//! types `data` via generated `$types` but never flags an unread RETURNED key
//! (the unused-input direction); no competitor catches this.
//!
//! Consumers credit the key through three channels (the extraction primitives
//! A #1255, B #1257, C #1260 supply the member accesses):
//! 1. the sibling `+page.svelte`'s `data.<key>` member accesses (route-pinned);
//! 2. project-wide `page.data.<key>` (Svelte 5 `$app/state`) member accesses;
//! 3. project-wide `$page.data.<key>` (Svelte 4 `$app/stores`) member accesses.
//!
//! The detector is built to never false-flag (degrade by abstaining):
//! - **Dep-gated** on `@sveltejs/kit`.
//! - **Harvest abstain** (`has_unharvestable_load`): a spread / non-literal /
//!   multi-return / computed-key / wrapped `load` harvests nothing.
//! - **Whole-`data` abstain** (FP-1): the sibling `+page.svelte` passing the
//!   whole `data` object opaquely (`has_load_data_whole_use` or a
//!   `whole_object_uses` of `data`) abstains the route's keys.
//! - **Server -> universal chain** (FP-2): a `+page.server.ts` whose sibling
//!   universal `+page.ts` reads / forwards its `data` param is credited (the
//!   universal load consumes the server keys the page never reads directly).
//! - **Global whole-object abstain** (cut A): any module's whole-object use of
//!   `page.data` / `$page.data` abstains all SvelteKit load keys project-wide,
//!   sets the observable `global_abstain` flag (S1), and does not suppress
//!   React Router or Remix route-loader findings.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::results::UnusedLoadDataKey;
use crate::suppress::{IssueKind, SuppressionContext};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The basenames of SvelteKit page-load producers (cut A: page loads only).
const PAGE_LOAD_PRODUCER_NAMES: &[&str] =
    &["+page.ts", "+page.server.ts", "+page.js", "+page.server.js"];

/// A server-load producer (`+page.server.ts` / `+page.server.js`) whose `data`
/// keys can be consumed by a sibling universal `+page.ts` / `+page.js`.
const SERVER_LOAD_PRODUCER_NAMES: &[&str] = &["+page.server.ts", "+page.server.js"];

/// The universal-load sibling basenames (cut A).
const UNIVERSAL_LOAD_NAMES: &[&str] = &["+page.ts", "+page.js"];

const ROUTE_LOADER_DATA_OBJECT: &str = "$fallow.routeLoaderData";

const REACT_ROUTER_DEPS: &[&str] = &["react-router", "react-router-dom", "@react-router/dev"];

const REMIX_DEPS: &[&str] = &[
    "@remix-run/react",
    "@remix-run/node",
    "@remix-run/cloudflare",
    "@remix-run/deno",
];

/// Result of the load-data-key detector: the surviving findings plus a flag set
/// when a global whole-object use of `page.data` / `$page.data` abstained
/// SvelteKit load keys project-wide (S1 observability).
pub struct LoadDataKeyResult {
    /// The surviving dead-key findings.
    pub findings: Vec<UnusedLoadDataKey>,
    /// `true` when the project-wide whole-object abstain (ladder ii) fired, so a
    /// `0` finding count is distinguishable from "abstained project-wide".
    pub global_abstain: bool,
}

/// Find SvelteKit `load()` return-object keys read by no consumer.
///
/// Returns an empty result unless the project declares `@sveltejs/kit`.
#[must_use]
pub fn find_unused_load_data_keys(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    root: &Path,
) -> LoadDataKeyResult {
    let has_sveltekit = declared_deps.contains("@sveltejs/kit");
    let has_route_loader_deps = declared_deps
        .iter()
        .any(|dep| REACT_ROUTER_DEPS.contains(&dep.as_str()) || REMIX_DEPS.contains(&dep.as_str()));
    if !has_sveltekit && !has_route_loader_deps {
        return empty_result();
    }

    // Ladder (ii): any module's whole-object use of `page.data` / `$page.data`
    // means a reflective read could consume any SvelteKit key, so abstain that
    // branch. Read the persisted `has_page_data_store_whole_use` signal (derived
    // in `release_resolution_payload` from `whole_object_uses` before that
    // vector is released), NOT the now-drained `whole_object_uses` itself.
    let global_abstain = has_sveltekit && modules.iter().any(|m| m.has_page_data_store_whole_use);

    let module_indexes = build_module_indexes(graph, modules);
    let global_used = collect_global_page_data_member_accesses(modules);
    let mut findings = Vec::new();
    if has_sveltekit && !global_abstain {
        findings.extend(collect_unused_load_data_key_findings(
            &LoadDataKeyScanInput {
                graph,
                module_indexes: &module_indexes,
                global_used: &global_used,
                root,
                suppressions,
                line_offsets_by_file,
            },
        ));
    }
    if has_route_loader_deps {
        findings.extend(collect_unused_route_loader_data_key_findings(
            &LoadDataKeyScanInput {
                graph,
                module_indexes: &module_indexes,
                global_used: &global_used,
                root,
                suppressions,
                line_offsets_by_file,
            },
        ));
    }

    LoadDataKeyResult {
        findings,
        global_abstain,
    }
}

fn collect_unused_route_loader_data_key_findings(
    input: &LoadDataKeyScanInput<'_>,
) -> Vec<UnusedLoadDataKey> {
    let mut findings = Vec::new();
    for node in &input.graph.modules {
        let Some(candidate) =
            route_loader_candidate_for_node(node, input.module_indexes, input.root)
        else {
            continue;
        };
        let ProducerCandidate {
            producer,
            file_id,
            producer_path,
            route_dir,
            route_used,
        } = candidate;
        let finding_input = ProducerFindingInput {
            producer,
            file_id,
            producer_path,
            route_dir,
            route_used: &route_used,
            suppressions: input.suppressions,
            line_offsets_by_file: input.line_offsets_by_file,
        };
        append_unused_keys_for_producer(&mut findings, &finding_input);
    }

    findings
}

fn route_loader_candidate_for_node<'a>(
    node: &ModuleNode,
    module_indexes: &ModuleIndexes<'a>,
    root: &Path,
) -> Option<ProducerCandidate<'a>> {
    let producer = module_indexes.module_by_id.get(&node.file_id).copied()?;
    if producer.load_return_keys.is_empty() || producer.has_unharvestable_load {
        return None;
    }
    if !is_conventional_route_loader_file(&node.path) {
        return None;
    }
    let route_used = collect_route_loader_used_keys(producer)?;
    let route_dir = node.path.parent();
    let producer_path = *module_indexes.path_by_id.get(&node.file_id)?;

    Some(ProducerCandidate {
        producer,
        file_id: node.file_id,
        producer_path,
        route_dir: route_dir.and_then(|dir| relativize_route_dir(dir, root)),
        route_used,
    })
}

fn collect_route_loader_used_keys(module: &ModuleInfo) -> Option<FxHashSet<&str>> {
    if module.has_route_loader_data_whole_use
        || module
            .whole_object_uses
            .iter()
            .any(|name| name == ROUTE_LOADER_DATA_OBJECT)
    {
        return None;
    }

    let mut used = FxHashSet::default();
    for access in &module.member_accesses {
        if access.object == ROUTE_LOADER_DATA_OBJECT || access.object == "loaderData" {
            used.insert(access.member.as_str());
        }
    }
    Some(used)
}

/// Inputs for the SvelteKit unused-load-data-key emit pass.
struct LoadDataKeyScanInput<'a> {
    graph: &'a ModuleGraph,
    module_indexes: &'a ModuleIndexes<'a>,
    global_used: &'a FxHashSet<&'a str>,
    root: &'a Path,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn collect_unused_load_data_key_findings(
    input: &LoadDataKeyScanInput<'_>,
) -> Vec<UnusedLoadDataKey> {
    let mut findings = Vec::new();
    for node in &input.graph.modules {
        let Some(candidate) =
            producer_candidate_for_node(node, input.module_indexes, input.global_used, input.root)
        else {
            continue;
        };

        let ProducerCandidate {
            producer,
            file_id,
            producer_path,
            route_dir,
            route_used,
        } = candidate;
        let finding_input = ProducerFindingInput {
            producer,
            file_id,
            producer_path,
            route_dir,
            route_used: &route_used,
            suppressions: input.suppressions,
            line_offsets_by_file: input.line_offsets_by_file,
        };
        append_unused_keys_for_producer(&mut findings, &finding_input);
    }

    findings
}

struct ProducerCandidate<'a> {
    producer: &'a ModuleInfo,
    file_id: FileId,
    producer_path: &'a Path,
    route_dir: Option<String>,
    route_used: FxHashSet<&'a str>,
}

fn producer_candidate_for_node<'a>(
    node: &ModuleNode,
    module_indexes: &ModuleIndexes<'a>,
    global_used: &FxHashSet<&'a str>,
    root: &Path,
) -> Option<ProducerCandidate<'a>> {
    let producer = module_indexes.module_by_id.get(&node.file_id).copied()?;
    if producer.load_return_keys.is_empty() || producer.has_unharvestable_load {
        return None;
    }
    if !is_page_load_producer(&node.path) {
        return None;
    }
    let route_dir = node.path.parent()?;
    let route_used = collect_route_used_keys(
        route_dir,
        &node.path,
        &module_indexes.module_by_path,
        global_used,
    )?;
    let producer_path = *module_indexes.path_by_id.get(&node.file_id)?;

    Some(ProducerCandidate {
        producer,
        file_id: node.file_id,
        producer_path,
        route_dir: relativize_route_dir(route_dir, root),
        route_used,
    })
}

fn empty_result() -> LoadDataKeyResult {
    LoadDataKeyResult {
        findings: Vec::new(),
        global_abstain: false,
    }
}

struct ModuleIndexes<'a> {
    /// Stable file identity -> module facts. Sparse when a source read failed.
    module_by_id: FxHashMap<FileId, &'a ModuleInfo>,
    /// Path -> ModuleInfo for sibling lookups, keyed by absolute path.
    module_by_path: FxHashMap<&'a Path, &'a ModuleInfo>,
    path_by_id: FxHashMap<FileId, &'a Path>,
}

fn build_module_indexes<'a>(
    graph: &'a ModuleGraph,
    modules: &'a [ModuleInfo],
) -> ModuleIndexes<'a> {
    let module_by_id: FxHashMap<FileId, &ModuleInfo> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    ModuleIndexes {
        module_by_path: graph
            .modules
            .iter()
            .filter_map(|node| {
                let module = module_by_id.get(&node.file_id).copied()?;
                Some((node.path.as_path(), module))
            })
            .collect(),
        module_by_id,
        path_by_id: graph
            .modules
            .iter()
            .map(|node| (node.file_id, node.path.as_path()))
            .collect(),
    }
}

fn collect_global_page_data_member_accesses(modules: &[ModuleInfo]) -> FxHashSet<&str> {
    // Channel 2/3 (project-wide): collect every `page.data.<key>` /
    // `$page.data.<key>` member access ONCE across all modules. The captured
    // object is already `page.data` (Svelte 5) or `$page.data` (Svelte 4); both
    // unify on the bare member name.
    let mut global_used: FxHashSet<&str> = FxHashSet::default();
    for module in modules {
        for access in &module.member_accesses {
            if access.object == "page.data" || access.object == "$page.data" {
                global_used.insert(access.member.as_str());
            }
        }
    }
    global_used
}

fn collect_route_used_keys<'a>(
    route_dir: &Path,
    producer_path: &Path,
    module_by_path: &FxHashMap<&Path, &'a ModuleInfo>,
    global_used: &FxHashSet<&'a str>,
) -> Option<FxHashSet<&'a str>> {
    // Route-pinned consumer channel (1): the sibling `+page.svelte`.
    let svelte_sibling = module_by_path
        .get(route_dir.join("+page.svelte").as_path())
        .copied();

    // FP-1 / ladder (i): the sibling passes the whole `data` opaquely.
    if let Some(sibling) = svelte_sibling
        && sibling_passes_whole_data(sibling)
    {
        return None;
    }

    // Collect the per-route used set: channel 1 (sibling `data.<key>`)
    // unioned with the project-wide channel 2/3.
    let mut route_used = global_used.clone();
    if let Some(sibling) = svelte_sibling {
        collect_data_member_accesses(sibling, &mut route_used);
    }

    // FP-2: a server producer's keys can be consumed by a sibling universal
    // load that reads / forwards its `data` param. Credit the universal
    // sibling's `data.<key>` accesses, and abstain wholesale if the universal
    // load forwards `data` opaquely.
    if is_server_load_producer(producer_path) {
        collect_universal_load_used_keys(route_dir, module_by_path, &mut route_used)?;
    }

    Some(route_used)
}

fn collect_universal_load_used_keys<'a>(
    route_dir: &Path,
    module_by_path: &FxHashMap<&Path, &'a ModuleInfo>,
    route_used: &mut FxHashSet<&'a str>,
) -> Option<()> {
    for universal_name in UNIVERSAL_LOAD_NAMES {
        let Some(universal) = module_by_path
            .get(route_dir.join(universal_name).as_path())
            .copied()
        else {
            continue;
        };
        if sibling_passes_whole_data(universal) {
            return None;
        }
        collect_data_member_accesses(universal, route_used);
    }

    Some(())
}

struct ProducerFindingInput<'a> {
    producer: &'a ModuleInfo,
    file_id: FileId,
    producer_path: &'a Path,
    route_dir: Option<String>,
    route_used: &'a FxHashSet<&'a str>,
    suppressions: &'a SuppressionContext<'a>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

fn append_unused_keys_for_producer(
    findings: &mut Vec<UnusedLoadDataKey>,
    input: &ProducerFindingInput<'_>,
) {
    for key in &input.producer.load_return_keys {
        if input.route_used.contains(key.name.as_str()) {
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(input.line_offsets_by_file, input.file_id, key.span_start);
        if input
            .suppressions
            .is_suppressed(input.file_id, line, IssueKind::UnusedLoadDataKey)
            || input
                .suppressions
                .is_file_suppressed(input.file_id, IssueKind::UnusedLoadDataKey)
        {
            continue;
        }
        findings.push(UnusedLoadDataKey {
            path: input.producer_path.to_path_buf(),
            key_name: key.name.clone(),
            line,
            col,
            route_dir: input.route_dir.clone(),
        });
    }
}

/// Whether a consumer SFC passes the whole `data` binding opaquely (so a child
/// can read arbitrary keys the detector cannot see). Uses the persisted
/// extraction FP-1 flag `has_load_data_whole_use` (`data={data}`, `{...data}`,
/// `fn(data)`, `const X = data`, plus the script spread / rest forms captured by
/// Primitive A), which already covers every whole-`data` shape; the raw
/// `whole_object_uses` vector is released before the detector runs, so it is not
/// consulted here.
fn sibling_passes_whole_data(module: &ModuleInfo) -> bool {
    module.has_load_data_whole_use
}

/// Credit every `data.<key>` member access on a consumer SFC into `used`.
fn collect_data_member_accesses<'a>(module: &'a ModuleInfo, used: &mut FxHashSet<&'a str>) {
    // Read the sibling's `data.<key>` reads from the raw `ModuleInfo` extraction
    // (complete by construction), NOT the resolved payload: a reachable route
    // `+page.svelte`'s file_id is not guaranteed to be in the `resolved_modules`
    // index, and `data` is never graph-narrowed (it is a prop, not an import), so
    // the resolved indirection only risked dropping a real consumer read.
    for access in &module.member_accesses {
        if access.object == "data" {
            used.insert(access.member.as_str());
        }
    }
}

/// Whether the file is a SvelteKit page-load producer (cut A).
fn is_page_load_producer(path: &Path) -> bool {
    matches_basename(path, PAGE_LOAD_PRODUCER_NAMES)
}

/// Whether the file is a SvelteKit SERVER page-load producer.
fn is_server_load_producer(path: &Path) -> bool {
    matches_basename(path, SERVER_LOAD_PRODUCER_NAMES)
}

fn is_conventional_route_loader_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('+') {
        return false;
    }
    if !matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ts" | "tsx" | "js" | "jsx")
    ) {
        return false;
    }
    if matches!(name, "root.ts" | "root.tsx" | "root.js" | "root.jsx")
        && path
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|part| part.to_str())
            .is_some_and(|part| matches!(part, "app" | "src"))
    {
        return true;
    }
    path_has_route_dir(path, "app") || path_has_route_dir(path, "src")
}

fn path_has_route_dir(path: &Path, app_dir: &str) -> bool {
    let mut previous = None;
    for part in path.components().filter_map(|c| c.as_os_str().to_str()) {
        if previous == Some(app_dir) && part == "routes" {
            return true;
        }
        previous = Some(part);
    }
    false
}

fn matches_basename(path: &Path, names: &[&str]) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| names.contains(&name))
}

/// The route directory relative to the project root (`src/routes/blog`), with
/// forward slashes for cross-platform stability. `None` when the route dir is
/// not under `root` (defensive; route files always are in practice).
fn relativize_route_dir(absolute_route_dir: &Path, root: &Path) -> Option<String> {
    absolute_route_dir
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::discover::{DiscoveredFile, EntryPoint, FileId};
    use fallow_types::extract::{LoadReturnKey, MemberAccess};
    use rustc_hash::{FxHashMap, FxHashSet};

    use crate::analyze::test_support::empty_module;
    use crate::graph::ModuleGraph;
    use crate::resolve::ResolvedModule;
    use crate::suppress::SuppressionContext;

    use super::{ROUTE_LOADER_DATA_OBJECT, find_unused_load_data_keys};

    fn graph_for_paths(paths: &[PathBuf]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = paths
            .iter()
            .enumerate()
            .map(|(idx, path)| DiscoveredFile {
                id: FileId(u32::try_from(idx).expect("test file count fits u32")),
                path: path.clone(),
                size_bytes: 0,
            })
            .collect();
        let resolved: Vec<ResolvedModule> = files
            .iter()
            .map(|file| ResolvedModule {
                file_id: file.id,
                path: file.path.clone(),
                ..Default::default()
            })
            .collect();
        let entry_points: Vec<EntryPoint> = Vec::new();
        ModuleGraph::build(&resolved, &entry_points, &files)
    }

    #[test]
    fn sveltekit_global_abstain_does_not_skip_route_loader_branch() {
        let root = Path::new("/repo");
        let global_path = root.join("src/lib/global.ts");
        let route_path = root.join("app/routes/home.tsx");
        let graph = graph_for_paths(&[global_path, route_path]);

        let mut global = empty_module();
        global.file_id = FileId(0);
        global.has_page_data_store_whole_use = true;

        let mut route = empty_module();
        route.file_id = FileId(1);
        route.load_return_keys = vec![
            LoadReturnKey {
                name: "used".to_string(),
                span_start: 0,
                span_end: 4,
            },
            LoadReturnKey {
                name: "dead".to_string(),
                span_start: 6,
                span_end: 10,
            },
        ];
        route.member_accesses = vec![MemberAccess {
            object: ROUTE_LOADER_DATA_OBJECT.to_string(),
            member: "used".to_string(),
        }];

        let mut declared_deps = FxHashSet::default();
        declared_deps.insert("@sveltejs/kit".to_string());
        declared_deps.insert("react-router".to_string());
        let line_offsets = FxHashMap::default();

        let result = find_unused_load_data_keys(
            &graph,
            &[global, route],
            &declared_deps,
            &SuppressionContext::empty(),
            &line_offsets,
            root,
        );

        let keys: Vec<&str> = result
            .findings
            .iter()
            .map(|finding| finding.key_name.as_str())
            .collect();
        assert!(result.global_abstain);
        assert_eq!(keys, vec!["dead"]);
    }

    #[test]
    fn released_route_loader_whole_use_still_abstains() {
        let root = Path::new("/repo");
        let route_path = root.join("app/routes/home.tsx");
        let graph = graph_for_paths(std::slice::from_ref(&route_path));

        let mut route = empty_module();
        route.file_id = FileId(0);
        route.load_return_keys = vec![LoadReturnKey {
            name: "opaque".to_string(),
            span_start: 0,
            span_end: 6,
        }];
        route.whole_object_uses = vec![ROUTE_LOADER_DATA_OBJECT.to_string()].into();
        route.release_resolution_payload();

        let mut declared_deps = FxHashSet::default();
        declared_deps.insert("react-router".to_string());
        let result = find_unused_load_data_keys(
            &graph,
            &[route],
            &declared_deps,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            root,
        );

        assert!(
            result.findings.is_empty(),
            "an opaque route-loader use must abstain after resolution payload release"
        );
    }

    #[test]
    fn sparse_module_ids_do_not_reassign_loader_keys_to_missing_file() {
        let root = Path::new("/repo");
        let first_path = root.join("app/routes/first.tsx");
        let missing_path = root.join("app/routes/missing.tsx");
        let producer_path = root.join("app/routes/producer.tsx");
        let graph = graph_for_paths(&[first_path, missing_path.clone(), producer_path.clone()]);

        let mut first = empty_module();
        first.file_id = FileId(0);
        let mut producer = empty_module();
        producer.file_id = FileId(2);
        producer.load_return_keys = vec![LoadReturnKey {
            name: "dead".to_string(),
            span_start: 0,
            span_end: 4,
        }];

        let mut declared_deps = FxHashSet::default();
        declared_deps.insert("react-router".to_string());
        let result = find_unused_load_data_keys(
            &graph,
            &[first, producer],
            &declared_deps,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            root,
        );

        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].path, producer_path);
        assert_ne!(result.findings[0].path, missing_path);
    }
}
