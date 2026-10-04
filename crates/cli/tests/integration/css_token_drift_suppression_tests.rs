use crate::common::{git, parse_json, run_fallow_in_root};
use std::path::Path;
use tempfile::tempdir;

const PLAIN: &str = "export const C = () => <div className=\"max-w-[1600px]\" />;\n";
const SUPPRESSED: &str = "// fallow-ignore-next-line css-token-drift -- intentional fixture value\nexport const C = () => <div className=\"max-w-[1600px]\" />;\n";
const CONTROL: &str = "export const Control = () => <div className=\"max-w-[1599px]\" />;\n";

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn setup(root: &Path) {
    write(
        root,
        "package.json",
        r#"{"name":"token-occurrence-fixture","private":true,"dependencies":{"react":"19.0.0","tailwindcss":"4.1.10"}}"#,
    );
    write(root, ".gitignore", ".fallow/\n");
    write(root, ".fallowrc.json", r#"{"entry":["src/**/*.tsx"]}"#);
    write(root, "src/control.tsx", CONTROL);
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Fixture User"]);
    git(root, &["config", "user.email", "fixture@example.com"]);
    git(root, &["-c", "commit.gpgsign=false", "add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "fixture base"],
    );
    write(root, "src/live.tsx", CONTROL);
}

fn sites(report: &serde_json::Value, value: &str) -> Vec<(String, u32)> {
    let findings: &[serde_json::Value] = match report.get("styling_findings") {
        None => &[],
        Some(value) => value.as_array().expect("styling_findings must be an array"),
    };
    findings
        .iter()
        .filter(|finding| {
            finding["sub_kind"] == "tailwind-arbitrary-value" && finding["value"] == value
        })
        .map(|finding| {
            (
                finding["path"].as_str().unwrap().replace('\\', "/"),
                u32::try_from(finding["line"].as_u64().unwrap()).unwrap(),
            )
        })
        .collect()
}

fn assert_health_and_audit(root: &Path, expected: &[(&str, u32)], uses: u32) {
    for command in ["health", "audit"] {
        for cold in [true, false, false] {
            let mut args = vec!["--format", "json", "--quiet"];
            if command == "health" {
                args.push("--css");
            } else {
                args.extend(["--base", "HEAD"]);
            }
            if cold {
                args.push("--no-cache");
            }
            let output = run_fallow_in_root(command, root, &args);
            assert_eq!(output.code, 0, "{command}: {}", output.stderr);
            let json = parse_json(&output);
            let report = if command == "audit" {
                &json["complexity"]
            } else {
                &json
            };
            let wanted: Vec<_> = expected
                .iter()
                .map(|(path, line)| ((*path).to_string(), *line))
                .collect();
            assert_eq!(
                sites(report, "max-w-[1600px]"),
                wanted,
                "{command}, cold={cold}"
            );
            if command == "audit" {
                assert_eq!(
                    sites(report, "max-w-[1599px]"),
                    vec![("src/live.tsx".to_string(), 1)]
                );
            }
            if command == "health" {
                assert_eq!(
                    sites(report, "max-w-[1599px]"),
                    vec![("src/control.tsx".to_string(), 1)]
                );
                let candidate = report["css_analytics"]["tailwind_arbitrary_values"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["value"] == "max-w-[1600px]")
                    .unwrap();
                assert_eq!(candidate["count"], uses);
                assert_eq!(
                    report["css_analytics"]["summary"]["tailwind_arbitrary_value_uses"],
                    uses + 2
                );
            }
        }
    }
}

#[test]
fn mixed_occurrence_orders_preserve_unsuppressed_sites() {
    let same_suppressed_first =
        format!("{SUPPRESSED}export const D = () => <div className=\"max-w-[1600px]\" />;\n");
    let same_plain_first = format!(
        "{PLAIN}// fallow-ignore-next-line css-token-drift -- intentional fixture value\nexport const D = () => <div className=\"max-w-[1600px]\" />;\n"
    );
    type MixedCase<'a> = (Vec<(&'a str, &'a str)>, &'a str, u32, u32);
    let cases: Vec<MixedCase<'_>> = vec![
        (vec![("src/a.tsx", PLAIN)], "src/a.tsx", 1, 1),
        (
            vec![("src/a.tsx", SUPPRESSED), ("src/z.tsx", PLAIN)],
            "src/z.tsx",
            1,
            2,
        ),
        (
            vec![("src/a.tsx", PLAIN), ("src/z.tsx", SUPPRESSED)],
            "src/a.tsx",
            1,
            2,
        ),
        (
            vec![("src/a.tsx", &same_suppressed_first)],
            "src/a.tsx",
            3,
            2,
        ),
        (vec![("src/a.tsx", &same_plain_first)], "src/a.tsx", 1, 2),
    ];
    for (files, path, line, uses) in cases {
        let dir = tempdir().unwrap();
        setup(dir.path());
        for (path, source) in files {
            write(dir.path(), path, source);
        }
        assert_health_and_audit(dir.path(), &[(path, line)], uses);
    }
}

