#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

#[path = "common/mod.rs"]
mod common;

use common::{fixture_path, parse_json, run_fallow, run_fallow_in_root};

#[test]
fn fix_dry_run_json_lists_fixes_without_applying() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "fix --dry-run should exit 0, stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dry_run"].as_bool(),
        Some(true),
        "dry_run should be true"
    );
    let fixes = json["fixes"].as_array().unwrap();
    assert!(!fixes.is_empty(), "basic-project should have fixable items");

    for fix in fixes {
        assert!(fix.get("type").is_some(), "fix should have 'type'");
        let has_path = fix.get("path").is_some() || fix.get("package").is_some();
        assert!(has_path, "fix should have 'path' or 'package'");
        assert!(
            fix.get("applied").is_none(),
            "dry-run fixes should not have 'applied' key"
        );
    }
}

#[test]
fn fix_removes_unused_exported_enum_declaration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"enum-fix","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "import './enum';\n").unwrap();
    std::fs::write(
        root.join("src/enum.ts"),
        "export enum MyEnum {\n  A,\n  B,\n}\n",
    )
    .unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--quiet"]);

    assert_eq!(
        output.code, 0,
        "fix should exit 0, stdout: {}, stderr: {}",
        output.stdout, output.stderr
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/enum.ts")).unwrap(),
        "\n"
    );

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(json["fixes"].as_array().unwrap().is_empty());
}

#[test]
fn fix_folds_imported_enum_with_all_members_unused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"enum-fold","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { MyEnum } from './enum';\nconsole.log(typeof MyEnum);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/enum.ts"),
        "export enum MyEnum {\n  A,\n  B,\n}\n",
    )
    .unwrap();

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();
    assert_eq!(
        fixes.len(),
        1,
        "fold should collapse the per-member fixes into a single remove_export entry"
    );
    assert_eq!(fixes[0]["type"], "remove_export");
    assert_eq!(fixes[0]["name"], "MyEnum");

    let output = run_fallow_in_root("fix", root, &["--yes", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix should exit 0, stdout: {}, stderr: {}",
        output.stdout, output.stderr
    );

    let after = std::fs::read_to_string(root.join("src/enum.ts")).unwrap();
    assert_eq!(
        after, "\n",
        "enum.ts should be empty after the fold (single trailing newline)"
    );

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    let json = parse_json(&output);
    assert!(
        json["fixes"].as_array().unwrap().is_empty(),
        "second pass should find nothing more to fix"
    );
}

#[test]
fn fix_adds_ignore_exports_config_rules_for_duplicate_exports() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/one")).unwrap();
    std::fs::create_dir_all(root.join("src/two")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"dup-config","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(root.join(".fallowrc.json"), "{}\n").unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export { Button } from './one';\nexport { Button as Button2 } from './two';\nconsole.log(Button2);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/one/index.ts"), "export const Button = 1;\n").unwrap();
    std::fs::write(root.join("src/two/index.ts"), "export const Button = 2;\n").unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix should exit 0, stdout: {}, stderr: {}",
        output.stdout, output.stderr
    );

    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();
    let config_fix = fixes
        .iter()
        .find(|fix| fix["type"] == "add_ignore_exports")
        .expect("fix output should include an ignoreExports config edit");
    assert_eq!(config_fix["applied"], true);
    assert_eq!(config_fix["config_key"], "ignoreExports");
    assert_eq!(config_fix["entries"].as_array().unwrap().len(), 2);

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".fallowrc.json")).unwrap())
            .unwrap();
    let ignore_exports = config["ignoreExports"].as_array().unwrap();
    assert_eq!(ignore_exports[0]["file"], "src/one/index.ts");
    assert_eq!(ignore_exports[1]["file"], "src/two/index.ts");

    let output = run_fallow_in_root(
        "dead-code",
        root,
        &["--duplicate-exports", "--format", "json", "--quiet"],
    );
    assert_eq!(
        output.code, 0,
        "post-fix check should pass: {}",
        output.stderr
    );
    let json = parse_json(&output);
    assert_eq!(json["summary"]["duplicate_exports"].as_u64(), Some(0));
}

