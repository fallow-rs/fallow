//! Recording the health pipeline's own degraded inputs as workspace
//! diagnostics.
//!
//! Every site that prints a note about an input which did not load also writes
//! the fact here, so it survives `--quiet --format json`: both shipped CI
//! integrations and the MCP read `workspace_diagnostics[]` and drop stderr, so
//! a degradation that exists only as a printed line reaches nobody (issue
//! #2689).
//!
//! # Why the registry rather than a threaded list
//!
//! The health envelope captures `workspace_diagnostics` before the analysis
//! starts, and the degradations are produced five call chains deep in scoring,
//! churn, ownership, trend comparison and coverage resolution. One merge at
//! finalize (see `super::result`) reaches all of them; threading a mutable list
//! would touch every signature in between for the same result.
//!
//! This is safe here for a reason the analyze stage's own comments call out as
//! the thing to check: these entries have a single writer per run and are not
//! produced under `rayon::join`, so a registry read cannot answer "whichever
//! pass wrote last".
//!
//! # Why `append` rather than `record`
//!
//! [`fallow_config::append_workspace_diagnostics`] writes without printing.
//! Each site keeps its own hand-written note, so
//! `record_workspace_diagnostics` would print every message a second time.

use std::path::Path;

use fallow_types::workspace::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

/// Record one health-pipeline diagnostic for `root`.
///
/// `path` names the file that triggered it. `None` anchors the diagnostic at
/// the project root, which renders as `.` rather than the empty string an
/// unanchored path would produce.
pub fn record_health_diagnostic(root: &Path, path: Option<&Path>, kind: WorkspaceDiagnosticKind) {
    let diagnostic = match path {
        Some(path) => WorkspaceDiagnostic::new(root, path.to_path_buf(), kind),
        None => WorkspaceDiagnostic::new(root, root.to_path_buf(), kind).into_root_relative(root),
    };
    fallow_config::append_workspace_diagnostics(root, vec![diagnostic]);
}
