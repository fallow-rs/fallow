//! Detection of misplaced `"use client"` / `"use server"` directives.
//!
//! Next.js (and every React Server Components bundler) only honors a `"use
//! client"` / `"use server"` directive when it sits in the leading PROLOGUE of
//! the file, before any other statement. oxc places honored prologue directives
//! in `program.directives`; the moment a non-directive statement (an `import`, a
//! `const`) precedes the string, oxc parses it as an ordinary
//! string-literal expression statement in `program.body` instead. The bundler
//! then SILENTLY IGNORES it: the developer intended a client (or server)
//! boundary, but the file is treated as a server module. This is a silent
//! footgun, so fallow flags it; the fix is to move the directive to the very
//! top of the file.
//!
//! The extract layer records every such misplaced directive string on
//! [`ModuleInfo::misplaced_directives`](fallow_types::extract::ModuleInfo); this
//! detector resolves each site to a `(line, col)` and emits one finding per
//! occurrence.
//!
//! Gated on the project using a React Server Components bundler (see
//! [`find_misplaced_directives`] and
//! [`crate::analyze::predicates::project_uses_rsc_directives`]): a body-position
//! `"use client"` / `"use server"` is silently ignored by every RSC toolchain,
//! so the rule applies to Next AND the framework-agnostic RSC bundlers (Waku,
//! Vite RSC, etc.). Without an RSC bundler the directives carry no special
//! meaning, so firing would be a false positive.

use rustc_hash::FxHashMap;

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::MisplacedDirective;
use crate::suppress::{IssueKind, SuppressionContext};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The React Server Components client-boundary directive, rendered verbatim into
/// the finding's `directive` field.
const USE_CLIENT: &str = "use client";

/// The React Server Components server-boundary directive, rendered verbatim into
/// the finding's `directive` field.
const USE_SERVER: &str = "use server";

/// Find misplaced `"use client"` / `"use server"` directives.
///
/// Returns empty unless the project uses an RSC bundler (gated on
/// `declared_deps` via [`crate::analyze::predicates::project_uses_rsc_directives`]):
/// without one the directives carry no special meaning.
///
/// For each module, every entry in
/// [`ModuleInfo::misplaced_directives`](fallow_types::extract::ModuleInfo) is
/// resolved to a `(line, col)` via [`byte_offset_to_line_col`] and emitted as a
/// [`MisplacedDirective`] anchored at the offending statement. Suppression is
/// routed through [`SuppressionContext`] with [`IssueKind::MisplacedDirective`]
/// so a `// fallow-ignore-next-line misplaced-directive` comment works and is
/// recorded as consumed.
#[must_use]
pub fn find_misplaced_directives(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &rustc_hash::FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<MisplacedDirective> {
    if !crate::analyze::predicates::project_uses_rsc_directives(declared_deps) {
        return Vec::new();
    }

    let path_by_id: FxHashMap<FileId, &std::path::Path> = graph
        .modules
        .iter()
        .map(|module| (module.file_id, module.path.as_path()))
        .collect();

    let mut findings = Vec::new();
    for module in modules {
        if module.misplaced_directives.is_empty() {
            continue;
        }
        let Some(path) = path_by_id.get(&module.file_id) else {
            continue;
        };

        for site in &module.misplaced_directives {
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, module.file_id, site.span_start);
            if suppressions.is_suppressed(module.file_id, line, IssueKind::MisplacedDirective) {
                continue;
            }

            let directive = if site.is_server {
                USE_SERVER
            } else {
                USE_CLIENT
            };
            findings.push(MisplacedDirective {
                path: path.to_path_buf(),
                directive: directive.to_string(),
                line,
                col,
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use rustc_hash::{FxHashMap, FxHashSet};

    use crate::graph::ModuleGraph;
    use crate::suppress::SuppressionContext;

    use super::find_misplaced_directives;

    #[test]
    fn rsc_gate_returns_empty_without_rsc_bundler_dependency() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        let modules = Vec::new();
        let declared: FxHashSet<String> = std::iter::once("react".to_string()).collect();
        let suppressions = SuppressionContext::empty();
        let offsets = FxHashMap::default();

        let findings =
            find_misplaced_directives(&graph, &modules, &declared, &suppressions, &offsets);
        assert!(
            findings.is_empty(),
            "no RSC bundler dependency means no findings"
        );
    }
}
