#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

//! Every surface that recommends acting on a dead-code finding has to say when
//! the evidence behind it is incomplete.
//!
//! The reproduction is the one the size guard produces at default settings: a
//! file larger than `FALLOW_MAX_FILE_SIZE` is dropped at discovery, so the
//! import on its first line is never read, never credits anything, and the
//! module it named surfaces as a confident `unused-file` finding carrying a
//! `delete-file` action. `fallow fix` already withholds that write. These tests
//! pin that every reporting surface says so too, because a reviewer or an
//! automation acting on a review suggestion never sees the fix path.
//!
//! Assertions are written on the caveat reaching the surface, not on the size
//! skip: the withholding follows the caveat, and every diagnostic kind
//! `WorkspaceDiagnosticKind::source_never_analyzed` accepts raises it.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::common::fallow_bin;

/// One megabyte, the smallest limit `FALLOW_MAX_FILE_SIZE` accepts.
const SIZE_LIMIT_MB: &str = "1";

/// Write a project whose only importer of `src/lib.ts` is a file the size guard
/// drops before reading it.
fn write_skipped_importer_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "caveat-surfaces", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.ts"), "export const needed = 1;\n").expect("write library");

    let mut oversized = String::from("import { needed } from \"./lib\";\nexport const pad = [\n");
    while oversized.len() < 2 * 1024 * 1024 {
        oversized.push_str("  \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n");
    }
    oversized.push_str("];\n");
    std::fs::write(root.join("src/huge.ts"), oversized).expect("write oversized importer");

    std::fs::write(
        root.join("src/index.ts"),
        "import \"./huge\";\n\nexport const run = (): void => {};\n",
    )
    .expect("write entry module");
}

/// The auto-fixable variant, and the dangerous one: `src/lib.ts` stays
/// reachable from the entry point, so only the export the skipped file imports
/// reads as unused, and that is the finding the review comment ships a literal
/// ```` ```suggestion ```` block for.
fn write_skipped_export_importer_project(root: &Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "caveat-export-surfaces", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/lib.ts"),
        "export const needed = 1;\nexport const alsoUsed = 2;\n",
    )
    .expect("write library");

    let mut oversized = String::from("import { needed } from \"./lib\";\nexport const pad = [\n");
    while oversized.len() < 2 * 1024 * 1024 {
        oversized.push_str("  \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n");
    }
    oversized.push_str("];\n");
    std::fs::write(root.join("src/huge.ts"), oversized).expect("write oversized importer");

    std::fs::write(
        root.join("src/index.ts"),
        "import \"./huge\";\nimport { alsoUsed } from \"./lib\";\n\nexport const run = (): number => alsoUsed;\n",
    )
    .expect("write entry module");
}

fn run_format(root: &Path, format: &str) -> String {
    let output = Command::new(fallow_bin())
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_MAX_FILE_SIZE", SIZE_LIMIT_MB)
        .args(["check", "--format", format, "--quiet", "--no-cache"])
        .output()
        .expect("run fallow check");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn project() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_skipped_importer_project(&root);
    (dir, root)
}

/// The reproduction itself: the finding exists, keeps its actions, and carries
/// the caveat on the wire. Everything below asserts that the caveat survives
/// into a rendered surface.
#[test]
fn the_json_finding_is_reported_with_its_actions_and_a_caveat() {
    let (_dir, root) = project();

    let stdout = run_format(&root, "json");
    let envelope: serde_json::Value = serde_json::from_str(&stdout).expect("json envelope");
    let finding = &envelope["unused_files"][0];

    assert_eq!(finding["path"], "src/lib.ts");
    assert_eq!(
        finding["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"])
    );
    assert!(
        !finding["actions"].as_array().expect("actions").is_empty(),
        "a caveat is advisory provenance and never trims the actions: {finding}"
    );
}

