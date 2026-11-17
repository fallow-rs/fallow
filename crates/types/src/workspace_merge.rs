//! The fold of two `workspace_diagnostics[]` lists into one.

use std::path::PathBuf;

use rustc_hash::FxHashSet;

use super::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Concatenate two diagnostic lists, keeping the first occurrence of each
/// `(kind, path)` pair and the order of `primary` followed by the entries only
/// `secondary` has.
///
/// The single place diagnostics from two observation points are folded
/// together: an engine session's own capture plus the process registry, and
/// the combined run's per-analysis lists (issue #2366). A combined run walks
/// the project once per analysis, and per-analysis `production` modes can make
/// those walks see different file sets, so no single observation point holds
/// everything the run recorded; the union does, and folding it the same way
/// everywhere is what keeps the CLI and the programmatic route answering
/// identically.
///
/// The key is the WHOLE kind, payload included, not its
/// [`id`](WorkspaceDiagnosticKind::id). Two entries can share a kind id and a
/// path and still be two distinct diagnostics: overlapping workspace globs
/// (`["packages/*", "packages/*/*"]`) each report the same package-less
/// directory with their own `pattern`, and the standalone envelopes report
/// both. An id-keyed fold silently dropped the second one.
#[must_use]
pub fn merge_workspace_diagnostics(
    primary: Vec<WorkspaceDiagnostic>,
    secondary: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    let mut merged = Vec::with_capacity(primary.len() + secondary.len());
    let mut seen: FxHashSet<(WorkspaceDiagnosticKind, PathBuf)> = FxHashSet::default();
    for diagnostic in primary.into_iter().chain(secondary) {
        let key = (diagnostic.kind.clone(), diagnostic.path.clone());
        if seen.insert(key) {
            merged.push(diagnostic);
        }
    }
    merged
}

/// Keep the first occurrence of each `(kind, path)` pair in one list.
///
/// The single-list form of [`merge_workspace_diagnostics`], applied where
/// diagnostics are produced rather than where two observation points are
/// folded: workspace discovery reads `package.json` `workspaces`,
/// `pnpm-workspace.yaml` `packages`, `deno.json` `workspace` and the root
/// `tsconfig.json` references additively, so a repository that declares one
/// glob in two of them reports every package-less directory under it twice.
/// Deduplicating at that source is what keeps the JSON envelopes, the
/// aggregated stderr warning and the process registry telling one story
/// (issue #2366).
#[must_use]
pub fn dedupe_workspace_diagnostics(
    diagnostics: Vec<WorkspaceDiagnostic>,
) -> Vec<WorkspaceDiagnostic> {
    merge_workspace_diagnostics(diagnostics, Vec::new())
}
