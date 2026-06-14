//! Detection of Next.js App Router dynamic-segment name conflicts.
//!
//! Two or more sibling dynamic route segments at the SAME tree position using
//! different param spellings (`[id]` vs `[slug]`, or a catch-all `[...x]` vs an
//! optional catch-all `[[...x]]`). Next.js throws "You cannot use different slug
//! names for the same dynamic path" when the router resolves a request to that
//! position. IMPORTANT: this is a dev / production RUNTIME error, NOT a build
//! error: verified against Next 15.5.0, `next build` SUCCEEDS on this shape, so
//! CI stays green while the route crashes the first time it is hit. That is
//! exactly why a static catch is valuable.
//!
//! A "position" is the parent URL a dynamic segment is a direct child of, with
//! route groups stripped and parallel slots forked (so two dynamic segments in
//! different `@slot` subtrees are different positions and never conflict). The
//! shared [`super::route_tree`] primitive enumerates each route file's dynamic
//! occurrences; this detector groups them by `(app_root, slot_key, position)`
//! and flags any position carrying more than one distinct spelling.
//!
//! Unlike route-collision, EVERY App Router convention file participates (not
//! just `page` / `route` leaves): a `layout.tsx` under `[id]` proves the `[id]`
//! dynamic directory exists, so it conflicts with a sibling `[slug]` even if the
//! `[id]` directory has no page of its own.
//!
//! Gated on the project declaring `next`.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{ResolvedConfig, WorkspaceInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::DynamicSegmentNameConflict;
use crate::suppress::{IssueKind, SuppressionContext};

use super::route_tree::classify_route_file;

/// The position key: `(app_root, slot_key, parent position URL)`.
type PositionKey = (String, String, String);

/// One dynamic-segment occurrence: its written spelling and the file it came
/// from.
struct Occurrence<'a> {
    spelling: String,
    file_id: FileId,
    path: &'a Path,
}

/// Find sibling dynamic segments at one App Router tree position that use
/// different param spellings.
///
/// Returns empty unless the project declares `next`. Emits one
/// [`DynamicSegmentNameConflict`] per involved file; `conflicting_segments`
/// lists the distinct spellings at the position and `conflicting_paths` lists
/// the other involved files. A file carrying a
/// `// fallow-ignore-file dynamic-segment-name-conflict` marker is skipped via
/// [`SuppressionContext`].
#[must_use]
pub fn find_dynamic_segment_name_conflicts(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
) -> Vec<DynamicSegmentNameConflict> {
    if !declared_deps.contains("next") {
        return Vec::new();
    }

    let pkg_roots = collect_pkg_roots(config, workspaces);
    let pkg_root_refs: Vec<&Path> = pkg_roots.iter().map(PathBuf::as_path).collect();

    let mut by_position: FxHashMap<PositionKey, Vec<Occurrence<'_>>> = FxHashMap::default();
    for module in &graph.modules {
        let Some(classified) = classify_route_file(module.path.as_path(), &pkg_root_refs) else {
            continue;
        };
        for occ in classified.dynamic_occurrences() {
            let key = (classified.app_root.clone(), occ.slot_key, occ.position);
            by_position.entry(key).or_default().push(Occurrence {
                spelling: occ.spelling,
                file_id: module.file_id,
                path: module.path.as_path(),
            });
        }
    }

    let mut findings = Vec::new();
    for ((_, _, position), occs) in by_position {
        // A position conflicts only when more than one distinct spelling appears.
        let mut distinct: Vec<&str> = occs.iter().map(|o| o.spelling.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() < 2 {
            continue;
        }
        let conflicting_segments: Vec<String> = distinct.iter().map(|s| (*s).to_string()).collect();

        // Dedupe involved files by path (a file has one occurrence per
        // position), path-sorted for stable output.
        let mut involved: Vec<(FileId, &Path)> = occs.iter().map(|o| (o.file_id, o.path)).collect();
        involved.sort_by(|a, b| a.1.cmp(b.1));
        involved.dedup_by(|a, b| a.1 == b.1);

        for &(file_id, path) in &involved {
            if suppressions.is_file_suppressed(file_id, IssueKind::DynamicSegmentNameConflict) {
                continue;
            }
            let conflicting_paths: Vec<PathBuf> = involved
                .iter()
                .filter(|(_, p)| *p != path)
                .map(|(_, p)| p.to_path_buf())
                .collect();
            findings.push(DynamicSegmentNameConflict {
                path: path.to_path_buf(),
                position: position.clone(),
                conflicting_segments: conflicting_segments.clone(),
                conflicting_paths,
                line: 1,
                col: 0,
            });
        }
    }

    findings
}

/// Package roots to anchor app-roots on: the project root plus every discovered
/// workspace package root.
fn collect_pkg_roots(config: &ResolvedConfig, workspaces: &[WorkspaceInfo]) -> Vec<PathBuf> {
    let mut roots = Vec::with_capacity(workspaces.len() + 1);
    roots.push(config.root.clone());
    roots.extend(workspaces.iter().map(|w| w.root.clone()));
    roots
}
