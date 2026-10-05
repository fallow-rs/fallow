//! Public selection, evidence, severity and baseline behavior for optional prop candidates.
#![expect(clippy::unwrap_used, reason = "behavior fixtures")]

use crate::common::write;
use fallow_api::{
    AnalysisOptions, DeadCodeOptions, run_dead_code_with_baseline,
    serialize_dead_code_programmatic_json,
};
use serde_json::Value;

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "package.json",
        r#"{"name":"prop-evidence","private":true,"main":"src/main.tsx","dependencies":{"react":"*"}}"#,
    );
    write(dir.path(), ".fallowrc.json", config);
    write(
        dir.path(),
        "src/main.tsx",
        "import {Card,Other} from './Cards';export const app=<><Card/><Other/></>;",
    );
    write(
        dir.path(),
        "src/Cards.tsx",
        "import type {JSX} from 'react';\ntype Props={flag?:boolean};\nexport function Card({flag=false}:Props):JSX.Element{return <p>{String(flag)}</p>;}\nexport function Other({flag=false}:Props):JSX.Element{return <p>{String(flag)}</p>;}",
    );
    dir
}

fn options(root: &std::path::Path, selected: bool) -> DeadCodeOptions {
    let mut options = DeadCodeOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..Default::default()
        },
        ..Default::default()
    };
    if selected {
        assert!(
            options
                .filters
                .enable_registry_selector("absent-component-props")
        );
    }
    options
}

fn run(options: &DeadCodeOptions, baseline: Option<&std::path::Path>) -> Value {
    serialize_dead_code_programmatic_json(run_dead_code_with_baseline(options, baseline).unwrap())
        .unwrap()
}

#[test]
fn explicit_selection_enables_candidates_with_root_relative_manual_evidence() {
    let dir = project("{}");
    let default = run(&options(dir.path(), false), None);
    assert!(default.get("absent_component_props").is_none());
    let report = run(&options(dir.path(), true), None);
    let finding = &report["absent_component_props"][0];
    assert_eq!(finding["path"], "src/Cards.tsx");
    assert_eq!(finding["inspected_call_sites"][0]["path"], "src/main.tsx");
    assert_eq!(finding["effective_severity"], "warn");
    assert!(
        finding["explanation"]
            .as_str()
            .unwrap()
            .contains("static analysis does not prove runtime unreachability")
    );
    assert!(
        finding["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["auto_fixable"] == false)
    );
    assert_eq!(report["summary"]["absent_component_props"], 2);
    assert_eq!(report["total_issues"], 2);
    assert_eq!(report["unused_exports"], serde_json::json!([]));
}

#[test]
fn selection_respects_error_and_declaration_override() {
    let dir = project(r#"{"rules":{"absent-component-props":"error"}}"#);
    let report = run(&options(dir.path(), true), None);
    assert_eq!(
        report["absent_component_props"][0]["effective_severity"],
        "error"
    );
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"rules":{"absent-component-props":"error"},"overrides":[{"files":["src/Cards.tsx"],"rules":{"absent-component-props":"off"}}]}"#,
    );
    assert!(
        run(&options(dir.path(), true), None)
            .get("absent_component_props")
            .is_none()
    );
}

#[test]
fn baseline_preserves_other_component_with_same_prop_and_callers_do_not_own_scope() {
    let dir = project("{}");
    let selected = options(dir.path(), true);
    assert_eq!(run(&selected, None)["summary"]["absent_component_props"], 2);
    write(
        dir.path(),
        "baseline.json",
        r#"{"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"absent_component_props":["src/Cards.tsx:Card:flag"]}"#,
    );
    let report = run(&selected, Some(std::path::Path::new("baseline.json")));
    assert_eq!(
        report["absent_component_props"][0]["component_name"],
        "Other"
    );
    assert_eq!(report["summary"]["absent_component_props"], 1);
    let mut caller_only = options(dir.path(), true);
    caller_only.files = vec!["src/main.tsx".into()];
    assert!(
        run(&caller_only, None)
            .get("absent_component_props")
            .is_none()
    );
}

