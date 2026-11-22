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
