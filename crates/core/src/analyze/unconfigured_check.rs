//! Diagnostics for a detector whose zero count measures nothing.

use fallow_config::{ResolvedConfig, WorkspaceDiagnosticKind};

/// Report that a detector produced structurally zero findings because it was
/// never configured or had no starting point, so a consumer does not read the
/// counter as a measurement.
///
/// `rules: off` is deliberately NOT reported here: that zero is the user's own
/// choice and is already visible in `fallow config`.
pub(super) fn record_unconfigured_check(config: &ResolvedConfig, kind: WorkspaceDiagnosticKind) {
    // Anchored at the project root, stored project-relative: the envelopes'
    // post-serialisation strip only removes a `root + separator` prefix, so a
    // root-anchored absolute path would reach JSON output verbatim.
    let diagnostic =
        fallow_config::WorkspaceDiagnostic::new(&config.root, config.root.clone(), kind)
            .into_root_relative(&config.root);
    fallow_config::record_workspace_diagnostics(&config.root, vec![diagnostic]);
}
