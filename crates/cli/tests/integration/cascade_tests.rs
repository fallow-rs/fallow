//! The unused-file cascade filter.
//!
//! The fixture has one unused file, `src/dead.ts`. Another unused file uses
//! some of its exports, so the export analysis also reads `src/dead.ts`. By
//! default the report lists the unused file and hides the export findings in
//! it. `--show-cascade` and the `showCascade` config key keep them.

use super::common::{copy_fixture, parse_json, run_fallow, run_fallow_in_root};

const FIXTURE: &str = "cascade-unused-file";
const DEAD_FILE: &str = "src/dead.ts";

fn json_args<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    args
}

fn dead_code_json(root: &std::path::Path, extra: &[&str]) -> serde_json::Value {
    let output = run_fallow_in_root("dead-code", root, &json_args(extra));
    assert!(
        output.code == 0 || output.code == 1,
        "dead-code failed: {}",
        output.stderr
    );
    parse_json(&output)
}

fn paths(json: &serde_json::Value, key: &str) -> Vec<String> {
    json[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key} is an array"))
        .iter()
        .map(|finding| finding["path"].as_str().unwrap().to_owned())
        .collect()
}

fn cascade_paths(json: &serde_json::Value) -> Vec<String> {
    ["unused_exports", "unused_types"]
        .iter()
        .flat_map(|key| paths(json, key))
        .filter(|path| path == DEAD_FILE)
        .collect()
}

/// `total_issues` and each `summary` count agree with the arrays.
fn assert_counts_agree(json: &serde_json::Value) {
    let summary = json["summary"].as_object().expect("summary is an object");
    let mut sum = 0;
    for (key, count) in summary {
        if key == "total_issues" {
            continue;
        }
        let listed = json[key].as_array().map_or(0, Vec::len);
        assert_eq!(count.as_u64(), Some(listed as u64), "summary.{key}");
        sum += listed;
    }
    assert_eq!(json["total_issues"].as_u64(), Some(sum as u64));
    assert_eq!(summary["total_issues"], json["total_issues"]);
}

#[test]
fn default_run_hides_export_findings_of_unused_files() {
    let root = super::common::fixture_path(FIXTURE);
    let json = dead_code_json(&root, &[]);

    assert!(paths(&json, "unused_files").contains(&DEAD_FILE.to_owned()));
    assert_eq!(cascade_paths(&json), Vec::<String>::new());
    assert_eq!(json["cascade_hidden"], 4);
    assert_counts_agree(&json);
}

#[test]
fn a_true_finding_in_a_reachable_file_stays_reported() {
    let root = super::common::fixture_path(FIXTURE);
    let json = dead_code_json(&root, &[]);

    let exports: Vec<_> = json["unused_exports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                finding["path"].as_str().unwrap(),
                finding["export_name"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(exports, vec!["src/used.ts:liveUnused".to_owned()]);
}

#[test]
fn show_cascade_keeps_the_hidden_findings() {
    let root = super::common::fixture_path(FIXTURE);
    let hidden = dead_code_json(&root, &[]);
    let shown = dead_code_json(&root, &["--show-cascade"]);

    assert_eq!(cascade_paths(&shown).len(), 4);
    assert!(shown.get("cascade_hidden").is_none());
    assert_counts_agree(&shown);
    // The flag adds exactly the findings that the default run counted.
    assert_eq!(
        shown["total_issues"].as_u64().unwrap() - hidden["total_issues"].as_u64().unwrap(),
        hidden["cascade_hidden"].as_u64().unwrap()
    );
}

#[test]
fn show_cascade_config_key_matches_the_flag() {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"showCascade": true}"#,
    )
    .unwrap();

    let json = dead_code_json(dir.path(), &[]);

    assert_eq!(cascade_paths(&json).len(), 4);
    assert!(json.get("cascade_hidden").is_none());
}

#[test]
fn file_suppression_of_the_unused_file_keeps_its_findings_hidden() {
    let dir = copy_fixture(FIXTURE);
    let dead = dir.path().join(DEAD_FILE);
    let source = std::fs::read_to_string(&dead).unwrap();
    std::fs::write(
        &dead,
        format!("// fallow-ignore-file unused-file\n{source}"),
    )
    .unwrap();

    let json = dead_code_json(dir.path(), &[]);

    assert!(!paths(&json, "unused_files").contains(&DEAD_FILE.to_owned()));
    assert_eq!(cascade_paths(&json), Vec::<String>::new());
    assert_eq!(json["cascade_hidden"], 4);
    assert_eq!(json["stale_suppressions"].as_array().map_or(0, Vec::len), 0);
}

#[test]
fn line_suppression_in_an_unused_file_is_not_stale() {
    let dir = copy_fixture(FIXTURE);
    let dead = dir.path().join(DEAD_FILE);
    let source = std::fs::read_to_string(&dead).unwrap().replace(
        "export const deadUnused",
        "// fallow-ignore-next-line unused-export\nexport const deadUnused",
    );
    std::fs::write(&dead, source).unwrap();

    for extra in [&[][..], &["--show-cascade"][..]] {
        let json = dead_code_json(dir.path(), extra);
        assert_eq!(
            json["stale_suppressions"].as_array().map_or(0, Vec::len),
            0,
            "{extra:?}"
        );
        let reported = json["unused_exports"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["export_name"] == "deadUnused");
        assert!(!reported, "{extra:?}");
    }
}

#[test]
fn baseline_entries_of_hidden_findings_are_not_stale() {
    let dir = copy_fixture(FIXTURE);
    let baseline = dir.path().join("baseline.json");
    let baseline = baseline.to_str().unwrap();
    let saved = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &json_args(&["--show-cascade", "--save-baseline", baseline]),
    );
    assert!(saved.code == 0 || saved.code == 1, "{}", saved.stderr);

    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &json_args(&["--baseline", baseline, "--fail-on-stale-baseline"]),
    );

    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let json = parse_json(&output);
    assert_eq!(json["baseline_staleness"]["stale_entries"], 0);
    assert_eq!(json["baseline_staleness"]["gate_trips"], false);
    assert_eq!(json["total_issues"], 0);
}

#[test]
fn fix_offers_no_fix_for_hidden_findings() {
    let fix_targets = |extra: &[&str]| -> Vec<String> {
        let mut args = vec!["--dry-run", "--format", "json", "--quiet"];
        args.extend_from_slice(extra);
        let output = run_fallow("fix", FIXTURE, &args);
        assert_eq!(output.code, 0, "stderr: {}", output.stderr);
        parse_json(&output)["fixes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|fix| fix["path"].as_str().map(str::to_owned))
            .collect()
    };

    assert!(
        !fix_targets(&[])
            .iter()
            .any(|path| path.ends_with(DEAD_FILE))
    );
    assert!(
        fix_targets(&["--show-cascade"])
            .iter()
            .any(|path| path.ends_with(DEAD_FILE))
    );
}

#[test]
fn sarif_lists_only_the_reported_findings() {
    let root = super::common::fixture_path(FIXTURE);
    let json = dead_code_json(&root, &[]);
    let output = run_fallow_in_root("dead-code", &root, &["--format", "sarif", "--quiet"]);
    let sarif: serde_json::Value = serde_json::from_str(&output.stdout).expect("sarif json");

    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len() as u64, json["total_issues"].as_u64().unwrap());
    assert!(!results.iter().any(|result| {
        result["ruleId"] == "fallow/unused-export"
            && result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with(DEAD_FILE))
    }));
}
