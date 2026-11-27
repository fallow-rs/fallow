//! Detection of Next.js server-only / route-segment config exports declared in
//! `"use client"` files.
//!
//! Next.js rejects an App Router entry file (`page`, `layout` or `route`) with
//! the client-boundary directive (`"use client"`) that also exports a
//! server-only or route-segment config name such as `metadata`,
//! `generateMetadata`, `revalidate`, or a route HTTP method (`GET`, `POST`,
//! ...). Fallow reports the export before the build runs.
//!
//! Next.js reads these names only from an entry file, and the route HTTP
//! methods only from a `route` file. Any other `"use client"` module (for
//! example a `page-client.tsx` that a `page.tsx` imports) can export them, so
//! the detector skips it.
//!
//! The detector is gated on the project declaring `next` as a dependency (see
//! `find_invalid_client_exports`): without Next.js the `"use client"` directive
//! has no special meaning and these export names are perfectly legal, so firing
//! would be a false positive.

use std::path::Path;

use rustc_hash::FxHashMap;

use fallow_types::extract::{ExportName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::InvalidClientExport;
use crate::suppress::{IssueKind, SuppressionContext};

use super::route_tree::app_router_entry_stem;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The file-level directive that marks a React Server Components client
/// boundary. Matched exactly against [`ModuleInfo::directives`].
const USE_CLIENT_DIRECTIVE: &str = "use client";

/// Export names Next.js reserves for the server or for route-segment config.
/// Exporting any of these from a `"use client"` file is a Next.js build error.
///
/// Covers the server data-fetching / metadata APIs, the route-segment config
/// options, the legacy Pages Router data functions, and the App Router route
/// HTTP method handlers. The default export is intentionally absent: it is the
/// client component itself and is always valid.
const ILLEGAL_CLIENT_EXPORTS: &[&str] = &[
    // Metadata APIs.
    "metadata",
    "generateMetadata",
    "viewport",
    "generateViewport",
    // Route-segment static params.
    "generateStaticParams",
    // Route-segment config options.
    "dynamic",
    "dynamicParams",
    "revalidate",
    "fetchCache",
    "runtime",
    "preferredRegion",
    "maxDuration",
    // Legacy Pages Router data functions.
    "getServerSideProps",
    "getStaticProps",
    "getStaticPaths",
    // App Router route HTTP method handlers.
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "HEAD",
    "OPTIONS",
];

/// The route HTTP method handlers of [`ILLEGAL_CLIENT_EXPORTS`]. Next.js reads
/// them only from a `route` file.
const ROUTE_HANDLER_EXPORTS: &[&str] =
    &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Find `"use client"` files exporting a Next.js server-only / route-config
/// name.
///
/// Returns empty unless the project declares `next` (gated on `declared_deps`):
/// without Next.js the directive and these names carry no special meaning.
///
/// For each App Router entry file (anchored on one of `pkg_roots`) carrying the
/// `"use client"` directive, every non-type-only export whose rendered name is
/// in [`ILLEGAL_CLIENT_EXPORTS`] is reported. A route HTTP method name is
/// reported only in a `route` file. The default export is never reported (it
/// is the client component itself). Suppression is routed through
/// [`SuppressionContext`] with [`IssueKind::InvalidClientExport`] so a
/// `// fallow-ignore-next-line invalid-client-export` comment works and is
/// recorded as consumed.
#[must_use]
pub fn find_invalid_client_exports(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    pkg_roots: &[std::path::PathBuf],
    declared_deps: &rustc_hash::FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<InvalidClientExport> {
    if !declared_deps.contains("next") {
        return Vec::new();
    }

    let path_by_id = path_by_file_id(graph);
    let pkg_roots: Vec<&Path> = pkg_roots.iter().map(std::path::PathBuf::as_path).collect();

    let mut findings = Vec::new();
    for module in modules {
        if !module_uses_client_directive(module) {
            continue;
        }
        let Some(path) = path_by_id.get(&module.file_id) else {
            continue;
        };
        let Some(entry_stem) = app_router_entry_stem(path, &pkg_roots) else {
            continue;
        };
        append_invalid_client_exports_for_module(
            module,
            path,
            entry_stem,
            suppressions,
            line_offsets_by_file,
            &mut findings,
        );
    }

    findings
}

fn path_by_file_id(graph: &ModuleGraph) -> FxHashMap<FileId, &Path> {
    graph
        .modules()
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect()
}

fn module_uses_client_directive(module: &ModuleInfo) -> bool {
    module
        .directives
        .iter()
        .any(|directive| directive == USE_CLIENT_DIRECTIVE)
}

fn append_invalid_client_exports_for_module(
    module: &ModuleInfo,
    path: &Path,
    entry_stem: &str,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    findings: &mut Vec<InvalidClientExport>,
) {
    for export in module.exports.iter() {
        if export.is_type_only {
            continue;
        }
        // The default export is the client component itself; always valid.
        let ExportName::Named(name) = &export.name else {
            continue;
        };
        if !ILLEGAL_CLIENT_EXPORTS.contains(&name.as_str())
            || (entry_stem != "route" && ROUTE_HANDLER_EXPORTS.contains(&name.as_str()))
        {
            continue;
        }

        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, module.file_id, export.span.start);
        if suppressions.is_suppressed(module.file_id, line, IssueKind::InvalidClientExport) {
            continue;
        }

        findings.push(InvalidClientExport {
            path: path.to_path_buf(),
            export_name: name.clone(),
            directive: USE_CLIENT_DIRECTIVE.to_string(),
            line,
            col,
        });
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::{FxHashMap, FxHashSet};

    use crate::graph::ModuleGraph;
    use crate::suppress::SuppressionContext;

    use super::{ILLEGAL_CLIENT_EXPORTS, ROUTE_HANDLER_EXPORTS, find_invalid_client_exports};

    #[test]
    fn next_gate_returns_empty_without_next_dependency() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        let modules = Vec::new();
        let declared: FxHashSet<String> = std::iter::once("react".to_string()).collect();
        let suppressions = SuppressionContext::empty();
        let offsets = FxHashMap::default();

        let findings =
            find_invalid_client_exports(&graph, &modules, &[], &declared, &suppressions, &offsets);
        assert!(
            findings.is_empty(),
            "no `next` dependency means no findings"
        );
    }

    #[test]
    fn illegal_set_covers_server_only_names_and_excludes_default() {
        // Server-only / route-config names that Next.js rejects in a client file.
        for name in [
            "metadata",
            "generateMetadata",
            "viewport",
            "generateStaticParams",
            "dynamic",
            "revalidate",
            "runtime",
            "getServerSideProps",
            "GET",
            "POST",
            "OPTIONS",
        ] {
            assert!(
                ILLEGAL_CLIENT_EXPORTS.contains(&name),
                "`{name}` should be in the illegal set"
            );
        }
        // The default export is the client component itself and is never illegal.
        assert!(!ILLEGAL_CLIENT_EXPORTS.contains(&"default"));
        // A perfectly ordinary export name is legal.
        assert!(!ILLEGAL_CLIENT_EXPORTS.contains(&"useThing"));
    }

    #[test]
    fn route_handler_names_are_in_the_illegal_set() {
        for name in ROUTE_HANDLER_EXPORTS {
            assert!(ILLEGAL_CLIENT_EXPORTS.contains(name), "{name}");
        }
    }

    #[test]
    fn illegal_set_has_no_duplicates() {
        let mut sorted = ILLEGAL_CLIENT_EXPORTS.to_vec();
        sorted.sort_unstable();
        let len = sorted.len();
        sorted.dedup();
        assert_eq!(len, sorted.len(), "ILLEGAL_CLIENT_EXPORTS has a duplicate");
    }
}
