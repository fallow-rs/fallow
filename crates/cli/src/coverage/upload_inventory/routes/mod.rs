//! Static entry routes for the coverage inventory upload.
//!
//! A framework plugin marks route files as entry points. This module turns the
//! path of each route file into a route pattern, and walks the module graph
//! forward from each route file. A function then carries the ids of the routes
//! whose files can statically reach the file of the function.
//!
//! Two route sources read the source code instead of the path. A TanStack
//! Router route file takes its pattern from the `createFileRoute` argument. An
//! Elysia app defines its routes in code, so the handler file of each
//! `app.get("/path", handler)` call is the route file.
//!
//! The result is "can reach", not "did reach". A pattern comes from the file
//! name or the source code only, never from a request path. Reach is
//! file-level: an import of one symbol from a file reaches every function in
//! that file. Type-only imports do not reach, because they cannot run code.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use colored::Colorize as _;
use fallow_engine::code_routes::CodeRoute;
use fallow_engine::discover::{EntryPoint, EntryPointSource};
use rustc_hash::FxHashMap;
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{InventoryFunction, LOG_PREFIX};
use crate::coverage::upload_common::{format_count, to_posix_string};

/// Cap on the routes in one upload. Routes past the cap, in pattern order, are
/// dropped and counted in [`RouteLimits::dropped_routes`].
pub(super) const MAX_ROUTES: usize = 500;

/// Cap on the route ids of one function. A function that more routes reach
/// keeps the first ids in route order and counts in
/// [`RouteLimits::truncated_functions`].
pub(super) const MAX_ROUTES_PER_FUNCTION: usize = 50;

/// Cap on the route ids of all functions together, so a large project with
/// many shared files cannot grow the body past the server limit. When the ids
/// do not fit, the per-function cap goes down until they fit, and the header
/// reports the cap that applied.
pub(super) const MAX_ROUTE_ID_REFS: usize = 250_000;

/// Cap on the files of one route (parallel slots and page plus server files
/// that serve one pattern).
const MAX_FILES_PER_ROUTE: usize = 20;

/// HTTP method exports of a route handler file (Next.js `route.ts`, SvelteKit
/// `+server.ts`). Each export gives one route with that method.
const HTTP_METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

/// HTTP method suffixes of a Nuxt server file (`users.get.ts`).
const NUXT_METHOD_SUFFIXES: &[(&str, &str)] = &[
    ("get", "GET"),
    ("head", "HEAD"),
    ("post", "POST"),
    ("put", "PUT"),
    ("patch", "PATCH"),
    ("delete", "DELETE"),
    ("options", "OPTIONS"),
];

/// Script extensions a route file can have.
const SCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts"];

/// Next.js page extensions: the script extensions plus the content extensions.
const NEXT_PAGE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "mdx", "md",
];

/// Plugin name of TanStack Router. Its route files take their pattern from the
/// `createFileRoute` argument, not from the file name.
const TANSTACK_ROUTER: &str = "tanstack-router";

/// Framework name of the routes that Elysia code defines.
const ELYSIA: &str = "elysia";

/// How a derived route gets its HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MethodRule {
    /// The route serves any method (a page, or a handler without a method split).
    Any,
    /// One route per exported HTTP method name; `Any` when the file exports none.
    FromExports,
    /// The file name fixes the method.
    Fixed(&'static str),
}

/// A route that a file path gives, before the method split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DerivedRoute {
    pub(super) framework: &'static str,
    pub(super) pattern: String,
    pub(super) methods: MethodRule,
}

/// Wire form of one route.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct RoutePayload {
    pub(super) id: String,
    pub(super) pattern: String,
    /// `null` when the route serves any method.
    pub(super) method: Option<&'static str>,
    /// Repo-relative posix path of the primary route file.
    pub(super) file: String,
    /// Every route file that serves this pattern and method, `file` first.
    pub(super) files: Vec<String>,
    pub(super) framework: &'static str,
}

/// Wire form of the route size guard.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(super) struct RouteLimits {
    #[serde(rename = "maxRoutes")]
    pub(super) max_routes: u32,
    #[serde(rename = "maxRoutesPerFunction")]
    pub(super) max_routes_per_function: u32,
    #[serde(rename = "droppedRoutes")]
    pub(super) dropped_routes: u32,
    #[serde(rename = "truncatedFunctions")]
    pub(super) truncated_functions: u32,
}

