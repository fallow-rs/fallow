#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;

use crate::common::{git, parse_json, run_fallow, run_fallow_in_root};
use tempfile::TempDir;

/// Find the per-file entry for `path` in the inventory JSON.
fn file_entry<'a>(json: &'a serde_json::Value, path: &str) -> &'a serde_json::Value {
    json["files"]
        .as_array()
        .expect("files array")
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no inventory entry for {path}: {json}"))
}

#[test]
fn json_envelope_has_kind_and_schema_version() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    assert_eq!(out.code, 0, "read-only inventory always exits 0");
    let json = parse_json(&out);
    assert_eq!(json["kind"], "suppression-inventory");
    assert_eq!(json["schema_version"], "1");
}

#[test]
fn line_level_marker_reports_line_kind_and_reason() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let json = parse_json(&out);

    let reasoned = file_entry(&json, "src/reasoned.ts");
    let entries = reasoned["suppressions"].as_array().expect("suppressions");
    assert_eq!(entries[0]["line"], 1);
    assert_eq!(entries[0]["kind"], "unused-export");
    assert_eq!(entries[0]["level"], "line");
    assert_eq!(entries[0]["origin"], "comment");
    assert_eq!(entries[0]["reason"], "public compatibility export");
    assert_eq!(entries[0]["reason_present"], true);
    // The second marker sits on line 3 and carries no reason.
    assert_eq!(entries[1]["line"], 3);
    assert!(entries[1]["reason"].is_null());
    assert_eq!(entries[1]["reason_present"], false);
}

#[test]
fn file_level_blanket_marker_reports_file_level_and_null_kind() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let json = parse_json(&out);

    let blanket = file_entry(&json, "src/blanket.ts");
    let entry = &blanket["suppressions"][0];
    assert_eq!(entry["line"], 2, "file-level marker sits on line 2");
    assert!(
        entry["kind"].is_null(),
        "blanket marker keeps JSON kind null"
    );
    assert_eq!(entry["level"], "file");
    assert_eq!(entry["reason_present"], false);
}

#[test]
fn summary_counts_totals_reasons_stale_and_by_kind() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    let json = parse_json(&out);

    let summary = &json["summary"];
    assert_eq!(summary["total"], 4);
    assert_eq!(summary["files"], 3);
    assert_eq!(summary["without_reason"], 3);
    // src/stale.ts suppresses unused-export on an export that IS used, so the
    // stale-suppression detector reports it and the inventory join counts it.
    assert_eq!(summary["stale"], 1);

    let by_kind = summary["by_kind"].as_array().expect("by_kind array");
    assert_eq!(by_kind[0]["kind"], "unused-export");
    assert_eq!(by_kind[0]["count"], 3);
    assert!(
        by_kind[1]["kind"].is_null(),
        "blanket bucket keeps null kind"
    );
    assert_eq!(by_kind[1]["count"], 1);
}

#[test]
fn file_scope_limits_inventory_to_selected_file() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &[
            "--no-cache",
            "--format",
            "json",
            "--quiet",
            "--file",
            "src/reasoned.ts",
        ],
    );
    assert_eq!(out.code, 0);
    let json = parse_json(&out);

    assert_eq!(json["summary"]["total"], 2);
    assert_eq!(json["summary"]["files"], 1);
    assert_eq!(json["files"][0]["path"], "src/reasoned.ts");
}

#[test]
fn human_output_renders_inventory_and_totals() {
    let out = run_fallow("suppressions", "suppression-inventory", &["--no-cache"]);
    assert_eq!(out.code, 0);
    assert!(
        out.stdout.contains("Suppression inventory (4)"),
        "stdout should carry the inventory header: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("blanket") && out.stdout.contains("(file-wide)"),
        "blanket file-level marker should render as such: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("Totals by kind"),
        "stdout should carry the totals section: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains("4 suppressions in 3 files") && out.stderr.contains("1 stale"),
        "stderr should carry the summary line: {}",
        out.stderr
    );
}

#[test]
fn quiet_suppresses_human_summary_line() {
    let out = run_fallow(
        "suppressions",
        "suppression-inventory",
        &["--no-cache", "--quiet"],
    );
    assert_eq!(out.code, 0);
    assert!(
        !out.stderr.contains("suppressions in"),
        "--quiet suppresses the stderr summary: {}",
        out.stderr
    );
}

#[test]
fn reuses_suppression_reasons_fixture_for_reason_capture() {
    let out = run_fallow(
        "suppressions",
        "suppression-reasons",
        &["--no-cache", "--format", "json", "--quiet"],
    );
    assert_eq!(out.code, 0);
    let json = parse_json(&out);

    let reasoned = file_entry(&json, "src/reasoned.ts");
    let entries = reasoned["suppressions"].as_array().expect("suppressions");
    assert_eq!(entries[0]["line"], 1);
    assert_eq!(entries[0]["reason_present"], true);
    assert_eq!(entries[1]["line"], 4);
    assert_eq!(entries[1]["reason_present"], false);
}

#[test]
fn workspace_scope_limits_inventory_to_selected_package() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    fs::write(
        root.join("package.json"),
        r#"{"name":"ws-root","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();
    for pkg in ["pkg-a", "pkg-b"] {
        let pkg_dir = root.join("packages").join(pkg).join("src");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(
            root.join("packages").join(pkg).join("package.json"),
            format!(r#"{{"name":"{pkg}","main":"src/index.ts"}}"#),
        )
        .unwrap();
        fs::write(
            pkg_dir.join("index.ts"),
            "// fallow-ignore-next-line unused-export\nexport const kept = 1;\n",
        )
        .unwrap();
    }

    let out = run_fallow_in_root(
        "suppressions",
        root,
        &[
            "--no-cache",
            "--format",
            "json",
            "--quiet",
            "--workspace",
            "pkg-a",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let json = parse_json(&out);

    let files = json["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1, "only pkg-a should remain: {json}");
    let path = files[0]["path"].as_str().unwrap();
    assert!(
        path.starts_with("packages/pkg-a/"),
        "workspace-scoped path should be in packages/pkg-a/, got: {path}"
    );
}

#[test]
fn changed_since_scope_limits_inventory_to_changed_files() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"changed-suppressions","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        root.join("src/index.ts"),
        "// fallow-ignore-next-line unused-export\nexport const base = 1;\n",
    )
    .unwrap();

    git(root, &["init", "--quiet"]);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-gpg-sign", "-m", "base"]);

    fs::write(
        root.join("src/added.ts"),
        "// fallow-ignore-next-line unused-export\nexport const added = 2;\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-gpg-sign", "-m", "add"]);

    let out = run_fallow_in_root(
        "suppressions",
        root,
        &[
            "--no-cache",
            "--format",
            "json",
            "--quiet",
            "--changed-since",
            "HEAD~1",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    let json = parse_json(&out);

    let files = json["files"].as_array().expect("files array");
    assert_eq!(
        files.len(),
        1,
        "only the file added since HEAD~1 should remain: {json}"
    );
    assert_eq!(files[0]["path"], "src/added.ts");
    assert_eq!(json["summary"]["total"], 1);
}
