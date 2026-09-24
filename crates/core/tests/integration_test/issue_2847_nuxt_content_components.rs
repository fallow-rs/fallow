//! Issue #2847: Nuxt global components that no template tag names.
//!
//! - With `@nuxt/content` registered, each `components/content/` directory of
//!   the project and of its local layers holds global components that Markdown
//!   content renders. Fallow does not read Markdown, so these files are entry
//!   points with `autoImports` on and off.
//! - Without `@nuxt/content`, a `components/content/` file is an ordinary
//!   component and reports with `autoImports` on when nothing renders it.
//! - `components/global/` and `*.global.*` files are global components. A
//!   string reference such as `resolveComponent('Bar')` can render them, so
//!   they stay entry points with `autoImports` on.

use std::path::Path;

use super::common::{create_config, fixture_path};
use fallow_types::results::AnalysisResults;

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn analyze_unused_files(fixture: &str, auto_imports: bool) -> Vec<String> {
    let root = fixture_path(fixture);
    let mut config = create_config(root.clone());
    config.auto_imports = auto_imports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    unused_file_paths(&results, &root)
}

const CONTENT_COMPONENTS: &[&str] = &[
    "components/content/Alert.vue",
    "layers/docs/components/content/Note.vue",
];

#[test]
fn content_components_stay_used_with_auto_imports_on() {
    let unused = analyze_unused_files("nuxt-content-components", true);

    for kept in CONTENT_COMPONENTS {
        assert!(
            !unused.contains(&(*kept).to_string()),
            "{kept} is a global @nuxt/content component and must not report, got: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"components/Dead.vue".to_string()),
        "an ordinary unreferenced component must still report, got: {unused:?}"
    );
}

#[test]
fn content_components_stay_used_with_auto_imports_off() {
    let unused = analyze_unused_files("nuxt-content-components", false);

    for kept in CONTENT_COMPONENTS {
        assert!(
            !unused.contains(&(*kept).to_string()),
            "{kept} must not report with autoImports off, got: {unused:?}"
        );
    }
}

#[test]
fn content_directory_without_the_module_reports_with_auto_imports_on() {
    let unused = analyze_unused_files("nuxt-content-components-without-module", true);

    for dead in CONTENT_COMPONENTS {
        assert!(
            unused.contains(&(*dead).to_string()),
            "without @nuxt/content, {dead} is an ordinary component and must report, \
             got: {unused:?}"
        );
    }
}

#[test]
fn global_components_stay_used_with_auto_imports_on() {
    let unused = analyze_unused_files("nuxt-global-components", true);

    for kept in ["components/global/Bar.vue", "components/Baz.global.vue"] {
        assert!(
            !unused.contains(&kept.to_string()),
            "{kept} is a global component and must not report, got: {unused:?}"
        );
    }
}
