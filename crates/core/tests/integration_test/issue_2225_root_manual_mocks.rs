//! Regression tests for issue #2225: root-level `__mocks__/` manual mocks for
//! node modules. Vitest resolves a factory-less `vi.mock('pkg')` /
//! `vi.mock('@scope/pkg')` to `__mocks__/<specifier>` at the project root, but
//! Fallow had no crediting path for bare specifiers (the speculative sibling
//! of issue #251 needs a `/` in the specifier, and the package-space sibling
//! is dropped by design since issue #2213), so those mock files surfaced as
//! unused files. The parity matrix with Jest:
//!
//! - root mock with a matching factory-less mock call: used under both.
//! - root mock without any mock call: unused under Vitest (manual mocks apply
//!   only through `vi.mock`), used under Jest (node-module manual mocks are
//!   applied automatically; the jest plugin declares `__mocks__` entry
//!   patterns).

use std::path::PathBuf;

use super::common::{create_config, fixture_path};

fn unused_files(root: &PathBuf, results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn vitest_root_manual_mocks_credited_by_factory_less_vi_mock() {
    let root = fixture_path("issue-2225-vitest-root-mocks");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_files(&root, &results);
    assert!(
        !unused.contains(&"__mocks__/lodash.ts".to_string()),
        "root manual mock of an unscoped node module should be credited by vi.mock, unused files: {unused:?}"
    );
    assert!(
        !unused.contains(&"__mocks__/@bacons/apple-targets.ts".to_string()),
        "root manual mock of a scoped node module should be credited by vi.mock, unused files: {unused:?}"
    );
    assert!(
        unused.contains(&"__mocks__/unmatched-pkg.ts".to_string()),
        "a root mock without any vi.mock call is never applied by vitest and must stay reported, unused files: {unused:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.iter().any(|name| name.contains("__mocks__")),
        "root-mock crediting must not fabricate phantom __mocks__ packages, got: {unlisted:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@bacons/apple-targets"),
        "the mocked package keeps its usage credit from the vi.mock target edge, got: {unused_deps:?}"
    );
}

#[test]
fn jest_root_manual_mocks_stay_used_with_and_without_mock_calls() {
    let root = fixture_path("issue-2225-jest-root-mocks");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_files(&root, &results);
    for mock in [
        "__mocks__/lodash.ts",
        "__mocks__/@bacons/apple-targets.ts",
        "__mocks__/unmatched-pkg.ts",
    ] {
        assert!(
            !unused.contains(&mock.to_string()),
            "jest applies root node-module manual mocks automatically, so {mock} must not be reported unused, unused files: {unused:?}"
        );
    }
}