/// The routes of one upload plus the bookkeeping of the size guard.
#[derive(Debug, Default)]
pub(super) struct InventoryRoutes {
    pub(super) routes: Vec<RoutePayload>,
    pub(super) dropped_routes: usize,
    pub(super) truncated_functions: usize,
    /// The per-function cap that applied, after the total budget.
    pub(super) max_routes_per_function: usize,
}

impl InventoryRoutes {
    /// Blob header for the routes, or `None` when there are no routes.
    pub(super) fn limits(&self) -> Option<RouteLimits> {
        if self.routes.is_empty() {
            return None;
        }
        Some(RouteLimits {
            max_routes: to_u32(MAX_ROUTES),
            max_routes_per_function: to_u32(self.max_routes_per_function),
            dropped_routes: to_u32(self.dropped_routes),
            truncated_functions: to_u32(self.truncated_functions),
        })
    }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A route candidate bound to a graph file index. Its id hashes the package
/// root, so two apps of a monorepo with one pattern stay two routes.
#[derive(Debug, Clone)]
pub(super) struct RouteCandidate {
    pub(super) file_index: usize,
    pub(super) payload: RoutePayload,
}

/// One merged route: every candidate file with one framework, root, pattern
/// and method.
struct MergedRoute {
    file_indices: Vec<usize>,
    payload: RoutePayload,
}

/// Derive the route of a framework route file from its path relative to its
/// package root. `None` when the file is not a URL owner of `plugin` (a layout,
/// a middleware, a config file, a private folder).
pub(super) fn derive_route(plugin: &str, rel: &str) -> Option<DerivedRoute> {
    let parts: Vec<&str> = rel.split('/').collect();
    let (dirs, file) = parts.split_at(parts.len().checked_sub(1)?);
    let file = *file.first()?;
    match plugin {
        "nextjs" => derive_next(dirs, file),
        "remix" => derive_remix(dirs, file),
        "sveltekit" => derive_sveltekit(dirs, file),
        "nuxt" => derive_nuxt(dirs, file),
        _ => None,
    }
}

fn split_ext<'a>(file: &'a str, allowed: &[&str]) -> Option<&'a str> {
    let (stem, ext) = file.rsplit_once('.')?;
    allowed.contains(&ext).then_some(stem)
}

fn strip_root<'a, 'b>(dirs: &'a [&'b str], roots: &[&[&'static str]]) -> Option<&'a [&'b str]> {
    roots.iter().find_map(|root| dirs.strip_prefix(*root))
}

fn join_pattern<'a>(segments: impl IntoIterator<Item = &'a str>) -> String {
    let parts: Vec<&str> = segments.into_iter().collect();
    format!("/{}", parts.join("/"))
}

const fn route(framework: &'static str, pattern: String, methods: MethodRule) -> DerivedRoute {
    DerivedRoute {
        framework,
        pattern,
        methods,
    }
}

/// True when `stem` has a `.` outside `[...]` brackets. A catch-all page
/// (`[...slug]`, `[[...slug]]`) has dots only inside its brackets.
fn has_dot_outside_brackets(stem: &str) -> bool {
    let mut depth = 0_usize;
    stem.chars().any(|ch| {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '.' => return depth == 0,
            _ => {}
        }
        false
    })
}

fn is_group(seg: &str) -> bool {
    seg.len() > 2 && seg.starts_with('(') && seg.ends_with(')')
}

fn derive_next(dirs: &[&str], file: &str) -> Option<DerivedRoute> {
    if let Some(rest) = strip_root(dirs, &[&["app"], &["src", "app"]]) {
        let methods = match split_ext(file, NEXT_PAGE_EXTENSIONS)? {
            "page" => MethodRule::Any,
            "route" => MethodRule::FromExports,
            _ => return None,
        };
        let intercepting = |seg: &&str| seg.starts_with("(.)") || seg.starts_with("(..)");
        if rest
            .iter()
            .any(|seg| seg.starts_with('_') || intercepting(seg))
        {
            return None;
        }
        let url = rest
            .iter()
            .copied()
            .filter(|seg| !is_group(seg) && !seg.starts_with('@'));
        return Some(route("nextjs", join_pattern(url), methods));
    }
    let rest = strip_root(dirs, &[&["pages"], &["src", "pages"]])?;
    let stem = split_ext(file, NEXT_PAGE_EXTENSIONS)?;
    // `_app`, `_document` and `_error` are not pages; a dotted stem is a test or
    // a colocated module, not a page.
    if stem.starts_with('_') || has_dot_outside_brackets(stem) {
        return None;
    }
    let leaf = (stem != "index").then_some(stem);
    let url = rest.iter().copied().chain(leaf);
    Some(route("nextjs", join_pattern(url), MethodRule::Any))
}

