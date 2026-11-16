//! Client roots and server-only sinks that the React Router plugin supplies to
//! the `client-server-leak` rule (issue #4582).
//!
//! React Router Framework Mode has no `"use client"` directive. Its bundler
//! decides what ships to the browser from file conventions:
//!
//! - A route module ships to the browser without its server-only exports
//!   (`loader`, `action`, `middleware`, `headers`). Dead-code elimination then
//!   drops each import that only those exports use. The cone skips these
//!   imports at each route module that it reaches, the root included (see
//!   [`super::route_module_split`]). React Router applies the transform to
//!   the route file wherever another module imports it.
//! - A test file under the route directory is not a root.
//! - A `*.client.*` file, or a file under a `.client/` directory, runs only in
//!   the browser. It is a whole-module client root.
//! - A `*.server.*` file, or a file under a `.server/` directory, must never
//!   reach the client build. It is a `server-only-import` sink.
//!
//! Only the route modules are parsed again, and only when the rule is on and
//! the React Router plugin is active. Extraction does not keep the
//! statement-level references that the split needs.

use std::path::{Component, Path};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::plugins::react_router::{REACT_ROUTER_PLUGIN_NAME, is_route_module_rule};
use crate::plugins::{AggregatedPluginResult, CompiledPathRule};

use super::route_module_split::server_only_import_spans;
use crate::analyze::predicates::is_test_or_spec_file;

/// The file-name and directory marker of a browser-only module.
const CLIENT_MARKER: &str = "client";
/// The file-name and directory marker of a server-only module.
const SERVER_MARKER: &str = "server";

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
    /// Each route module, with the start offsets of the imports that only its
    /// server-only exports use. The cone does not follow these imports.
    route_roots: FxHashMap<FileId, FxHashSet<u32>>,
    client_roots: FxHashSet<FileId>,
    server_sinks: FxHashSet<FileId>,
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
        let route_rules: Vec<CompiledPathRule> = plugin_result
            .used_exports
            .iter()
            .filter(|rule| is_route_module_rule(&rule.plugin_name, &rule.rule.exports))
            .filter_map(|rule| {
                CompiledPathRule::for_used_export_rule(&rule.rule.path, "route module pattern")
            })
            .collect();
        let mut roots = Self::default();
        for node in &graph.modules {
            let relative = node.path.strip_prefix(root).unwrap_or(&node.path);
            if has_marker(relative, SERVER_MARKER) {
                roots.server_sinks.insert(node.file_id);
                continue;
            }
            // A test file runs in the test runner, not in the browser.
            if is_test_or_spec_file(relative) {
                continue;
            }
            if has_marker(relative, CLIENT_MARKER) {
                roots.client_roots.insert(node.file_id);
                continue;
            }
            let relative_str = relative.to_string_lossy();
            if route_rules.iter().any(|rule| rule.matches(&relative_str)) {
                roots
                    .route_roots
                    .insert(node.file_id, server_only_import_spans(&node.path));
            }
        }
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

    /// The import start offsets of a route module that only its server-only
    /// exports use.
    pub(super) fn server_only_imports(&self, file_id: FileId) -> Option<&FxHashSet<u32>> {
        self.route_roots.get(&file_id)
    }

    /// Each route module with the import start offsets that the cone skips.
    pub(super) fn route_server_only_spans(
        &self,
    ) -> impl Iterator<Item = (FileId, &FxHashSet<u32>)> + '_ {
        self.route_roots
            .iter()
            .map(|(&file_id, spans)| (file_id, spans))
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