#[test]
fn saved_candidate_renders_in_ci_formats_with_stable_identity_and_caller_context() {
    let root = std::path::Path::new("/project");
    let mut results = fallow_types::results::AnalysisResults {
        absent_component_props: serde_json::from_value(serde_json::json!([{
            "path": "/project/src/Card.tsx",
            "component_name": "Card",
            "framework": "react",
            "prop_name": "flag",
            "line": 3,
            "col": 4,
            "has_default": true,
            "inspected_call_sites": [{"path":"/project/src/main.tsx","line":8,"col":2}],
            "explanation": "Known reachable callers do not supply this optional prop. Review defaults and API intent before changing the component; static analysis does not prove runtime unreachability.",
            "effective_severity": "error",
            "actions": []
        }])).unwrap(),
        ..Default::default()
    };
    fallow_types::identity::stamp_dead_code_finding_ids(&mut results, root);
    let rules = fallow_config::RulesConfig::default();
    let sarif = fallow_api::build_sarif(
        &results,
        root,
        &rules,
        &|id, name, level| serde_json::json!({"id":id,"shortDescription":{"text":name},"defaultConfiguration":{"level":level}}),
    );
    let finding = &sarif["runs"][0]["results"][0];
    assert_eq!(finding["ruleId"], "fallow/absent-component-prop");
    assert_eq!(finding["level"], "error");
    assert_eq!(
        finding["relatedLocations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "src/main.tsx"
    );
    assert!(finding.get("fixes").is_none());
    let cc = serde_json::to_value(fallow_api::build_codeclimate(&results, root, &rules)).unwrap();
    assert_eq!(cc[0]["check_name"], "fallow/absent-component-prop");
    assert_eq!(cc[0]["severity"], "major");
    let first = cc[0]["fingerprint"].clone();
    results.absent_component_props[0].prop.line += 5;
    results.absent_component_props[0].prop.inspected_call_sites[0].line += 2;
    let moved =
        serde_json::to_value(fallow_api::build_codeclimate(&results, root, &rules)).unwrap();
    assert_eq!(moved[0]["fingerprint"], first);
    let markdown = fallow_api::build_markdown(&results, root);
    assert!(
        markdown.contains("Card.flag")
            && markdown.contains("static analysis does not prove runtime unreachability")
    );
    assert!(
        fallow_api::build_compact_lines(&results, root)
            .iter()
            .any(|line| line.starts_with("absent-component-prop:"))
    );
}

#[test]
fn markdown_candidate_preserves_backticks_in_paths_and_punctuation_in_identity() {
    let root = std::path::Path::new("/project");
    let results = fallow_types::results::AnalysisResults {
        absent_component_props: serde_json::from_value(serde_json::json!([{
            "path": "/project/src/Card`mobile.tsx",
            "component_name": "Card*Mobile",
            "framework": "react",
            "prop_name": "flag`enabled",
            "line": 3,
            "col": 0,
            "has_default": true,
            "inspected_call_sites": [{
                "path": "/project/src/main`desktop.tsx",
                "line": 8,
                "col": 0
            }],
            "explanation": "Review this optional prop.",
            "actions": []
        }]))
        .unwrap(),
        ..Default::default()
    };
    let markdown = fallow_api::build_markdown(&results, root);
    assert!(markdown.contains("``Card*Mobile.flag`enabled``"));
    assert!(markdown.contains("``src/Card`mobile.tsx:3``"));
    assert!(markdown.contains("``src/main`desktop.tsx:8``"));
}

#[test]
fn path_override_can_enable_default_off_candidates_without_filter() {
    let dir = project(
        r#"{"overrides":[{"files":["src/Cards.tsx"],"rules":{"absent-component-props":"warn"}}]}"#,
    );
    let report = run(&options(dir.path(), false), None);
    assert_eq!(
        report["summary"]["absent_component_props"], 2,
        "matching declaration override enables analysis"
    );
}

#[test]
fn sarif_candidate_columns_use_utf16_for_unicode_declarations_and_callers() {
    let dir = project("{}");
    let declaration = "const note='😀'; type Props={flag?:boolean};";
    let caller = "const text='é😀'; export const app=<Card/>;";
    write(
        dir.path(),
        "src/Cards.tsx",
        &format!(
            "import type {{JSX}} from 'react';\n{declaration}\nexport function Card({{flag=false}}:Props):JSX.Element{{return <p>{{String(flag)}}</p>;}}"
        ),
    );
    write(
        dir.path(),
        "src/main.tsx",
        &format!("import {{Card}} from './Cards';\n{caller}"),
    );
    let report = run(&options(dir.path(), true), None);
    let results: fallow_types::results::AnalysisResults = serde_json::from_value(report).unwrap();
    let sarif = fallow_api::build_sarif(
        &results,
        dir.path(),
        &fallow_config::RulesConfig::default(),
        &|id, _, _| serde_json::json!({"id":id}),
    );
    let finding = &sarif["runs"][0]["results"][0];
    let declaration_col = declaration[..declaration.find("flag").unwrap()]
        .encode_utf16()
        .count()
        + 1;
    let caller_col = caller[..caller.find("<Card").unwrap()]
        .encode_utf16()
        .count()
        + 1;
    assert_eq!(
        finding["locations"][0]["physicalLocation"]["region"]["startColumn"],
        declaration_col
    );
    assert_eq!(
        finding["relatedLocations"][0]["physicalLocation"]["region"]["startColumn"],
        caller_col
    );
}

#[test]
fn ignore_findings_owns_declaration_and_preserves_caller_context() {
    let dir = project("{}");
    assert_eq!(
        run(&options(dir.path(), true), None)["summary"]["absent_component_props"],
        2
    );
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"ignoreFindings":["src/main.tsx"]}"#,
    );
    assert_eq!(
        run(&options(dir.path(), true), None)["summary"]["absent_component_props"],
        2
    );
    write(
        dir.path(),
        ".fallowrc.json",
        r#"{"ignoreFindings":["src/Cards.tsx"]}"#,
    );
    assert_eq!(
        run(&options(dir.path(), true), None)["summary"]["absent_component_props"],
        0
    );
}

