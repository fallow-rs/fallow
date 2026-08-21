//! Integration test for the bun.lockb-only override-resolution skip
//! diagnostic (issue #2358).
//!
//! Fixture under `tests/fixtures/issue-2358-bun-lockb-diagnostic/` is a bun
//! workspace whose root `package.json` declares an `overrides` entry (`ws`)
//! and whose only lockfile is a placeholder `bun.lockb` (never parsed; only
//! its existence matters). The member `packages/app` declares no overrides.
//!
//! Since #2341 the unused-override check stays silent here because bun's
//! binary lockfile carries no readable resolution data. This test pins that
//! the silence is explained: exactly one
//! `bun-lockb-override-resolution-skipped` workspace diagnostic, anchored at
//! the root `package.json`, with the text-lockfile hint, and no duplicate on
//! a second analysis of the same root.

use fallow_config::{FallowConfig, OutputFormat, WorkspaceDiagnostic, WorkspaceDiagnosticKind};

use super::common::fixture_path;

const FIXTURE: &str = "issue-2358-bun-lockb-diagnostic";

fn skip_diagnostics(root: &std::path::Path) -> Vec<WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped
            )
        })
        .collect()
}

#[test]
fn bun_lockb_only_workspace_records_one_skip_diagnostic_at_root_manifest() {
    let root = fixture_path(FIXTURE);
    let config =
        FallowConfig::default().resolve(root.clone(), OutputFormat::Human, 4, true, true, None);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.unused_dependency_overrides.is_empty(),
        "bun.lockb is unreadable, so the unused-override check must stay skipped; flagged: {:?}",
        results.unused_dependency_overrides
    );

    let diagnostics = skip_diagnostics(&root);
    assert_eq!(
        diagnostics.len(),
        1,
        "one diagnostic per affected root manifest, none for the member without overrides: {diagnostics:?}"
    );
    let diagnostic = &diagnostics[0];
    assert_eq!(
        diagnostic.path.display().to_string().replace('\\', "/"),
        root.join("package.json")
            .display()
            .to_string()
            .replace('\\', "/"),
        "the diagnostic is anchored at the root package.json"
    );
    assert_eq!(
        diagnostic.kind.id(),
        "bun-lockb-override-resolution-skipped"
    );
    assert!(
        diagnostic.message.contains("'package.json'")
            && diagnostic.message.contains("only bun.lockb was found")
            && diagnostic
                .message
                .contains("bun install --save-text-lockfile"),
        "message names the manifest, the cause, and the text-lockfile next step: {}",
        diagnostic.message
    );

    // Watch-mode and combined-mode reruns analyze the same root again; the
    // registry must keep a single entry.
    let _ = fallow_core::analyze(&config).expect("second analysis should succeed");
    assert_eq!(
        skip_diagnostics(&root).len(),
        1,
        "a second analysis must not stack a duplicate diagnostic"
    );
}