fn derive_remix(dirs: &[&str], file: &str) -> Option<DerivedRoute> {
    let rest = strip_root(dirs, &[&["app", "routes"]])?;
    let name = match rest {
        [] => split_ext(file, SCRIPT_EXTENSIONS)?,
        [folder] if split_ext(file, SCRIPT_EXTENSIONS)? == "route" => folder,
        _ => return None,
    };
    let segments = split_remix_name(name);
    // A name that ends in a pathless segment (`_auth`) is a layout, not a URL.
    let last = segments.last()?;
    if last.starts_with('_') && last != "_index" {
        return None;
    }
    let mut url: Vec<String> = Vec::new();
    for seg in segments {
        // `_index` is the index route; a leading `_` is a pathless layout.
        if seg.starts_with('_') && !seg.starts_with("[_") {
            continue;
        }
        let seg = seg.strip_suffix('_').unwrap_or(&seg);
        url.push(seg.replace(['[', ']'], ""));
    }
    Some(route(
        "remix",
        join_pattern(url.iter().map(String::as_str)),
        MethodRule::Any,
    ))
}

/// Split a Remix flat-route name on `.`, keeping a `.` inside `[...]` escapes.
fn split_remix_name(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in name.chars() {
        match ch {
            '[' => escaped = true,
            ']' => escaped = false,
            '.' if !escaped => {
                out.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(ch);
    }
    out.push(current);
    out
}

fn derive_sveltekit(dirs: &[&str], file: &str) -> Option<DerivedRoute> {
    let rest = strip_root(dirs, &[&["src", "routes"]])?;
    let (stem, ext) = file.rsplit_once('.')?;
    let methods = match (stem, ext) {
        ("+server", "ts" | "js") => MethodRule::FromExports,
        ("+page" | "+page.server", "ts" | "js") => MethodRule::Any,
        (stem, "svelte") if stem == "+page" || stem.starts_with("+page@") => MethodRule::Any,
        _ => return None,
    };
    let url = rest.iter().copied().filter(|seg| !is_group(seg));
    Some(route("sveltekit", join_pattern(url), methods))
}

fn derive_nuxt(dirs: &[&str], file: &str) -> Option<DerivedRoute> {
    if let Some(rest) = strip_root(dirs, &[&["pages"], &["app", "pages"]]) {
        let stem = split_ext(file, &["vue", "ts", "tsx", "js", "jsx"])?;
        if has_dot_outside_brackets(stem) {
            return None;
        }
        let leaf = (stem != "index").then_some(stem);
        let url = rest
            .iter()
            .copied()
            .filter(|seg| !is_group(seg))
            .chain(leaf);
        return Some(route("nuxt", join_pattern(url), MethodRule::Any));
    }
    let (base, rest): (&[&str], _) = if let Some(rest) = dirs.strip_prefix(&["server", "api"]) {
        (&["api"], rest)
    } else {
        (&[], dirs.strip_prefix(&["server", "routes"])?)
    };
    let stem = split_ext(file, &["ts", "js"])?;
    // `users.get.ts` fixes the method; any other dot stays in the pattern
    // (`feed.xml.ts` serves `/feed.xml`).
    let method_suffix = stem.rsplit_once('.').and_then(|(name, suffix)| {
        NUXT_METHOD_SUFFIXES
            .iter()
            .find(|(s, _)| *s == suffix)
            .map(|(_, method)| (name, MethodRule::Fixed(method)))
    });
    let (stem, methods) = method_suffix.unwrap_or((stem, MethodRule::Any));
    let leaf = (stem != "index").then_some(stem);
    let url = base.iter().chain(rest).copied().chain(leaf);
    Some(route("nuxt", join_pattern(url), methods))
}

/// The URL pattern of a TanStack Router route id (the `createFileRoute`
/// argument). `None` for a pathless layout route (`/_auth`).
///
/// A pathless segment (`_auth`) and a route group (`(group)`) add no URL
/// segment. A trailing `_` (`posts_`) only opts out of the parent layout. A
/// trailing `/` marks the index route of its parent, also under a pathless
/// layout (`/_auth/` serves `/`).
pub(super) fn tanstack_pattern(route_id: &str) -> Option<String> {
    let is_index = route_id.ends_with('/');
    let segments: Vec<&str> = route_id.split('/').filter(|seg| !seg.is_empty()).collect();
    if !is_index && segments.last().is_some_and(|seg| seg.starts_with('_')) {
        return None;
    }
    let url = segments
        .into_iter()
        .filter(|seg| !seg.starts_with('_') && !is_group(seg))
        .map(|seg| seg.strip_suffix('_').unwrap_or(seg));
    Some(join_pattern(url))
}

/// The route of a TanStack Router route file, from the `createFileRoute`
/// argument in its source. `None` when the file has no literal route id.
fn tanstack_route(path: &Path) -> Option<DerivedRoute> {
    let source = std::fs::read_to_string(path).ok()?;
    let route_id = fallow_engine::code_routes::file_route_id(path, &source)?;
    Some(route(
        TANSTACK_ROUTER,
        tanstack_pattern(&route_id)?,
        MethodRule::Any,
    ))
}

/// Stable id of a route: a hash of its framework, package root, pattern and
/// method, so the id does not change between uploads while the pattern stays.
fn route_id(framework: &str, root: &str, pattern: &str, method: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    for part in [framework, root, pattern, method.unwrap_or("")] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(12);
    for byte in &digest[..6] {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("rt_{hex}")
}

/// Expand one derived route into its wire routes, one per HTTP method when the
/// rule reads the exports of the file.
pub(super) fn expand_methods(
    derived: &DerivedRoute,
    root: &str,
    file: &str,
    exports: &[&str],
) -> Vec<RoutePayload> {
    let methods: Vec<Option<&'static str>> = match derived.methods {
        MethodRule::Any => vec![None],
        MethodRule::Fixed(method) => vec![Some(method)],
        MethodRule::FromExports => {
            let found: Vec<Option<&'static str>> = HTTP_METHODS
                .iter()
                .filter(|method| exports.contains(method))
                .map(|method| Some(*method))
                .collect();
            if found.is_empty() { vec![None] } else { found }
        }
    };
    methods
        .into_iter()
        .map(|method| RoutePayload {
            id: route_id(derived.framework, root, &derived.pattern, method),
            pattern: derived.pattern.clone(),
            method,
            file: file.to_owned(),
            files: vec![file.to_owned()],
            framework: derived.framework,
        })
        .collect()
}

/// Repo-relative posix form of `path`, or the full path when it is outside the
/// project root.
fn repo_relative(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .map_or_else(|_| to_posix_string(path), to_posix_string)
}

/// The longest root in `roots` that holds `path`.
fn root_of<'a>(path: &Path, roots: &[&'a Path]) -> Option<&'a Path> {
    roots
        .iter()
        .filter(|root| path.starts_with(root))
        .max_by_key(|root| root.components().count())
        .copied()
}

