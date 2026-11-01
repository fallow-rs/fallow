//! A `require` function based on the file itself loads the same modules as the
//! CommonJS `require`, in any spelling: the inline `createRequire(...)(...)`
//! call, `.resolve` on that call, and a binding with another name. A call with
//! another base resolves from another directory, so it credits nothing.

use super::common::{create_config, fixture_path};

fn file_name(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

fn analyze_fixture() -> fallow_core::results::AnalysisResults {
    let config = create_config(fixture_path("create-require-inline"));
    fallow_core::analyze(&config).expect("analysis should succeed")
}

#[cfg_attr(miri, ignore)]
#[test]
fn this_file_require_forms_credit_the_package() {
    let results = analyze_fixture();
    let unused: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    for credited in [
        "inline-load",
        "inline-subpath",
        "inline-chained",
        "inline-resolve",
        "renamed-load",
        "renamed-resolve",
        "hoisted-load",
        "aliased-import",
    ] {
        assert!(
            !unused.contains(&credited),
            "{credited} is loaded through a require function of this file, got: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"never-loaded"),
        "a package that no code loads must stay unused, got: {unused:?}"
    );
    assert!(
        unused.contains(&"other-base-pkg"),
        "a call with another base resolves elsewhere and must not credit, got: {unused:?}"
    );
}

#[cfg_attr(miri, ignore)]
#[test]
fn inline_load_credits_the_file_and_narrows_its_exports() {
    let results = analyze_fixture();
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file_name(&file.file.path))
        .collect();
    assert_eq!(unused_files, ["orphan.js"]);

    let mut unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|e| (file_name(&e.export.path), e.export.export_name.clone()))
        .filter(|(file, _)| file != "index.js")
        .collect();
    unused_exports.sort();
    assert_eq!(
        unused_exports,
        [
            ("destructured.js".to_string(), "d".to_string()),
            ("whole.js".to_string(), "b".to_string()),
        ]
    );
}

#[cfg_attr(miri, ignore)]
#[test]
fn inline_load_reports_the_same_findings_as_the_named_form() {
    let results = analyze_fixture();
    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.import.specifier.as_str())
        .collect();
    assert_eq!(unresolved, ["./missing.js"]);

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert_eq!(unlisted, ["not-listed-pkg"]);
}