/// The two formats that already carried it, pinned so a refactor cannot quietly
/// drop them while the newer ones stay green.
#[test]
fn the_surfaces_that_already_carried_the_caveat_still_do() {
    let (_dir, root) = project();

    assert!(
        run_format(&root, "human").contains("(caveat: incomplete import graph)"),
        "human report"
    );
    assert!(
        run_format(&root, "sarif").contains("(caveat: incomplete import graph)"),
        "sarif message"
    );
}

/// `--summary` prints category counts and no finding lines, so a CI job that
/// captures only this output would otherwise lose the caveat entirely.
#[test]
fn the_summary_view_states_the_caveat() {
    let (_dir, root) = project();

    let output = Command::new(fallow_bin())
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_MAX_FILE_SIZE", SIZE_LIMIT_MB)
        .args(["check", "--summary", "--quiet", "--no-cache"])
        .output()
        .expect("run fallow check --summary");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("carry a caveat: incomplete import graph"),
        "{stdout}"
    );
}

/// Every rendered surface that has to name the caveat, and the same list a
/// clean project has to render without one. The two directions share one
/// constant so they cannot drift: `sarif` sat in the clean list and not in the
/// caveat list, so deleting the caveat from every SARIF message left the caveat
/// test green, and `pr-comment-gitlab` and `review-gitlab` sat in the caveat
/// list and not the clean one.
const CAVEAT_BEARING_FORMATS: [&str; 11] = [
    "human",
    "compact",
    "markdown",
    "codeclimate",
    "sarif",
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
    "github-annotations",
    "github-summary",
];

/// The machine-consumed formats. `review-github` and `review-gitlab` are the
/// severe ones: they emit an inline review comment on the diff line, with a
/// literal suggestion block for an export removal, and an automation acting on
/// review suggestions never sees the human report or the fix path.
#[test]
fn every_machine_consumed_format_carries_the_caveat() {
    let (_dir, root) = project();

    for format in CAVEAT_BEARING_FORMATS {
        let rendered = run_format(&root, format);
        let names_caveat = rendered.contains("incomplete import graph")
            || rendered.contains("incomplete-import-graph");
        assert!(
            names_caveat,
            "`--format {format}` recommends acting on a caveated finding without saying the evidence is incomplete:\n{rendered}"
        );
    }
}

/// The review comment is the one that ships a machine-actionable edit, so the
/// hedge has to sit in the same body as the fix intent rather than somewhere
/// else in the envelope.
#[test]
fn the_review_comment_body_hedges_next_to_its_fix_intent() {
    let (_dir, root) = project();

    let envelope: serde_json::Value =
        serde_json::from_str(&run_format(&root, "review-github")).expect("review envelope");
    let body = envelope["comments"][0]["body"]
        .as_str()
        .expect("inline comment body");

    // The description is run through the CommonMark inline escaper before it
    // reaches the body, so the parentheses arrive backslash-escaped. Assert on
    // the words, which is what a reader and an automation actually see.
    assert!(
        body.contains("caveat: incomplete import graph"),
        "the inline comment must hedge: {body}"
    );
    assert!(
        body.contains("Fix intent:"),
        "the guard is only meaningful while the comment still offers the mutation: {body}"
    );
    assert!(
        body.find("caveat:").expect("caveat position")
            < body.find("Fix intent:").expect("fix intent position"),
        "the hedge has to be read before the recommendation: {body}"
    );
}

/// A project where every discovered file was read must render exactly as it did
/// before this mechanism existed, on every surface.
#[test]
fn a_clean_project_carries_no_caveat_on_any_surface() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "clean", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(root.join("src/lib.ts"), "export const needed = 1;\n").expect("write library");
    std::fs::write(
        root.join("src/index.ts"),
        "export const run = (): void => {};\n",
    )
    .expect("write entry module");

    for format in CAVEAT_BEARING_FORMATS {
        let rendered = run_format(&root, format);
        assert!(
            !rendered.to_lowercase().contains("caveat"),
            "`--format {format}` invented a caveat on a fully analyzed project:\n{rendered}"
        );
    }
}

