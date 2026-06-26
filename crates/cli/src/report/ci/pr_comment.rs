use crate::report::sink::outln;
use fallow_output::CodeClimateIssue;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::Value;

pub use fallow_output::{
    CiIssue, CiProvider as Provider, issues_from_codeclimate, issues_from_codeclimate_issues,
};
#[cfg(test)]
use fallow_output::{escape_md, is_project_level_rule};

/// Workspace name, set once by `main()` when the binary is invoked with
/// `--workspace <name>`. Read by `sticky_marker_id` to auto-suffix the
/// sticky-comment marker per workspace, which keeps parallel per-workspace
/// jobs from racing each other's sticky body on the same PR/MR.
///
/// `OnceLock` gives us safe cross-function read-after-set without env-var
/// indirection. Only main writes; readers always observe the post-CLI-parse
/// state.
static WORKSPACE_MARKER: OnceLock<String> = OnceLock::new();

/// Set the workspace marker from a `--workspace` selection list.
///
/// Single workspace -> the name itself, sanitised for marker grammar.
/// N>1 workspaces -> a stable 6-char hex hash of the sorted, comma-joined
/// list, prefixed with `w-`. Sort + join is deterministic so the same
/// selection produces the same suffix across runs; two jobs with disjoint
/// selections get distinct markers and don't race.
#[allow(
    dead_code,
    reason = "called from main.rs bin target; lib target sees no caller"
)]
pub fn set_workspace_marker_from_list(values: &[String]) {
    let trimmed: Vec<&str> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if trimmed.is_empty() {
        return;
    }
    let marker = if let [single] = trimmed.as_slice() {
        (*single).to_owned()
    } else {
        let mut sorted = trimmed.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        sorted.sort();
        let joined = sorted.join(",");
        format!("w-{}", short_hex_hash(&joined))
    };
    let _ = WORKSPACE_MARKER.set(marker);
}

/// 6-char FNV-1a hex digest. Stable across Rust versions (FNV is content-
/// determined), short enough for a marker suffix, wide enough that the
/// chance of two real-world workspace selections colliding is ~1/16M.
fn short_hex_hash(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:06x}", (hash & 0x00ff_ffff) as u32)
}

#[must_use]
pub fn render_pr_comment(command: &str, provider: Provider, issues: &[CiIssue]) -> String {
    fallow_output::render_pr_comment(&fallow_output::PrCommentRenderInput {
        command,
        provider,
        issues,
        marker_id: sticky_marker_id(),
        max_comments: max_comments(),
        category_for_rule: &category_for_rule,
    })
}

/// Map a fallow rule id to its category for sticky-comment grouping.
///
/// Single source of truth lives on `RuleDef::category` in `explain.rs`. This
/// helper does the lookup so callers don't need to know about the registry;
/// the look-up-then-fallback shape also keeps the renderer working for
/// rules a downstream consumer added without registering (rare; produces
/// the conservative "Dead code" default).
#[must_use]
pub fn category_for_rule(rule_id: &str) -> &'static str {
    crate::explain::rule_by_id(rule_id).map_or("Dead code", |def| def.category)
}

