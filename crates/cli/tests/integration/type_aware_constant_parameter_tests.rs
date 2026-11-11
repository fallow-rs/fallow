//! Type evidence for the opt-in `constant-parameter` rule.

#![expect(clippy::expect_used, reason = "tests use expect for fixture setup")]

use crate::common::{parse_json, run_fallow_raw, run_fallow_raw_with_type_aware_sidecar};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir(root.join("src")).expect("source directory");
    for (path, source) in [
        (
            "package.json",
            r#"{"name":"call-evidence","private":true,"main":"src/index.ts"}"#,
        ),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":true,"module":"esnext","moduleResolution":"bundler","target":"es2022"},"include":["src"]}"#,
        ),
        (
            "src/format.ts",
            "export const pad = (text: string, width: number = 2): string => text.padStart(width);\n",
        ),
        (
            "src/service.ts",
            "export class Store {\n  save(id: string, retries: number = 3): string { return id + retries; }\n  load(id: string, cache?: boolean): string { return cache ? id : id; }\n}\n",
        ),
        (
            "src/index.ts",
            "import { pad } from './format';\nimport { Store } from './service';\nexport const run = (n: number) => {\n  const store = new Store();\n  store.save('a', n);\n  store.save('b', 2);\n  store.load('a');\n  store.load('b');\n  return [pad('x', n), pad('y', n + 1)];\n};\n",
        ),
    ] {
        std::fs::write(root.join(path), source).expect("fixture source");
    }
    dir
}

fn findings(output: &serde_json::Value) -> Vec<(String, String, String, Option<String>)> {
    output["constant_parameters"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["function_name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        item["parameter_name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        item["reason"].as_str().unwrap_or_default().to_string(),
                        item["evidence"].as_str().map(str::to_string),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn type_aware_mode_adds_findings_that_need_type_evidence() {
    let dir = project();
    let root = dir.path().to_string_lossy();
    let args = [
        "dead-code",
        "--root",
        root.as_ref(),
        "--constant-parameters",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ];
    let syntactic = parse_json(&run_fallow_raw(&args));
    assert!(
        findings(&syntactic).is_empty(),
        "the call facts alone prove no finding: {syntactic:#}"
    );
    assert!(syntactic["_meta"]["type_aware"].is_null());

    let mut type_aware = args.to_vec();
    type_aware.push("--type-aware");
    let output = parse_json(&run_fallow_raw_with_type_aware_sidecar(&type_aware));
    let evidence = Some("type-aware".to_string());
    assert_eq!(
        findings(&output),
        vec![
            (
                "pad".to_string(),
                "width".to_string(),
                "unused-default".to_string(),
                evidence.clone()
            ),
            (
                "Store.save".to_string(),
                "retries".to_string(),
                "unused-default".to_string(),
                evidence.clone()
            ),
            (
                "Store.load".to_string(),
                "cache".to_string(),
                "never-passed".to_string(),
                evidence
            ),
        ],
        "{output:#}"
    );
    let meta = &output["_meta"]["type_aware"]["call_evidence"];
    assert_eq!(meta["findings"], 3, "{meta:#}");
    assert_eq!(meta["default_candidates"], 1, "{meta:#}");
}

#[test]
fn a_suppression_hides_a_method_finding() {
    let dir = project();
    std::fs::write(
        dir.path().join("src/service.ts"),
        "export class Store {\n  // fallow-ignore-next-line constant-parameter\n  save(id: string, retries: number = 3): string { return id + retries; }\n  load(id: string, cache?: boolean): string { return cache ? id : id; }\n}\n",
    )
    .expect("fixture source");
    let root = dir.path().to_string_lossy();
    let output = parse_json(&run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        root.as_ref(),
        "--constant-parameters",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let names: Vec<String> = findings(&output)
        .into_iter()
        .map(|finding| finding.0)
        .collect();
    assert!(!names.contains(&"Store.save".to_string()), "{output:#}");
    assert!(names.contains(&"Store.load".to_string()), "{output:#}");
}

#[test]
fn an_overloaded_method_does_not_break_the_request() {
    let dir = project();
    std::fs::write(
        dir.path().join("src/service.ts"),
        "export class Store {\n  save(id: string, retries: number = 3): string { return id + retries; }\n  load(id: string): string;\n  load(id: number): string;\n  load(id: string | number, cache?: boolean): string { return cache ? String(id) : String(id); }\n}\n",
    )
    .expect("fixture source");
    let root = dir.path().to_string_lossy();
    let output = parse_json(&run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        root.as_ref(),
        "--constant-parameters",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    let meta = &output["_meta"]["type_aware"];
    assert_eq!(
        meta["warnings"].as_array().map_or(0, Vec::len),
        0,
        "{meta:#}"
    );
    assert_eq!(
        meta["call_evidence"]["abstention_reasons"]["overload"], 1,
        "{meta:#}"
    );
    assert!(
        findings(&output)
            .iter()
            .any(|finding| finding.0 == "Store.save"),
        "{output:#}"
    );
}
