//! Semantic consumers must belong to the selected discovery scope.

#![expect(clippy::expect_used, reason = "tests use expect for fixture setup")]

use crate::common::{parse_json, run_fallow_raw_with_type_aware_sidecar};

#[test]
fn production_semantics_cannot_credit_excluded_test_consumers() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir(root.join("src")).expect("source directory");
    for (path, source) in [
        (
            "package.json",
            r#"{"name":"semantic-scope","private":true,"main":"src/index.ts"}"#,
        ),
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"strict":true,"module":"esnext","moduleResolution":"bundler","target":"es2022"},"include":["src"]}"#,
        ),
        (
            "src/index.ts",
            "import { Service } from './library';\nvoid new Service();\n",
        ),
        (
            "src/library.ts",
            "export const testOnly = 1;\nexport type TestOnly = { value: number };\nexport class Service { testOnly(): number { return 1; } }\nexport const unused = 2;\n",
        ),
        (
            "src/library.test.ts",
            "import { testOnly, Service } from './library';\nimport type { TestOnly } from './library';\nconst value: TestOnly = { value: testOnly };\nvoid value;\nvoid new Service().testOnly();\n",
        ),
    ] {
        std::fs::write(root.join(path), source).expect("fixture source");
    }
    let root_arg = root.to_string_lossy();
    for production in [true, false] {
        if !production {
            std::fs::write(
                root.join(".fallowrc.json"),
                r#"{"ignorePatterns":["**/*.test.ts"]}"#,
            )
            .expect("discovery exclusions");
        }
        for cache_arg in [Some("--no-cache"), None, None] {
            let mut args = vec![
                "dead-code",
                "--root",
                &root_arg,
                "--type-aware",
                "--unused-exports",
                "--unused-types",
                "--unused-class-members",
                "--format",
                "json",
                "--quiet",
            ];
            if production {
                args.push("--production");
            }
            if let Some(cache_arg) = cache_arg {
                args.push(cache_arg);
            }
            let output = run_fallow_raw_with_type_aware_sidecar(&args);
            assert_ne!(output.code, 2, "{}", output.stderr);
            let json = parse_json(&output);
            assert_eq!(json["_meta"]["type_aware"]["executed"], true);
            assert_findings(&json, true);
        }
    }
    std::fs::write(root.join(".fallowrc.json"), "{}").expect("full discovery scope");
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--unused-exports",
        "--unused-types",
        "--unused-class-members",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert_ne!(output.code, 2, "{}", output.stderr);
    assert_findings(&parse_json(&output), false);
}

fn assert_findings(json: &serde_json::Value, excluded: bool) {
    for (category, key, name) in [
        ("unused_exports", "export_name", "testOnly"),
        ("unused_types", "export_name", "TestOnly"),
        ("unused_class_members", "member_name", "testOnly"),
    ] {
        let findings = json[category].as_array().expect("finding category");
        let finding = findings.iter().find(|finding| finding[key] == name);
        assert_eq!(
            finding.is_some(),
            excluded,
            "selected discovery scope must control {category} {name}: {json}"
        );
        if let Some(finding) = finding {
            assert_eq!(
                finding["semantic"]["decision"], "retained-abstained",
                "{finding}"
            );
            assert_eq!(
                finding["semantic"]["evidence"][0]["path"],
                "src/library.test.ts"
            );
        }
    }
    assert!(
        json["unused_exports"]
            .as_array()
            .expect("export findings")
            .iter()
            .any(|finding| finding["export_name"] == "unused"),
        "the detector must retain an actually unused control: {json}"
    );
}
