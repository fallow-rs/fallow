//! Production mode keeps test files out of the health findings, but the static
//! coverage estimate behind CRAP must still see them. Without the test files,
//! every function looks untested and fails the CRAP gate.

use crate::common::{parse_json, redact_all, run_fallow_in_root};

const GRADE_SOURCE: &str = r#"export function grade(score: number): string {
    if (score > 90) return "A";
    if (score > 80) return "B";
    if (score > 70) return "C";
    if (score > 60) return "D";
    if (score > 50) return "E";
    return "F";
}
"#;

const GRADE_TEST: &str = r#"import { expect, test } from "vitest";
import { grade } from "./grade";

test("grade", () => {
    expect(grade(95)).toBe("A");
});
"#;

fn tested_function_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "repro", "private": true, "type": "module", "main": "src/index.ts", "devDependencies": { "vitest": "3.2.0" } }"#,
    )
    .expect("write package.json");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/grade.ts"), GRADE_SOURCE).expect("write grade.ts");
    std::fs::write(
        root.join("src/index.ts"),
        "import { grade } from \"./grade\";\nconsole.log(grade(75));\n",
    )
    .expect("write index.ts");
    std::fs::write(root.join("src/grade.test.ts"), GRADE_TEST).expect("write grade.test.ts");
    dir
}

fn crap_findings(json: &serde_json::Value) -> Vec<&serde_json::Value> {
    json["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .filter(|finding| {
                    finding["exceeded"]
                        .as_str()
                        .is_some_and(|exceeded| exceeded.contains("crap"))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn health_production_estimates_crap_with_the_test_files() {
    let project = tested_function_project();
    let full = run_fallow_in_root(
        "health",
        project.path(),
        &["--format", "json", "--quiet", "--no-cache"],
    );
    let full_json = parse_json(&full);
    assert!(
        crap_findings(&full_json).is_empty(),
        "a tested function must not fail CRAP: {}",
        redact_all(&full.stdout, project.path())
    );

    let production = run_fallow_in_root(
        "health",
        project.path(),
        &["--production", "--format", "json", "--quiet", "--no-cache"],
    );
    let production_json = parse_json(&production);
    assert!(
        crap_findings(&production_json).is_empty(),
        "production mode must still credit the test file in the CRAP estimate: {}",
        redact_all(&production.stdout, project.path())
    );
    assert_eq!(
        production.code,
        0,
        "a tested function must not fail health in production mode: {}",
        redact_all(&production.stdout, project.path())
    );
}

#[test]
fn health_production_coverage_gaps_credit_the_test_files() {
    let project = tested_function_project();
    let output = run_fallow_in_root(
        "health",
        project.path(),
        &[
            "--production",
            "--coverage-gaps",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    );
    let json = parse_json(&output);
    let summary = &json["coverage_gaps"]["summary"];
    let untested_files: Vec<&str> = json["coverage_gaps"]["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        untested_files,
        vec!["src/index.ts"],
        "the test file reaches `src/grade.ts`, and no test reaches the entry: {}",
        redact_all(&output.stdout, project.path())
    );
    assert_eq!(
        summary["untested_exports"].as_u64(),
        Some(0),
        "the test file references `grade`: {}",
        redact_all(&output.stdout, project.path())
    );
}
