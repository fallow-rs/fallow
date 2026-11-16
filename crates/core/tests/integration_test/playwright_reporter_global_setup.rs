use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};

const FIXTURE: &str = "playwright-reporter-global-setup";

/// A config can pick its reporter, `globalSetup` and `globalTeardown` with a
/// condition. The plugin reads the string values of each branch (issue #4620).
#[test]
fn conditional_reporter_and_global_setup_files_are_used() {
    let root = fixture_path(FIXTURE);
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");
    let unused = collect_unused_files(&root, &results);

    for file in [
        "reporters/conditional-reporter.ts",
        "reporters/second-reporter.ts",
        "conditional-setup.ts",
        "external-teardown.ts",
        "local-teardown.ts",
    ] {
        assert!(
            !unused.iter().any(|path| path == file),
            "Playwright loads {file} from the config, found: {unused:?}"
        );
    }
}

/// Playwright reads the default export of each reporter, `globalSetup` and
/// `globalTeardown` file. Other exports of these files stay reported under
/// `includeEntryExports` (issue #4621).
#[test]
fn reporter_and_global_setup_default_exports_are_used() {
    let root = fixture_path(FIXTURE);
    let mut config = create_config(root.clone());
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis");
    let unused = collect_unused_exports(&root, &results);

    for file in [
        "reporters/conditional-reporter.ts",
        "reporters/second-reporter.ts",
        "conditional-setup.ts",
        "external-teardown.ts",
        "local-teardown.ts",
    ] {
        assert!(
            !has_unused_export(&unused, file, "default"),
            "Playwright reads the default export of {file}, found: {unused:?}"
        );
    }
    assert!(
        has_unused_export(
            &unused,
            "reporters/second-reporter.ts",
            "strayReporterHelper"
        ),
        "a reporter export that Playwright does not read stays reported, found: {unused:?}"
    );
    assert!(
        has_unused_export(&unused, "conditional-setup.ts", "straySetupHelper"),
        "a setup export that Playwright does not read stays reported, found: {unused:?}"
    );
}
