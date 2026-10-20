//! Integration test for the `source-parse-degraded` diagnostic.
//!
//! A file that does not parse is not a neutral event. Its imports credit
//! nothing, so every module it was the only consumer of becomes an
//! `unused-file` with a `delete-file` action, and the run gives no hint that
//! the verdict rests on a partial parse. These tests pin that the degradation
//! is now recorded, that it survives a warm cache, and that it never withholds
//! a finding: a broken file must still be analyzed as far as the parser got.

use std::path::Path;

use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};
use fallow_types::output_dead_code::ReachabilityCaveat;

use super::common::create_config_with_cache;

fn degraded_diagnostics(root: &Path) -> Vec<WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::SourceParseDegraded { .. }
            )
        })
        .collect()
}

/// Write a project whose entry file has an unclosed call expression.
fn write_broken_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "parse-degraded", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/helper.ts"),
        "export const helper = (): string => \"helper\";\n",
    )
    .expect("write helper module");
    std::fs::write(
        root.join("src/index.ts"),
        "import { helper } from \"./helper\";\n\nexport const run = (): string => {\n  return helper(\n};\n",
    )
    .expect("write broken entry module");
}

#[test]
fn a_file_that_fails_to_parse_is_reported_as_degraded() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_broken_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let results = fallow_core::analyze(&config).expect("analysis succeeds on a broken source");

    let degraded = degraded_diagnostics(&config.root);
    assert_eq!(
        degraded.len(),
        1,
        "exactly the broken file should be reported, got {degraded:?}"
    );
    assert!(
        degraded[0].path.ends_with("src/index.ts"),
        "the diagnostic must anchor at the file that failed to parse, got {:?}",
        degraded[0].path
    );
    let WorkspaceDiagnosticKind::SourceParseDegraded { error_count, .. } = degraded[0].kind else {
        panic!("expected a source-parse-degraded kind");
    };
    assert!(error_count > 0, "a degraded parse must report its errors");

    // The unusable import is exactly why this has to be reported: without the
    // diagnostic the run confidently offers to delete a file the source imports.
    assert!(
        results
            .unused_files
            .iter()
            .any(|issue| issue.file.path.ends_with("src/helper.ts")),
        "the finding itself is unchanged; the degradation is reported, never used to gate"
    );
}

#[test]
fn a_degraded_parse_is_still_reported_from_a_warm_cache() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_broken_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let _ = fallow_core::analyze(&config).expect("cold analysis succeeds");
    let _ = fallow_core::analyze(&config).expect("warm analysis succeeds");

    let degraded = degraded_diagnostics(&config.root);
    assert_eq!(
        degraded.len(),
        1,
        "a cache hit must not lose the degradation, got {degraded:?}"
    );
}

#[test]
fn a_project_that_parses_cleanly_reports_no_degradation() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_broken_project(&root);
    std::fs::write(
        root.join("src/index.ts"),
        "import { helper } from \"./helper\";\n\nexport const run = (): string => helper();\n",
    )
    .expect("write valid entry module");
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let results = fallow_core::analyze(&config).expect("analysis succeeds");

    assert!(
        degraded_diagnostics(&config.root).is_empty(),
        "a clean parse must not be reported as degraded"
    );
    assert!(
        results
            .unused_files
            .iter()
            .all(|issue| issue.reachability_caveats.is_empty())
            && results
                .unused_exports
                .iter()
                .all(|issue| issue.reachability_caveats.is_empty()),
        "a project that parses cleanly must carry no caveat marker anywhere"
    );
}

/// The diagnostic alone leaves the caveat at the top of the envelope while the
/// finding it distorts sits far away with a `delete-file` action on it. The
/// finding has to carry the caveat itself.
#[test]
fn a_finding_a_degraded_parse_can_distort_carries_the_caveat() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    write_broken_project(&root);
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let results = fallow_core::analyze(&config).expect("analysis succeeds on a broken source");

    let helper = results
        .unused_files
        .iter()
        .find(|issue| issue.file.path.ends_with("src/helper.ts"))
        .expect("the file the broken entry imports is still reported unused");
    assert_eq!(
        helper.reachability_caveats,
        vec![ReachabilityCaveat::IncompleteImportGraph],
        "the entry file that failed to parse is reachable, so the verdict on the file it \
         imports rests on an import graph fallow knows is incomplete"
    );

    // Report-only stands: the caveat is advisory provenance, not a gate.
    assert_eq!(
        helper.actions.len(),
        2,
        "the marker must not withhold or trim the finding's actions"
    );
}

