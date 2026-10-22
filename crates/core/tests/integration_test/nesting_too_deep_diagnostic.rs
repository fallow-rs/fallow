//! Integration test for the `source-nesting-too-deep` diagnostic.
//!
//! A file that nests brackets past the cap is not parsed at all. It is valid
//! code, so it must not be reported as a syntax error, but it degrades the run
//! in the same way: its imports credit nothing.

use std::path::Path;

use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

use super::common::create_config_with_cache;

fn kinds_for(root: &Path) -> Vec<WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SourceNestingTooDeep { .. }
                    | WorkspaceDiagnosticKind::SourceParseDegraded { .. }
            )
        })
        .collect()
}

/// A project whose entry imports a helper and nests brackets past the cap.
fn write_deep_project(root: &Path) {
    let depth = fallow_extract::MAX_NESTING_DEPTH as usize + 10;
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "nesting-too-deep", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/helper.ts"),
        "export const helper = (): number => 1;\n",
    )
    .expect("write helper module");
    std::fs::write(
        root.join("src/index.ts"),
        format!(
            "import {{ helper }} from \"./helper\";\nexport const run = {}helper(){};\n",
            "(".repeat(depth),
            ")".repeat(depth)
        ),
    )
    .expect("write deep entry module");
}

#[test]
fn a_file_past_the_nesting_cap_is_reported_as_too_deep() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_deep_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("analysis succeeds on a deep source");

    let diagnostics = kinds_for(&config.root);
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(diagnostics[0].path.ends_with("src/index.ts"));
    assert_eq!(
        diagnostics[0].kind,
        WorkspaceDiagnosticKind::SourceNestingTooDeep {
            limit: fallow_extract::MAX_NESTING_DEPTH
        },
        "a valid file that is too deep is not a syntax error"
    );
}

#[test]
fn the_too_deep_kind_survives_a_warm_cache() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_deep_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("cold analysis succeeds");
    let _ = fallow_core::analyze(&config).expect("warm analysis succeeds");

    let diagnostics = kinds_for(&config.root);
    assert_eq!(diagnostics.len(), 1, "got {diagnostics:?}");
    assert!(matches!(
        diagnostics[0].kind,
        WorkspaceDiagnosticKind::SourceNestingTooDeep { .. }
    ));
}
