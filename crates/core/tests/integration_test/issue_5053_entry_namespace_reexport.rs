//! Issue #5053: with `includeEntryExports`, an `export * as Name` of an entry
//! file credits only the members that a consumer reads through the namespace
//! binding, like an `import * as Name`. A nested chain (`Outer.Inner.x`), a
//! read through `import * as Root` (`Root.Lib.x`) and a consumer in the same
//! package each credit the member they read. A binding that escapes as a
//! value credits every export of its module. The same reads apply to an
//! `export * as Name` of a file that is not an entry file.

use super::common::{create_config, fixture_path};

const FIXTURE: &str = "issue-5053-entry-namespace-reexport";

fn sorted_findings(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    let mut findings: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|finding| {
            let path = finding.export.path.to_string_lossy().replace('\\', "/");
            let file = path.rsplit('/').next().unwrap_or_default().to_string();
            (file, finding.export.export_name.clone())
        })
        .collect();
    findings.sort_unstable();
    findings
}

fn config_from_fixture_file() -> fallow_config::ResolvedConfig {
    let root = fixture_path(FIXTURE);
    let (loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    )
}

#[test]
fn namespace_re_exports_of_an_entry_file_stay_credited_by_default() {
    let results = fallow_core::analyze(&create_config(fixture_path(FIXTURE)))
        .expect("analysis should succeed");

    // The app reads `Local.Deep.deepRead` through a barrel that is not an
    // entry file, and hands `Handed` on as a value.
    assert_eq!(sorted_findings(&results), local_findings());
}

fn local_findings() -> Vec<(String, String)> {
    vec![
        ("local-deep.ts".to_string(), "deepUnread".to_string()),
        ("local-outer.ts".to_string(), "localUnused".to_string()),
    ]
}

#[test]
fn include_entry_exports_reports_unread_members_behind_an_entry_namespace() {
    let results =
        fallow_core::analyze(&config_from_fixture_file()).expect("analysis should succeed");

    // `escaped.ts` and `passed.ts` stay credited: the consumer reads
    // `Object.keys(Escaped)` and hands `Passed` on as a value. `plain.ts` is
    // behind an entry file that the option does not cover. `Unread` has no
    // consumer, so the entry export and the export of its module are both
    // unused. `readSelf` is an export of a covered entry file with no
    // importer.
    let expected: Vec<(String, String)> = [
        ("barreled.ts", "barrelUnused"),
        ("home.ts", "unusedNsStar"),
        ("index.ts", "Unread"),
        ("inner.ts", "innerUnused"),
        ("lib-ns.ts", "libUnused"),
        ("outer.ts", "outerUnused"),
        ("self-consumer.ts", "readSelf"),
        ("self.ts", "selfUnused"),
        ("unread.ts", "unreadA"),
    ]
    .into_iter()
    .map(|(file, name)| (file.to_string(), name.to_string()))
    .chain(local_findings())
    .collect();
    let mut expected = expected;
    expected.sort_unstable();
    assert_eq!(sorted_findings(&results), expected);
}

#[test]
fn ignore_exports_rule_for_the_namespace_name_credits_its_module() {
    let root = fixture_path(FIXTURE);
    let (mut loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    loaded.ignore_exports.push(fallow_config::IgnoreExportRule {
        file: "packages/lib/src/index.ts".to_owned(),
        exports: vec!["Home".to_owned(), "Outer".to_owned()],
    });
    let config = loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let findings = sorted_findings(&results);

    // The rule covers the namespace objects `Home` and `Outer`, so each
    // export behind them stays credited, also behind the nested `Inner`.
    for (file, name) in [
        ("home.ts", "unusedNsStar"),
        ("outer.ts", "outerUnused"),
        ("inner.ts", "innerUnused"),
    ] {
        assert!(
            !findings.contains(&(file.to_string(), name.to_string())),
            "{file}:{name} must be credited; unused exports: {findings:?}"
        );
    }
    assert!(findings.contains(&("barreled.ts".to_string(), "barrelUnused".to_string())));
}
