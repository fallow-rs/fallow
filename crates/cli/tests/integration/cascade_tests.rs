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

/// SARIF has no result for a hidden finding, so the run `properties` carry
/// the count. The property is absent when nothing is hidden.
#[test]
fn sarif_run_properties_carry_the_hidden_count() {
    let root = super::common::fixture_path(FIXTURE);
    let sarif = |extra: &[&str]| -> serde_json::Value {
        let mut args = vec!["--format", "sarif", "--quiet"];
        args.extend_from_slice(extra);
        let output = run_fallow_in_root("dead-code", &root, &args);
        serde_json::from_str(&output.stdout).expect("sarif json")
    };

    assert_eq!(sarif(&[])["runs"][0]["properties"]["cascadeHidden"], 4);
    let shown = sarif(&["--show-cascade"]);
    assert!(
        shown["runs"][0]["properties"]
            .get("cascadeHidden")
            .is_none()
    );
}

/// Markdown has no entry for a hidden finding, so one line names the count.
#[test]
fn markdown_names_the_hidden_count() {
    let root = super::common::fixture_path(FIXTURE);
    let markdown = |extra: &[&str]| -> String {
        let mut args = vec!["--format", "markdown", "--quiet"];
        args.extend_from_slice(extra);
        run_fallow_in_root("dead-code", &root, &args).stdout
    };
    let note = "_4 findings in unused files are hidden; use `--show-cascade` to list them._";

    assert!(markdown(&[]).contains(note), "{}", markdown(&[]));
    assert!(!markdown(&["--show-cascade"]).contains("--show-cascade"));
}

/// The vital signs measure the code, so they count the hidden findings. The
/// combined run reuses the dead-code results and must give the value of the
/// standalone `health` command.
#[test]
fn vital_signs_count_hidden_exports_in_every_command() {
    let root = super::common::fixture_path(FIXTURE);
    let standalone = parse_json(&run_fallow_in_root(
        "health",
        &root,
        &["--format", "json", "--quiet"],
    ));
    let standalone = &standalone["vital_signs"];
    assert_eq!(standalone["counts"]["dead_exports"], 5);

    for extra in [&[][..], &["--show-cascade"][..]] {
        let mut cmd_args = vec![
            "--root",
            root.to_str().unwrap(),
            "--format",
            "json",
            "--quiet",
        ];
        cmd_args.extend_from_slice(extra);
        let combined = parse_json(&super::common::run_fallow_raw(&cmd_args));
        let combined = &combined["health"]["vital_signs"];
        assert_eq!(
            combined["dead_export_pct"], standalone["dead_export_pct"],
            "{extra:?}"
        );
        assert_eq!(
            combined["counts"]["dead_exports"], standalone["counts"]["dead_exports"],
            "{extra:?}"
        );
    }
}

/// An issue-type filter without unused files removes the file from the
/// report, so the findings in it must stay.
#[test]
fn issue_type_filter_without_unused_files_hides_nothing() {
    let root = super::common::fixture_path(FIXTURE);
    let full = dead_code_json(&root, &["--show-cascade"]);

    for flag in ["--unused-exports", "--unused-types"] {
        let json = dead_code_json(&root, &[flag]);
        let key = flag.trim_start_matches("--").replace('-', "_");
        assert_eq!(json[&key], full[&key], "{flag}");
        assert!(paths(&json, &key).contains(&DEAD_FILE.to_owned()), "{flag}");
        assert!(json.get("cascade_hidden").is_none(), "{flag}");
        assert_counts_agree(&json);
    }

    let json = dead_code_json(&root, &["--unused-files", "--unused-exports"]);
    assert!(paths(&json, "unused_files").contains(&DEAD_FILE.to_owned()));
    assert!(!paths(&json, "unused_exports").contains(&DEAD_FILE.to_owned()));
    assert_eq!(json["cascade_hidden"], 3);
}

/// A changed-since scope that leaves the unused file out of the report hides
/// nothing: the scope already removed the findings of that file.
#[test]
fn changed_since_scope_without_the_unused_file_hides_nothing() {
    let dir = copy_fixture(FIXTURE);
    let root = dir.path();
    super::common::git(root, &["init", "-q", "-b", "main"]);
    super::common::commit_all(root, "base");
    std::fs::write(
        root.join("src/used.ts"),
        "export const used = 1;\nexport const liveUnused = 2;\nexport const added = 3;\n",
    )
    .unwrap();
    super::common::commit_all(root, "change");

    for extra in [&[][..], &["--unused-exports"][..]] {
        let mut args = vec!["--changed-since", "HEAD~1"];
        args.extend_from_slice(extra);
        let json = dead_code_json(root, &args);

        assert!(!paths(&json, "unused_files").contains(&DEAD_FILE.to_owned()));
        assert_eq!(cascade_paths(&json), Vec::<String>::new(), "{extra:?}");
        assert!(json.get("cascade_hidden").is_none(), "{extra:?}");
        assert_counts_agree(&json);
    }
}

/// The grouped envelope carries the count at its root, as the flat envelope
/// does, and no group carries it.
#[test]
fn grouped_output_carries_the_hidden_count_at_the_root() {
    let dir = copy_fixture(FIXTURE);
    std::fs::write(dir.path().join("CODEOWNERS"), "[Core]\n* @team\n").unwrap();
    let flat = dead_code_json(dir.path(), &[]);

    for mode in ["directory", "owner", "section"] {
        let json = dead_code_json(dir.path(), &["--group-by", mode]);
        assert_eq!(json["cascade_hidden"], flat["cascade_hidden"], "{mode}");
        assert_eq!(json["total_issues"], flat["total_issues"], "{mode}");
        for group in json["groups"].as_array().unwrap() {
            assert!(group.get("cascade_hidden").is_none(), "{mode}: {group}");
        }
    }

    let shown = dead_code_json(dir.path(), &["--group-by", "directory", "--show-cascade"]);
    assert!(shown.get("cascade_hidden").is_none());
}
