//! Integration test for the skipped-source-dotdir diagnostic (issue #461).
//!
//! Fixture `tests/fixtures/hidden-dir-allowlist/` holds one dot-prefixed
//! directory of each shape the predicate has to separate: `.hidden-other`
//! (holds `secret.ts` and is neither allowlisted nor denylisted, so it is the
//! one skip worth reporting), `.storybook` (on `ALLOWED_HIDDEN_DIRS`, so it is
//! traversed and never a candidate), and `.fallow` (on
//! `SCRIPT_SCOPE_DENYLIST`, and holding only cache blobs anyway).
//!
//! Discovery has always dropped `.hidden-other` silently. This test pins that
//! the skip is now observable: exactly one `skipped-source-dotdir` diagnostic
//! anchored at that directory, carrying both real remedies and the plain
//! statement that no config field traverses it, with no duplicate on a second
//! analysis of the same root. Traversal itself is unchanged.

use std::path::Path;

use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "hidden-dir-allowlist";

fn skip_diagnostics(root: &Path) -> Vec<WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SkippedSourceDotdir
            )
        })
        .collect()
}

#[test]
fn skipped_source_dotdir_is_reported_once_with_both_real_remedies() {
    let root = fixture_path(FIXTURE);
    let config = create_config(root.clone());

    let _ = fallow_core::analyze(&config).expect("analysis should succeed");

    let diagnostics = skip_diagnostics(&root);
    assert_eq!(
        diagnostics.len(),
        1,
        "only the non-allowlisted, non-denylisted dotdir with source files reports: {diagnostics:?}"
    );
    let diagnostic = &diagnostics[0];
    assert_eq!(
        diagnostic.path.display().to_string().replace('\\', "/"),
        root.join(".hidden-other")
            .display()
            .to_string()
            .replace('\\', "/"),
        "the diagnostic is anchored at the skipped directory"
    );
    assert_eq!(diagnostic.kind.id(), "skipped-source-dotdir");
    assert!(
        diagnostic.message.contains(".hidden-other")
            && diagnostic
                .message
                .contains("Its imports and exports are not analyzed.")
            && diagnostic.message.contains("--root")
            && diagnostic.message.contains("ignorePatterns")
            && diagnostic.message.contains("no config field"),
        "message names the directory, the consequence, both remedies, and the absence of a \
         config field: {}",
        diagnostic.message
    );

    // Watch-mode and combined-mode reruns analyze the same root again; the
    // registry must keep a single entry.
    let _ = fallow_core::analyze(&config).expect("second analysis should succeed");
    assert_eq!(
        skip_diagnostics(&root).len(),
        1,
        "a repeat walk replaces its own source-discovery set instead of stacking"
    );
}