/// The severe case the whole arm is about: a review comment carrying a literal
/// code-edit block for a mutation `fallow fix` itself refuses.
///
/// This used to assert that the block MAY stand as long as the body hedges,
/// on the rule that a caveat never withholds a finding. That rule is right and
/// this was the wrong place to apply it: a ```` ```suggestion ```` block is not
/// part of the finding, it is the mutation, and on GitHub it is one click from
/// a commit on the contributor's branch. Every other mutation surface asks
/// `MutationEvidence::may_auto_apply_mutation` first. A parenthetical several
/// lines above the button is a disclosure, not a gate.
///
/// So the finding still ships, in the same comment, at the same location, with
/// its caveat text intact. Only the committable edit is withheld, and the body
/// says why.
#[test]
fn a_caveated_finding_ships_its_text_but_no_committable_edit() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_skipped_export_importer_project(&root);

    for (format, fence) in [
        ("review-github", "```suggestion"),
        ("review-gitlab", "```suggestion:-0+0"),
    ] {
        let envelope: serde_json::Value =
            serde_json::from_str(&run_format(&root, format)).expect("review envelope");
        let body = envelope["comments"][0]["body"]
            .as_str()
            .expect("inline comment body")
            .to_owned();

        assert!(
            !body.contains(fence),
            "`--format {format}` shipped a one-click edit for a mutation `fix` refuses: {body}"
        );
        assert!(
            body.contains("caveat: incomplete import graph"),
            "`--format {format}` dropped the caveat text along with the block: {body}"
        );
        assert!(
            body.contains("No one-click fix offered"),
            "`--format {format}` withheld the block without saying so: {body}"
        );
    }
}

/// The other half of the same rule: a run that read every file it discovered
/// keeps exactly the review comment it had, edit block included. A gate that
/// withheld every suggestion would be as wrong as one that withheld none.
#[test]
fn a_clean_run_keeps_its_committable_edit() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "clean", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/lib.ts"),
        "export const used = 1;\nexport const needed = 2;\n",
    )
    .expect("write library");
    std::fs::write(
        root.join("src/index.ts"),
        "import { used } from './lib';\n\nexport const run = (): number => used;\n",
    )
    .expect("write entry module");

    for (format, fence) in [
        ("review-github", "```suggestion"),
        ("review-gitlab", "```suggestion:-0+0"),
    ] {
        let envelope: serde_json::Value =
            serde_json::from_str(&run_format(&root, format)).expect("review envelope");
        let bodies = envelope["comments"]
            .as_array()
            .expect("inline comments")
            .iter()
            .filter_map(|comment| comment["body"].as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            bodies.contains(fence),
            "`--format {format}` must keep the edit block on a fully analyzed run: {bodies}"
        );
        assert!(
            !bodies.contains("No one-click fix offered"),
            "`--format {format}` withheld an edit the run has the evidence for: {bodies}"
        );
    }
}

/// `fallow fix` refuses the same write, which is the asymmetry that made the
/// review comment dangerous: the two surfaces must agree about the evidence
/// even though only one of them declines to act on it.
#[test]
fn the_fix_path_withholds_the_write_the_review_comment_hedges() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_skipped_export_importer_project(&root);

    let output = Command::new(fallow_bin())
        .current_dir(&root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env("FALLOW_MAX_FILE_SIZE", SIZE_LIMIT_MB)
        .args([
            "fix",
            "--dry-run",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ])
        .output()
        .expect("run fallow fix --dry-run");
    let envelope: serde_json::Value = serde_json::from_slice(&output.stdout).expect("fix envelope");

    assert_eq!(
        envelope["fixes"][0]["skip_reason"], "low_confidence_incomplete_analysis",
        "{envelope}"
    );

    // The other half of the agreement, on the same project: the review comment
    // still reports the finding `fix` declined to write, and hedges it.
    let review: serde_json::Value =
        serde_json::from_str(&run_format(&root, "review-github")).expect("review envelope");
    let body = review["comments"][0]["body"]
        .as_str()
        .expect("inline comment body");

    assert!(
        body.contains("caveat: incomplete import graph"),
        "the surface that reports what `fix` withheld must hedge it: {body}"
    );
    assert!(
        !body.contains("```suggestion"),
        "the review comment must not offer a one-click edit for the write `fix` refused: {body}"
    );
}

