use std::collections::BTreeSet;
use std::path::Path;

use fallow_config::RulesConfig;
use fallow_types::results::AnalysisResults;

use super::*;

fn test_rule_builder(id: &str, description: &str, level: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "shortDescription": { "text": description },
        "defaultConfiguration": { "level": level }
    })
}

/// A SARIF consumer reads the message text, not the JSON envelope, so the
/// degraded-parse caveat has to travel in the message. A clean finding
/// keeps the previous text byte-for-byte.
#[test]
fn sarif_messages_name_the_degraded_parse_caveat() {
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: Path::new("/p/src/clean.ts").to_path_buf(),
        }));
    let mut flagged = UnusedFileFinding::with_actions(UnusedFile {
        path: Path::new("/p/src/orphan.ts").to_path_buf(),
    });
    flagged.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_files.push(flagged);

    // Every member array carries the same caveat off the same
    // reachability-free access walk, so all three have to reach the
    // message. Store members were rendered bare while the array itself was
    // stamped.
    let member = |parent: &str, name: &str, kind| UnusedMember {
        path: Path::new("/p/src/lib.ts").to_path_buf(),
        parent_name: parent.to_owned(),
        member_name: name.to_owned(),
        kind,
        line: 7,
        col: 2,
    };
    let mut enum_member = UnusedEnumMemberFinding::with_actions(member(
        "Mode",
        "Legacy",
        fallow_types::extract::MemberKind::EnumMember,
    ));
    enum_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_enum_members.push(enum_member);
    let mut class_member = UnusedClassMemberFinding::with_actions(member(
        "Widget",
        "render",
        fallow_types::extract::MemberKind::ClassMethod,
    ));
    class_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_class_members.push(class_member);
    let mut store_member = UnusedStoreMemberFinding::with_actions(member(
        "useCart",
        "subtotal",
        fallow_types::extract::MemberKind::StoreMember,
    ));
    store_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_store_members.push(store_member);

    let sarif = build_dead_code_sarif(
        &results,
        Path::new("/p"),
        &RulesConfig::default(),
        &test_rule_builder,
    );
    let messages: Vec<String> = sarif
        .pointer("/runs/0/results")
        .and_then(serde_json::Value::as_array)
        .expect("SARIF results")
        .iter()
        .filter_map(|entry| {
            entry
                .pointer("/message/text")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();

    assert!(
        messages.contains(&"File is not reachable from any entry point".to_owned()),
        "a clean finding keeps its exact message: {messages:?}"
    );
    assert!(
        messages.contains(
            &"File is not reachable from any entry point (caveat: incomplete import graph)"
                .to_owned()
        ),
        "a caveated finding names it in the message: {messages:?}"
    );
    for expected in [
        "Enum member 'Mode.Legacy' is never referenced (caveat: incomplete import graph)",
        "Class member 'Widget.render' is never referenced (caveat: incomplete import graph)",
        "Store member 'useCart.subtotal' is never referenced (caveat: incomplete import graph)",
    ] {
        assert!(
            messages.contains(&expected.to_owned()),
            "every member kind names the caveat: {messages:?}"
        );
    }
}

#[test]
fn root_dependency_message_names_the_workspaces_that_declare_and_import_it() {
    let root = Path::new("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "left-pad".to_owned(),
            location: fallow_types::results::DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 0,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: vec![root.join("packages/app")],
        }));

    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);
    let message = sarif
        .pointer("/runs/0/results/0/message/text")
        .and_then(serde_json::Value::as_str)
        .expect("SARIF message");
    assert_eq!(
        message,
        "Package 'left-pad' is in dependencies but never imported; declared and imported in other workspaces: packages/app"
    );
}

