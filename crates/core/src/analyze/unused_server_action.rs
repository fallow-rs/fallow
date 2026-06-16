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
//! server" }` in a non-`"use server"` file) are ALSO reclassified: the extract
//! layer records the export local name of every exported function / const-arrow
//! whose body carries an inline `"use server"` directive on
//! [`ModuleInfo::inline_server_action_exports`](fallow_types::extract::ModuleInfo),
//! and an unused export whose name appears there is moved into
//! `unused_server_actions` just like a whole-`"use server"`-file export. The same
//! `is_type_only` / `is_re_export` skips apply, so this inherits every
//! unused-export abstain as well.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::{AnalysisResults, UnusedServerAction, UnusedServerActionFinding};
use crate::suppress::{IssueKind, SuppressionContext};

/// Move unused exports of `"use server"` files (and inline `"use server"` body
/// actions) into `unused_server_actions`.
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

    // Per-file inline `"use server"` body action export names (a non-use-server
    // file can still export an inline Server Action). Keyed by FileId so the
    // membership check can ask "does this file's inline set contain the export
    // name?".
    let inline_actions_by_id: FxHashMap<FileId, &Vec<String>> = modules
        .iter()
        .filter(|m| !m.inline_server_action_exports.is_empty())
        .map(|m| (m.file_id, &m.inline_server_action_exports))
        .collect();

    if use_server_ids.is_empty() && inline_actions_by_id.is_empty() {
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
        // Conservative: only direct value exports (a use-server file export, or
        // an inline `"use server"` body action in any file).
        if export.is_type_only || export.is_re_export {
            return true;
        }
        let Some(&file_id) = file_id_by_path.get(export.path.as_path()) else {
            return true;
        };
        let is_whole_file_action = use_server_ids.contains(&file_id);
        let is_inline_action = inline_actions_by_id
            .get(&file_id)
            .is_some_and(|names| names.contains(&export.export_name));
        if !is_whole_file_action && !is_inline_action {
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
