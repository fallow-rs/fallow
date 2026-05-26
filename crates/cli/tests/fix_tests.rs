#[path = "common/mod.rs"]
mod common;

use common::{fixture_path, parse_json, run_fallow, run_fallow_in_root};

// ---------------------------------------------------------------------------
// fix --dry-run
// ---------------------------------------------------------------------------

#[test]
fn fix_dry_run_exits_0() {
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
}

#[test]
fn fix_dry_run_json_has_dry_run_flag() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    assert_eq!(
        json["dry_run"].as_bool(),
        Some(true),
        "dry_run should be true"
    );
}

#[test]
fn fix_dry_run_finds_fixable_items() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();
    assert!(!fixes.is_empty(), "basic-project should have fixable items");

    // Each fix should have a type
    for fix in fixes {
        assert!(fix.get("type").is_some(), "fix should have 'type'");
        // Export fixes have "path", dependency fixes have "package"
        let has_path = fix.get("path").is_some() || fix.get("package").is_some();
        assert!(has_path, "fix should have 'path' or 'package'");
    }
}

#[test]
fn fix_dry_run_does_not_have_applied_key() {
    let output = run_fallow(
        "fix",
        "basic-project",
        &["--dry-run", "--format", "json", "--quiet"],
    );
    let json = parse_json(&output);
    let fixes = json["fixes"].as_array().unwrap();
    for fix in fixes {
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
    // Regression for issue #232: an exported enum that has importers but
    // whose members are all unused should be removed entirely, not stripped
    // member-by-member into a zombie `export enum X {}` shell.
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

    // Second pass: the empty-shell zombie that 2.54.3 would have left behind
    // must not be present, and the fold must not produce any new fix.
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

    // Round-trip: a follow-up analysis must still load this config cleanly.
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

    // The symlink must still BE a symlink after the write (atomic_write
    // canonicalized to the target).
    let meta = std::fs::symlink_metadata(root.join(".fallowrc.json")).unwrap();
    assert!(
        meta.file_type().is_symlink(),
        "symlink was replaced with regular file by atomic_write"
    );

    // The target must contain the new ignoreExports entries.
    let target_content = std::fs::read_to_string(&real_path).unwrap();
    assert!(
        target_content.contains("\"ignoreExports\""),
        "symlink target was not updated, got: {target_content}"
    );
}

// ---------------------------------------------------------------------------
// fix without --yes in non-TTY
// ---------------------------------------------------------------------------

#[test]
fn fix_without_yes_in_non_tty_exits_2() {
    // Running fix without --dry-run and without --yes in a non-TTY (test runner)
    // should exit 2 with an error
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
    // Regression: `# fallow-keep` marker preserves a comment block even
    // under `policy: always`. Mirrors the inline-suppression convention
    // (`fallow-ignore-*`) so users discover it without docs.
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

// ---------------------------------------------------------------------------
// fix --yes on the canonical pnpm-catalog fixture (issue #335)
// ---------------------------------------------------------------------------

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

    // Regression assertion: `react17` is the named catalog whose only entries
    // (react, react-dom) get removed by the fix. The header MUST be rewritten
    // to `react17: {}`, not left bare as `react17:`.
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

    // Sanity: the sibling `legacy` catalog is untouched (its `is-odd` entry
    // is still consumed by `packages/lib/package.json`).
    let legacy = parsed
        .get("catalogs")
        .and_then(|c| c.get("legacy"))
        .and_then(serde_yaml_ng::Value::as_mapping)
        .expect("catalogs.legacy must remain a mapping");
    assert!(
        legacy.contains_key(serde_yaml_ng::Value::String("is-odd".to_string())),
        "catalogs.legacy must still declare `is-odd`. Got: {legacy:?}"
    );

    // Sanity: the default `catalog:` map still has the entry that was kept
    // (`react` is consumed via `catalog:` from `packages/app`).
    let default_catalog = parsed
        .get("catalog")
        .and_then(serde_yaml_ng::Value::as_mapping)
        .expect("catalog: must remain a mapping");
    assert!(
        default_catalog.contains_key(serde_yaml_ng::Value::String("react".to_string())),
        "default catalog must still declare `react` (it has consumers). Got: {default_catalog:?}"
    );

    // The fix output's JSON envelope must include the new top-level
    // `skipped` count, with one skip (hardcoded-pkg, which has a
    // hardcoded consumer in this fixture).
    let json = parse_json(&output);
    assert_eq!(
        json["skipped"].as_u64(),
        Some(1),
        "fixture has one hardcoded-pkg skip; envelope must report skipped: 1, got: {}",
        json["skipped"]
    );
}

// ---------------------------------------------------------------------------
// Issue #454: hash precondition + batch atomicity
// ---------------------------------------------------------------------------

#[test]
fn fix_json_envelope_carries_skipped_content_changed_count() {
    // The orchestrator MUST surface the new envelope field even when
    // every fixer ran cleanly; the count is 0 here but the field's
    // presence is the contract that consumers (CI scripts, MCP tools)
    // depend on.
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
    // Round-trip: apply fix on a fresh tmpdir, then re-run check, then
    // assert the targeted unused-exports findings are gone and no new
    // findings surfaced. Validates batch-atomic commit + the per-file
    // edits land coherent enough that the next analysis passes.
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

    // Re-analyze; the same exports must NOT reappear, and no new
    // findings should have been introduced by the rewrite.
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
    // Batch atomicity: when staging a write fails for one target, NO
    // renames must have occurred. We make a sibling source file's parent
    // directory read-only so the temp-file-in-same-dir stage fails for
    // that path; the orchestrator must leave the OTHER, healthy source
    // file untouched.
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

    // Seal the sealed/ directory so NamedTempFile::new_in() inside it fails.
    // We chmod 0o555 (read+exec, no write) so the existing file is still
    // readable but no new temp can be created beside it.
    let sealed_dir = root.join("src/sealed");
    let mut perms = std::fs::metadata(&sealed_dir).unwrap().permissions();
    perms.set_mode(0o555);
    std::fs::set_permissions(&sealed_dir, perms).unwrap();

    let fix = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);

    // Restore permissions before any assertion can panic and skip cleanup.
    let mut restore = std::fs::metadata(&sealed_dir).unwrap().permissions();
    restore.set_mode(0o755);
    std::fs::set_permissions(&sealed_dir, restore).unwrap();

    assert_eq!(
        fix.code, 2,
        "batch commit failure must surface as exit 2; stdout: {} stderr: {}",
        fix.stdout, fix.stderr,
    );
    // Healthy file must be untouched (the batch aborted before any rename).
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

// ---------------------------------------------------------------------------
// issue #602: low-confidence off-graph export gate
// ---------------------------------------------------------------------------

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
    // Entry imports one export from each module, leaving `deadSrc` and
    // `deadE2e` unreferenced (and therefore reported as unused-exports),
    // while keeping both files reachable so neither is an unused-FILE.
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

    // The high-confidence src export is planned for removal.
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

    // The off-graph file surfaces as a low-confidence skip record.
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

    // High-confidence export removed from disk.
    let lib = std::fs::read_to_string(root.join("src/lib.ts")).unwrap();
    assert_eq!(
        lib, "export const realUsed = 1;\nconst deadSrc = 2;\n",
        "high-confidence src export should have lost its `export` keyword",
    );
    // Off-graph export untouched on disk.
    let shared = std::fs::read_to_string(root.join("e2e/shared.ts")).unwrap();
    assert_eq!(
        shared, "export const e2eUsed = 1;\nexport const deadE2e = 2;\n",
        "off-graph e2e export must be preserved verbatim",
    );

    let json = parse_json(&fix);
    assert_eq!(json["skipped_low_confidence_exports"].as_u64(), Some(1));
}

#[test]
fn fix_envelope_always_carries_skipped_low_confidence_exports() {
    // Presence-of-field contract: consumers (CI jq, MCP, VS Code) gate on
    // the key existing even when the count is 0.
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
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            // Includes regular files AND symlinks; the fixture contains
            // only regular files but the broader match is safer than
            // `is_file()` (which excludes symlinks).
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