/// The reported defect: `npm init -y` writes a `package.json` whose
/// dependency block can sit on one line, and `find_dep_line_in_json` also
/// falls back to line 1 for a key it cannot locate. Every unused dependency
/// then reported the same rule id, the same URI, and the same source
/// snippet, which is the whole of a SARIF fingerprint, so GitHub code
/// scanning showed one alert for all of them. CodeClimate never had the
/// defect: it keys on the package name.
#[test]
fn two_dependencies_on_one_manifest_line_are_two_alerts() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"compact","dependencies":{"lodash":"^4.17.21","chalk":"^5.3.0"}}"#,
    )
    .expect("write manifest");

    let mut results = AnalysisResults::default();
    for name in ["lodash", "chalk"] {
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: name.to_owned(),
                location: fallow_types::results::DependencyLocation::Dependencies,
                path: root.join("package.json"),
                line: 1,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }));
    }

    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);
    let entries = sarif
        .pointer("/runs/0/results")
        .and_then(serde_json::Value::as_array)
        .expect("SARIF results");

    let fingerprints = entries
        .iter()
        .map(|entry| {
            entry
                .pointer("/partialFingerprints/tools.fallow.fingerprint~1v1")
                .and_then(serde_json::Value::as_str)
                .expect("fingerprint")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        fingerprints.len(),
        entries.len(),
        "two dependencies declared on one line are two alerts: {entries:#?}"
    );

    let columns = entries
        .iter()
        .map(|entry| {
            entry
                .pointer("/locations/0/physicalLocation/region/startColumn")
                .and_then(serde_json::Value::as_u64)
                .expect("start column")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        columns.len(),
        entries.len(),
        "each dependency points at its own key in the manifest line: {entries:#?}"
    );
}

/// Why the column and not only the run-level uniqueness pass: that pass
/// separates repeats by occurrence index, so a dependency's identity would
/// depend on its siblings and fixing the first one would renumber, and
/// close, the alert on the second. The column is the dependency's own
/// position, so an unrelated dependency added above it moves neither.
#[test]
fn a_dependency_added_above_leaves_the_others_alert_alone() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let manifest = root.join("package.json");

    let chalk_fingerprint = |manifest_text: &str, chalk_line: u32| {
        std::fs::write(&manifest, manifest_text).expect("write manifest");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "chalk".to_owned(),
                location: fallow_types::results::DependencyLocation::Dependencies,
                path: manifest.clone(),
                line: chalk_line,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }));
        build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder)
            .pointer("/runs/0/results/0/partialFingerprints/tools.fallow.fingerprint~1v1")
            .and_then(serde_json::Value::as_str)
            .expect("chalk fingerprint")
            .to_owned()
    };

    let before = "{\n  \"dependencies\": {\n    \"chalk\": \"^5.3.0\"\n  }\n}\n";
    let after = "{\n  \"dependencies\": {\n    \"lodash\": \"^4.17.21\",\n    \"chalk\": \"^5.3.0\"\n  }\n}\n";

    assert_eq!(
        chalk_fingerprint(before, 3),
        chalk_fingerprint(after, 4),
        "a dependency declared above it must not move chalk's alert"
    );
}

