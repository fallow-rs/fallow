//! CSS-in-JS first-class, Phase 3a (characterization): a dead `styled` / `css` /
//! vanilla-extract `style` binding is already a normal JS value export, so
//! `unused-export` covers it with zero gap, and the CSS-in-JS libraries are
//! imported as value bindings (`import styled from "styled-components"`) so they
//! are credited as used (no false `unused-dependency`). This test pins that
//! coverage so a future refactor that accidentally credits a dead styled binding
//! as used, or flags a CSS-in-JS library as unused, is caught. 3a adds NO new
//! detection code and NO new `IssueKind`; this is a no-regression characterization.

use super::common::{create_config, fixture_path};

#[test]
fn dead_styled_bindings_report_as_unused_export_and_libs_are_credited() {
    let root = fixture_path("css-in-js-styled");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // (criterion 1) A dead styled-components binding reports as `unused-export`.
    assert!(
        unused_export_names.contains(&"DeadCard"),
        "a dead styled-components binding should report as unused-export: {unused_export_names:?}"
    );
    // (criterion 1) A dead emotion `css` binding reports as `unused-export`.
    assert!(
        unused_export_names.contains(&"deadStyle"),
        "a dead emotion css binding should report as unused-export: {unused_export_names:?}"
    );
    // (criterion 4) A dead vanilla-extract `.css.ts` `style` export reports.
    assert!(
        unused_export_names.contains(&"deadBox"),
        "a dead vanilla-extract style export should report as unused-export: {unused_export_names:?}"
    );

    // (criterion 2) Every LIVE styled binding is credited (never flagged).
    for live in ["Button", "PrimaryButton", "Box", "liveStyle", "container"] {
        assert!(
            !unused_export_names.contains(&live),
            "live styled binding {live} must not be flagged: {unused_export_names:?}"
        );
    }

    // The CSS-in-JS libraries are imported as value bindings, so they are
    // credited as used: none must surface as an unused dependency.
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    for lib in [
        "styled-components",
        "@emotion/styled",
        "@emotion/react",
        "@vanilla-extract/css",
    ] {
        assert!(
            !unused_dep_names.contains(&lib),
            "CSS-in-JS library {lib} must be credited as used, not flagged unused: {unused_dep_names:?}"
        );
    }
}