/// The route candidates of the plugin entry points. `roots` holds the project
/// root and every workspace root; a file takes its pattern relative to the
/// longest root that holds it.
pub(super) fn route_candidates(
    entry_points: &[EntryPoint],
    roots: &[&Path],
    project_root: &Path,
    file_index_by_path: &FxHashMap<&Path, usize>,
    exports_by_file: &FxHashMap<usize, Vec<&str>>,
) -> Vec<RouteCandidate> {
    let mut out = Vec::new();
    for entry in entry_points {
        let EntryPointSource::Plugin { name } = &entry.source else {
            continue;
        };
        let Some(&file_index) = file_index_by_path.get(entry.path.as_path()) else {
            continue;
        };
        let Some(root) = root_of(&entry.path, roots) else {
            continue;
        };
        let Ok(rel) = entry.path.strip_prefix(root) else {
            continue;
        };
        let derived = if name == TANSTACK_ROUTER {
            tanstack_route(&entry.path)
        } else {
            derive_route(name, &to_posix_string(rel))
        };
        let Some(derived) = derived else {
            continue;
        };
        let root = repo_relative(root, project_root);
        let file = repo_relative(&entry.path, project_root);
        let exports = exports_by_file
            .get(&file_index)
            .map_or(&[][..], Vec::as_slice);
        for payload in expand_methods(&derived, &root, &file, exports) {
            out.push(RouteCandidate {
                file_index,
                payload,
            });
        }
    }
    out
}

