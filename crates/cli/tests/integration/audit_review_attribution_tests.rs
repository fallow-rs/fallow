//! Inline review comments of `audit --gate new-only` for dead-code,
//! duplication and styling findings.
//!
//! Each comment states if the change introduced the finding. A comment on an
//! inherited finding states that `--gate new-only` does not count it. The
//! live audit and `fallow report --from` on the saved audit JSON give the
//! same text, for GitHub and for GitLab (issue #4195).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

const CONFIG: &str = r#"{"entry": ["src/index.ts"], "rules": {"css-selector-complexity": "error"}, "duplicates": {"minTokens": 20, "minLines": 3}, "health": {"maxCrap": 999999999}}"#;

const INHERITED_LABEL: &str = "**error** · inherited · not gated (`--gate new-only`) `";
const INHERITED_WARN_LABEL: &str = "**warn** · inherited · not gated (`--gate new-only`) `";
const INTRODUCED_LABEL: &str = "**error** · introduced `";
const INTRODUCED_WARN_LABEL: &str = "**warn** · introduced `";

/// A function that the duplication analysis finds in each file that holds
/// it. `statement` sets the shape of the loop body, so two different
/// statements give two different clone groups.
fn clone_block(name: &str, statement: &str) -> String {
    let body = format!("  for (const item of items) {{ {statement} }}\n").repeat(8);
    format!(
        "export function {name}(items: number[]): number {{\n  let total = 0;\n{body}  return total;\n}}\n"
    )
}

const OLD_STATEMENT: &str = "total += item * 3 - 1;";
const NEW_STATEMENT: &str = "if (item > total) { total = item; } else { total -= 1; }";

fn write(dir: &Path, path: &str, content: &str) {
    let full = dir.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, content).unwrap();
}

/// The base branch holds one inherited finding per family. The head branch
/// adds one introduced finding per family.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    write(
        dir,
        "package.json",
        r#"{"name": "audit-review", "private": true}"#,
    );
    write(dir, ".fallowrc.json", CONFIG);
    write(
        dir,
        "src/index.ts",
        "import { used } from './util';\nimport { oldA } from './old-a';\nimport { oldB } from './old-b';\nimport './styles.css';\nexport const value = used + oldA([1]) + oldB([2]);\n",
    );
    write(
        dir,
        "src/util.ts",
        "export const used = 1;\nexport const legacyUnused = 2;\n",
    );
    write(dir, "src/old-a.ts", &clone_block("oldA", OLD_STATEMENT));
    write(dir, "src/old-b.ts", &clone_block("oldB", OLD_STATEMENT));
    write(
        dir,
        "src/styles.css",
        "#app .legacy .title { color: red; }\n",
    );
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    git(dir, &["checkout", "-b", "head"]);
    write(
        dir,
        "src/index.ts",
        "import { used } from './util';\nimport { oldA } from './old-a';\nimport { oldB } from './old-b';\nimport { newA } from './new-a';\nimport { newB } from './new-b';\nimport './styles.css';\nexport const value = used + oldA([1]) + oldB([2]) + newA([3]) + newB([4]);\n",
    );
    write(
        dir,
        "src/util.ts",
        "export const used = 1;\nexport const legacyUnused = 2;\nexport const freshUnused = 3;\n",
    );
    // An inline comment needs a changed file, so the head commit adds one
    // line after the inherited clone.
    write(
        dir,
        "src/old-a.ts",
        &format!("{}// changed\n", clone_block("oldA", OLD_STATEMENT)),
    );
    write(dir, "src/new-a.ts", &clone_block("newA", NEW_STATEMENT));
    write(dir, "src/new-b.ts", &clone_block("newB", NEW_STATEMENT));
    write(
        dir,
        "src/styles.css",
        "#app .legacy .title { color: red; }\n#app .fresh .title { color: green; }\n",
    );
    commit_all(dir, "head");
    tmp
}

/// The paragraph about `rule` in the inline comment on `path` at `line`.
/// The comment merges the findings of one line, one paragraph each.
fn comment_body<'a>(review: &'a serde_json::Value, path: &str, line: u64, rule: &str) -> &'a str {
    let marker = format!("`{rule}`");
    review["comments"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|comment| {
            let github = comment["path"] == path && comment["line"] == line;
            let position = &comment["position"];
            let gitlab = position["new_path"] == path && position["new_line"] == line;
            github || gitlab
        })
        .filter_map(|comment| comment["body"].as_str())
        .flat_map(|body| body.split("\n\n"))
        .find(|paragraph| paragraph.contains(&marker))
        .unwrap_or_else(|| panic!("no {rule} comment at {path}:{line}: {review:#}"))
}

fn assert_label(body: &str, expected: &str, context: &str) {
    assert!(body.starts_with(expected), "{context}: {body}");
}

#[test]
fn review_comments_carry_the_attribution_for_every_finding_family() {
    let tmp = fixture();
    let dir = tmp.path();
    let root = dir.to_str().unwrap();
    let json = parse_json(&run_fallow_raw(&[
        "audit",
        "--root",
        root,
        "--base",
        "main",
        "--format",
        "json",
        "--quiet",
        "--no-cache",
    ]));
    assert_eq!(json["attribution"]["gate"], "new-only", "{json:#}");
    let saved = dir.join("saved-audit.json");
    fs::write(&saved, serde_json::to_string(&json).unwrap()).unwrap();
    let saved_path = saved.to_str().unwrap();
    for format in ["review-github", "review-gitlab"] {
        let live = parse_json(&run_fallow_raw(&[
            "audit",
            "--root",
            root,
            "--base",
            "main",
            "--format",
            format,
            "--quiet",
            "--no-cache",
        ]));
        let from_saved = parse_json(&run_fallow_raw(&[
            "report", "--from", saved_path, "--root", root, "--format", format, "--quiet",
        ]));
        for (source, review) in [("live", &live), ("saved", &from_saved)] {
            let context = format!("{format} {source}");
            let unused = "fallow/unused-export";
            assert_label(
                comment_body(review, "src/util.ts", 2, unused),
                INHERITED_LABEL,
                &context,
            );
            assert_label(
                comment_body(review, "src/util.ts", 3, unused),
                INTRODUCED_LABEL,
                &context,
            );
            let clone = "fallow/code-duplication";
            assert_label(
                comment_body(review, "src/old-a.ts", 1, clone),
                INHERITED_WARN_LABEL,
                &context,
            );
            assert_label(
                comment_body(review, "src/new-a.ts", 1, clone),
                INTRODUCED_WARN_LABEL,
                &context,
            );
            let styling = "fallow/css-selector-complexity";
            assert_label(
                comment_body(review, "src/styles.css", 1, styling),
                INHERITED_LABEL,
                &context,
            );
            assert_label(
                comment_body(review, "src/styles.css", 2, styling),
                INTRODUCED_LABEL,
                &context,
            );
        }
    }
}
