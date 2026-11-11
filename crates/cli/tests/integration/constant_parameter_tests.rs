//! Process-level opt-in, output formats and manual-fix contract of the
//! `constant-parameter` rule.
use super::common::{copy_fixture, parse_json, run_fallow_in_root};

const JSON_ARGS: &[&str] = &[
    "--constant-parameters",
    "--format",
    "json",
    "--quiet",
    "--no-cache",
];

#[test]
fn constant_parameters_stay_off_without_the_flag() {
    let dir = copy_fixture("constant-parameter");
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let report = parse_json(&output);
    assert!(report.get("constant_parameters").is_none(), "{report}");
    assert!(report["summary"].get("constant_parameters").is_none());
}

#[test]
fn constant_parameters_json_carries_the_evidence_and_manual_actions() {
    let dir = copy_fixture("constant-parameter");
    let output = run_fallow_in_root("dead-code", dir.path(), JSON_ARGS);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let report = parse_json(&output);
    assert_eq!(report["summary"]["constant_parameters"], 2);
    let findings = report["constant_parameters"].as_array().expect("array");
    let compact = findings
        .iter()
        .find(|finding| finding["parameter_name"] == "compact")
        .expect("compact finding");
    assert_eq!(compact["path"], "src/format.ts");
    assert_eq!(compact["function_name"], "format");
    assert_eq!(compact["parameter_index"], 1);
    assert_eq!(compact["reason"], "identical-argument");
    assert_eq!(compact["value"]["kind"], "boolean");
    assert_eq!(compact["value"]["text"], "true");
    assert_eq!(compact["condition_reads"], 1);
    assert_eq!(compact["call_site_count"], 3);
    assert_eq!(compact["inspected_call_sites"][0]["path"], "src/main.ts");
    assert!(
        compact["finding_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("dc1:constant-parameter:"))
    );
    let actions = compact["actions"].as_array().expect("actions");
    assert_eq!(actions[0]["type"], "review-constant-parameter");
    assert_eq!(actions[1]["type"], "suppress-line");
    assert_eq!(
        actions[1]["comment"],
        "// fallow-ignore-next-line constant-parameter"
    );
    assert!(actions.iter().all(|action| action["auto_fixable"] == false));
    let locale = findings
        .iter()
        .find(|finding| finding["parameter_name"] == "locale")
        .expect("locale finding");
    assert_eq!(locale["reason"], "never-passed");
    assert!(locale.get("value").is_none());
    assert_eq!(locale["optional"], true);
}

#[test]
fn constant_parameters_render_in_each_format() {
    let dir = copy_fixture("constant-parameter");
    let root = dir.path();
    let human = run_fallow_in_root(
        "dead-code",
        root,
        &["--constant-parameters", "--quiet", "--no-cache"],
    );
    assert_eq!(human.code, 0, "{}", human.stderr);
    assert!(
        human
            .stdout
            .contains("format(compact): every call passes true (3 calls, 1 condition read)"),
        "{}",
        human.stdout
    );
    assert!(human.stdout.contains("format(locale): no call passes it"));
    let compact = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--constant-parameters",
            "--format",
            "compact",
            "--quiet",
            "--no-cache",
        ],
    );
    assert!(
        compact
            .stdout
            .contains("constant-parameter:src/format.ts:1:format(compact) identical-argument"),
        "{}",
        compact.stdout
    );
    let saved = root.join("report.json");
    let json = run_fallow_in_root("dead-code", root, JSON_ARGS);
    std::fs::write(&saved, &json.stdout).unwrap();
    for (format, needle) in [
        ("sarif", "fallow/constant-parameter"),
        ("codeclimate", "fallow/constant-parameter"),
        ("markdown", "Constant parameters"),
        ("github-summary", "Constant parameters"),
        ("github-annotations", "Constant parameter"),
    ] {
        let rendered = run_fallow_in_root(
            "report",
            root,
            &[
                "--from",
                saved.to_str().unwrap(),
                "--format",
                format,
                "--quiet",
            ],
        );
        assert_eq!(rendered.code, 0, "{format}: {}", rendered.stderr);
        assert!(
            rendered.stdout.contains(needle) && rendered.stdout.contains("compact"),
            "{format} omitted the finding: {}",
            rendered.stdout
        );
    }
}