/// Run any fallow invocation against `root` under the size limit that produces
/// the caveat, without assuming the working directory is the project.
fn run_fallow(root: &Path, cwd: &Path, fallow_root: Option<&Path>, args: &[&str]) -> String {
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env_remove("FALLOW_ROOT")
        .env("FALLOW_MAX_FILE_SIZE", SIZE_LIMIT_MB)
        .args(args)
        .args(["--quiet", "--no-cache", "-r"])
        .arg(root);
    if let Some(fallow_root) = fallow_root {
        command.env("FALLOW_ROOT", fallow_root);
    }
    let output = command.output().expect("run fallow");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The combined envelope keys the caveated arrays under `check`. It has no
/// `dead_code` block; that name belongs to the audit brief, and a consumer
/// reading the compatibility entry has to be pointed at the right one.
#[test]
fn the_combined_envelope_carries_the_caveat_under_check() {
    let (_dir, root) = project();

    let envelope: serde_json::Value =
        serde_json::from_str(&run_fallow(&root, &root, None, &["--format", "json"]))
            .expect("combined envelope");

    assert!(
        envelope.get("dead_code").is_none(),
        "the combined envelope names its dead-code block `check`: {}",
        envelope
            .as_object()
            .map(|map| map.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    );
    assert_eq!(
        envelope["check"]["unused_files"][0]["reachability_caveats"][0], "incomplete-import-graph",
        "{envelope}"
    );
}

/// The annotation appends the caveat, so the message ENDS with the explanation
/// line and the `Caveat:` sentence sits after the remediation guidance, not
/// before it. The compatibility entry used to claim the reverse.
#[test]
fn the_annotation_appends_the_caveat_after_its_remediation_guidance() {
    let (_dir, root) = project();

    let rendered = run_format(&root, "github-annotations");
    let annotation = rendered
        .lines()
        .find(|line| line.contains("title=Unused file::"))
        .expect("an annotation for the caveated finding");

    let caveat = annotation
        .find("Caveat: incomplete import graph.")
        .expect("the caveat sentence");
    let guidance = annotation
        .find("Consider removing it")
        .expect("the remediation guidance");

    assert!(
        guidance < caveat,
        "the caveat is appended after the guidance, not inserted before it: {annotation}"
    );
    assert!(
        annotation.ends_with("so verify before removing."),
        "the message ends with the explanation, not with the `Caveat:` sentence: {annotation}"
    );
}

/// `Fix intent:` is a review-comment line. The sticky summary comment renders a
/// table and never carries one, so the caveat there sits in the description
/// cell with nothing to sit above.
#[test]
fn only_the_review_formats_carry_a_fix_intent_line() {
    let (_dir, root) = project();

    for format in ["pr-comment-github", "pr-comment-gitlab"] {
        let rendered = run_format(&root, format);
        assert!(
            rendered.contains("caveat: incomplete import graph"),
            "`--format {format}` dropped the caveat: {rendered}"
        );
        assert!(
            !rendered.contains("Fix intent:"),
            "`--format {format}` is a summary table and has no fix-intent line: {rendered}"
        );
    }

    for format in ["review-github", "review-gitlab"] {
        let rendered = run_format(&root, format);
        assert!(
            rendered.contains("Fix intent:"),
            "`--format {format}` is the surface that carries the fix-intent line: {rendered}"
        );
    }
}

/// `skipped_low_confidence_exports` counts FILES, not withheld exports. Three
/// withheld exports across two files report `2`, and the entry naming each file
/// carries no export name and no line, so a consumer cannot recover which
/// exports were withheld from this envelope.
#[test]
fn withheld_export_removals_are_counted_by_file() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "caveat-counts", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/a.ts"),
        "export const a1 = 1;\nexport const a2 = 2;\nexport const aKeep = 3;\n",
    )
    .expect("write first library");
    std::fs::write(
        root.join("src/b.ts"),
        "export const b1 = 1;\nexport const bKeep = 2;\n",
    )
    .expect("write second library");

    let mut oversized = String::from(
        "import { a1, a2 } from \"./a\";\nimport { b1 } from \"./b\";\nexport const pad = [\n",
    );
    while oversized.len() < 2 * 1024 * 1024 {
        oversized.push_str("  \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n");
    }
    oversized.push_str("];\n");
    std::fs::write(root.join("src/huge.ts"), oversized).expect("write oversized importer");
    std::fs::write(
        root.join("src/index.ts"),
        "import \"./huge\";\nimport { aKeep } from \"./a\";\nimport { bKeep } from \"./b\";\n\nexport const run = (): number => aKeep + bKeep;\n",
    )
    .expect("write entry module");

    let envelope: serde_json::Value = serde_json::from_str(&run_fallow(
        &root,
        &root,
        None,
        &["fix", "--dry-run", "--format", "json"],
    ))
    .expect("fix envelope");

    assert_eq!(
        envelope["skipped_low_confidence_exports"], 2,
        "three withheld exports across two files count as two files: {envelope}"
    );

    let entries = envelope["fixes"].as_array().expect("fix entries");
    assert_eq!(entries.len(), 2, "one entry per file: {envelope}");
    for entry in entries {
        assert_eq!(entry["type"], "skipped", "{entry}");
        assert_eq!(
            entry["skip_reason"], "low_confidence_incomplete_analysis",
            "{entry}"
        );
        assert!(
            entry.get("name").is_none() && entry.get("line").is_none(),
            "a withheld export removal names no export and no line: {entry}"
        );
    }
    assert_eq!(envelope["skipped_low_confidence_dependencies"], 0);
    assert_eq!(envelope["skipped_low_confidence_members"], 0);
}