/// `partialFingerprints` is the alert identity GitHub code scanning uses to
/// carry a finding across runs. It is built from rule id plus location (or a
/// normalized snippet), never from the message, so a finding that gains the
/// caveat must keep its fingerprint. If the caveat ever reached the
/// fingerprint inputs, every open alert on a degraded repository would close
/// and reopen as new on the next scan.
#[test]
fn the_caveat_does_not_move_the_sarif_fingerprint() {
    let build = |caveated: bool| {
        let mut results = AnalysisResults::default();
        let mut finding = UnusedFileFinding::with_actions(UnusedFile {
            path: Path::new("/p/src/orphan.ts").to_path_buf(),
        });
        if caveated {
            finding.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        }
        results.unused_files.push(finding);
        build_dead_code_sarif(
            &results,
            Path::new("/p"),
            &RulesConfig::default(),
            &test_rule_builder,
        )
    };

    let read = |sarif: &serde_json::Value, pointer: &str| {
        sarif
            .pointer("/runs/0/results")
            .and_then(serde_json::Value::as_array)
            .expect("SARIF results")
            .iter()
            .map(|entry| {
                entry
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .expect("SARIF field")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };

    let clean = build(false);
    let caveated = build(true);

    for key in [
        "/partialFingerprints/tools.fallow.fingerprint~1v1",
        "/partialFingerprints/primaryLocationLineHash~1v1",
    ] {
        assert_eq!(
            read(&clean, key),
            read(&caveated, key),
            "the caveat must not move {key}"
        );
    }

    assert_ne!(
        read(&clean, "/message/text"),
        read(&caveated, "/message/text"),
        "the guard is only meaningful while the message actually changed"
    );
}

#[test]
fn sarif_rule_list_is_backed_by_issue_contracts() {
    let sarif = build_dead_code_sarif(
        &AnalysisResults::default(),
        Path::new("."),
        &RulesConfig::default(),
        &test_rule_builder,
    );
    let Some(rules) = sarif
        .pointer("/runs/0/tool/driver/rules")
        .and_then(serde_json::Value::as_array)
    else {
        panic!("SARIF document should contain driver rules");
    };

    let actual_ids = rules
        .iter()
        .filter_map(|rule| {
            rule.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>();
    let expected_ids = issue_output_contracts()
        .flat_map(|contract| contract.sarif_rule_ids)
        .collect::<BTreeSet<_>>();

    assert_eq!(actual_ids, expected_ids);

    for rule in rules {
        let id = rule
            .get("id")
            .and_then(serde_json::Value::as_str)
            .expect("SARIF rule should have id");
        let description = rule
            .pointer("/shortDescription/text")
            .and_then(serde_json::Value::as_str)
            .expect("SARIF rule should have short description");
        assert_eq!(
            description,
            issue_sarif_rule_description(id).expect("SARIF rule description should resolve")
        );
    }
}

const FINDING_ID_POINTER: &str = "/partialFingerprints/fallowFinding~1v1";

/// One finding per single-result kind, plus the two kinds that fan out to
/// one SARIF result per location.
fn identity_results(root: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/orphan.ts"),
        }));
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_owned(),
            location: fallow_types::results::DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            fallow_types::results::UnlistedDependency {
                package_name: "chalk".to_owned(),
                imported_from: vec![
                    fallow_types::results::ImportSite {
                        path: root.join("src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    fallow_types::results::ImportSite {
                        path: root.join("src/b.ts"),
                        line: 2,
                        col: 0,
                    },
                ],
            },
        ));
    results
        .duplicate_exports
        .push(DuplicateExportFinding::with_actions(
            fallow_types::results::DuplicateExport {
                export_name: "Button".to_owned(),
                locations: vec![
                    fallow_types::results::DuplicateLocation {
                        path: root.join("src/a.ts"),
                        line: 4,
                        col: 0,
                    },
                    fallow_types::results::DuplicateLocation {
                        path: root.join("src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            },
        ));
    results
}

fn sarif_entries(sarif: &serde_json::Value) -> Vec<serde_json::Value> {
    sarif
        .pointer("/runs/0/results")
        .and_then(serde_json::Value::as_array)
        .expect("SARIF results")
        .clone()
}

/// A finding that maps to one SARIF result carries its `finding_id` under
/// `fallowFinding/v1`, next to the location-based keys.
#[test]
fn a_single_result_finding_carries_its_id_as_a_partial_fingerprint() {
    let root = Path::new("/p");
    let mut results = identity_results(root);
    fallow_types::identity::stamp_dead_code_finding_ids(&mut results, root);
    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);
    let entries = sarif_entries(&sarif);

    let id_of_rule = |rule: &str| {
        entries
            .iter()
            .find(|entry| entry["ruleId"] == rule)
            .and_then(|entry| entry.pointer(FINDING_ID_POINTER))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(
        id_of_rule("fallow/unused-file").as_deref(),
        results.unused_files[0].finding_id.as_deref(),
    );
    assert_eq!(
        id_of_rule("fallow/unused-dependency").as_deref(),
        results.unused_dependencies[0].finding_id.as_deref(),
    );
    assert!(
        id_of_rule("fallow/unused-file").is_some_and(|id| id.starts_with("dc1:unused-file:")),
        "the key holds the finding id: {entries:#?}"
    );
}

/// A partial fingerprint must identify one result. An unlisted dependency
/// and a duplicate export give one result per location, so one id would
/// name several results. These results do not carry the key.
#[test]
fn a_fanned_out_finding_does_not_carry_the_id() {
    let root = Path::new("/p");
    let mut results = identity_results(root);
    fallow_types::identity::stamp_dead_code_finding_ids(&mut results, root);
    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);

    for entry in sarif_entries(&sarif) {
        let rule = entry["ruleId"].as_str().expect("rule id");
        if matches!(
            rule,
            "fallow/unlisted-dependency" | "fallow/duplicate-export"
        ) {
            assert!(
                entry.pointer(FINDING_ID_POINTER).is_none(),
                "{rule} fans out and must not carry the id: {entry:#?}"
            );
        }
    }
}

/// GitHub code scanning matches alerts on the location-based keys. The new
/// key must not change any other byte of the document, or open alerts
/// close and reopen on the next upload.
#[test]
fn the_finding_id_key_leaves_every_other_sarif_byte_alone() {
    let root = Path::new("/p");
    let without_ids = identity_results(root);
    let mut with_ids = without_ids.clone();
    fallow_types::identity::stamp_dead_code_finding_ids(&mut with_ids, root);

    let before = build_dead_code_sarif(
        &without_ids,
        root,
        &RulesConfig::default(),
        &test_rule_builder,
    );
    let mut after =
        build_dead_code_sarif(&with_ids, root, &RulesConfig::default(), &test_rule_builder);

    let mut removed = 0;
    for entry in after
        .pointer_mut("/runs/0/results")
        .and_then(serde_json::Value::as_array_mut)
        .expect("SARIF results")
    {
        let prints = entry["partialFingerprints"]
            .as_object_mut()
            .expect("partial fingerprints");
        if prints.remove("fallowFinding/v1").is_some() {
            removed += 1;
        }
    }
    assert!(
        removed > 0,
        "the guard needs at least one result with the key"
    );
    assert_eq!(
        serde_json::to_string(&before).expect("serialize"),
        serde_json::to_string(&after).expect("serialize"),
    );
}

/// A saved envelope from an older version has no ids. The key is then
/// absent, not empty and not null.
#[test]
fn a_finding_without_an_id_has_no_finding_id_key() {
    let root = Path::new("/p");
    let sarif = build_dead_code_sarif(
        &identity_results(root),
        root,
        &RulesConfig::default(),
        &test_rule_builder,
    );
    for entry in sarif_entries(&sarif) {
        assert!(
            entry["partialFingerprints"]
                .get("fallowFinding/v1")
                .is_none(),
            "no id, no key: {entry:#?}"
        );
    }
}

/// SARIF reads `startColumn` in UTF-16 code units when the run sets no
/// `columnKind` (SARIF 2.1.0, section 3.14.17). The prefix `/* é 😀 */ ` is
/// 14 bytes but 11 UTF-16 code units, so `unused` starts at column 25 and not
/// at the byte column 28. See issue #4405.
#[test]
fn sarif_start_column_counts_utf16_code_units() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let source_line = "/* é 😀 */ export const unused = 1;";
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/a.ts"), format!("{source_line}\n")).expect("write source");
    let byte_col = u32::try_from(source_line.find("unused").expect("name")).expect("small");

    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/a.ts"),
            export_name: "unused".to_owned(),
            is_type_only: false,
            line: 1,
            col: byte_col,
            span_start: byte_col,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));

    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);

    let region = sarif
        .pointer("/runs/0/results/0/locations/0/physicalLocation/region")
        .expect("region");
    assert_eq!(region["startLine"], 1);
    assert_eq!(region["startColumn"], 25, "{region:#?}");
    assert!(
        sarif.pointer("/runs/0/columnKind").is_none(),
        "the run keeps the SARIF default column kind"
    );
}

/// Fallow columns on line 1 start after a byte order mark, so the mark is
/// not counted in the SARIF column either.
#[test]
fn sarif_start_column_skips_the_byte_order_mark() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(root.join("src/a.ts"), "\u{feff}export const unused = 1;\n")
        .expect("write source");

    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/a.ts"),
            export_name: "unused".to_owned(),
            is_type_only: false,
            line: 1,
            col: 13,
            span_start: 16,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));

    let sarif = build_dead_code_sarif(&results, root, &RulesConfig::default(), &test_rule_builder);

    assert_eq!(
        sarif.pointer("/runs/0/results/0/locations/0/physicalLocation/region/startColumn"),
        Some(&serde_json::json!(14)),
    );
}