/// A Windows-authored `.fallowrc.json` with a UTF-8 BOM must round-trip
/// through `fallow fix --yes` without breaking the parse on the next run.
/// The CST parser (jsonc-parser) rejects a leading BOM, so the writer must
/// strip and restore it.
#[test]
fn fix_round_trips_utf8_bom_on_json_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/one")).unwrap();
    std::fs::create_dir_all(root.join("src/two")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"bom-config","main":"src/index.ts"}"#,
    )
    .unwrap();
    let bom_input = "\u{FEFF}{\n  \"entry\": [\"src/index.ts\"]\n}\n";
    std::fs::write(root.join(".fallowrc.json"), bom_input).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export { Button } from './one';\nexport { Button as Button2 } from './two';\nconsole.log(Button2);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/one/index.ts"), "export const Button = 1;\n").unwrap();
    std::fs::write(root.join("src/two/index.ts"), "export const Button = 2;\n").unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix should succeed on BOM-prefixed config: {}",
        output.stderr
    );

    let written = std::fs::read_to_string(root.join(".fallowrc.json")).unwrap();
    assert!(
        written.starts_with('\u{FEFF}'),
        "BOM stripped from output (got bytes {:?})",
        &written.as_bytes()[..written.len().min(8)]
    );

    let post = run_fallow_in_root(
        "dead-code",
        root,
        &["--duplicate-exports", "--format", "json", "--quiet"],
    );
    assert_eq!(
        post.code, 0,
        "post-fix analysis must succeed on BOM-preserved config: {}",
        post.stderr
    );
    let json = parse_json(&post);
    assert_eq!(json["summary"]["duplicate_exports"].as_u64(), Some(0));
}

/// `fallow fix` on a symlinked config file must write through to the target
/// rather than replacing the symlink with a regular file. Common in Docker
/// images where configs are mounted from a sibling directory.
#[cfg(unix)]
#[test]
fn fix_writes_through_symlinked_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let real_dir = root.join("config-source");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::create_dir_all(root.join("src/one")).unwrap();
    std::fs::create_dir_all(root.join("src/two")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"symlink-config","main":"src/index.ts"}"#,
    )
    .unwrap();
    let real_path = real_dir.join(".fallowrc.json");
    std::fs::write(&real_path, "{}\n").unwrap();
    std::os::unix::fs::symlink(&real_path, root.join(".fallowrc.json")).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "export { Button } from './one';\nexport { Button as Button2 } from './two';\nconsole.log(Button2);\n",
    )
    .unwrap();
    std::fs::write(root.join("src/one/index.ts"), "export const Button = 1;\n").unwrap();
    std::fs::write(root.join("src/two/index.ts"), "export const Button = 2;\n").unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix on symlinked config should succeed: {}",
        output.stderr
    );

    let meta = std::fs::symlink_metadata(root.join(".fallowrc.json")).unwrap();
    assert!(
        meta.file_type().is_symlink(),
        "symlink was replaced with regular file by atomic_write"
    );

    let target_content = std::fs::read_to_string(&real_path).unwrap();
    assert!(
        target_content.contains("\"ignoreExports\""),
        "symlink target was not updated, got: {target_content}"
    );
}

#[test]
fn fix_without_yes_in_non_tty_exits_2() {
    let output = run_fallow("fix", "basic-project", &["--format", "json", "--quiet"]);
    assert_eq!(output.code, 2, "fix without --yes in non-TTY should exit 2");
}

#[test]
fn fix_catalog_delete_preceding_comments_config_is_consumed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("packages/app")).unwrap();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "fix": {
    "catalog": {
      "deletePrecedingComments": "always"
    }
  }
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalog:\n  is-odd: ^1.0.0\n  # pinned for issue #360\n  is-even: ^1.0.0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{
  "name": "app",
  "version": "0.0.0",
  "dependencies": {
    "is-odd": "catalog:"
  }
}
"#,
    )
    .unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix should exit 0, stdout: {}, stderr: {}",
        output.stdout, output.stderr
    );

    let after = std::fs::read_to_string(root.join("pnpm-workspace.yaml")).unwrap();
    assert_eq!(
        after,
        "packages:\n  - 'packages/*'\n\ncatalog:\n  is-odd: ^1.0.0\n"
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();
    let catalog_fix = fixes
        .iter()
        .find(|fix| fix["type"] == "remove_catalog_entry")
        .expect("fix output should include the catalog entry removal");
    assert_eq!(catalog_fix["line"], 6, "line tracks the deletion start");
    assert_eq!(
        catalog_fix["entry_line"], 7,
        "entry_line tracks the original catalog entry position"
    );
    assert_eq!(catalog_fix["removed_lines"], 2);
}

