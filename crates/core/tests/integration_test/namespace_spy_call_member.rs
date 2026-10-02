//! A test spy call that takes a namespace import and a static member name
//! (`vi.spyOn(ns, 'helper')`, `jest.spyOn`, a test-framework `spyOn`,
//! `mock.method` from `node:test`) reads one member of the namespace. It must
//! credit that member only, as `ns.helper` does, and leave the other exports
//! of the module open to the unused-export check.
//!
//! A spy call with a computed member name, and a local function that happens to
//! be named `spyOn`, can reach every export, so they keep crediting the whole
//! namespace.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "namespace-spy-call-member";

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.path.to_string_lossy().replace('\\', "/"),
                e.export.export_name.clone(),
            )
        })
        .collect()
}

fn is_reported(unused: &[(String, String)], path: &str, name: &str) -> bool {
    unused.iter().any(|(p, n)| p.ends_with(path) && n == name)
}

#[test]
fn spy_call_with_static_member_credits_only_that_member() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    for target in [
        "src/vitest-target.ts",
        "src/jest-target.ts",
        "src/bun-target.ts",
        "src/global-target.ts",
        "src/node-target.ts",
        "src/context-target.ts",
    ] {
        assert!(
            is_reported(&unused, target, "DEAD"),
            "{target}:DEAD must be reported: the spy call reads only `helper`; unused exports: {unused:?}"
        );
        assert!(
            !is_reported(&unused, target, "helper"),
            "{target}:helper must be credited by the spy call; unused exports: {unused:?}"
        );
        assert!(
            !is_reported(&unused, target, "run"),
            "{target}:run must be credited by the entry; unused exports: {unused:?}"
        );
    }
}

#[test]
fn spy_call_without_static_member_keeps_whole_namespace_credit() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");
    let unused = unused_export_pairs(&results);

    for target in ["src/dynamic-target.ts", "src/local-target.ts"] {
        for name in ["DEAD", "helper", "run"] {
            assert!(
                !is_reported(&unused, target, name),
                "{target}:{name} must stay credited: the call can reach every export; unused exports: {unused:?}"
            );
        }
    }
}
