//! Process-level opt-in, saved reports and manual-fix contracts.
use super::common::{copy_fixture, parse_json, run_fallow_in_root};

#[test]
fn absent_component_props_cli_selection_and_saved_native_formats() {
    let dir = copy_fixture("absent-component-prop");
    let root = dir.path();
    let output = run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--absent-component-props",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stderr);
    let report = parse_json(&output);
    assert_eq!(report["summary"]["absent_component_props"], 1);
    assert_eq!(
        report["absent_component_props"][0]["prop_name"],
        "highlight"
    );
    assert_eq!(
        report["absent_component_props"][0]["inspected_call_sites"][0]["path"],
        "src/main.tsx"
    );
    let saved = root.join("report.json");
    std::fs::write(&saved, &output.stdout).unwrap();
    for format in ["human", "compact"] {
        let rendered = run_fallow_in_root(
            "dead-code",
            root,
            &[
                "--absent-component-props",
                "--format",
                format,
                "--quiet",
                "--no-cache",
            ],
        );
        assert_eq!(rendered.code, 0, "{format}: {}", rendered.stderr);
        assert!(
            rendered.stdout.contains("Card.highlight"),
            "live report omitted candidate in {format}: {}",
            rendered.stdout
        );
    }
    for format in [
        "github-summary",
        "github-annotations",
        "markdown",
        "sarif",
        "codeclimate",
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
            rendered.stdout.contains("highlight"),
            "saved report omitted candidate in {format}: {}",
            rendered.stdout
        );
        assert!(
            !rendered.stdout.contains("remove it or use it"),
            "candidate advice conflates local usage in {format}"
        );
    }
    let summary = run_fallow_in_root(
        "report",
        root,
        &[
            "--from",
            saved.to_str().unwrap(),
            "--format",
            "github-summary",
            "--quiet",
        ],
    );
    assert!(summary.stdout.contains("Card.highlight"));
    assert!(summary.stdout.contains("src/main.tsx"));
    assert!(
        summary
            .stdout
            .contains("static analysis does not prove runtime unreachability")
    );
}

#[test]
fn configured_error_candidates_gate_cli_and_remain_manual_in_fix_dry_run() {
    let dir = copy_fixture("absent-component-prop");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"rules":{"absent-component-props":"error"}}"#,
    )
    .unwrap();
    let output = run_fallow_in_root(
        "dead-code",
        dir.path(),
        &[
            "--absent-component-props",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    assert_eq!(output.code, 1, "{}", output.stderr);
    let finding = parse_json(&output)["absent_component_props"][0].clone();
    assert_eq!(finding["effective_severity"], "error");
    assert!(
        finding["actions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|action| action["auto_fixable"] == false)
    );
    let before = std::fs::read(dir.path().join("src/Card.tsx")).unwrap();
    let dry = run_fallow_in_root(
        "fix",
        dir.path(),
        &["--dry-run", "--format", "json", "--quiet", "--no-cache"],
    );
    assert!(dry.code == 0 || dry.code == 1, "{}", dry.stderr);
    let report = parse_json(&dry);
    assert!(
        !report["fixes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|fix| fix["name"] == "highlight")
    );
    assert_eq!(
        std::fs::read(dir.path().join("src/Card.tsx")).unwrap(),
        before
    );
}