#[test]
fn fix_catalog_fallow_keep_marker_preserves_block() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("packages/app")).unwrap();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "fix": {
    "catalog": {
      "deletePrecedingComments": "always"
    }
  }
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n\ncatalog:\n  is-odd: ^1.0.0\n  # fallow-keep audit trail\n  is-even: ^1.0.0\n",
    )
    .unwrap();
    std::fs::write(
        root.join("packages/app/package.json"),
        r#"{
  "name": "app",
  "version": "0.0.0",
  "dependencies": {
    "is-odd": "catalog:"
  }
}
"#,
    )
    .unwrap();

    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix should exit 0, stderr: {}",
        output.stderr
    );

    let after = std::fs::read_to_string(root.join("pnpm-workspace.yaml")).unwrap();
    assert_eq!(
        after,
        "packages:\n  - 'packages/*'\n\ncatalog:\n  is-odd: ^1.0.0\n  # fallow-keep audit trail\n",
        "fallow-keep marker must preserve the comment even when the entry is removed under `always`"
    );
}

/// End-to-end regression for the issue #335 fix: running `fallow fix --yes`
/// against the canonical `issue-329-pnpm-catalog` fixture must produce a
/// `pnpm-workspace.yaml` whose emptied named catalog (`react17`, whose
/// only entries `react` and `react-dom` are unused) parses as an EMPTY
/// MAPPING, not as `null`. Bare `react17:` in YAML is null; pnpm rejects
/// null-valued catalogs with `Cannot convert undefined or null to object`
/// at install time.
///
/// This is the integration test the original implementation lacked. The
/// unit tests asserted on synthetic strings, which is the right shape for
/// helper coverage but does not exercise the end-to-end flow through the
/// binary against a real fixture. A parallel reviewer caught the bug by
/// running `fallow fix` against this exact fixture and inspecting the
/// resulting YAML; this test bakes that workflow into the suite.
#[test]
fn fix_catalog_issue_335_empties_parent_to_empty_map_not_null() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let fixture = fixture_path("issue-329-pnpm-catalog");
    copy_dir_recursive(&fixture, &root).expect("copy fixture");

    let output = run_fallow_in_root("fix", &root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "fix --yes should exit 0, stderr: {}",
        output.stderr
    );

    let workspace_path = root.join("pnpm-workspace.yaml");
    let after = std::fs::read_to_string(&workspace_path).expect("read workspace file");

    let parsed: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&after).expect("post-fix YAML must parse");
    let react17 = parsed
        .get("catalogs")
        .and_then(|c| c.get("react17"))
        .unwrap_or_else(|| panic!("post-fix YAML missing catalogs.react17:\n{after}"));
    assert!(
        react17
            .as_mapping()
            .is_some_and(serde_yaml_ng::Mapping::is_empty),
        "catalogs.react17 must be an empty mapping `{{}}`, not null. \
         Got value: {react17:?}\nFile content:\n{after}"
    );

    let legacy = parsed
        .get("catalogs")
        .and_then(|c| c.get("legacy"))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .expect("catalogs.legacy must remain a mapping");
    assert!(
        legacy.contains_key(serde_yaml_ng::Value::String("is-odd".to_string())),
        "catalogs.legacy must still declare `is-odd`. Got: {legacy:?}"
    );

    let default_catalog = parsed
        .get("catalog")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .expect("catalog: must remain a mapping");
    assert!(
        default_catalog.contains_key(serde_yaml_ng::Value::String("react".to_string())),
        "default catalog must still declare `react` (it has consumers). Got: {default_catalog:?}"
    );

    let json = parse_json(&output);
    assert_eq!(
        json["skipped"].as_u64(),
        Some(1),
        "fixture has one hardcoded-pkg skip; envelope must report skipped: 1, got: {}",
        json["skipped"]
    );
}

