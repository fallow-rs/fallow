//! Client roots and server-only sinks that the React Router plugin supplies to
//! the `client-server-leak` rule (issue #4582).
//!
//! React Router Framework Mode has no `"use client"` directive. Its bundler
//! decides what ships to the browser from file conventions:
//!
//! - A route module ships to the browser without its server-only exports
//!   (`loader`, `action`, `middleware`, `headers`). Dead-code elimination then
//!   drops each import and env read that only those exports use. The leak
//!   scan applies the same split to each route module, as a root and where
//!   the cone reaches it (see [`super::route_module_split`]). React Router
//!   applies the transform to the route file wherever another module imports
//!   it.
//! - A `*.client.*` file, or a file under a `.client/` directory, runs only in
//!   the browser. It is a whole-module client root.
//! - A `*.server.*` file, or a file under a `.server/` directory, must never
//!   reach the client build. It is a `server-only-import` sink.
//!
//! The route modules are the files that the route config names, the root
//! route, and the route files of the route directory. A helper module next to
//! a route file is not a root: it is client code only when the cone reaches
//! it. A test file is never a root. The `.client` and `.server` markers apply
//! only in the packages where the React Router plugin is active.
//!
//! Only the route modules are parsed again, and only when the rule is on and
//! the plugin is active. Extraction does not keep the statement-level
//! references that the split needs.

use std::path::{Component, Path};

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::plugins::react_router::{
    REACT_ROUTER_PLUGIN_NAME, is_route_export_name, is_route_module_rule,
};
use crate::plugins::{AggregatedPluginResult, CompiledPathRule};

use super::route_module_split::{RouteModuleSplit, split_route_module};
use crate::analyze::predicates::is_test_or_spec_file;

/// The file-name and directory marker of a browser-only module.
const CLIENT_MARKER: &str = "client";
/// The file-name and directory marker of a server-only module.
const SERVER_MARKER: &str = "server";
/// The file stem of a route module in a flat-routes folder (`users/route.tsx`).
const FOLDER_ROUTE_STEM: &str = "route";
/// The glob part that a route directory pattern of the plugin ends with.
const ROUTE_DIRECTORY_GLOB: &str = "/**/";

/// What makes a file a starting point of the leak scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientRootKind {
    /// A file with a top-level `"use client"` directive.
    UseClient,
    /// A React Router `*.client.*` file or a file under a `.client/` directory.
    ClientModule,
    /// A React Router route module. Only its client exports start the cone.
    RouteModule,
}

impl ClientRootKind {
    /// The subject of the evidence sentence for a finding on this root.
    pub(super) const fn subject(self) -> &'static str {
        match self {
            Self::UseClient => "This \"use client\" file",
            Self::ClientModule => "This React Router .client module",
            Self::RouteModule => "The client code of this React Router route module",
        }
    }
}

/// The client roots and server-only sinks of the active framework plugins.
#[derive(Debug, Default)]
pub struct FrameworkClientRoots {
    /// Each route module with its split. `None` when the file did not parse
    /// cleanly: the module then counts as a whole.
    route_roots: FxHashMap<FileId, Option<RouteModuleSplit>>,
    client_roots: FxHashSet<FileId>,
    server_sinks: FxHashSet<FileId>,
}

/// The route patterns of the plugin, split by shape.
struct RouteRules {
    /// Patterns of single route files: the root route and the files that the
    /// route config names.
    files: Vec<CompiledPathRule>,
    /// Route directories (`app/routes/**/*.{ts,tsx,js,jsx}`), with the
    /// project-relative directory.
    directories: Vec<(CompiledPathRule, String)>,
}

impl RouteRules {
    fn new(plugin_result: &AggregatedPluginResult) -> Self {
        let mut rules = Self {
            files: Vec::new(),
            directories: Vec::new(),
        };
        let route_rules = plugin_result
            .used_exports
            .iter()
            .filter(|rule| is_route_module_rule(&rule.plugin_name, &rule.rule.exports));
        for rule in route_rules {
            let Some(matcher) =
                CompiledPathRule::for_used_export_rule(&rule.rule.path, "route module pattern")
            else {
                continue;
            };
            match rule.rule.path.pattern.split_once(ROUTE_DIRECTORY_GLOB) {
                Some((directory, _)) => rules.directories.push((matcher, directory.to_owned())),
                None => rules.files.push(matcher),
            }
        }
        rules
    }
}

impl FrameworkClientRoots {
    /// Collect the roots and sinks. The result is empty unless the React
    /// Router plugin is active.
    #[must_use]
    pub fn collect(
        graph: &ModuleGraph,
        root: &Path,
        plugin_result: Option<&AggregatedPluginResult>,
    ) -> Self {
        let Some(plugin_result) = plugin_result.filter(|result| {
            result
                .active_plugins
                .iter()
                .any(|plugin| plugin == REACT_ROUTER_PLUGIN_NAME)
        }) else {
            return Self::default();
        };
        let scopes: Vec<&str> = plugin_result
            .active_plugin_scopes
            .iter()
            .filter(|(plugin, _)| plugin == REACT_ROUTER_PLUGIN_NAME)
            .map(|(_, scope)| scope.as_str())
            .collect();
        let rules = RouteRules::new(plugin_result);
        let relative_paths: Vec<String> = graph
            .modules
            .iter()
            .map(|node| relative_path(&node.path, root))
            .collect();
        let folder_routes = folder_route_directories(&relative_paths, &rules);

        let mut roots = Self::default();
        let mut route_files: Vec<&ModuleNode> = Vec::new();
        for (node, relative) in graph.modules.iter().zip(&relative_paths) {
            let path = Path::new(relative);
            let in_scope = is_in_scope(relative, &scopes);
            if in_scope && has_marker(path, SERVER_MARKER) {
                roots.server_sinks.insert(node.file_id);
                continue;
            }
            // A test file runs in the test runner, not in the browser.
            if is_test_or_spec_file(path) {
                continue;
            }
            if in_scope && has_marker(path, CLIENT_MARKER) {
                roots.client_roots.insert(node.file_id);
                continue;
            }
            if is_route_file(node, relative, &rules, &folder_routes) {
                route_files.push(node);
            }
        }
        roots.route_roots = route_files
            .par_iter()
            .map(|node| (node.file_id, split_route_module(&node.path)))
            .collect();
        roots
    }