/// The review renderers read the finding's source line through `FALLOW_ROOT`,
/// which does not follow `--root`. Without it a `--root` run renders neither
/// the edit block nor the note that replaces one, so the compatibility entry
/// has to name the variable rather than promise the note unconditionally.
#[test]
fn the_withheld_fix_note_needs_fallow_root_under_an_external_root() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_skipped_export_importer_project(&root);
    let elsewhere = dir.path();

    let without: serde_json::Value = serde_json::from_str(&run_fallow(
        &root,
        elsewhere,
        None,
        &["check", "--format", "review-github"],
    ))
    .expect("review envelope");
    let body = without["comments"][0]["body"]
        .as_str()
        .expect("inline comment body");
    assert!(
        body.contains("caveat: incomplete import graph"),
        "the caveat text does not depend on reading the source: {body}"
    );
    assert!(
        !body.contains("```suggestion") && !body.contains("No one-click fix offered"),
        "neither the block nor its replacement renders without `FALLOW_ROOT`: {body}"
    );

    let with: serde_json::Value = serde_json::from_str(&run_fallow(
        &root,
        elsewhere,
        Some(&root),
        &["check", "--format", "review-github"],
    ))
    .expect("review envelope");
    let body = with["comments"][0]["body"]
        .as_str()
        .expect("inline comment body");
    assert!(
        body.contains("No one-click fix offered") && !body.contains("```suggestion"),
        "with `FALLOW_ROOT` the note replaces the withheld block: {body}"
    );
}
