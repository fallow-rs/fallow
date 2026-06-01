//! Issue #739: script-level convention auto-import resolution for Nuxt.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn normalize_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| normalize_path(root, &finding.file.path))
        .collect()
}

fn unused_exports(results: &AnalysisResults, root: &Path) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                normalize_path(root, &finding.export.path),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

#[test]
fn flag_off_keeps_script_convention_files_alive() {
    let root = fixture_path("nuxt-script-auto-imports");
    let config = create_config(root.clone());
    assert!(!config.auto_imports, "default is additive");

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        !unused.contains(&"composables/useLocalThing.ts".to_string()),
        "flag off keeps top-level composable fallback patterns: {unused:?}"
    );
}

#[test]
fn flag_on_keeps_script_auto_import_providers_reachable() {
    let root = fixture_path("nuxt-script-auto-imports");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    for reachable in [
        "composables/useCounter.ts",
        "composables/index.ts",
        "composables/useCollision.ts",
        "composables/useExplicitThing.ts",
        "utils/format-price.ts",
        "utils/useCollision.ts",
        "utils/useTsOnly.ts",
        "shared/utils/sharedThing.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} should be reachable via real or synthesized import edge: {unused:?}"
        );
    }

    for unreachable in [
        "composables/useLocalThing.ts",
        "composables/useRoute.ts",
        "composables/UseTypeOnly.ts",
    ] {
        assert!(
            unused.contains(&unreachable.to_string()),
            "{unreachable} should not be credited by local, built-in, or type-only refs: {unused:?}"
        );
    }
}

#[test]
fn include_entry_exports_credits_script_auto_import_exports() {
    let root = fixture_path("nuxt-script-auto-imports");
    let mut config = create_config(root.clone());
    config.auto_imports = true;
    config.include_entry_exports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_exports(&results, &root);

    assert!(
        !unused.contains(&(
            "composables/useCounter.ts".to_string(),
            "useCounter".to_string()
        )),
        "auto-imported named export should be credited: {unused:?}"
    );
    assert!(
        unused.contains(&(
            "composables/useCounter.ts".to_string(),
            "neverUsedCounterExport".to_string()
        )),
        "unreferenced sibling export should still report: {unused:?}"
    );
    assert!(
        !unused.contains(&("composables/index.ts".to_string(), "fromIndex".to_string())),
        "named exports from index files should be credited: {unused:?}"
    );
    assert!(
        unused.contains(&(
            "composables/index.ts".to_string(),
            "unusedIndexExport".to_string()
        )),
        "unreferenced index export should still report: {unused:?}"
    );
}

#[test]
fn custom_imports_config_keeps_script_fallback_patterns() {
    let root = fixture_path("nuxt-script-auto-imports-custom-imports");
    let mut config = create_config(root.clone());
    config.auto_imports = true;

    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results, &root);

    assert!(
        !unused.contains(&"composables/unusedCustomFallback.ts".to_string()),
        "imports config should keep composable fallback entry patterns: {unused:?}"
    );
}