/// The route candidates of the routes that Elysia code defines. Each route
/// starts at the file of its handler. `path_by_index` maps a graph file index
/// to its absolute path.
pub(super) fn code_route_candidates(
    code_routes: &[CodeRoute],
    roots: &[&Path],
    project_root: &Path,
    path_by_index: &FxHashMap<usize, &Path>,
) -> Vec<RouteCandidate> {
    code_routes
        .iter()
        .filter_map(|code_route| {
            let file_index = code_route.file_id.0 as usize;
            let path = *path_by_index.get(&file_index)?;
            let root = repo_relative(root_of(path, roots)?, project_root);
            let file = repo_relative(path, project_root);
            Some(RouteCandidate {
                file_index,
                payload: RoutePayload {
                    id: route_id(ELYSIA, &root, &code_route.pattern, code_route.method),
                    pattern: code_route.pattern.clone(),
                    method: code_route.method,
                    files: vec![file.clone()],
                    file,
                    framework: ELYSIA,
                },
            })
        })
        .collect()
}

/// Primary file order: a file outside a parallel slot (`@slot`) first, then the
/// shorter path, then the path itself.
fn primary_file_key(file: &str) -> (usize, usize, &str) {
    let slots = file.split('/').filter(|seg| seg.starts_with('@')).count();
    (slots, file.len(), file)
}

/// Merge the candidates that share a route id (one framework, root, pattern and
/// method), sort them by pattern, method and primary file, and cap them.
fn merge_candidates(candidates: Vec<RouteCandidate>) -> (Vec<MergedRoute>, usize) {
    let mut by_id: BTreeMap<String, Vec<RouteCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_id
            .entry(candidate.payload.id.clone())
            .or_default()
            .push(candidate);
    }
    let mut merged: Vec<MergedRoute> = by_id
        .into_values()
        .filter_map(|mut group| {
            group.sort_by(|a, b| {
                primary_file_key(&a.payload.file).cmp(&primary_file_key(&b.payload.file))
            });
            group.dedup_by(|a, b| a.payload.file == b.payload.file);
            let file_indices = group.iter().map(|c| c.file_index).collect();
            let mut files: Vec<String> = group.iter().map(|c| c.payload.file.clone()).collect();
            files.truncate(MAX_FILES_PER_ROUTE);
            let mut payload = group.into_iter().next()?.payload;
            payload.files = files;
            Some(MergedRoute {
                file_indices,
                payload,
            })
        })
        .collect();
    merged.sort_by(|a, b| {
        a.payload
            .pattern
            .cmp(&b.payload.pattern)
            .then_with(|| a.payload.method.cmp(&b.payload.method))
            .then_with(|| a.payload.file.cmp(&b.payload.file))
    });
    let dropped = merged.len().saturating_sub(MAX_ROUTES);
    merged.truncate(MAX_ROUTES);
    (merged, dropped)
}

/// Route indices per graph file: every route whose files reach the file
/// through `forward`. A list stops one past [`MAX_ROUTES_PER_FUNCTION`], so the
/// memory stays bounded and a cut is still visible.
fn reach_by_file(routes: &[MergedRoute], forward: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut routes_by_file: Vec<Vec<usize>> = vec![Vec::new(); forward.len()];
    let mut seen = vec![usize::MAX; forward.len()];
    let mut stack = Vec::new();
    for (route_index, route) in routes.iter().enumerate() {
        for &start in &route.file_indices {
            if start < seen.len() && seen[start] != route_index {
                seen[start] = route_index;
                stack.push(start);
            }
        }
        while let Some(file) = stack.pop() {
            let list = &mut routes_by_file[file];
            if list.len() <= MAX_ROUTES_PER_FUNCTION {
                list.push(route_index);
            }
            for &next in &forward[file] {
                if next < seen.len() && seen[next] != route_index {
                    seen[next] = route_index;
                    stack.push(next);
                }
            }
        }
    }
    routes_by_file
}

