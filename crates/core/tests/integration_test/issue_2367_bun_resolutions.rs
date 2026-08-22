//! Integration test for Yarn-style `resolutions` as a bun override source
//! (issue #2367).
//!
//! Fixture under `tests/fixtures/issue-2367-bun-resolutions/` is a bun
//! repository (`packageManager: bun@1.3.2`, text `bun.lock`) whose root
//! `package.json` declares no `overrides` and pins three packages under
//! `resolutions`: `ws` (declared and resolved, USED), `left-pad` (absent
//! everywhere, UNUSED), and the yarn glob form `**/trim-newlines` (absent,
//! UNUSED). The lockfile resolves `ws` only.

use fallow_config::{
    FallowConfig, IgnoreDependencyOverrideRule, OutputFormat, WorkspaceDiagnosticKind,
};
use fallow_types::results::DependencyOverrideSource;

use super::common::fixture_path;

const FIXTURE: &str = "issue-2367-bun-resolutions";

fn config_for_fixture(ignore: Vec<IgnoreDependencyOverrideRule>) -> fallow_config::ResolvedConfig {
    FallowConfig {
        ignore_dependency_overrides: ignore,
        ..Default::default()
    }
    .resolve(
        fixture_path(FIXTURE),
        OutputFormat::Human,
        4,
        true,
        true,
        None,
    )
}

#[test]
fn bun_resolutions_entries_are_reported_as_unused_overrides() {
    let root = fixture_path(FIXTURE);
    let config = config_for_fixture(vec![]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut flagged: Vec<(&str, &str, u32)> = results
        .unused_dependency_overrides
        .iter()
        .map(|finding| {
            (
                finding.entry.raw_key.as_str(),
                finding.entry.target_package.as_str(),
                finding.entry.line,
            )
        })
        .collect();
    flagged.sort_unstable();
    assert_eq!(
        flagged,
        vec![
            ("**/trim-newlines", "trim-newlines", 12),
            ("left-pad", "left-pad", 11),
        ],
        "ws is declared and resolved; the two unresolved pins report at their resolutions lines"
    );

    let expected_path = root
        .join("package.json")
        .display()
        .to_string()
        .replace('\\', "/");
    for finding in &results.unused_dependency_overrides {
        assert_eq!(
            finding.entry.source,
            DependencyOverrideSource::PnpmPackageJson,
            "resolutions live in package.json"
        );
        assert_eq!(
            finding.entry.path.display().to_string().replace('\\', "/"),
            expected_path
        );
        let hint = finding.entry.hint.as_deref().unwrap_or_default();
        assert!(
            hint.contains("resolutions") && hint.contains("bun install --frozen-lockfile"),
            "the bun hint names the resolutions origin: {hint}"
        );
    }

    assert!(
        results.misconfigured_dependency_overrides.is_empty(),
        "every key in the fixture is a shape bun honours: {:?}",
        results.misconfigured_dependency_overrides
    );
    assert!(
        fallow_config::workspace_diagnostics_for(&root)
            .iter()
            .all(|diagnostic| !matches!(
                diagnostic.kind,
                WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped
            )),
        "a parseable bun.lock means resolution ran, so no skip diagnostic"
    );
}

#[test]
fn ignore_rule_suppresses_bun_resolutions_entry() {
    let config = config_for_fixture(vec![IgnoreDependencyOverrideRule {
        package: "left-pad".to_string(),
        source: Some("package.json".to_string()),
    }]);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let flagged: Vec<&str> = results
        .unused_dependency_overrides
        .iter()
        .map(|finding| finding.entry.target_package.as_str())
        .collect();
    assert_eq!(
        flagged,
        vec!["trim-newlines"],
        "the package.json source label suppresses a resolutions entry"
    );
}
