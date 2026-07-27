//! Regression tests for issue #2005: `canvas` reported as an unused
//! devDependency in a vitest + jsdom project.
//!
//! `canvas` is an optional peer of `jsdom`, loaded lazily at runtime when it is
//! installed, so a project that installs it to give jsdom real canvas support
//! has no import of it anywhere in its own source. The reporter acted on the
//! finding shape that `fallow fix` automates, which would have removed the
//! dependency and broken every canvas-backed test.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// The reporter's shape: `canvas` and an unrelated unused package are both
/// declared, and the vitest config selects the jsdom environment.
fn create_project(root: &Path, environment: &str) {
    write(
        &root.join("package.json"),
        r#"{
            "name": "jsdom-canvas-repro",
            "private": true,
            "devDependencies": {
                "vitest": "^4.1.5",
                "jsdom": "^29.1.1",
                "canvas": "^3.2.3",
                "left-pad": "^1.3.0"
            }
        }"#,
    );
    write(
        &root.join("vite.config.ts"),
        &format!(
            r#"export default {{
                test: {{ environment: "{environment}" }},
            }};"#
        ),
    );
    write(
        &root.join("src/index.ts"),
        r"export const render = (): string => 'ok';",
    );
    write(
        &root.join("tests/render.test.ts"),
        r#"import { render } from "../src/index";
           it("renders", () => { render(); });"#,
    );
}

fn unused_dev_dependencies(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect()
}

#[test]
#[cfg_attr(miri, ignore)]
fn issue_2005_jsdom_environment_credits_optional_canvas_peer() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path(), "jsdom");

    let config = create_config(dir.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_dev_dependencies(&results);
    assert!(
        !unused.contains(&"canvas"),
        "canvas is an optional jsdom peer loaded at runtime, got {unused:?}"
    );
    assert!(
        unused.contains(&"left-pad"),
        "an unrelated unused devDependency must still be reported, got {unused:?}"
    );
}

/// A jest project selecting jsdom hits the same optional peer. Both the fully
/// qualified package name and the bare shorthand select the same environment,
/// and the inline `projects[]` form is a separate code path from the scalar one.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_2005_jest_jsdom_environment_credits_optional_canvas_peer() {
    for config in [
        r#"module.exports = { testEnvironment: "jsdom" };"#,
        r#"module.exports = { testEnvironment: "jest-environment-jsdom" };"#,
        r#"module.exports = { projects: [{ testEnvironment: "jest-environment-jsdom" }] };"#,
    ] {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{
                "name": "jest-jsdom-canvas-repro",
                "private": true,
                "devDependencies": {
                    "jest": "^29.0.0",
                    "jest-environment-jsdom": "^29.0.0",
                    "canvas": "^3.2.3",
                    "left-pad": "^1.3.0"
                }
            }"#,
        );
        write(&root.join("jest.config.js"), config);
        write(
            &root.join("src/index.ts"),
            r"export const render = (): string => 'ok';",
        );
        write(
            &root.join("tests/render.test.ts"),
            r#"import { render } from "../src/index";
               it("renders", () => { render(); });"#,
        );

        let results = fallow_core::analyze(&create_config(root.to_path_buf()))
            .expect("analysis should succeed");
        let unused = unused_dev_dependencies(&results);
        assert!(
            !unused.contains(&"canvas"),
            "`{config}` should credit the optional jsdom peer, got {unused:?}"
        );
        assert!(
            !unused.contains(&"jest-environment-jsdom"),
            "the declared environment package must stay credited for `{config}`, got {unused:?}"
        );
        assert!(
            unused.contains(&"left-pad"),
            "an unrelated unused devDependency must still be reported for `{config}`, got {unused:?}"
        );
    }
}

/// The node environment takes no canvas peer, so canonicalization must not have
/// widened the built-in arm into crediting canvas everywhere.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_2005_jest_node_environment_does_not_credit_canvas() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "jest-node-canvas-repro",
            "private": true,
            "devDependencies": { "jest": "^29.0.0", "canvas": "^3.2.3" }
        }"#,
    );
    write(
        &root.join("jest.config.js"),
        r#"module.exports = { testEnvironment: "node" };"#,
    );
    write(
        &root.join("src/index.ts"),
        r"export const render = (): string => 'ok';",
    );
    write(
        &root.join("tests/render.test.ts"),
        r#"import { render } from "../src/index";
           it("renders", () => { render(); });"#,
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    let unused = unused_dev_dependencies(&results);
    assert!(
        unused.contains(&"canvas"),
        "the node environment takes no canvas peer, got {unused:?}"
    );
}

/// happy-dom ships its own canvas implementation and declares no `canvas` peer,
/// so selecting it must not silently exempt a genuinely unused `canvas`.
#[test]
#[cfg_attr(miri, ignore)]
fn issue_2005_happy_dom_environment_does_not_credit_canvas() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path(), "happy-dom");

    let config = create_config(dir.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_dev_dependencies(&results);
    assert!(
        unused.contains(&"canvas"),
        "happy-dom takes no canvas peer, so canvas stays unused, got {unused:?}"
    );
}