#[test]
fn fix_json_envelope_carries_skipped_content_changed_count() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert!(
        json.get("skipped_content_changed").is_some(),
        "fix envelope must include `skipped_content_changed` field: {}",
        output.stdout,
    );
    assert_eq!(
        json["skipped_content_changed"].as_u64(),
        Some(0),
        "no files should be skipped on a clean dry-run",
    );
}

#[test]
fn fix_round_trip_clears_targeted_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"round-trip","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { kept } from './utils';\nconsole.log(kept);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/utils.ts"),
        "export const kept = 1;\nexport const stale = 2;\nexport const orphan = 3;\n",
    )
    .unwrap();

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        fix.code, 0,
        "fix should exit 0 on a clean run; stderr: {}",
        fix.stderr
    );
    let fix_json = parse_json(&fix);
    let total_fixed = fix_json["total_fixed"].as_u64().unwrap_or(0);
    assert!(total_fixed >= 2, "fix should remove both stale exports");

    let check = run_fallow_in_root("check", root, &["--format", "json", "--quiet"]);
    let check_json = parse_json(&check);
    let unused_exports = check_json["unused_exports"].as_array().map_or(0, Vec::len);
    assert_eq!(
        unused_exports, 0,
        "fixed exports must not reappear; check output: {}",
        check.stdout
    );
}

#[cfg(unix)]
#[test]
fn fix_batch_aborts_when_a_target_directory_is_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/sealed")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"batch-atomic","main":"src/index.ts"}"#,
    )
    .unwrap();
    let entry = "import { kept } from './open/utils';\nimport { also } from './sealed/locked';\n\
                 console.log(kept, also);\n";
    std::fs::create_dir_all(root.join("src/open")).unwrap();
    std::fs::write(root.join("src/index.ts"), entry).unwrap();
    let open_original = "export const kept = 1;\nexport const stale = 2;\n";
    std::fs::write(root.join("src/open/utils.ts"), open_original).unwrap();
    let sealed_original = "export const also = 1;\nexport const sealed_stale = 2;\n";
    std::fs::write(root.join("src/sealed/locked.ts"), sealed_original).unwrap();

    let sealed_dir = root.join("src/sealed");
    let mut perms = std::fs::metadata(&sealed_dir).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&sealed_dir, perms).unwrap();

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);

    let mut restore = std::fs::metadata(&sealed_dir).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(&sealed_dir, restore).unwrap();

    assert_eq!(
        fix.code, 2,
        "batch commit failure must surface as exit 2; stdout: {} stderr: {}",
        fix.stdout, fix.stderr,
    );
    let post_open = std::fs::read_to_string(root.join("src/open/utils.ts")).unwrap();
    assert_eq!(
        post_open, open_original,
        "healthy file must be untouched when a sibling file's stage failed",
    );
    let post_sealed = std::fs::read_to_string(root.join("src/sealed/locked.ts")).unwrap();
    assert_eq!(
        post_sealed, sealed_original,
        "sealed file must be untouched (stage couldn't even land its temp)",
    );
}

/// Build a project where two unused exports are reachable: one in a normal
/// `src/` file (high confidence) and one in an off-graph `e2e/` directory
/// (consumers may be invisible to static analysis). Both are genuine
/// `unused-export` findings; the gate must remove the first and withhold
/// the second.
fn write_off_graph_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("e2e")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"off-graph","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { realUsed } from './lib';\nimport { e2eUsed } from '../e2e/shared';\nconsole.log(realUsed, e2eUsed);\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.ts"),
        "export const realUsed = 1;\nexport const deadSrc = 2;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("e2e/shared.ts"),
        "export const e2eUsed = 1;\nexport const deadE2e = 2;\n",
    )
    .unwrap();
}