/// The largest per-function cap, at most [`MAX_ROUTES_PER_FUNCTION`], that
/// keeps the sum of the capped list lengths within `budget`.
fn per_function_cap(lengths: &[usize], budget: usize) -> usize {
    let total = |cap: usize| -> usize { lengths.iter().map(|&n| n.min(cap)).sum() };
    let mut cap = MAX_ROUTES_PER_FUNCTION;
    while cap > 0 && total(cap) > budget {
        cap -= 1;
    }
    cap
}

/// Merge and cap the candidates, walk the graph forward from each route file,
/// and set the route ids of each function.
///
/// `forward[i]` lists the files that file `i` imports at runtime.
/// `file_index_by_repo_path` maps the repo-relative path of a function to its
/// graph file index.
pub(super) fn attach_routes(
    candidates: Vec<RouteCandidate>,
    forward: &[Vec<usize>],
    file_index_by_repo_path: &FxHashMap<String, usize>,
    functions: &mut [InventoryFunction],
) -> InventoryRoutes {
    attach_routes_with_budget(
        candidates,
        forward,
        file_index_by_repo_path,
        functions,
        MAX_ROUTE_ID_REFS,
    )
}

pub(super) fn attach_routes_with_budget(
    candidates: Vec<RouteCandidate>,
    forward: &[Vec<usize>],
    file_index_by_repo_path: &FxHashMap<String, usize>,
    functions: &mut [InventoryFunction],
    budget: usize,
) -> InventoryRoutes {
    let (routes, dropped_routes) = merge_candidates(candidates);
    let routes_by_file = reach_by_file(&routes, forward);
    let empty: Vec<usize> = Vec::new();
    let lists: Vec<&Vec<usize>> = functions
        .iter()
        .map(|function| {
            file_index_by_repo_path
                .get(&function.identity.file)
                .and_then(|&file| routes_by_file.get(file))
                .unwrap_or(&empty)
        })
        .collect();
    let lengths: Vec<usize> = lists.iter().map(|list| list.len()).collect();
    let cap = per_function_cap(&lengths, budget);

    let mut truncated_functions = 0_usize;
    for (function, list) in functions.iter_mut().zip(lists) {
        if list.len() > cap {
            truncated_functions += 1;
        }
        function.route_ids = list
            .iter()
            .take(cap)
            .map(|&i| routes[i].payload.id.clone())
            .collect();
    }

    InventoryRoutes {
        routes: routes.into_iter().map(|r| r.payload).collect(),
        dropped_routes,
        truncated_functions,
        max_routes_per_function: cap,
    }
}

/// Tell the user when the route size guard dropped data.
pub(super) fn warn_on_route_limits(routes: &InventoryRoutes) {
    if routes.dropped_routes > 0 {
        eprintln!(
            "{LOG_PREFIX}: {}: the project has more than {} entry routes; {} route(s) were dropped from the upload.",
            "warning".yellow().bold(),
            format_count(MAX_ROUTES),
            format_count(routes.dropped_routes),
        );
    }
    if routes.truncated_functions > 0 {
        eprintln!(
            "{LOG_PREFIX}: {}: {} function(s) are reachable from more than {} routes; the extra route ids were dropped from the upload.",
            "warning".yellow().bold(),
            format_count(routes.truncated_functions),
            format_count(routes.max_routes_per_function),
        );
    }
}

/// HTTP method names among the value exports of each graph file.
pub(super) fn method_exports_by_file<'a>(
    exports: impl IntoIterator<Item = (usize, &'a str)>,
) -> FxHashMap<usize, Vec<&'a str>> {
    let mut out: FxHashMap<usize, Vec<&'a str>> = FxHashMap::default();
    for (file, name) in exports {
        if HTTP_METHODS.contains(&name) {
            out.entry(file).or_default().push(name);
        }
    }
    out
}

/// Count of routes per framework, for the dry-run summary.
pub(super) fn routes_per_framework(routes: &[RoutePayload]) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    for route in routes {
        *out.entry(route.framework).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests;