#[test]
fn suppressed_only_has_a_live_positive_control() {
    let file_wide =
        format!("// fallow-ignore-file css-token-drift -- intentional fixture values\n{PLAIN}");
    for source in [SUPPRESSED, file_wide.as_str()] {
        let dir = tempdir().unwrap();
        setup(dir.path());
        write(dir.path(), "src/a.tsx", source);
        assert_health_and_audit(dir.path(), &[], 1);
    }
}

#[test]
fn normalized_variants_choose_one_unsuppressed_representative() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    write(
        dir.path(),
        "src/a.tsx",
        "// fallow-ignore-next-line css-token-drift -- intentional fixture value\nexport const A = () => <div className=\"md:grid-rows-[auto_auto]\" />;\n",
    );
    write(
        dir.path(),
        "src/z.tsx",
        "export const Z = () => <div className=\"lg:grid-rows-[auto_auto]\" />;\n",
    );
    for command in ["health", "audit"] {
        for cold in [true, false, false] {
            let mut args = vec!["--format", "json", "--quiet"];
            if command == "health" {
                args.push("--css");
            } else {
                args.extend(["--base", "HEAD"]);
            }
            if cold {
                args.push("--no-cache");
            }
            let output = run_fallow_in_root(command, dir.path(), &args);
            assert_eq!(output.code, 0, "{}", output.stdout);
            let json = parse_json(&output);
            let report = if command == "audit" {
                &json["complexity"]
            } else {
                &json
            };
            assert_eq!(
                sites(report, "grid-rows-[auto_auto]"),
                vec![("src/z.tsx".to_string(), 1)]
            );
            let control = if command == "audit" {
                "src/live.tsx"
            } else {
                "src/control.tsx"
            };
            assert_eq!(
                sites(report, "max-w-[1599px]"),
                vec![(control.to_string(), 1)]
            );
        }
    }
}

#[test]
fn changed_scope_uses_later_site_even_when_analytics_representative_is_outside_scope() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    write(dir.path(), "src/a.tsx", SUPPRESSED);
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "suppressed base",
        ],
    );
    write(dir.path(), "src/z.tsx", PLAIN);
    for (command, extra) in [
        ("health", vec!["--css", "--changed-since", "HEAD"]),
        ("audit", vec!["--base", "HEAD", "--css-deep"]),
        ("audit", vec!["--base", "HEAD", "--no-css-deep"]),
    ] {
        for cold in [true, false, false] {
            let mut args = vec!["--format", "json", "--quiet"];
            args.extend(&extra);
            if cold {
                args.push("--no-cache");
            }
            let output = run_fallow_in_root(command, dir.path(), &args);
            assert_eq!(output.code, 0, "{} {}", output.stderr, output.stdout);
            let json = parse_json(&output);
            let report = if command == "audit" {
                &json["complexity"]
            } else {
                &json
            };
            assert_eq!(
                sites(report, "max-w-[1600px]"),
                vec![("src/z.tsx".to_string(), 1)]
            );
            assert!(sites(report, "max-w-[1599px]").is_empty());
            if extra.contains(&"--css-deep") {
                assert!(report["css_analytics"].is_object());
                assert!(
                    report["css_analytics"]
                        .get("tailwind_arbitrary_values")
                        .is_none()
                );
            }
        }
    }
}

#[test]
fn unsuppressed_out_of_scope_site_cannot_steal_changed_representative() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    write(dir.path(), "src/a.tsx", PLAIN);
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "unsuppressed base",
        ],
    );
    write(dir.path(), "src/z.tsx", PLAIN);
    let output = run_fallow_in_root(
        "audit",
        dir.path(),
        &[
            "--base",
            "HEAD",
            "--css-deep",
            "--format",
            "json",
            "--quiet",
        ],
    );
    assert_eq!(output.code, 0, "{}", output.stdout);
    let json = parse_json(&output);
    assert_eq!(
        sites(&json["complexity"], "max-w-[1600px]"),
        vec![("src/z.tsx".to_string(), 1)]
    );
}

