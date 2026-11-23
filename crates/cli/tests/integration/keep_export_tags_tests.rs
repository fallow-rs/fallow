#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::Path;

use crate::common::{copy_fixture, parse_json, run_fallow_in_root};

const FIXTURE: &str = "keep-export-tags";

fn write_keep_export_tags(root: &Path, tags: &str) {
    std::fs::write(
        root.join(".fallowrc.json"),
        format!(r#"{{ "keepExportTags": {tags} }}"#),
    )
    .expect("write config");
}

fn unused_export_names(root: &Path, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["--unused-exports", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let json = parse_json(&run_fallow_in_root("dead-code", root, &args));
    let mut names: Vec<String> = json["unused_exports"]
        .as_array()
        .expect("unused_exports array")
        .iter()
        .map(|finding| finding["export_name"].as_str().unwrap().to_owned())
        .collect();
    names.sort();
    names
}

fn trace(root: &Path, target: &str) -> serde_json::Value {
    parse_json(&run_fallow_in_root(
        "dead-code",
        root,
        &[
            "--trace",
            target,
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ))
}

#[test]
fn default_keeps_every_visibility_tag() {
    let dir = copy_fixture(FIXTURE);
    assert_eq!(
        unused_export_names(dir.path(), &["--no-cache"]),
        ["plainUnused"]
    );
}

#[test]
fn public_only_reports_internal_alpha_and_beta_exports() {
    let dir = copy_fixture(FIXTURE);
    write_keep_export_tags(dir.path(), r#"["public"]"#);
    assert_eq!(
        unused_export_names(dir.path(), &["--no-cache"]),
        [
            "alphaUnused",
            "betaUnused",
            "forwarded",
            "forwarded",
            "internalUnused",
            "plainUnused",
        ],
        "the dropped @internal tag on the re-export also stops its credit to the source"
    );
}

#[test]
fn empty_list_also_reports_public_exports() {
    let dir = copy_fixture(FIXTURE);
    write_keep_export_tags(dir.path(), "[]");
    let names = unused_export_names(dir.path(), &["--no-cache"]);
    assert!(names.contains(&"publicUnused".to_owned()), "{names:?}");
    assert!(!names.contains(&"expectedUnused".to_owned()), "{names:?}");
}

#[test]
fn a_changed_setting_misses_the_graph_cache() {
    let dir = copy_fixture(FIXTURE);
    assert_eq!(unused_export_names(dir.path(), &[]), ["plainUnused"]);
    write_keep_export_tags(dir.path(), r#"["public"]"#);
    assert!(
        unused_export_names(dir.path(), &[]).contains(&"internalUnused".to_owned()),
        "a cached graph from the default setting must not hide the finding"
    );
}

#[test]
fn trace_names_the_tag_that_keeps_the_export() {
    let dir = copy_fixture(FIXTURE);
    let json = trace(dir.path(), "src/a.ts:internalUnused");
    assert_eq!(json["is_used"], false);
    assert_eq!(json["kept_by_tag"], "internal");
    assert_eq!(
        json["reason"],
        "No references found, but the @internal tag keeps the export (keepExportTags)"
    );
}

#[test]
fn trace_agrees_with_the_report_when_the_tag_is_not_kept() {
    let dir = copy_fixture(FIXTURE);
    write_keep_export_tags(dir.path(), r#"["public"]"#);
    let json = trace(dir.path(), "src/a.ts:internalUnused");
    assert!(json.get("kept_by_tag").is_none(), "{json}");
    assert_eq!(json["reason"], "No references found, export is unused");

    let public = trace(dir.path(), "src/a.ts:publicUnused");
    assert_eq!(public["kept_by_tag"], "public");
}

#[test]
fn invalid_entry_is_a_config_error() {
    let dir = copy_fixture(FIXTURE);
    write_keep_export_tags(dir.path(), r#"["-internal"]"#);
    let output = run_fallow_in_root("dead-code", dir.path(), &["--quiet", "--no-cache"]);
    assert_eq!(output.code, 2, "{}", output.stderr);
    assert!(
        output.stderr.contains("without a `+` or `-` prefix"),
        "{}",
        output.stderr
    );
}

#[test]
fn trace_names_no_tag_when_the_file_decides() {
    let dir = copy_fixture(FIXTURE);
    let entry = trace(dir.path(), "src/index.ts:entryInternal");
    assert!(entry.get("kept_by_tag").is_none(), "{entry}");
    assert_eq!(entry["is_entry_point"], true);

    let orphan = trace(dir.path(), "src/orphan.ts:orphanInternal");
    assert!(orphan.get("kept_by_tag").is_none(), "{orphan}");
    assert_eq!(orphan["reason"], "File is unreachable from any entry point");
}

#[test]
fn constant_parameter_follows_the_kept_tags() {
    let dir = copy_fixture("constant-parameter");
    let format = dir.path().join("src/format.ts");
    let source = std::fs::read_to_string(&format).expect("read format.ts");
    std::fs::write(&format, format!("/** @internal */\n{source}")).expect("write format.ts");
    let count = |root: &Path| {
        let json = parse_json(&run_fallow_in_root(
            "dead-code",
            root,
            &[
                "--constant-parameters",
                "--format",
                "json",
                "--quiet",
                "--no-cache",
            ],
        ));
        json["constant_parameters"].as_array().map_or(0, Vec::len)
    };
    assert_eq!(count(dir.path()), 0, "a kept tag marks public API");
    write_keep_export_tags(dir.path(), r#"["public"]"#);
    assert_eq!(count(dir.path()), 2, "a dropped tag has no effect");
}
