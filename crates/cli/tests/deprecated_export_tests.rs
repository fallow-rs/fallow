#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use std::path::Path;

use common::{canonical_report, parse_json, run_fallow, run_fallow_in_root};

const FIXTURE: &str = "deprecated-export-in-use";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create directory");
    for entry in std::fs::read_dir(from).expect("read directory") {
        let entry = entry.expect("directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

fn finding<'a>(json: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    json["deprecated_exports_in_use"]
        .as_array()
        .expect("deprecated_exports_in_use array")
        .iter()
        .find(|finding| finding["export_name"] == name)
}

#[test]
fn json_reports_count_capped_sample_and_plain_message() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);

    let old = finding(&json, "oldHelper").expect("oldHelper is reported");
    assert_eq!(old["consumer_count"], 12);
    assert_eq!(old["consumers"].as_array().unwrap().len(), 10);
    assert_eq!(old["consumers"][0]["path"], "src/consumers/c01.ts");
    assert_eq!(old["consumers"][0]["kind"], "named-import");
    assert_eq!(
        old["deprecated_reason"],
        "Use newHelper instead. It goes away in the next major."
    );
    assert_eq!(old["effective_severity"], "warn");
    assert_eq!(old["actions"][0]["type"], "migrate-deprecated-export");
    assert_eq!(old["actions"][0]["auto_fixable"], false);
    assert!(
        old["actions"][0]["note"]
            .as_str()
            .unwrap()
            .contains("fallow dead-code --trace <path>:oldHelper"),
        "the action names the uncapped trace path"
    );
    assert_eq!(
        old["actions"][1]["comment"],
        "// fallow-ignore-next-line deprecated-export-in-use"
    );

    let public = finding(&json, "publicOld").expect("publicOld is reported");
    assert_eq!(public["public_api"], true);
    assert!(public.get("re_export_only").is_none());
    assert!(
        public["actions"][0]["note"]
            .as_str()
            .unwrap()
            .starts_with("This export is public API."),
    );

    assert_eq!(
        json["summary"]["deprecated_exports_in_use"],
        json["deprecated_exports_in_use"].as_array().unwrap().len()
    );
}

#[test]
fn per_path_override_resolves_on_the_export_site() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    assert!(
        finding(&json, "offOld").is_none(),
        "the override turns the rule off for the file that declares the export"
    );
    assert!(finding(&json, "oldHelper").is_some());
}

#[test]
fn deprecated_unused_export_carries_the_decoration() {
    let output = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let json = parse_json(&output);
    let unused = json["unused_exports"].as_array().unwrap();
    let dead = unused
        .iter()
        .find(|finding| finding["export_name"] == "deadOld")
        .expect("deadOld is an unused export");
    assert_eq!(dead["deprecated"], true);
    assert_eq!(dead["deprecated_reason"], "gone soon");
    let plain = unused
        .iter()
        .find(|finding| finding["export_name"] == "newHelper")
        .expect("newHelper is an unused export");
    assert!(plain.get("deprecated").is_none());
    assert!(plain.get("deprecated_reason").is_none());

    let human = run_fallow(
        "dead-code",
        FIXTURE,
        &["--unused-exports", "--top", "50", "--quiet", "--no-cache"],
    );
    assert!(
        human.stdout.contains("deadOld (marked @deprecated)"),
        "{}",
        human.stdout
    );
}

#[test]
fn ci_formats_carry_the_rule_and_a_plain_text_message() {
    let sarif = parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "sarif", "--quiet", "--no-cache"],
    ));
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    let old = results
        .iter()
        .find(|result| {
            result["ruleId"] == "fallow/deprecated-export-in-use"
                && result["message"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("'oldHelper'")
        })
        .expect("SARIF result for oldHelper");
    let text = old["message"]["text"].as_str().unwrap();
    assert_eq!(
        text,
        "Deprecated export 'oldHelper' is still used by 12 consumers: Use newHelper instead. It goes away in the next major."
    );
    assert!(!text.contains("{@link"));
    assert_eq!(old["level"], "warning");

    let codeclimate = parse_json(&run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "codeclimate", "--quiet", "--no-cache"],
    ));
    assert!(
        codeclimate
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["check_name"] == "fallow/deprecated-export-in-use")
    );

    let compact = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "compact", "--quiet", "--no-cache"],
    );
    assert!(
        compact
            .stdout
            .contains("deprecated-export-in-use:src/lib/helpers.ts:7:oldHelper"),
        "{}",
        compact.stdout
    );

    let markdown = run_fallow(
        "dead-code",
        FIXTURE,
        &["--format", "markdown", "--quiet", "--no-cache"],
    );
    assert!(markdown.stdout.contains("### Deprecated exports in use"));
    assert!(
        markdown.stdout.contains(
            "`oldHelper` still used by 12 consumers: Use newHelper instead. It goes away in the next major."
        ),
        "{}",
        markdown.stdout
    );
}

#[test]
fn filter_flag_opts_in_and_reports_only_this_issue_type() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"deprecated-opt-in","private":true,"main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { old } from './old';\nexport const run = old;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/old.ts"),
        "/** @deprecated use new */\nexport const old = 1;\nexport const unused = 2;\n",
    )
    .unwrap();

    let default_run = parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &["--format", "json", "--quiet", "--no-cache"],
    ));
    assert_eq!(
        default_run["deprecated_exports_in_use"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "the rule defaults to off"
    );
    assert_eq!(default_run["unused_exports"].as_array().unwrap().len(), 1);

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--deprecated-exports-in-use",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "the opt-in severity is warn");
    let json = parse_json(&output);
    assert_eq!(
        json["deprecated_exports_in_use"].as_array().unwrap().len(),
        1
    );
    assert_eq!(json["unused_exports"].as_array().unwrap().len(), 0);
    assert_eq!(json["total_issues"], 1);
}

#[test]
fn cold_and_warm_cache_runs_are_byte_identical() {
    let dir = tempfile::tempdir().expect("temporary project");
    copy_dir(&common::fixture_path(FIXTURE), dir.path());
    let args = ["--format", "json", "--quiet"];
    let cold = run_fallow_in_root("dead-code", dir.path(), &args);
    let warm = run_fallow_in_root("dead-code", dir.path(), &args);
    assert_eq!(canonical_report(&cold), canonical_report(&warm));
    assert!(
        parse_json(&warm)["deprecated_exports_in_use"]
            .as_array()
            .is_some_and(|findings| !findings.is_empty())
    );
}
