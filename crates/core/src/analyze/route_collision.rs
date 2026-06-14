//! Detection of Next.js App Router route collisions.
//!
//! Two or more route files (`page` / `route`) that resolve to the SAME URL
//! within one app-root are a guaranteed `next build` failure ("You cannot have
//! two parallel pages that resolve to the same path"). A URL has at most one
//! owner, whether a Page or a Route Handler, so `page` and `route` files share
//! one URL-owner namespace: two pages, two route handlers, or a page + a route
//! at the same URL all collide.
//!
//! The collision key is `(app_root, slot_key, url)` computed by the shared
//! [`super::route_tree`] primitive: route groups `(x)` are stripped from the
//! URL, parallel `@slot` segments fork into `slot_key` (so two leaves in
//! different slots never collide), and dynamic segments keep their written name
//! (so `[id]` vs `[slug]` is the dynamic-segment-name-conflict detector's job,
//! not a collision). Files under a private `_folder` or an intercepting marker
//! are excluded by the primitive.
//!
//! Per-app-root scoping is the load-bearing false-positive gate: a Turborepo /
//! Nx monorepo where `apps/web/app/about/page.tsx` and
//! `apps/admin/app/about/page.tsx` both resolve to `/about` is NOT a collision
//! (independent apps, separate builds). The app-root is anchored on discovered
//! workspace package roots, never a bare `app` dirname.
//!
//! Gated on the project declaring `next`: without Next.js the App Router file
//! conventions carry no meaning, so firing would be a false positive.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{ResolvedConfig, WorkspaceInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::RouteCollision;
use crate::suppress::{IssueKind, SuppressionContext};

use super::route_tree::classify_route_file;

/// The collision bucket key: `(app_root, slot_key, url)`.
type BucketKey = (String, String, String);

/// Find App Router route files that resolve to the same URL within one app-root.
///
/// Returns empty unless the project declares `next`. Emits one
/// [`RouteCollision`] per colliding file; `conflicting_paths` lists the other
/// files that own the same URL (path-sorted). A file carrying a
/// `// fallow-ignore-file route-collision` marker is skipped via
/// [`SuppressionContext`], but it still appears in its siblings'
/// `conflicting_paths` because the collision is real regardless of suppression.
#[must_use]
pub fn find_route_collisions(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
) -> Vec<RouteCollision> {
    if !declared_deps.contains("next") {
        return Vec::new();
    }

    let pkg_roots = collect_pkg_roots(config, workspaces);
    let pkg_root_refs: Vec<&Path> = pkg_roots.iter().map(PathBuf::as_path).collect();

    let mut buckets: FxHashMap<BucketKey, Vec<(FileId, &Path)>> = FxHashMap::default();
    for module in &graph.modules {
        let Some(classified) = classify_route_file(module.path.as_path(), &pkg_root_refs) else {
            continue;
        };
        if !classified.is_url_leaf() {
            continue;
        }
        let key = classified.collision_bucket();
        buckets
            .entry(key)
            .or_default()
            .push((module.file_id, module.path.as_path()));
    }

    let mut findings = Vec::new();
    for ((_, _, url), mut files) in buckets {
        if files.len() < 2 {
            continue;
        }
        // Path-sort for stable `conflicting_paths` / fingerprints (ADR-004).
        files.sort_by(|a, b| a.1.cmp(b.1));
        for &(file_id, path) in &files {
            if suppressions.is_file_suppressed(file_id, IssueKind::RouteCollision) {
                continue;
            }
            let conflicting_paths: Vec<PathBuf> = files
                .iter()
                .filter(|(_, p)| *p != path)
                .map(|(_, p)| p.to_path_buf())
                .collect();
            findings.push(RouteCollision {
                path: path.to_path_buf(),
                url: url.clone(),
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