#[test]
fn empty_changed_scope_omits_report_but_suppressed_scope_keeps_analytics() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &["-c", "commit.gpgsign=false", "commit", "-m", "control base"],
    );
    let args = [
        "--css",
        "--changed-since",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ];
    let empty = run_fallow_in_root("health", dir.path(), &args);
    assert_eq!(empty.code, 0, "{}", empty.stdout);
    let empty_json = parse_json(&empty);
    assert!(empty_json.get("css_analytics").is_none());
    assert!(empty_json.get("styling_findings").is_none());
    write(dir.path(), "src/z.tsx", SUPPRESSED);
    let suppressed = run_fallow_in_root("health", dir.path(), &args);
    assert_eq!(suppressed.code, 0, "{}", suppressed.stdout);
    let report = parse_json(&suppressed);
    assert!(report["css_analytics"].is_object());
    assert_eq!(
        report["css_analytics"]["tailwind_arbitrary_values"][0]["count"],
        1
    );
    assert!(sites(&report, "max-w-[1600px]").is_empty());
    assert!(report.get("styling_findings").is_none());
    write(dir.path(), "src/z.tsx", PLAIN);
    let positive = run_fallow_in_root("health", dir.path(), &args);
    assert_eq!(positive.code, 0, "{}", positive.stdout);
    assert_eq!(
        sites(&parse_json(&positive), "max-w-[1600px]"),
        vec![("src/z.tsx".to_string(), 1)]
    );
}

#[test]
fn analytics_attribution_actions_and_severity_contract_remain_stable() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    write(dir.path(), "src/a.tsx", SUPPRESSED);
    write(dir.path(), "src/z.tsx", PLAIN);
    for severity in ["warn", "error", "off"] {
        write(
            dir.path(),
            ".fallowrc.json",
            &format!(r#"{{"entry":["src/**/*.tsx"],"rules":{{"css-token-drift":"{severity}"}}}}"#),
        );
        let output = run_fallow_in_root(
            "health",
            dir.path(),
            &["--css", "--format", "json", "--quiet"],
        );
        assert_eq!(output.code, 0, "{}", output.stdout);
        let report = parse_json(&output);
        let analytics = &report["css_analytics"];
        let candidate = analytics["tailwind_arbitrary_values"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["value"] == "max-w-[1600px]")
            .unwrap();
        assert_eq!(candidate["path"], "src/a.tsx");
        assert_eq!(candidate["line"], 2);
        assert_eq!(candidate["count"], 2);
        assert!(report.get("tailwind_occurrences").is_none());
        assert!(analytics.get("tailwind_occurrences").is_none());
        assert!(candidate.get("occurrences").is_none());
        if severity == "off" {
            assert!(sites(&report, "max-w-[1600px]").is_empty());
            continue;
        }
        assert_eq!(
            sites(&report, "max-w-[1600px]"),
            vec![("src/z.tsx".to_string(), 1)]
        );
        let finding = report["styling_findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["value"] == "max-w-[1600px]")
            .unwrap();
        assert_eq!(finding["effective_severity"], severity);
        assert_eq!(finding["actions"], candidate["actions"]);
        assert_eq!(finding["actions"][0]["type"], "replace-with-token");
        assert_eq!(finding["actions"][0]["auto_fixable"], false);
        if severity == "error" {
            let audit = run_fallow_in_root(
                "audit",
                dir.path(),
                &["--base", "HEAD", "--format", "json", "--quiet"],
            );
            assert_eq!(audit.code, 1, "{}", audit.stdout);
            assert_eq!(
                sites(&parse_json(&audit)["complexity"], "max-w-[1600px]"),
                vec![("src/z.tsx".to_string(), 1)]
            );
        }
    }
}

#[test]
fn workspace_scope_selects_occurrences_from_requested_package() {
    let dir = tempdir().unwrap();
    setup(dir.path());
    write(
        dir.path(),
        "package.json",
        r#"{"name":"workspace-fixture","private":true,"workspaces":["packages/*"],"dependencies":{"react":"19.0.0","tailwindcss":"4.1.10"}}"#,
    );
    for name in ["a", "z"] {
        write(
            dir.path(),
            &format!("packages/{name}/package.json"),
            &format!(r#"{{"name":"{name}","main":"src/index.tsx"}}"#),
        );
    }
    write(dir.path(), "packages/a/src/index.tsx", PLAIN);
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "workspace base",
        ],
    );
    write(dir.path(), "packages/z/src/index.tsx", PLAIN);
    for command in ["health", "audit"] {
        for cold in [true, false, false] {
            let mut args = vec!["--workspace", "z", "--format", "json", "--quiet"];
            if command == "health" {
                args.push("--css");
            } else {
                args.extend(["--base", "HEAD"]);
            }
            if cold {
                args.push("--no-cache");
            }
            let output = run_fallow_in_root(command, dir.path(), &args);
            assert_eq!(output.code, 0, "{}", output.stdout);
            let json = parse_json(&output);
            let report = if command == "audit" {
                &json["complexity"]
            } else {
                &json
            };
            assert_eq!(
                sites(report, "max-w-[1600px]"),
                vec![("packages/z/src/index.tsx".to_string(), 1)]
            );
            assert!(sites(report, "max-w-[1599px]").is_empty());
        }
    }
}
