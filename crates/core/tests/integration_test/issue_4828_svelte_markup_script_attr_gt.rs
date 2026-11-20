//! Issue #4828: a `>` inside a brace attribute expression of a `<script>`
//! element in Svelte markup must not end the opening tag. When it does, the
//! rest of the expression becomes a script body that fails to parse, and the
//! `source-parse-degraded` diagnostic then caveats every finding of the run.

use fallow_config::WorkspaceDiagnosticKind;
use fallow_types::output_dead_code::ReachabilityCaveat;

use super::common::{create_config, fixture_path};

#[test]
fn a_gt_in_a_markup_script_attribute_parses_without_degradation() {
    let config = create_config(fixture_path("issue-4828-svelte-markup-script-attr-gt"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let degraded: Vec<_> = fallow_config::workspace_diagnostics_for(&config.root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SourceParseDegraded { .. }
            )
        })
        .collect();
    assert!(degraded.is_empty(), "no file is degraded: {degraded:?}");

    let unused = results
        .unused_exports
        .iter()
        .find(|issue| issue.export.export_name == "neverImported")
        .expect("the export that no module imports is reported");
    assert!(
        !unused
            .reachability_caveats
            .contains(&ReachabilityCaveat::IncompleteImportGraph),
        "a clean parse gives no graph caveat, got {:?}",
        unused.reachability_caveats
    );
    assert!(
        results
            .unused_exports
            .iter()
            .all(|issue| issue.export.export_name != "embedUrl"),
        "the instance script import of the component credits `embedUrl`"
    );
}
