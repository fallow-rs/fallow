//! Detection of barrel files that re-export BOTH a `"use client"` origin module
//! AND a server-only origin module (the Next.js App Router footgun).
//!
//! A barrel (a file whose exports are re-exports, i.e. `export ... from`) that
//! forwards a name from a `"use client"` module alongside a name from a
//! server-only module is dangerous: importing one name from such a barrel drags
//! the other's directive context across the React Server Components boundary.
//!
//! The trigger is deliberately precise to avoid false positives:
//! - It requires a client origin AND a SERVER-ONLY origin (see the shared
//!   [`is_server_only_module`] predicate). A barrel re-exporting a
//!   `"use client"` component and an
//!   ordinary undirected utility module is completely normal and MUST NOT flag.
//! - Type-only re-exports (`export type { X } from './client'`) are erased and
//!   carry no runtime directive context, so they are skipped when classifying an
//!   origin.
//! - Only DIRECT re-export origins are classified. Origins are resolved one hop
//!   via the graph; transitive re-export chains are not walked (keeps it precise
//!   and sidesteps the `react-server` conditional-exports concern, which only
//!   affects npm package re-exports). Origins that do not resolve to a LOCAL
//!   source module (npm packages, unresolved) are ignored.
//!
//! Gated on the project using a React Server Components bundler (see
//! [`crate::analyze::predicates::project_uses_rsc_directives`]): the barrel
//! footgun applies to Next AND the framework-agnostic RSC bundlers (Waku, Vite
//! RSC, etc.). Without an RSC bundler the `"use client"` / `"use server"`
//! directives have no special meaning, so firing would be a false positive.

use rustc_hash::FxHashMap;

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::MixedClientServerBarrel;
use crate::suppress::{IssueKind, SuppressionContext};

use super::server_only::is_server_only_module;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The React Server Components client-boundary directive.
const USE_CLIENT: &str = "use client";

/// Find barrel files that re-export BOTH a `"use client"` origin module AND a
/// server-only origin module.
///
/// Returns empty unless the project uses an RSC bundler (gated on
/// `declared_deps` via [`crate::analyze::predicates::project_uses_rsc_directives`]):
/// without one the directives carry no special meaning.
///
/// For each resolved module with at least one re-export, the DIRECT re-export
/// origins are resolved one hop to their `ModuleInfo` and classified as a client
/// origin (carries `"use client"`) or a server-only origin (per
/// [`is_server_only_module`]). A barrel with at least one of each emits ONE
/// finding anchored at the barrel's first offending re-export, naming the first
/// client origin and the first server-only origin (by re-export source order).
/// Type-only re-export entries are skipped (erased at build, no directive
/// context). Suppression routes through [`SuppressionContext`] with
/// [`IssueKind::MixedClientServerBarrel`].
#[must_use]
pub fn find_mixed_client_server_barrels(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    resolved_modules: &[ResolvedModule],
    declared_deps: &rustc_hash::FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<MixedClientServerBarrel> {
    if !crate::analyze::predicates::project_uses_rsc_directives(declared_deps) {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();

    let mut findings = Vec::new();
    for resolved in resolved_modules {
        if resolved.re_exports.is_empty() {
            continue;
        }

        // Walk the barrel's DIRECT re-export origins in source order, capturing
        // the FIRST client origin and the FIRST server-only origin plus the
        // offending re-export span (the earlier of the two, so the finding
        // anchors at the first re-export that participates in the mix).
        let mut client: Option<OffendingOrigin<'_>> = None;
        let mut server: Option<OffendingOrigin<'_>> = None;
        for re in &resolved.re_exports {
            // Type-only re-exports are erased and carry no directive context.
            if re.info.is_type_only {
                continue;
            }
            // Only DIRECT origins that resolve to a LOCAL source module are
            // classified; npm packages and unresolved sources are ignored.
            let Some(origin_id) = re.target.internal_file_id() else {
                continue;
            };
            let Some(origin) = modules_by_id.get(&origin_id) else {
                continue;
            };
            let span_start = re.info.span.start;
            if client.is_none() && origin.directives.iter().any(|d| d == USE_CLIENT) {
                client = Some(OffendingOrigin {
                    source: re.info.source.as_str(),
                    span_start,
                });
            } else if server.is_none() && is_server_only_module(origin) {
                // `else if` so a single origin that is somehow both client and
                // server-only counts as the client origin only (a client
                // boundary cannot also be a server-only module in practice).
                server = Some(OffendingOrigin {
                    source: re.info.source.as_str(),
                    span_start,
                });
            }
        }

        let (Some(client), Some(server)) = (client, server) else {
            continue;
        };

        let barrel_id = resolved.file_id;
        let Some(path) = path_by_id.get(&barrel_id) else {
            continue;
        };

        // Anchor at the earlier of the two offending re-exports.
        let anchor_span = client.span_start.min(server.span_start);
        let (line, col) = byte_offset_to_line_col(line_offsets_by_file, barrel_id, anchor_span);
        if suppressions.is_suppressed(barrel_id, line, IssueKind::MixedClientServerBarrel) {
            continue;
        }

        findings.push(MixedClientServerBarrel {
            path: path.to_path_buf(),
            client_origin: client.source.to_string(),
            server_origin: server.source.to_string(),
            line,
            col,
        });
    }

    findings
}

/// A re-export entry that participates in the client/server mix: the source
/// specifier as written and the start of the re-export declaration's span.
struct OffendingOrigin<'a> {
    source: &'a str,
    span_start: u32,
}
