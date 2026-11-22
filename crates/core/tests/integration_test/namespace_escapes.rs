use std::path::Path;

use fallow_config::Severity;
use fallow_core::results::AnalysisResults;

use super::common::{copy_fixture_tree, create_config, fixture_path};

const FIXTURE: &str = "namespace-escapes";

fn analyze(root: &Path, rule_on: bool) -> AnalysisResults {
    let mut config = create_config(root.to_path_buf());
    if rule_on {
        config.rules.namespace_escapes = Severity::Warn;
    }
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn escapes(root: &Path, results: &AnalysisResults) -> Vec<(String, String, u32, &'static str)> {
    let mut rows: Vec<(String, String, u32, &'static str)> = results
        .namespace_escapes
        .iter()
        .map(|finding| {
            let escape = &finding.escape;
            (
                relative(root, &escape.path),
                escape.local_name.clone(),
                escape.hidden_exports,
                escape.reason.as_str(),
            )
        })
        .collect();
    rows.sort();
    rows
}

fn unused_export_names(results: &AnalysisResults) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect()
}

/// Each escaping binding gives one finding with the count of exports that the
/// full credit hides. `ByName` reads its members by name, and `NoHide` hides
/// no export, so neither is a finding. `Reexported` hides both exports of its
/// target: `consumer.ts` reads `reexportedUsed` through the re-exported
/// binding, and that read gives no credit by name to the target.
#[test]
fn reports_each_escaping_namespace_binding() {
    let root = fixture_path(FIXTURE);
    let results = analyze(&root, true);
    let row = |path: &str, name: &str, hidden: u32, reason: &'static str| {
        (path.to_string(), name.to_string(), hidden, reason)
    };
    assert_eq!(
        escapes(&root, &results),
        vec![
            row("src/consumer.ts", "Named", 2, "whole-object-use"),
            row("src/forms.ts", "Assign", 2, "whole-object-use"),
            row("src/forms.ts", "Barrel", 3, "whole-object-use"),
            row("src/forms.ts", "Call", 2, "whole-object-use"),
            row("src/forms.ts", "Cast", 2, "whole-object-use"),
            row("src/forms.ts", "Computed", 2, "whole-object-use"),
            row("src/forms.ts", "Keys", 2, "whole-object-use"),
            row("src/forms.ts", "Spread", 2, "whole-object-use"),
            row("src/reexport.ts", "Reexported", 2, "re-exported-binding"),
        ]
    );
    assert!(
        unused_export_names(&results).contains(&"bynameUnused".to_string()),
        "a namespace read by name keeps its unused export: {:?}",
        unused_export_names(&results)
    );
}

/// The rule is off by default, so the default output does not change.
#[test]
fn reports_no_namespace_escape_by_default() {
    let root = fixture_path(FIXTURE);
    let results = analyze(&root, false);
    assert!(
        results.namespace_escapes.is_empty(),
        "{:?}",
        escapes(&root, &results)
    );
}

/// A line suppression above the import removes only the finding of that import.
#[test]
fn line_suppression_removes_only_that_binding() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = tmp.path().join("project");
    copy_fixture_tree(&fixture_path(FIXTURE), &root);
    let forms = root.join("src/forms.ts");
    let source = std::fs::read_to_string(&forms).expect("read forms.ts");
    std::fs::write(
        &forms,
        format!("// fallow-ignore-next-line namespace-escape\n{source}"),
    )
    .expect("write forms.ts");

    let results = analyze(&root, true);
    let names: Vec<String> = escapes(&root, &results)
        .into_iter()
        .map(|(_, name, _, _)| name)
        .collect();
    assert!(!names.contains(&"Cast".to_string()), "{names:?}");
    assert_eq!(names.len(), 8, "{names:?}");
    assert!(
        results.stale_suppressions.is_empty(),
        "{:?}",
        results.stale_suppressions
    );
}

const STAR_READERS_FIXTURE: &str = "namespace-escapes-star-readers";

/// `Lib` escapes, and the escape opens the `export *` surface of
/// `lib/index.ts`. `named.ts` still reads `aUsed` and `bUsed` by name through
/// that barrel, so the escape hides only `aUnused` and `AUnusedType`.
/// `Root.Inner` hands on the namespace object of `export * as Inner`, so it
/// is an escape of `lib/c.ts`.
#[test]
fn star_barrel_named_readers_are_not_hidden() {
    let root = fixture_path(STAR_READERS_FIXTURE);
    let results = analyze(&root, true);
    assert_eq!(
        escapes(&root, &results),
        vec![
            (
                "src/chain.ts".to_string(),
                "Root.Inner".to_string(),
                2,
                "whole-object-use"
            ),
            (
                "src/escape.ts".to_string(),
                "Lib".to_string(),
                2,
                "whole-object-use"
            ),
        ]
    );
}

