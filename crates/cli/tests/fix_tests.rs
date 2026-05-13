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
