use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::collect_unused_files;

#[test]
fn electron_vite_rollup_input_entries_keep_renderer_and_preload_trees_alive() {
    let root = fixture_path("issue-600-electron-vite-rollup-input");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = collect_unused_files(&root, &results);

    // Acceptance criterion 1 + 4: multi-window renderer HTML entries declared in
    // `renderer.build.rollupOptions.input` are treated as entry points, so the
    // `<script src>` trees behind both windows stop reporting as unused.
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

    // Acceptance criterion 2: a preload entry declared in config at a path NOT
    // covered by the static `src/preload/**` globs is seeded as an entry point,
    // and its imported helper becomes reachable.
    for credited in ["electron/preload-bridge.ts", "electron/bridge-helper.ts"] {
        assert!(
            !unused_files.iter().any(|path| path == credited),
            "{credited} should be reachable via a declared preload rollup input, unused files: {unused_files:?}"
        );
    }

    // Scope guard: a renderer source file linked from no declared entry must stay
    // reportable. The fix credits declared entries, not the whole renderer tree.
    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/renderer/orphan.ts"),
        "orphan renderer file must remain reportable, unused files: {unused_files:?}"
    );
}