#[test]
fn constant_parameter_errors_gate_and_stay_out_of_fix() {
    let dir = copy_fixture("constant-parameter");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"rules":{"constant-parameters":"error"}}"#,
    )
    .unwrap();
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    assert_eq!(output.code, 1, "{}", output.stderr);
    let report = parse_json(&output);
    assert_eq!(
        report["constant_parameters"][0]["effective_severity"],
        "error"
    );
    let before = std::fs::read(dir.path().join("src/format.ts")).unwrap();
    let dry = run_fallow_in_root(
        "fix",
        dir.path(),
        &["--dry-run", "--format", "json", "--quiet", "--no-cache"],
    );
    assert!(dry.code == 0 || dry.code == 1, "{}", dry.stderr);
    let fixes = parse_json(&dry);
    assert!(
        !fixes["fixes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fix| fix["name"] == "compact" || fix["name"] == "locale")
    );
    assert_eq!(
        std::fs::read(dir.path().join("src/format.ts")).unwrap(),
        before
    );
}

#[test]
fn a_suppression_hides_the_finding_and_is_not_stale() {
    let dir = copy_fixture("constant-parameter");
    let source = std::fs::read_to_string(dir.path().join("src/format.ts")).unwrap();
    std::fs::write(
        dir.path().join("src/format.ts"),
        format!("// fallow-ignore-next-line constant-parameter\n{source}"),
    )
    .unwrap();
    let output = run_fallow_in_root("dead-code", dir.path(), JSON_ARGS);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let report = parse_json(&output);
    assert!(report.get("constant_parameters").is_none(), "{report}");
    assert!(
        report
            .get("stale_suppressions")
            .is_none_or(|stale| stale.as_array().is_some_and(Vec::is_empty)),
        "{report}"
    );
}

#[test]
fn production_mode_drops_test_callers() {
    let dir = copy_fixture("constant-parameter");
    std::fs::write(
        dir.path().join("src/format.test.ts"),
        "import { format } from './format';\ntest('long', () => format(4, false, 'nl'));\n",
    )
    .unwrap();
    let default_mode = parse_json(&run_fallow_in_root("dead-code", dir.path(), JSON_ARGS));
    assert!(
        default_mode.get("constant_parameters").is_none(),
        "{default_mode}"
    );
    let mut production_args = JSON_ARGS.to_vec();
    production_args.push("--production");
    let production = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        &production_args,
    ));
    assert_eq!(
        production["summary"]["constant_parameters"], 2,
        "{production}"
    );
}

const CACHED_JSON_ARGS: &[&str] = &["--constant-parameters", "--format", "json", "--quiet"];

fn cache_size(root: &std::path::Path) -> u64 {
    std::fs::metadata(root.join(".fallow/cache.bin"))
        .expect("the run writes the parse cache")
        .len()
}

#[test]
fn a_run_without_the_rule_stores_no_call_facts() {
    let dir = copy_fixture("constant-parameter");
    let plain = run_fallow_in_root("dead-code", dir.path(), &["--format", "json", "--quiet"]);
    assert_eq!(plain.code, 0, "{}", plain.stderr);
    let without_facts = cache_size(dir.path());

    let with_rule = run_fallow_in_root("dead-code", dir.path(), CACHED_JSON_ARGS);
    assert_eq!(with_rule.code, 0, "{}", with_rule.stderr);
    assert!(
        cache_size(dir.path()) > without_facts,
        "the run with the rule adds the call facts that the run without it left out"
    );
}

#[test]
fn a_warm_cache_without_call_facts_gives_the_findings_of_a_cold_run() {
    let dir = copy_fixture("constant-parameter");
    let cold = parse_json(&run_fallow_in_root("dead-code", dir.path(), JSON_ARGS));

    let plain = run_fallow_in_root("dead-code", dir.path(), &["--format", "json", "--quiet"]);
    assert_eq!(plain.code, 0, "{}", plain.stderr);
    let upgrade = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        CACHED_JSON_ARGS,
    ));
    let warm = parse_json(&run_fallow_in_root(
        "dead-code",
        dir.path(),
        CACHED_JSON_ARGS,
    ));
    assert_eq!(cold["summary"]["constant_parameters"], 2, "{cold}");
    for report in [&upgrade, &warm] {
        assert_eq!(
            report["constant_parameters"], cold["constant_parameters"],
            "{report}"
        );
    }
}