#[test]
fn fix_dry_run_withholds_off_graph_export_but_plans_high_confidence_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_off_graph_project(root);

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "off-graph skips are intentional, dry-run exits 0; stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();

    let removed_names: Vec<&str> = fixes
        .iter()
        .filter(|f| f["type"] == "remove_export")
        .filter_map(|f| f["name"].as_str())
        .collect();
    assert!(
        removed_names.contains(&"deadSrc"),
        "high-confidence src export must be planned for removal: {}",
        output.stdout
    );
    assert!(
        !removed_names.contains(&"deadE2e"),
        "off-graph e2e export must NOT be planned for removal: {}",
        output.stdout
    );

    let skip = fixes
        .iter()
        .find(|f| f["skip_reason"].as_str() == Some("low_confidence_off_graph"));
    let skip = skip.expect("an off-graph skip record must be present");
    assert_eq!(
        skip["path"].as_str().map(|p| p.replace('\\', "/")),
        Some("e2e/shared.ts".to_string()),
        "skip record anchors the off-graph file",
    );
    assert_eq!(
        json["skipped_low_confidence_exports"].as_u64(),
        Some(1),
        "envelope counts the single low-confidence skip: {}",
        output.stdout
    );
}

#[test]
fn fix_apply_keeps_off_graph_export_and_removes_high_confidence_one() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_off_graph_project(root);

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        fix.code, 0,
        "an apply whose only skip is intentional must exit 0; stderr: {}",
        fix.stderr
    );

    let lib = std::fs::read_to_string(root.join("src/lib.ts")).unwrap();
    assert_eq!(
        lib, "export const realUsed = 1;\nconst deadSrc = 2;\n",
        "high-confidence src export should have lost its `export` keyword",
    );
    let shared = std::fs::read_to_string(root.join("e2e/shared.ts")).unwrap();
    assert_eq!(
        shared, "export const e2eUsed = 1;\nexport const deadE2e = 2;\n",
        "off-graph e2e export must be preserved verbatim",
    );

    let json = parse_json(&fix);
    assert_eq!(json["skipped_low_confidence_exports"].as_u64(), Some(1));
}

/// The reproduction that made this a defect rather than a gap: a syntax error
/// on line 1 hides the import on line 2, `needed` reads as unused, the finding
/// correctly carries the degraded-parse caveat AND `auto_fixable: true`, and
/// `fix` used to plan and apply the removal anyway, breaking the build with a
/// mutation fallow itself had flagged as resting on incomplete evidence. The
/// declared `lodash` is the same hole one array over: the only import of it
/// sits under the same syntax error, and `remove-dependency` empties the
/// manifest.
fn write_degraded_parse_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"degraded","main":"src/index.ts","dependencies":{"lodash":"^4.17.21"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { other } from './lib';\nimport { start } from './consumer';\nother();\nstart();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/consumer.ts"),
        "const broken = = 1;\nimport { needed } from './lib';\nimport { chunk } from 'lodash';\nexport const start = (): void => { needed(); chunk([1], 1); };\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.ts"),
        "export const needed = (): void => {};\nexport const other = (): void => {};\n",
    )
    .unwrap();
}

#[test]
fn fix_dry_run_withholds_every_removal_a_degraded_parse_distorted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_degraded_parse_project(root);

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    assert_eq!(
        output.code, 0,
        "a degraded-parse withholding is intentional and must not move the exit code; stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();

    assert!(
        !fixes
            .iter()
            .any(|fix| fix["type"] == "remove_export" && fix["name"] == "needed"),
        "the export the broken file imports must not be planned for removal: {}",
        output.stdout
    );
    let export_skip = fixes
        .iter()
        .find(|fix| {
            fix["skip_reason"].as_str() == Some("low_confidence_incomplete_analysis")
                && fix["type"] == "skipped"
        })
        .expect("an export skip record must be present");
    assert_eq!(
        export_skip["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"]),
        "the skip entry carries the marker so a caller gates on it: {}",
        output.stdout
    );
    assert_eq!(json["skipped_low_confidence_exports"].as_u64(), Some(1));

    let dep_entry = fixes
        .iter()
        .find(|fix| fix["type"] == "remove_dependency")
        .expect("the dependency entry is still reported, as a withholding");
    assert_eq!(dep_entry["package"], "lodash");
    assert_eq!(
        dep_entry["skip_reason"],
        "low_confidence_incomplete_analysis"
    );
    assert_eq!(
        dep_entry["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"])
    );
    assert_eq!(
        json["skipped_low_confidence_dependencies"].as_u64(),
        Some(1)
    );
}