/// An export whose rule is off is not a finding, so the escape does not hide it.
#[test]
fn hidden_count_skips_exports_whose_rule_is_off() {
    let root = fixture_path(STAR_READERS_FIXTURE);
    let mut config = create_config(root.clone());
    config.rules.namespace_escapes = Severity::Warn;
    config.rules.unused_types = Severity::Off;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let lib = escapes(&root, &results)
        .into_iter()
        .find(|(_, name, _, _)| name == "Lib")
        .expect("Lib escape");
    assert_eq!(lib.2, 1, "{lib:?}");
}

/// Write a small project with `src/index.ts` as its entry and return its root.
fn write_project(dir: &tempfile::TempDir, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"probe","private":true,"type":"module","main":"src/index.ts"}"#,
    )
    .expect("write package.json");
    for (path, source) in files {
        std::fs::write(root.join(path), source).expect("write source");
    }
    root
}

const LIB: &str = "export const libUsed = 1;\nexport const libUnused = (): number => 2;\n";

/// `export { Ns }` in an entry file is public API. Without
/// `includeEntryExports` that credit stands with or without the escape, so
/// the binding hides nothing. With the option on, the binding hides both
/// exports of `lib.ts`.
#[test]
fn entry_re_export_is_reported_only_when_entry_exports_are_checked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = write_project(
        &dir,
        &[
            ("src/lib.ts", LIB),
            (
                "src/index.ts",
                "import * as Ns from \"./lib\";\n\nexport { Ns };\n",
            ),
        ],
    );
    assert!(analyze(&root, true).namespace_escapes.is_empty());

    let mut config = create_config(root.clone());
    config.rules.namespace_escapes = Severity::Warn;
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert_eq!(
        escapes(&root, &results),
        vec![(
            "src/index.ts".to_string(),
            "Ns".to_string(),
            2,
            "re-exported-binding"
        )]
    );
}

/// A reference from an unreachable file still credits an export of a
/// reachable module, so the escape in that file is reported.
#[test]
fn escape_in_an_unreachable_file_is_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = write_project(
        &dir,
        &[
            ("src/lib.ts", LIB),
            (
                "src/index.ts",
                "import { libUsed } from \"./lib\";\n\nconsole.log(libUsed);\n",
            ),
            (
                "src/dead.ts",
                "import * as N from \"./lib\";\n\nexport const keys = Object.keys(N);\n",
            ),
        ],
    );
    assert_eq!(
        escapes(&root, &analyze(&root, true)),
        vec![(
            "src/dead.ts".to_string(),
            "N".to_string(),
            1,
            "whole-object-use"
        )]
    );
}

/// Each binding counts only the hidden exports that its own credit reaches.
/// `B` escapes a barrel that re-exports only `x`, so it does not count `y`.
#[test]
fn each_escape_counts_only_the_exports_that_it_credits() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = write_project(
        &dir,
        &[
            ("src/a.ts", "export const x = 1;\nexport const y = 2;\n"),
            ("src/barrel.ts", "export { x } from \"./a\";\n"),
            (
                "src/index.ts",
                "import * as B from \"./barrel\";\nimport * as A from \"./a\";\n\nconsole.log(Object.keys(B), Object.keys(A));\n",
            ),
        ],
    );
    assert_eq!(
        escapes(&root, &analyze(&root, true)),
        vec![
            (
                "src/index.ts".to_string(),
                "A".to_string(),
                2,
                "whole-object-use"
            ),
            (
                "src/index.ts".to_string(),
                "B".to_string(),
                2,
                "whole-object-use"
            ),
        ]
    );
}

/// The escape of a barrel also hides the exports behind its `export * as`,
/// which the barrel hands on as a whole namespace object.
#[test]
fn escape_counts_the_exports_behind_a_namespace_re_export() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = write_project(
        &dir,
        &[
            (
                "src/inner.ts",
                "export const i1 = 1;\nexport const i2 = 2;\n",
            ),
            ("src/barrel.ts", "export * as Inner from \"./inner\";\n"),
            (
                "src/index.ts",
                "import * as Root from \"./barrel\";\n\nconsole.log(Object.keys(Root));\n",
            ),
        ],
    );
    assert_eq!(
        escapes(&root, &analyze(&root, true)),
        vec![(
            "src/index.ts".to_string(),
            "Root".to_string(),
            3,
            "whole-object-use"
        )]
    );
}
