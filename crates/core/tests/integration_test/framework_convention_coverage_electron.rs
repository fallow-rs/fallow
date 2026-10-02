use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};

#[test]
fn electron_vite_rollup_input_entries_keep_renderer_and_preload_trees_alive() {
    let root = fixture_path("issue-600-electron-vite-rollup-input");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);

    for credited in [
        "src/renderer/main-window.ts",
        "src/renderer/shared.ts",
        "src/renderer/settings/settings.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == credited),
            "{credited} should be reachable via a declared renderer HTML entry, unused files: {unused_files:?}"
        );
    }

    for credited in ["electron/preload-bridge.ts", "electron/bridge-helper.ts"] {
        assert!(
            !unused_files.iter().any(|path| path == credited),
            "{credited} should be reachable via a declared preload rollup input, unused files: {unused_files:?}"
        );
    }

    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/renderer/orphan.ts"),
        "orphan renderer file must remain reportable, unused files: {unused_files:?}"
    );
}

#[test]
fn electron_vite_empty_sections_use_default_entries_only() {
    let root = fixture_path("electron-vite-default-entries");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for entry in ["src/main/index.ts", "src/preload/index.ts"] {
        assert!(
            !unused_files.iter().any(|path| path == entry),
            "{entry} is an electron-vite default entry, unused files: {unused_files:?}"
        );
    }

    let unused_exports = collect_unused_exports(&root, &results);
    assert!(
        has_unused_export(&unused_exports, "src/main/helper.ts", "unusedHelper"),
        "a module imported from the main entry is not an entry itself, unused exports: {unused_exports:?}"
    );
    assert!(
        !has_unused_export(&unused_exports, "src/main/helper.ts", "used"),
        "used must stay credited through the main entry import, unused exports: {unused_exports:?}"
    );
}

fn assert_declared_main_entry_replaces_default(fixture: &str) {
    let root = fixture_path(fixture);
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);
    for reachable in [
        "src/main/app.ts",
        "src/main/start.ts",
        "src/preload/index.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == reachable),
            "{reachable} should stay reachable, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files.iter().any(|path| path == "src/main/index.ts"),
        "a declared main entry replaces the default, so src/main/index.ts must be unused: {unused_files:?}"
    );
}

#[test]
fn electron_vite_main_rollup_input_replaces_default_entry() {
    assert_declared_main_entry_replaces_default("electron-vite-main-rollup-input");
}

#[test]
fn electron_vite_main_lib_entry_replaces_default_entry() {
    assert_declared_main_entry_replaces_default("electron-vite-main-lib-entry");
}