/// A quiet plan that lists every removal it WILL make and none of the ones it
/// REFUSED reads as complete when it is partial.
///
/// `--quiet` drops progress, not measurements. The `Would remove` lines are
/// gated on output format alone, so the withheld lines beside them must be too.
/// `Dry run complete` is progress and stays gated.
#[test]
fn fix_quiet_dry_run_still_reports_what_it_refused_to_remove() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_degraded_parse_project(root);

    let quiet = run_fallow_in_root("fix", root, &["--dry-run", "--quiet"]);
    assert_eq!(quiet.code, 0, "stdout:\n{}", quiet.stdout);
    assert!(
        quiet.stderr.contains("Kept unused export(s) in"),
        "the quiet plan must also name the export it refused to remove; stderr:\n{}",
        quiet.stderr
    );
    assert!(
        quiet.stderr.contains("Kept `lodash`"),
        "the quiet plan must also name the dependency it refused to remove; stderr:\n{}",
        quiet.stderr
    );
    assert!(
        quiet.stderr.contains("Kept unused exports in"),
        "the quiet plan must keep the trailing withheld summary; stderr:\n{}",
        quiet.stderr
    );
    assert!(
        !quiet.stderr.contains("Dry run complete"),
        "progress stays gated on --quiet; stderr:\n{}",
        quiet.stderr
    );

    let loud = run_fallow_in_root("fix", root, &["--dry-run"]);
    assert!(
        loud.stderr.contains("Dry run complete"),
        "stderr:\n{}",
        loud.stderr
    );
    for withheld in [
        "Kept unused export(s) in",
        "Kept `lodash`",
        "Kept unused exports in",
    ] {
        assert!(
            loud.stderr.contains(withheld),
            "--quiet must withhold nothing the loud run reported: {withheld:?} missing from\n{}",
            loud.stderr
        );
    }
}

/// The other half of the asymmetry: a removal fallow WILL make already survived
/// `--quiet`, and must keep doing so.
#[test]
fn fix_quiet_dry_run_still_lists_the_removals_it_would_make() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"cleanfix","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import { kept } from './util';\nkept();\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/util.ts"),
        "export const kept = (): void => {};\nexport const removable = (): void => {};\n",
    )
    .unwrap();

    let quiet = run_fallow_in_root("fix", root, &["--dry-run", "--quiet"]);
    assert!(
        quiet
            .stderr
            .contains("Would remove export from src/util.ts:2 `removable`"),
        "stderr:\n{}",
        quiet.stderr
    );
    assert!(
        !quiet.stderr.contains("Dry run complete"),
        "progress stays gated on --quiet; stderr:\n{}",
        quiet.stderr
    );
}

/// JSON callers read the skip records off the envelope, so the human stream
/// stays out of their way exactly as the `Would remove` lines do.
#[test]
fn fix_json_output_keeps_the_withheld_lines_off_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_degraded_parse_project(root);

    let output = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json"]);
    assert_eq!(output.code, 0, "stderr:\n{}", output.stderr);
    assert!(
        !output.stderr.contains("Kept "),
        "the human withheld lines must not leak into a JSON run; stderr:\n{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("Would remove"),
        "stderr:\n{}",
        output.stderr
    );
}

#[test]
fn fix_apply_leaves_a_degraded_parse_project_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_degraded_parse_project(root);
    let lib_before = std::fs::read_to_string(root.join("src/lib.ts")).unwrap();
    let manifest_before = std::fs::read_to_string(root.join("package.json")).unwrap();

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(
        fix.code, 0,
        "an apply whose only skips are intentional must exit 0; stderr: {}",
        fix.stderr
    );

    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.ts")).unwrap(),
        lib_before,
        "the export the broken file imports must survive verbatim",
    );
    assert_eq!(
        std::fs::read_to_string(root.join("package.json")).unwrap(),
        manifest_before,
        "the package whose only import sits under the syntax error must survive",
    );
}

