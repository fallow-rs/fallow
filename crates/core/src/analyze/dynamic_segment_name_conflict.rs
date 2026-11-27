//! Detection of Next.js App Router dynamic-segment name conflicts.
//!
//! The Next.js router keeps three dynamic slots at each tree position: one for
//! a single segment (`[id]`), one for a catch-all (`[...rest]`), and one for an
//! optional catch-all (`[[...rest]]`). A position conflicts when one slot holds
//! two param names (`[id]` vs `[slug]`, `[...a]` vs `[...b]`), or when a
//! catch-all and an optional catch-all are siblings. Next.js then throws "You
//! cannot use different slug names for the same dynamic path" or "You cannot
//! use both an required and optional catch-all route at the same level". A
//! single segment next to a catch-all (`[slug]` and `[...rest]`) is valid,
//! because each one fills its own slot.
//!
//! IMPORTANT: this is a dev / production RUNTIME error, NOT a build
//! error: verified against Next 15.5.0, `next build` SUCCEEDS on this shape, so
//! CI stays green while the route crashes the first time it is hit. That is
//! exactly why a static catch is valuable.
//!
//! A "position" is the parent URL a dynamic segment is a direct child of, with
//! route groups stripped and parallel slots forked (so two dynamic segments in
//! different `@slot` subtrees are different positions and never conflict). The
//! shared [`super::route_tree`] primitive enumerates each route file's dynamic
//! occurrences; this detector groups them by `(app_root, slot_key, position)`
//! and flags the occurrences that break the slot rules above.
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

use super::route_tree::{DynKind, classify_route_file, collect_pkg_roots};

/// The position key: `(app_root, slot_key, parent position URL)`.
type PositionKey = (String, String, String);

/// One dynamic-segment occurrence: its written spelling and the file it came
/// from.
struct Occurrence<'a> {
    spelling: String,
    kind: DynKind,
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
    let by_position = collect_occurrences_by_position(graph, &pkg_root_refs);

    let mut findings = Vec::new();
    for ((_, _, position), occs) in by_position {
        append_conflicts_for_position(&mut findings, &position, &occs, suppressions);
    }

    findings
}

fn collect_occurrences_by_position<'a>(
    graph: &'a ModuleGraph,
    pkg_root_refs: &[&Path],
) -> FxHashMap<PositionKey, Vec<Occurrence<'a>>> {
    let mut by_position: FxHashMap<PositionKey, Vec<Occurrence<'_>>> = FxHashMap::default();
    for module in graph.modules() {
        let Some(classified) = classify_route_file(module.path.as_path(), pkg_root_refs) else {
            continue;
        };
        for occ in classified.dynamic_occurrences() {
            let key = (classified.app_root.clone(), occ.slot_key, occ.position);
            by_position.entry(key).or_default().push(Occurrence {
                spelling: occ.spelling,
                kind: occ.kind,
                file_id: module.file_id,
                path: module.path.as_path(),
            });
        }
    }
    by_position
}

fn append_conflicts_for_position(
    findings: &mut Vec<DynamicSegmentNameConflict>,
    position: &str,
    occs: &[Occurrence<'_>],
    suppressions: &SuppressionContext<'_>,
) {
    let conflicting = conflicting_spellings(occs);
    if conflicting.is_empty() {
        return;
    }
    let conflicting_segments: Vec<String> = conflicting.iter().map(|s| (*s).to_string()).collect();

    // Dedupe involved files by path (a file has one occurrence per position),
    // path-sorted for stable output.
    let mut involved: Vec<(FileId, &Path)> = occs
        .iter()
        .filter(|o| conflicting.contains(&o.spelling.as_str()))
        .map(|o| (o.file_id, o.path))
        .collect();
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
            position: position.to_string(),
            conflicting_segments: conflicting_segments.clone(),
            conflicting_paths,
            line: 1,
            col: 0,
        });
    }
}

/// The sorted, distinct spellings at one position that break the Next.js
/// router slot rules: every spelling of a slot that holds more than one name,
/// plus every catch-all and optional catch-all spelling when both kinds are
/// present. Empty when the position is valid.
fn conflicting_spellings<'o>(occs: &'o [Occurrence<'_>]) -> Vec<&'o str> {
    let spellings_of = |kind: DynKind| -> Vec<&'o str> {
        let mut spellings: Vec<&str> = occs
            .iter()
            .filter(|o| o.kind == kind)
            .map(|o| o.spelling.as_str())
            .collect();
        spellings.sort_unstable();
        spellings.dedup();
        spellings
    };
    let single = spellings_of(DynKind::Required);
    let catch_all = spellings_of(DynKind::CatchAll);
    let optional = spellings_of(DynKind::OptionalCatchAll);
    let mixed_catch_all = !catch_all.is_empty() && !optional.is_empty();

    let mut conflicting = Vec::new();
    for (spellings, conflicts) in [
        (&single, single.len() > 1),
        (&catch_all, catch_all.len() > 1 || mixed_catch_all),
        (&optional, optional.len() > 1 || mixed_catch_all),
    ] {
        if conflicts {
            conflicting.extend_from_slice(spellings);
        }
    }
    conflicting.sort_unstable();
    conflicting
}
