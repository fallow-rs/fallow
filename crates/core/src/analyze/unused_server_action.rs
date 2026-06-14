//! Reclassification of unused Next.js Server Actions.
//!
//! A Next.js Server Action is an export of a `"use server"` file. When no code in
//! the project references such an export (no import-and-call, no `action={fn}`
//! JSX binding, no `<form action={fn}>`), it is ALSO an unused export, because
//! the `action={...}` / `<form action={...}>` bindings already credit the export
//! as a value-position reference through `oxc_semantic` (see `unused_exports`).
//!
//! This pass MOVES that server-action subset out of `unused_exports` into
//! `unused_server_actions`, the more specific and more actionable finding, so the
//! two never double-report. Reclassifying from the already-computed
//! `unused_exports` findings (rather than re-deriving the reachability predicate)
//! inherits EVERY abstain `unused-exports` already applies (entry-point skip,
//! public-API re-export crediting, whole-object / namespace opacity,
//! reachability). The marginal false-positive surface over `unused-exports` is
//! therefore just the literal `"use server"` directive gate.
//!
//! It does NOT mean the endpoint is unreachable: Next.js still registers a
//! generated action id, so the action stays POST-able. It means no project code
//! references it (likely forgotten / dead, and a candidate for removal).
//!
//! Conservative additional abstains kept as plain `unused-export`:
//! - type-only exports (an action is a runtime function, never a type),
//! - re-export shapes (`export { x } from './y'`): the definition lives
//!   elsewhere, so the directive on this barrel does not make `x` an action.
//!
//! Inline `"use server"` body directives (`export async function f() { "use
//! server" }` in a non-`"use server"` file) are deferred to a later revision;
//! such dead actions still surface as `unused-export` until then.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::{AnalysisResults, UnusedServerAction, UnusedServerActionFinding};
use crate::suppress::{IssueKind, SuppressionContext};

/// Move unused exports of `"use server"` files into `unused_server_actions`.
///
/// Gated on the project declaring `next`. The caller only invokes this when the
/// `unused-server-action` rule is enabled; when it is `off`, the findings stay
/// under `unused_exports` unchanged (no reclassification, no gate relaxation).
///
/// A finding suppressed under `unused-server-action` is dropped from BOTH buckets
/// and the suppression is recorded as consumed, so it is not later reported stale.
pub fn reclassify_unused_server_actions(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    suppressions: &SuppressionContext<'_>,
    results: &mut AnalysisResults,
) {
    if !declared_deps.contains("next") {
        return;
    }

    // FileIds of `"use server"` files (the directive lives on ModuleInfo).
    let use_server_ids: FxHashSet<FileId> = modules
        .iter()
        .filter(|m| m.directives.iter().any(|d| d == "use server"))
        .map(|m| m.file_id)
        .collect();

    if use_server_ids.is_empty() {
        return;
    }

    // The export `path` is the graph node path; map it back to a FileId so the
    // use-server membership and suppression checks can key on the right module.
    let file_id_by_path: FxHashMap<&Path, FileId> = graph
        .modules
        .iter()
        .map(|node| (node.path.as_path(), node.file_id))
        .collect();

    let mut reclassified: Vec<UnusedServerAction> = Vec::new();
    results.unused_exports.retain(|finding| {
        let export = &finding.export;
        // Conservative: only direct value exports defined in a use-server file.
        if export.is_type_only || export.is_re_export {
            return true;
        }
        let Some(&file_id) = file_id_by_path.get(export.path.as_path()) else {
            return true;
        };
        if !use_server_ids.contains(&file_id) {
            return true;
        }
        // Suppressed as unused-server-action: drop from both buckets and mark
        // the marker consumed (so it is not reported stale).
        if suppressions.is_suppressed(file_id, export.line, IssueKind::UnusedServerAction)
            || suppressions.is_file_suppressed(file_id, IssueKind::UnusedServerAction)
        {
            return false;
        }
        reclassified.push(UnusedServerAction {
            path: export.path.clone(),
            action_name: export.export_name.clone(),
            line: export.line,
            col: export.col,
        });
        false
    });

    results.unused_server_actions = reclassified
        .into_iter()
        .map(UnusedServerActionFinding::with_actions)
        .collect();
}