/// The second door onto the same build-breaking mutation, and the one this
/// project's default settings leave open: no syntax error anywhere, just a file
/// the per-file size guard skipped before reading it. `src/huge.ts` imports
/// `needed` on its first line and is never opened, so `needed` reads as an
/// unused export with an auto-fixable `remove-export` action, and `fix --yes`
/// used to strip the `export` keyword while `huge.ts` still imported it.
///
/// The withholding is not wired to the size skip. It follows the caveat the
/// analysis stamps on the finding, which every diagnostic kind
/// `WorkspaceDiagnosticKind::source_never_analyzed` accepts raises.
fn write_skipped_file_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"skipped","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.ts"),
        "export const needed = 1;\nexport const alsoUsed = 2;\n",
    )
    .unwrap();
    let mut oversized = String::from("import { needed } from './lib';\nexport const pad = [\n");
    while oversized.len() < 1_200_000 {
        oversized.push_str("  \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n");
    }
    oversized.push_str("];\nexport const use = needed;\n");
    std::fs::write(root.join("src/huge.ts"), oversized).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import './huge';\nimport { alsoUsed } from './lib';\nexport const run = (): number => alsoUsed;\n",
    )
    .unwrap();
}

#[test]
fn fix_dry_run_withholds_a_removal_a_skipped_file_distorted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_skipped_file_project(root);

    let output = run_fallow_in_root(
        "fix",
        root,
        &[
            "--dry-run",
            "--format",
            "json",
            "--quiet",
            "--max-file-size",
            "1",
        ],
    );
    assert_eq!(
        output.code, 0,
        "an intentional withholding must not move the exit code; stderr: {}",
        output.stderr
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();

    assert!(
        !fixes
            .iter()
            .any(|fix| fix["type"] == "remove_export" && fix["name"] == "needed"),
        "the export the skipped file imports must not be planned for removal: {}",
        output.stdout
    );
    let skip = fixes
        .iter()
        .find(|fix| {
            fix["skip_reason"].as_str() == Some("low_confidence_incomplete_analysis")
                && fix["type"] == "skipped"
        })
        .expect("an export skip record must be present");
    assert_eq!(
        skip["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"]),
        "the skip entry names the caveat a caller gates on: {}",
        output.stdout
    );
    assert_eq!(json["skipped_low_confidence_exports"].as_u64(), Some(1));
}

#[test]
fn fix_apply_leaves_a_skipped_file_project_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_skipped_file_project(root);
    let lib_before = std::fs::read_to_string(root.join("src/lib.ts")).unwrap();

    let fix = run_fallow_in_root(
        "fix",
        root,
        &[
            "--yes",
            "--format",
            "json",
            "--quiet",
            "--max-file-size",
            "1",
        ],
    );
    assert_eq!(
        fix.code, 0,
        "an apply whose only skips are intentional must exit 0; stderr: {}",
        fix.stderr
    );
    assert_eq!(
        parse_json(&fix)["total_fixed"].as_u64(),
        Some(0),
        "nothing may be rewritten while a caveat stands: {}",
        fix.stdout
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/lib.ts")).unwrap(),
        lib_before,
        "the export the skipped file imports must survive verbatim",
    );
}

/// The caveat gate must not become a blanket refusal: one broken file cannot
/// stop `fix` from removing a package no module ever imported.
#[test]
fn fix_still_removes_a_dependency_no_degraded_file_could_have_imported() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"clean-dep","main":"src/index.ts","dependencies":{"lodash":"^4.17.21"}}"#,
    )
    .unwrap();
    std::fs::write(root.join("src/index.ts"), "export const run = 1;\n").unwrap();

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert_eq!(fix.code, 0, "stderr: {}", fix.stderr);

    let manifest = std::fs::read_to_string(root.join("package.json")).unwrap();
    assert!(
        !manifest.contains("lodash"),
        "a clean run must still auto-fix: {manifest}"
    );
    let json = parse_json(&fix);
    assert_eq!(
        json["skipped_low_confidence_dependencies"].as_u64(),
        Some(0)
    );
}

/// A project whose only reference to `Color.Blue` lives in a file the size
/// guard skips before reading it. Reproduced end to end before the fix:
/// `fix --yes` deleted the member while `huge.ts` still referenced it, because
/// member usage is collected by walking the member accesses of every module
/// the run PARSED and no caveat had ever reached `unused_enum_members[]`.
fn write_skipped_enum_member_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"skipped-member","main":"src/index.ts"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/colors.ts"),
        "export enum Color {\n  Red = \"red\",\n  Blue = \"blue\",\n}\n",
    )
    .unwrap();
    let mut oversized = String::from("import { Color } from './colors';\nexport const pad = [\n");
    while oversized.len() < 1_200_000 {
        oversized.push_str("  \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n");
    }
    oversized.push_str("];\nexport const pick = (): Color => Color.Blue;\n");
    std::fs::write(root.join("src/huge.ts"), oversized).unwrap();
    std::fs::write(
        root.join("src/index.ts"),
        "import './huge';\nimport { Color } from './colors';\nexport const run = (): Color => Color.Red;\n",
    )
    .unwrap();
}