    /// The root kind of a file without a `"use client"` directive, if any.
    pub(super) fn root_kind(&self, file_id: FileId) -> Option<ClientRootKind> {
        if self.client_roots.contains(&file_id) {
            Some(ClientRootKind::ClientModule)
        } else if self.route_roots.contains_key(&file_id) {
            Some(ClientRootKind::RouteModule)
        } else {
            None
        }
    }

    /// Each route module that parsed cleanly, with its split.
    pub(super) fn route_splits(&self) -> impl Iterator<Item = (FileId, &RouteModuleSplit)> + '_ {
        self.route_roots
            .iter()
            .filter_map(|(&file_id, split)| split.as_ref().map(|split| (file_id, split)))
    }

    /// Whether a file is a React Router `.server` module.
    pub(super) fn is_server_sink(&self, file_id: FileId) -> bool {
        self.server_sinks.contains(&file_id)
    }

    /// The `.server` modules, which are server-only sinks.
    pub(super) fn server_sinks(&self) -> impl Iterator<Item = FileId> + '_ {
        self.server_sinks.iter().copied()
    }
}

fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether a project-relative path lies in a package where the plugin is
/// active. An empty scope is the root project, which holds every path.
fn is_in_scope(relative: &str, scopes: &[&str]) -> bool {
    scopes.iter().any(|scope| {
        scope.is_empty()
            || relative
                .strip_prefix(scope)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

/// The project-relative folders that hold a flat-routes `route.*` file inside
/// a route directory. The other files in such a folder are helpers.
fn folder_route_directories(relative_paths: &[String], rules: &RouteRules) -> FxHashSet<String> {
    relative_paths
        .iter()
        .filter(|relative| file_stem(relative) == Some(FOLDER_ROUTE_STEM))
        .filter(|relative| {
            rules
                .directories
                .iter()
                .any(|(rule, _)| rule.matches(relative))
        })
        .filter_map(|relative| {
            relative
                .rsplit_once('/')
                .map(|(folder, _)| folder.to_owned())
        })
        .collect()
}

/// Whether a module is a route module:
///
/// - a file that the route config names, or the root route;
/// - a file directly in a route directory (`routes/about.tsx`);
/// - a `route.*` file in a folder of a route directory
///   (`routes/users/route.tsx`);
/// - a deeper file outside such a folder that exports a route module export.
///   Route conventions that nest files in folders (`routes/_auth/login.tsx`)
///   give this shape.
fn is_route_file(
    node: &ModuleNode,
    relative: &str,
    rules: &RouteRules,
    folder_routes: &FxHashSet<String>,
) -> bool {
    if rules.files.iter().any(|rule| rule.matches(relative)) {
        return true;
    }
    let Some(directory) = rules
        .directories
        .iter()
        .find(|(rule, _)| rule.matches(relative))
        .map(|(_, directory)| directory.as_str())
    else {
        return false;
    };
    let Some(inner) = relative
        .strip_prefix(directory)
        .and_then(|rest| rest.strip_prefix('/'))
    else {
        return false;
    };
    let Some((folders, _)) = inner.rsplit_once('/') else {
        return true;
    };
    if file_stem(relative) == Some(FOLDER_ROUTE_STEM) {
        return true;
    }
    let in_route_folder = folders
        .split('/')
        .scan(directory.to_owned(), |path, folder| {
            path.push('/');
            path.push_str(folder);
            Some(path.clone())
        })
        .any(|folder| folder_routes.contains(&folder));
    !in_route_folder
        && node
            .exports
            .iter()
            .any(|export| !export.is_type_only && is_route_export_name(&export.name.to_string()))
}

/// The file name up to its first dot.
fn file_stem(relative: &str) -> Option<&str> {
    let name = relative.rsplit('/').next()?;
    name.split('.').next()
}

/// Whether a project-relative path is a `*.<marker>.*` file or lies under a
/// `.<marker>/` directory.
fn has_marker(relative: &Path, marker: &str) -> bool {
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            continue;
        };
        let name = name.to_string_lossy();
        if components.peek().is_some() {
            if name.strip_prefix('.') == Some(marker) {
                return true;
            }
            continue;
        }
        let mut parts = name.split('.');
        let _stem = parts.next();
        let mut middle: Vec<&str> = parts.collect();
        middle.pop();
        return middle.contains(&marker);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_match_file_names_and_directories() {
        assert!(has_marker(
            Path::new("app/lib/auth.server.ts"),
            SERVER_MARKER
        ));
        assert!(has_marker(Path::new("app/.server/db.ts"), SERVER_MARKER));
        assert!(has_marker(Path::new("app/entry.client.tsx"), CLIENT_MARKER));
        assert!(has_marker(
            Path::new("app/.client/storage.ts"),
            CLIENT_MARKER
        ));
        assert!(!has_marker(Path::new("app/server.ts"), SERVER_MARKER));
        assert!(!has_marker(Path::new("app/server/db.ts"), SERVER_MARKER));
        assert!(!has_marker(Path::new("app/lib/client.ts"), CLIENT_MARKER));
        assert!(!has_marker(Path::new("app/lib/server.d.ts"), CLIENT_MARKER));
    }
}