#[test]
fn audit_distinguishes_same_file_components_and_ignores_caller_default_line_changes() {
    use crate::common::{commit, git};
    let dir = project(r#"{"rules":{"absent-component-props":"warn"}}"#);
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    write(
        root,
        "src/main.tsx",
        "import {Card,Other} from './Cards';export const app=<><Card/><Other flag/></>;",
    );
    commit(root, "base candidate");
    write(
        root,
        "src/main.tsx",
        "import {Card,Other} from './Cards';\nexport const app=<><Card/><Other/></>;",
    );
    let source = std::fs::read_to_string(root.join("src/Cards.tsx")).unwrap();
    write(
        root,
        "src/Cards.tsx",
        &format!(
            "// Source moved; caller evidence and defaults changed.\n{}",
            source.replacen("flag=false", "flag=true", 1)
        ),
    );
    commit(root, "change optional callers");
    let options = fallow_api::AuditOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..Default::default()
        },
        base: Some("HEAD~1".to_string()),
        ..Default::default()
    };
    let report =
        fallow_api::serialize_audit_programmatic_json(fallow_api::run_audit(&options).unwrap())
            .unwrap();
    let items = report["dead_code"]["absent_component_props"]
        .as_array()
        .unwrap();
    let card = items
        .iter()
        .find(|finding| finding["component_name"] == "Card")
        .unwrap();
    let other = items
        .iter()
        .find(|finding| finding["component_name"] == "Other")
        .unwrap();
    assert_eq!(
        card["introduced"], false,
        "line/default/caller changes keep identity"
    );
    assert_eq!(
        other["introduced"], true,
        "same-file component names remain distinct"
    );
}