#[test]
fn fix_apply_does_not_delete_an_enum_member_a_skipped_file_references() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_skipped_enum_member_project(root);
    let colors_before = std::fs::read_to_string(root.join("src/colors.ts")).unwrap();

    let fix = run_fallow_in_root(
        "fix",
        root,
        &[
            "--yes",
            "--format",
            "json",
            "--quiet",
            "--max-file-size",
            "1",
        ],
    );
    assert_eq!(
        fix.code, 0,
        "an intentional withholding must not move the exit code; stderr: {}",
        fix.stderr
    );

    assert_eq!(
        std::fs::read_to_string(root.join("src/colors.ts")).unwrap(),
        colors_before,
        "Color.Blue must survive: the only reference lives in a file the run never read",
    );

    let json = parse_json(&fix);
    let withheld = json["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fix| fix["type"] == "remove_enum_member")
        .expect("the withheld member is reported, not silently dropped");
    assert_eq!(withheld["skipped"], serde_json::json!(true));
    assert_eq!(
        withheld["skip_reason"].as_str(),
        Some("low_confidence_incomplete_analysis"),
        "the entry carries the marker a caller gates on: {}",
        fix.stdout
    );
    assert_eq!(
        withheld["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"])
    );
    assert_eq!(json["skipped_low_confidence_members"].as_u64(), Some(1));
}

/// BLOCKER: `analyze` advertised `auto_fixable: true` on the very mutations
/// `fix` refuses, and AGENTS.md tells agents to plan against that flag. The
/// two commands must agree on the same project.
#[test]
fn analyze_never_advertises_a_mutation_fix_would_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_skipped_enum_member_project(root);
    // One dependency per location, so the loop below reaches all three
    // dependency arrays rather than only the production one.
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"skipped-member","main":"src/index.ts","dependencies":{"lodash":"^4.17.21"},"devDependencies":{"chalk":"^5.3.0"},"optionalDependencies":{"fsevents":"^2.3.3"}}"#,
    )
    .unwrap();

    let analyze = run_fallow_in_root(
        "dead-code",
        root,
        &["--format", "json", "--quiet", "--max-file-size", "1"],
    );
    let json = parse_json(&analyze);

    let mut caveated = 0_usize;
    for array in [
        "unused_files",
        "unused_exports",
        "unused_types",
        "unused_enum_members",
        "unused_class_members",
        "unused_store_members",
        "unused_dependencies",
        "unused_dev_dependencies",
        "unused_optional_dependencies",
    ] {
        for finding in json[array].as_array().into_iter().flatten() {
            if finding["reachability_caveats"].is_null() {
                continue;
            }
            caveated += 1;
            for action in finding["actions"].as_array().into_iter().flatten() {
                assert_eq!(
                    action["auto_fixable"],
                    serde_json::json!(false),
                    "{array}: a caveated finding advertises an applicable {} action that \
                     `fallow fix` withholds: {}",
                    action["type"],
                    analyze.stdout
                );
            }
        }
    }
    assert!(
        caveated >= 4,
        "the loop is only meaningful while the run still produces caveated findings in the \
         member array and all three dependency arrays: {}",
        analyze.stdout
    );
}

#[test]
fn fix_envelope_always_carries_skipped_low_confidence_exports() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert_eq!(
        json["skipped_low_confidence_exports"].as_u64(),
        Some(0),
        "clean run must still carry the field at 0: {}",
        output.stdout
    );
}

/// Helper: recursively copy a directory tree so we don't mutate the
/// canonical fixture during the integration test.
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        // A fixture's cache directory is not part of the fixture, and another
        // test in this binary may be writing it while this copy walks it.
        if entry.file_name() == ".fallow" {
            continue;
        }
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
