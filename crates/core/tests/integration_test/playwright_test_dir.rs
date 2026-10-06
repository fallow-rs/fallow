use super::common::{create_config, fixture_path};

struct Findings {
    unused_exports: Vec<String>,
    unused_files: Vec<String>,
}

fn analyze_fixture(name: &str) -> Findings {
    let root = fixture_path(name);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    Findings {
        unused_exports: results
            .unused_exports
            .iter()
            .map(|e| e.export.export_name.clone())
            .collect(),
        unused_files: results
            .unused_files
            .iter()
            .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
            .collect(),
    }
}

fn assert_spec_reachable(findings: &Findings, spec: &str) {
    assert!(
        !findings.unused_files.iter().any(|p| p.ends_with(spec)),
        "{spec} is inside the configured testDir and must stay an entry, got {:?}",
        findings.unused_files
    );
}

fn assert_helper_export_reported(findings: &Findings) {
    assert!(
        findings
            .unused_exports
            .iter()
            .any(|name| name == "unusedHelper"),
        "a helper outside the configured testDir is not a test entry, so its unused export must be reported, got {:?}",
        findings.unused_exports
    );
}

/// A top-level `testDir` replaces the default test directories. A helper
/// under `e2e/` outside that directory is an ordinary module. A `*.test.ts`
/// file outside `testDir` stays an entry, because another test runner can
/// select it.
#[test]
fn top_level_test_dir_scopes_test_entries() {
    let findings = analyze_fixture("playwright-test-dir");
    assert_spec_reachable(&findings, "ui/a.pw.ts");
    assert!(
        !findings
            .unused_files
            .iter()
            .any(|p| p.ends_with("unit/helper.test.ts")),
        "a test file name outside testDir must stay an entry, got {:?}",
        findings.unused_files
    );
    assert_helper_export_reported(&findings);
}

/// A config in a subdirectory resolves `testDir` against its own directory.
#[test]
fn nested_config_test_dir_resolves_from_config_dir() {
    let findings = analyze_fixture("playwright-test-dir-nested");
    assert_spec_reachable(&findings, "e2e/ui/a.pw.ts");
    assert_helper_export_reported(&findings);
}

/// Each `projects[].testDir` contributes its own test entries.
#[test]
fn project_test_dirs_scope_test_entries() {
    let findings = analyze_fixture("playwright-test-dir-projects");
    assert_spec_reachable(&findings, "a/one.pw.ts");
    assert_spec_reachable(&findings, "b/two.pw.ts");
    assert_helper_export_reported(&findings);
}

/// Two config files in one project keep the test entries of both configs.
#[test]
fn multiple_configs_keep_each_test_dir() {
    let findings = analyze_fixture("playwright-test-dir-multi-config");
    assert_spec_reachable(&findings, "apps/one/ui/one.pw.ts");
    assert_spec_reachable(&findings, "apps/two/checks/two.pw.ts");
    assert_helper_export_reported(&findings);
}

#[test]
fn unknown_project_test_dir_keeps_known_custom_entries() {
    let findings = analyze_fixture("playwright-test-dir-mixed-projects");
    assert_spec_reachable(&findings, "a/one.pw.ts");
    assert!(
        findings
            .unused_files
            .iter()
            .any(|p| p.ends_with("b/two.pw.ts")),
        "a custom file outside every known testDir must still be reported, got {:?}",
        findings.unused_files
    );
}

#[test]
fn test_match_is_case_insensitive() {
    let findings = analyze_fixture("playwright-test-dir-case-insensitive");
    assert_spec_reachable(&findings, "ui/a.PW.ts");
    assert_helper_export_reported(&findings);
}

#[test]
fn test_dir_is_a_literal_path() {
    let findings = analyze_fixture("playwright-test-dir-literal");
    assert_spec_reachable(&findings, "ui[1]/a.pw.ts");
    assert_helper_export_reported(&findings);
}

#[test]
fn later_spread_does_not_narrow_to_an_overwritten_test_dir() {
    let findings = analyze_fixture("playwright-test-dir-spread");
    assert_spec_reachable(&findings, "tests/live.pw.ts");
    assert!(
        findings
            .unused_files
            .iter()
            .any(|p| p.ends_with("orphan.ts")),
        "unrelated files must still report, got {:?}",
        findings.unused_files
    );
}

fn assert_file_reported(findings: &Findings, file: &str) {
    assert!(
        findings.unused_files.iter().any(|p| p.ends_with(file)),
        "{file} is not a test entry and must be reported, got {:?}",
        findings.unused_files
    );
}

/// `defineConfig` merges all of its arguments. An imported argument can set
/// `testMatch`, so the `testDir` of a later argument keeps every script.
#[test]
fn define_config_with_imported_argument_keeps_its_test_files() {
    let findings = analyze_fixture("playwright-define-config-imported-base");
    assert_spec_reachable(&findings, "e2e/login.pw.ts");
    assert_spec_reachable(&findings, "e2e/helper.ts");
    assert_file_reported(&findings, "orphan.ts");
}

/// A later `defineConfig` argument overrides the `testDir` of an earlier one.
#[test]
fn define_config_later_argument_wins() {
    let findings = analyze_fixture("playwright-define-config-two-objects");
    assert_spec_reachable(&findings, "e2e/login.pw.ts");
    assert_file_reported(&findings, "a/old.pw.ts");
}

/// Without `testDir`, Playwright applies `testMatch` below the config
/// directory.
#[test]
fn test_match_without_test_dir_applies_below_config_dir() {
    let findings = analyze_fixture("playwright-test-match-without-test-dir");
    assert_spec_reachable(&findings, "src/login.e2e.ts");
    assert_file_reported(&findings, "src/orphan.ts");
}