/// A dependency is reported unused when NO module imports its specifier, and
/// fallow credits an import from an unreachable module too. So the reachability
/// narrowing that keeps the caveat off a clean `unused_files[]` entry must not
/// be applied here: any degraded parse can hide the import that would have kept
/// the package. `remove-dependency` is the most destructive write `fallow fix`
/// has, so this array has to carry the caveat as well.
#[test]
fn a_dependency_only_the_broken_file_imports_carries_the_caveat() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "degraded-dep", "version": "1.0.0", "main": "src/index.ts", "dependencies": { "lodash": "^4.17.21" } }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/index.ts"),
        "const broken = = 1;\nimport { chunk } from \"lodash\";\n\nexport const run = (): void => {\n  chunk([1], 1);\n};\n",
    )
    .expect("write broken entry module");
    let config = create_config_with_cache(root, temp.path().join("cache"));

    let results = fallow_core::analyze(&config).expect("analysis succeeds on a broken source");

    let lodash = results
        .unused_dependencies
        .iter()
        .find(|issue| issue.dep.package_name == "lodash")
        .expect("the import the parser never reached leaves the package looking unused");
    assert_eq!(
        lodash.reachability_caveats,
        vec![ReachabilityCaveat::IncompleteImportGraph],
        "the verdict rests on a specifier list fallow knows is incomplete"
    );
    assert!(
        !lodash
            .reachability_caveats
            .contains(&ReachabilityCaveat::IncompleteFileAnalysis),
        "the file a dependency finding names is a package.json, never a parsed module"
    );
}

/// A syntax-error test fixture is broken on purpose. When `ignoreFindings`
/// covers it, the run must not report it as degraded or caveat other findings
/// because of it. A broken file outside the patterns is still reported.
#[test]
fn ignore_findings_hides_the_degradation_of_a_covered_file() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create project src");
    std::fs::create_dir_all(root.join("test/fixtures/bad")).expect("create fixture dir");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "ignored-degraded", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/index.ts"),
        "import { helper } from \"./helper\";\n\nexport const run = (): string => helper();\n",
    )
    .expect("write entry module");
    std::fs::write(
        root.join("src/helper.ts"),
        "export const helper = (): string => \"helper\";\nexport const unused = 1;\n",
    )
    .expect("write helper module");
    std::fs::write(
        root.join("test/fixtures/bad/broken.js"),
        "const broken = = 1;\n",
    )
    .expect("write covered broken fixture");
    std::fs::write(root.join("src/also-broken.ts"), "export const x = = 1;\n")
        .expect("write uncovered broken file");
    let mut config = create_config_with_cache(root, temp.path().join("cache"));
    config.ignore_findings =
        fallow_config::FindingIgnoreMatcher::try_compile(&["test/fixtures/**".to_owned()])
            .expect("valid ignoreFindings pattern");

    let results = fallow_core::analyze(&config).expect("analysis succeeds on a broken source");

    let degraded = degraded_diagnostics(&config.root);
    assert_eq!(
        degraded.len(),
        1,
        "only the broken file outside ignoreFindings is reported, got {degraded:?}"
    );
    assert!(degraded[0].path.ends_with("src/also-broken.ts"));
    let unused = results
        .unused_exports
        .iter()
        .find(|issue| issue.export.export_name == "unused")
        .expect("the unused helper export is reported");
    assert!(
        !unused
            .reachability_caveats
            .contains(&ReachabilityCaveat::IncompleteImportGraph),
        "an unreachable broken file and a covered fixture must not caveat the graph, got {:?}",
        unused.reachability_caveats
    );
    assert!(
        config.ignore_findings.unmatched_patterns().is_empty(),
        "a pattern that covers a broken fixture is a used pattern"
    );
}
