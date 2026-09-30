#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! End-to-end tests that `fallow dupes` and combined-mode dupes respect
//! `--workspace` and `--changed-workspaces` scoping.
//!
//! The fixture builds three packages with intentionally duplicated code across
//! packages. Without workspace scoping, dupes detects the cross-package clone
//! groups. With scoping, clone groups that have no instance under the selected
//! workspace root(s) are dropped.

use crate::common::{git, parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Two-workspace monorepo with a repeating block of TypeScript duplicated
/// across both workspaces. The block is sized to clear fallow's default
/// clone-detection thresholds (min_tokens, min_lines).
fn build_dupes_fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    fs::create_dir_all(dir.join("packages/ui/src")).unwrap();
    fs::create_dir_all(dir.join("packages/api/src")).unwrap();

    fs::write(
        dir.join("package.json"),
        r#"{"name":"monorepo","private":true,"workspaces":["packages/*"]}"#,
    )
    .unwrap();

    let duplicated_block = r"
export function transform(input: { items: number[]; scale: number }) {
    const { items, scale } = input;
    const normalized = items.map((n) => n * scale);
    const sum = normalized.reduce((acc, n) => acc + n, 0);
    const mean = sum / normalized.length;
    const variance = normalized.reduce((acc, n) => acc + (n - mean) ** 2, 0) / normalized.length;
    const stddev = Math.sqrt(variance);
    return { sum, mean, stddev, values: normalized };
}
";

    fs::write(
        dir.join("packages/ui/package.json"),
        r#"{"name":"@mono/ui","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("packages/ui/src/index.ts"), duplicated_block).unwrap();

    fs::write(
        dir.join("packages/api/package.json"),
        r#"{"name":"@mono/api","main":"src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("packages/api/src/index.ts"), duplicated_block).unwrap();

    git_init_and_commit(dir);
    tmp
}

fn git_init_and_commit(dir: &Path) {
    git(dir, &["init", "-b", "main"]);
    git(dir, &["add", "."]);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
}

fn count_clone_groups(json: &serde_json::Value) -> usize {
    json.get("clone_groups")
        .and_then(|v| v.as_array())
        .map_or(0, std::vec::Vec::len)
}

fn combined_dupes_clone_groups(json: &serde_json::Value) -> usize {
    json.get("dupes")
        .and_then(|d| d.get("clone_groups"))
        .and_then(|v| v.as_array())
        .map_or(0, std::vec::Vec::len)
}

#[test]
fn package_baselines_scope_standalone_and_combined_dupes() {
    let tmp = build_dupes_fixture();
    let root = tmp.path();
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/ui":"HEAD","packages/api":"HEAD"}}}"#,
    )
    .unwrap();
    let root_arg = root.to_str().unwrap();
    let standalone = run_fallow_raw(&["dupes", "--root", root_arg, "--format", "json", "--quiet"]);
    assert_eq!(standalone.code, 0, "{}", standalone.stderr);
    assert_eq!(count_clone_groups(&parse_json(&standalone)), 0);
    assert_eq!(
        parse_json(&standalone)["package_baselines"],
        serde_json::json!([
            {"workspace_root":"packages/api","reference":"HEAD"},
            {"workspace_root":"packages/ui","reference":"HEAD"}
        ])
    );

    let combined = run_fallow_raw(&["--root", root_arg, "--format", "json", "--quiet"]);
    assert!(
        combined.code == 0 || combined.code == 1,
        "{}",
        combined.stderr
    );
    assert_eq!(combined_dupes_clone_groups(&parse_json(&combined)), 0);
    assert_eq!(
        parse_json(&combined)["package_baselines"],
        parse_json(&standalone)["package_baselines"]
    );

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/ui":"HEAD"}}}"#,
    )
    .unwrap();
    let unmapped = run_fallow_raw(&["dupes", "--root", root_arg, "--format", "json", "--quiet"]);
    assert!(count_clone_groups(&parse_json(&unmapped)) > 0);

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/ui":"missing-ref"}}}"#,
    )
    .unwrap();
    let stood_down = run_fallow_raw(&["dupes", "--root", root_arg, "--format", "json", "--quiet"]);
    assert_eq!(stood_down.code, 0, "{}", stood_down.stderr);
    let json = parse_json(&stood_down);
    assert!(
        count_clone_groups(&json) > 0,
        "an unresolved ref gives full scope"
    );
    assert!(json.get("package_baselines").is_none());
    assert_eq!(
        json["request_outcomes"]["package-baselines"]["status"],
        "not-applied"
    );
    assert!(
        stood_down
            .stderr
            .contains("workspaces.changedSince was ignored"),
        "{}",
        stood_down.stderr
    );
}

#[test]
fn clone_trace_respects_package_baselines() {
    let tmp = build_dupes_fixture();
    let root = tmp.path();
    let root_arg = root.to_str().unwrap();
    let unscoped = run_fallow_raw(&["dupes", "--root", root_arg, "--format", "json", "--quiet"]);
    let fingerprint = parse_json(&unscoped)["clone_groups"][0]["fingerprint"]
        .as_str()
        .unwrap()
        .to_string();

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/ui":"HEAD","packages/api":"HEAD"}}}"#,
    )
    .unwrap();
    let excluded = run_fallow_raw(&[
        "dupes",
        "--root",
        root_arg,
        "--trace",
        &fingerprint,
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(excluded.code, 2, "{}", excluded.stdout);

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"workspaces":{"changedSince":{"packages/ui":"HEAD"}}}"#,
    )
    .unwrap();
    let retained = run_fallow_raw(&[
        "dupes",
        "--root",
        root_arg,
        "--trace",
        &fingerprint,
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(retained.code, 0, "{}", retained.stdout);
}

#[test]
fn dupes_without_scope_finds_cross_package_clone() {
    let tmp = build_dupes_fixture();
    let out = run_fallow_raw(&[
        "dupes",
        "--root",
        tmp.path().to_str().unwrap(),
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(out.code, 0, "dupes should exit 0 without a threshold");
    let json = parse_json(&out);
    assert!(
        count_clone_groups(&json) >= 1,
        "expected at least 1 clone group across ui+api without scoping, got {}",
        count_clone_groups(&json)
    );
}

#[test]
fn dupes_workspace_scope_drops_cross_package_only_group() {
    let tmp = build_dupes_fixture();
    let out = run_fallow_raw(&[
        "dupes",
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace",
        "@mono/ui",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(out.code, 0);
    let json = parse_json(&out);
    assert!(
        count_clone_groups(&json) >= 1,
        "group with an instance under ui should be retained, got {}",
        count_clone_groups(&json)
    );
}

#[test]
fn combined_changed_workspaces_head_drops_all_dupes() {
    let tmp = build_dupes_fixture();
    let out = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--changed-workspaces",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        out.code, 0,
        "no issues should yield exit 0, stderr={}",
        out.stderr
    );
    let json = parse_json(&out);
    let dupes_count = combined_dupes_clone_groups(&json);
    assert_eq!(
        dupes_count, 0,
        "combined mode must apply --changed-workspaces to dupes; got {dupes_count} groups"
    );
}

#[test]
fn combined_workspace_scope_applies_to_dupes() {
    let tmp = build_dupes_fixture();
    let out_with_scope = run_fallow_raw(&[
        "--root",
        tmp.path().to_str().unwrap(),
        "--workspace",
        "@mono/ui",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&out_with_scope);
    assert!(
        combined_dupes_clone_groups(&json) >= 1,
        "ui scope keeps the cross-package group (instance under ui)"
    );
}
