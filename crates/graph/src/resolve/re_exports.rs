//! Resolution of re-export sources (`export { x } from './y'`).
//!
//! Maps each re-export's source specifier to a resolution target, producing
//! [`ResolvedReExport`] entries. These are consumed by the graph builder to
//! construct re-export edges, which are later propagated through barrel file
//! chains in the graph's re-export resolution phase.
//!
//! Like `static_imports`, this is a direct 1:1 mapping — the interesting
//! chain resolution logic lives in `graph/re_exports/`, not here.

use std::path::Path;

use fallow_types::extract::ReExportInfo;

use super::ResolvedReExport;
use super::specifier::resolve_specifier;
use super::types::ResolveContext;

/// Resolve re-export sources (`export { x } from './y'`).
pub(super) fn resolve_re_exports(
    ctx: &ResolveContext,
    file_path: &Path,
    re_exports: &[ReExportInfo],
) -> Vec<ResolvedReExport> {
    re_exports
        .iter()
        .map(|re| ResolvedReExport {
            info: re.clone(),
            target: resolve_specifier(ctx, file_path, &re.source, false),
        })
        .collect()
}
