//! The syntactic pass names a member of a default-exported class after the
//! export (`default.update`). The type-aware pass must still resolve that
//! member to its exact class declaration (issue #4792).

#![expect(clippy::expect_used, reason = "tests use expect for fixture setup")]

use crate::common::{fixture_path, parse_json, run_fallow_raw_with_type_aware_sidecar};

const RENDERER_FILES: [&str; 3] = [
    "src/anonymous-renderer.ts",
    "src/assigned-renderer.ts",
    "src/declared-renderer.ts",
];

#[test]
fn type_aware_resolves_members_of_default_exported_classes() {
    let root = fixture_path("type-aware-default-export-class-members");
    let root_arg = root.to_string_lossy();
    let output = run_fallow_raw_with_type_aware_sidecar(&[
        "dead-code",
        "--root",
        &root_arg,
        "--type-aware",
        "--unused-class-members",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]);
    assert_ne!(
        output.code, 2,
        "stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    let json = parse_json(&output);
    let type_aware = &json["_meta"]["type_aware"];
    assert_eq!(type_aware["executed"], true, "{type_aware}");
    assert_eq!(
        type_aware["abstention_reasons"]["unknown_symbol"], 0,
        "{type_aware}"
    );

    let members: Vec<(String, String, String)> = json["unused_class_members"]
        .as_array()
        .expect("unused class members")
        .iter()
        .map(|member| {
            (
                member["path"]
                    .as_str()
                    .expect("member path")
                    .replace('\\', "/"),
                member["parent_name"].as_str().expect("parent").to_owned(),
                member["member_name"].as_str().expect("member").to_owned(),
            )
        })
        .collect();
    let mut expected: Vec<(String, String, String)> = RENDERER_FILES
        .iter()
        .map(|path| {
            (
                (*path).to_owned(),
                "default".to_owned(),
                "neverCalled".to_owned(),
            )
        })
        .collect();
    let mut actual = members;
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected, "{type_aware}");

    let decisions = type_aware["candidate_decisions"]
        .as_array()
        .expect("candidate decisions");
    for decision in decisions {
        let expected_decision = if decision["subject"]["exported_name"] == "update" {
            "confirmed-used"
        } else {
            "confirmed-no-static-references"
        };
        assert_eq!(decision["decision"], expected_decision, "{decision}");
        assert_eq!(decision["subject"]["owner"], "default", "{decision}");
    }
}