fn max_comments() -> usize {
    std::env::var("FALLOW_MAX_COMMENTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50)
}

/// Compute the sticky-comment marker id. Precedence (highest first):
///
/// 1. `FALLOW_COMMENT_ID` set by the user explicitly: use as-is.
/// 2. `WORKSPACE_MARKER` populated by `main()` from `--workspace <name>`:
///    suffix the default to avoid colliding with a sibling per-workspace
///    job's sticky on the same PR/MR.
/// 3. Plain `fallow-results`.
///
/// The collision case (2) is the common monorepo shape: parallel jobs each
/// run fallow scoped to one workspace package and post their own sticky.
/// Without a per-workspace suffix every job edits the same marker, racing
/// each other's bodies on every CI re-run.
fn sticky_marker_id() -> String {
    if let Ok(value) = std::env::var("FALLOW_COMMENT_ID")
        && !value.trim().is_empty()
    {
        return value;
    }
    let suffix = WORKSPACE_MARKER
        .get()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(sanitize_marker_segment);
    match suffix {
        Some(workspace) => format!("fallow-results-{workspace}"),
        None => "fallow-results".to_owned(),
    }
}

/// Strip characters that would break the HTML-comment marker. The marker
/// shape is `<!-- fallow-id: <id> -->`; `<`, `>`, and `--` are reserved by
/// the HTML comment grammar, and whitespace would split the id when the
/// reader scans for it.
fn sanitize_marker_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[must_use]
pub fn print_pr_comment(command: &str, provider: Provider, codeclimate: &Value) -> ExitCode {
    let issues =
        super::diff_filter::filter_issues_for_summary(issues_from_codeclimate(codeclimate));
    print_pr_comment_from_ci_issues(command, provider, &issues)
}

#[must_use]
pub fn print_pr_comment_from_codeclimate_issues(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
) -> ExitCode {
    let issues =
        super::diff_filter::filter_issues_for_summary(issues_from_codeclimate_issues(codeclimate));
    print_pr_comment_from_ci_issues(command, provider, &issues)
}

#[must_use]
fn print_pr_comment_from_ci_issues(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
) -> ExitCode {
    outln!("{}", render_pr_comment(command, provider, issues));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{
        CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation, CodeClimateSeverity,
    };

    #[test]
    fn extracts_issues_from_codeclimate() {
        let value = serde_json::json!([{
            "check_name": "fallow/unused-export",
            "description": "Export x is never imported",
            "severity": "minor",
            "fingerprint": "abc",
            "location": { "path": "src/a.ts", "lines": { "begin": 7 } }
        }]);
        let issues = issues_from_codeclimate(&value);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "src/a.ts");
        assert_eq!(issues[0].line, 7);
    }

    #[test]
    fn typed_codeclimate_issues_extract_like_json_codeclimate() {
        let severities = [
            (CodeClimateSeverity::Info, "info"),
            (CodeClimateSeverity::Minor, "minor"),
            (CodeClimateSeverity::Major, "major"),
            (CodeClimateSeverity::Critical, "critical"),
            (CodeClimateSeverity::Blocker, "blocker"),
        ];
        let typed = severities
            .iter()
            .enumerate()
            .map(|(index, (severity, _))| CodeClimateIssue {
                kind: CodeClimateIssueKind::Issue,
                check_name: format!("fallow/rule-{index}"),
                description: format!("Finding {index}"),
                categories: vec!["Complexity".to_owned()],
                severity: *severity,
                fingerprint: format!("fp-{index}"),
                location: CodeClimateLocation {
                    path: format!("src/{index}.ts"),
                    lines: CodeClimateLines {
                        begin: u32::try_from(index + 1).expect("small fixture index"),
                    },
                },
                owner: None,
                group: None,
            })
            .collect::<Vec<_>>();
        let value = serde_json::to_value(&typed).expect("typed fixture serializes");

        assert_eq!(
            issues_from_codeclimate_issues(&typed),
            issues_from_codeclimate(&value)
        );
        let typed_labels = issues_from_codeclimate_issues(&typed)
            .into_iter()
            .map(|issue| issue.severity)
            .collect::<Vec<_>>();
        let expected_labels = severities
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(typed_labels, expected_labels);
    }

    #[test]
    fn sticky_marker_id_default_when_nothing_set() {
        let body = render_pr_comment("check", Provider::Github, &[]);
        assert!(body.contains("<!-- fallow-id: fallow-results"));
        assert!(body.contains("No GitHub PR/MR findings."));
    }

    #[test]
    fn short_hex_hash_is_deterministic_and_six_chars() {
        let a = short_hex_hash("api,worker");
        assert_eq!(a.len(), 6);
        assert_eq!(a, short_hex_hash("api,worker"));
        assert_ne!(a, short_hex_hash("admin,web"));
    }

    #[test]
    fn sanitize_marker_segment_collapses_unsafe_chars_to_dashes() {
        assert_eq!(sanitize_marker_segment("@fallow/runtime"), "fallow-runtime");
        assert_eq!(
            sanitize_marker_segment("packages/web ui"),
            "packages-web-ui"
        );
        assert_eq!(sanitize_marker_segment("plain"), "plain");
        assert_eq!(
            sanitize_marker_segment("--leading-trailing--"),
            "leading-trailing"
        );
    }

    #[test]
    fn escape_md_escapes_inline_commonmark_specials() {
        let raw = "foo*bar_baz [a](u) `c` <h> #x !i ~s | p";
        let escaped = escape_md(raw);
        for ch in [
            '*', '_', '[', ']', '(', ')', '`', '<', '>', '#', '!', '~', '|',
        ] {
            let raw_count = raw.chars().filter(|c| c == &ch).count();
            let escaped_count = escaped.matches(&format!("\\{ch}")).count();
            assert_eq!(
                raw_count, escaped_count,
                "char {ch:?}: raw {raw_count} occurrences, escaped {escaped_count} in {escaped:?}"
            );
        }
    }

    #[test]
    fn escape_md_escapes_ampersand_to_block_numeric_entity_bypass() {
        let raw = "value &#42;suspicious&#42; here";
        let escaped = escape_md(raw);
        assert!(escaped.contains(r"\&"), "got: {escaped}");
        assert!(escaped.contains(r"\#"), "got: {escaped}");
        assert!(!escaped.contains(" *suspicious"), "got: {escaped}");
    }

    #[test]
    fn summary_label_foreshadows_truncation() {
        assert_eq!(
            fallow_output::summary_label("Duplication", 160, 50),
            "Duplication (160, showing 50)"
        );
        assert_eq!(
            fallow_output::summary_label("Health", 12, 50),
            "Health (12)"
        );
        assert_eq!(
            fallow_output::summary_label("Dependencies", 50, 50),
            "Dependencies (50)"
        );
    }

    #[test]
    fn escape_md_does_not_escape_block_only_markers() {
        let raw = "fallow/test-only-dependency package.json:12";
        let escaped = escape_md(raw);
        assert!(!escaped.contains("\\-"), "should not escape `-`");
        assert!(!escaped.contains("\\."), "should not escape `.`");
        assert_eq!(escaped, raw);
    }

    #[test]
    fn escape_md_collapses_newlines_to_spaces() {
        let raw = "first\nsecond\nthird";
        assert_eq!(escape_md(raw), "first second third");
    }

    #[test]
    fn escape_md_leaves_safe_chars_unchanged() {
        let raw = "Export 'helperFn' is never imported by other modules";
        assert_eq!(
            escape_md(raw),
            r"Export 'helperFn' is never imported by other modules"
        );
    }

    #[test]
    fn is_project_level_rule_covers_config_anchored_dependency_findings() {
        for rule_id in fallow_output::PROJECT_LEVEL_RULE_IDS {
            assert!(
                is_project_level_rule(rule_id),
                "{rule_id} must be project-level"
            );
        }
        for rule_id in [
            "fallow/unused-file",
            "fallow/unused-export",
            "fallow/unused-type",
            "fallow/unused-enum-member",
            "fallow/unused-class-member",
            "fallow/unused-store-member",
            "fallow/unresolved-import",
            "fallow/unlisted-dependency",
            "fallow/duplicate-export",
            "fallow/circular-dependency",
            "fallow/re-export-cycle",
            "fallow/boundary-violation",
            "fallow/stale-suppression",
            "fallow/private-type-leak",
            "fallow/high-complexity",
            "fallow/high-crap-score",
        ] {
            assert!(
                !is_project_level_rule(rule_id),
                "{rule_id} must NOT be project-level"
            );
        }
    }

    #[test]
    fn project_level_rule_ids_each_register_in_explain_registry() {
        for rule_id in fallow_output::PROJECT_LEVEL_RULE_IDS {
            assert!(
                crate::explain::rule_by_id(rule_id).is_some(),
                "{rule_id} listed in PROJECT_LEVEL_RULE_IDS but not in explain registry"
            );
        }
    }

    #[test]
    fn escape_md_double_apply_is_safe() {
        let raw = "code with `backticks` and *stars*";
        let once = escape_md(raw);
        let twice = escape_md(&once);
        assert!(twice.contains(r"\\"));
    }
}
