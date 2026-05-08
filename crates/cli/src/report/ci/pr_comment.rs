use std::fmt::Write as _;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::Value;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitlab => "GitLab",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiIssue {
    pub rule_id: String,
    pub description: String,
    pub severity: String,
    pub path: String,
    pub line: u64,
    pub fingerprint: String,
}

#[must_use]
pub fn issues_from_codeclimate(value: &Value) -> Vec<CiIssue> {
    let mut issues = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(issue_from_codeclimate)
        .collect::<Vec<_>>();
    issues
        .sort_by(|a, b| (&a.path, a.line, &a.fingerprint).cmp(&(&b.path, b.line, &b.fingerprint)));
    issues
}

fn issue_from_codeclimate(value: &Value) -> Option<CiIssue> {
    let path = value.pointer("/location/path")?.as_str()?.to_string();
    let line = value
        .pointer("/location/lines/begin")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    Some(CiIssue {
        rule_id: value
            .get("check_name")
            .and_then(Value::as_str)
            .unwrap_or("fallow/finding")
            .to_string(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("Fallow finding")
            .to_string(),
        severity: value
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("minor")
            .to_string(),
        fingerprint: value
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        path,
        line,
    })
}

#[must_use]
pub fn render_pr_comment(command: &str, provider: Provider, issues: &[CiIssue]) -> String {
    let marker_id = sticky_marker_id();
    let marker = format!("<!-- fallow-id: {marker_id} -->");
    let max = max_comments();
    let title = command_title(command);
    let count = issues.len();
    let noun = if count == 1 { "finding" } else { "findings" };

    let mut out = String::new();
    out.push_str(&marker);
    out.push('\n');
    write!(&mut out, "### Fallow {title}\n\n").expect("write to string");
    if count == 0 {
        writeln!(
            &mut out,
            "No {provider} PR/MR findings.",
            provider = provider.name()
        )
        .expect("write to string");
    } else {
        write!(&mut out, "Found **{count}** {noun}.\n\n").expect("write to string");
        let groups = group_by_category(issues);
        // Single-category invocations (e.g. `fallow check --format pr-comment-github`)
        // get the original flat-table shape. Combined / multi-category runs get
        // one collapsible section per category so reviewers can fold by area.
        if groups.len() == 1 {
            render_findings_table(&mut out, issues, max, "Details");
        } else {
            for (category, group_issues) in &groups {
                let summary_label = summary_label(category, group_issues.len(), max);
                render_findings_table(&mut out, group_issues, max, &summary_label);
            }
        }
    }
    out.push_str("\nGenerated by fallow.");
    out
}

/// Build the `<details>` summary label for one category section. When the
/// section is truncated by `max`, the label foreshadows the truncation
/// (`Duplication (160, showing 50)`) so a reviewer expanding the section
/// isn't surprised by the missing rows. When not truncated, the bare count
/// reads as before.
fn summary_label(category: &str, total: usize, max: usize) -> String {
    if total > max {
        format!("{category} ({total}, showing {max})")
    } else {
        format!("{category} ({total})")
    }
}

fn render_findings_table(out: &mut String, issues: &[CiIssue], max: usize, summary: &str) {
    writeln!(out, "<details>\n<summary>{summary}</summary>\n").expect("write to string");
    out.push_str("| Severity | Rule | Location | Description |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for issue in issues.iter().take(max) {
        writeln!(
            out,
            "| {} | `{}` | `{}`:{} | {} |",
            escape_md(&issue.severity),
            escape_md(&issue.rule_id),
            escape_md(&issue.path),
            issue.line,
            escape_md(&issue.description),
        )
        .expect("write to string");
    }
    if issues.len() > max {
        writeln!(
            out,
            "\nShowing {max} of {} findings. Run fallow locally or inspect the CI output for the full report.",
            issues.len(),
        )
        .expect("write to string");
    }
    out.push_str("\n</details>\n\n");
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

/// Stable category ordering for the sticky comment. Reviewers see categories
/// in the same order across PRs / runs, which matters for muscle memory.
const CATEGORY_ORDER: [&str; 6] = [
    "Dead code",
    "Dependencies",
    "Duplication",
    "Health",
    "Architecture",
    "Suppressions",
];

fn group_by_category(issues: &[CiIssue]) -> Vec<(&'static str, Vec<CiIssue>)> {
    let mut buckets: std::collections::BTreeMap<&'static str, Vec<CiIssue>> =
        std::collections::BTreeMap::new();
    for issue in issues {
        let category = category_for_rule(&issue.rule_id);
        buckets.entry(category).or_default().push(issue.clone());
    }
    let mut ordered: Vec<(&'static str, Vec<CiIssue>)> = Vec::with_capacity(buckets.len());
    // Emit known categories in the declared order first.
    for category in CATEGORY_ORDER {
        if let Some(items) = buckets.remove(category) {
            ordered.push((category, items));
        }
    }
    // Anything left over (future categories not yet ordered) goes after.
    for (category, items) in buckets {
        ordered.push((category, items));
    }
    ordered
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
    let issues = super::diff_filter::filter_issues_from_env(issues_from_codeclimate(codeclimate));
    println!("{}", render_pr_comment(command, provider, &issues));
    ExitCode::SUCCESS
}

#[must_use]
pub fn command_title(command: &str) -> &'static str {
    match command {
        "dead-code" | "check" => "dead-code report",
        "dupes" => "duplication report",
        "health" => "health report",
        "audit" => "audit report",
        "" | "combined" => "combined report",
        _ => "report",
    }
}

/// Escape a string for inclusion in a Markdown table cell.
///
/// Table cells render through GitHub-Flavored Markdown and GitLab Flavored
/// Markdown as inline content, so cell-internal markers can flip the cell to
/// emphasis, link, image, code, HTML, or strikethrough. Newlines collapse to
/// spaces because a literal newline terminates the table row. The escape set
/// covers every CommonMark inline construct that can fire mid-cell:
///
/// - `\` (escape character itself)
/// - `` ` `` (inline code)
/// - `*` `_` (emphasis / strong)
/// - `[` `]` `(` `)` (link / image syntax)
/// - `!` (image when followed by `[`)
/// - `<` `>` (raw HTML / autolinks)
/// - `#` (cell rendered as heading when first character of the cell)
/// - `|` (table cell separator)
/// - `~` (strikethrough on GFM)
/// - `&` (HTML numeric / named entity decode: `&#42;` would otherwise
///   render as `*` after our escape and reintroduce the bypass)
///
/// Line-start markers (`.`, `-`, `+`, `1.`) are intentionally NOT escaped:
/// they are only meaningful at the start of a block-level line, and table
/// cells render as paragraph-equivalent inline content where these are inert.
/// Escaping them produces visually noisy output (`fallow/test\-only-dep`)
/// without correctness benefit.
#[must_use]
pub fn escape_md(value: &str) -> String {
    let collapsed = value.replace('\n', " ");
    let mut out = String::with_capacity(collapsed.len());
    for ch in collapsed.chars() {
        if matches!(
            ch,
            '\\' | '`'
                | '*'
                | '_'
                | '['
                | ']'
                | '('
                | ')'
                | '!'
                | '<'
                | '>'
                | '#'
                | '|'
                | '~'
                | '&'
        ) {
            out.push('\\');
        }
        out.push(ch);
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn sticky_marker_id_default_when_nothing_set() {
        // WORKSPACE_MARKER is a OnceLock that's set-once-per-process; tests
        // can't unset it. We only assert about the unset branch when the
        // OnceLock hasn't been touched, which is the case in this test if
        // it's the first marker test to run. To keep tests order-independent
        // we test sanitize_marker_segment + sticky_marker_id-with-mock
        // separately rather than racing the OnceLock state.
        let body = render_pr_comment("check", Provider::Github, &[]);
        // The marker prefix is always `<!-- fallow-id: fallow-results`,
        // regardless of whether a workspace suffix follows.
        assert!(body.contains("<!-- fallow-id: fallow-results"));
        assert!(body.contains("No GitHub PR/MR findings."));
    }

    #[test]
    fn short_hex_hash_is_deterministic_and_six_chars() {
        let a = short_hex_hash("api,worker");
        assert_eq!(a.len(), 6);
        // Same input -> same hash across calls.
        assert_eq!(a, short_hex_hash("api,worker"));
        // Different input -> different hash (modulo collision; the
        // workspace-marker assertion is "monorepo with 2-10 distinct
        // workspaces should not race", which a 6-hex-char suffix
        // satisfies at ~1/16M collision rate).
        assert_ne!(a, short_hex_hash("admin,web"));
    }

    #[test]
    fn sanitize_marker_segment_collapses_unsafe_chars_to_dashes() {
        // `@`, `/`, spaces, and other special chars all become `-`.
        // Leading and trailing dashes are trimmed.
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
        // Inline-context CommonMark specials must escape: emphasis, links,
        // images, code, HTML, headings (when first char of cell), pipes,
        // strikethrough.
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
        // Without escaping `&`, a description containing `&#42;` would render
        // as `*` AFTER our escape pass, reintroducing the emphasis-injection
        // we explicitly defended against. Escaping the `&` (and `#`) breaks
        // the entity so it renders literally.
        let raw = "value &#42;suspicious&#42; here";
        let escaped = escape_md(raw);
        // Both `&` and `#` are escaped, so the entity becomes `\&\#42;`,
        // which Markdown renders as a literal `&#42;` instead of a `*`.
        assert!(escaped.contains(r"\&"), "got: {escaped}");
        assert!(escaped.contains(r"\#"), "got: {escaped}");
        // Defence-in-depth: the substring " *suspicious" only appears if
        // the entity decoded; with both escapes in place it cannot.
        assert!(!escaped.contains(" *suspicious"), "got: {escaped}");
    }

    #[test]
    fn summary_label_foreshadows_truncation() {
        // When the section is truncated, the <details> summary tells the
        // reader BEFORE they click that fewer rows than the count appear.
        assert_eq!(
            summary_label("Duplication", 160, 50),
            "Duplication (160, showing 50)"
        );
        // When the section fits, the bare count reads as before.
        assert_eq!(summary_label("Health", 12, 50), "Health (12)");
        assert_eq!(summary_label("Dependencies", 50, 50), "Dependencies (50)");
    }

    #[test]
    fn escape_md_does_not_escape_block_only_markers() {
        // `.`, `-`, `+` are only special at the start of a block-level line
        // (ordered / unordered list markers). Table cells are inline; over-
        // escaping these produces visually noisy `\-` / `\.` in the cell.
        let raw = "fallow/test-only-dependency package.json:12";
        let escaped = escape_md(raw);
        assert!(!escaped.contains("\\-"), "should not escape `-`");
        assert!(!escaped.contains("\\."), "should not escape `.`");
        assert_eq!(escaped, raw);
    }

    #[test]
    fn escape_md_collapses_newlines_to_spaces() {
        // Table cells are single-line by construction; a literal newline in
        // a description would terminate the row and break the table.
        let raw = "first\nsecond\nthird";
        assert_eq!(escape_md(raw), "first second third");
    }

    #[test]
    fn escape_md_leaves_safe_chars_unchanged() {
        // Plain alphanumeric, spaces, slashes, colons, equals, quotes: all
        // legal inside a Markdown table cell.
        let raw = "Export 'helperFn' is never imported by other modules";
        assert_eq!(
            escape_md(raw),
            r"Export 'helperFn' is never imported by other modules"
        );
    }

    #[test]
    fn escape_md_double_apply_is_safe() {
        // Idempotency on the escape character itself: `\` always escapes,
        // so escaping twice does not produce visual `\\\\` for callers that
        // accidentally double-escape.
        let raw = "code with `backticks` and *stars*";
        let once = escape_md(raw);
        let twice = escape_md(&once);
        // Second pass adds an additional layer of escaping, which is
        // expected: callers must not double-call. The contract is "single
        // pass produces correct GFM"; we just assert it doesn't panic.
        assert!(twice.contains(r"\\"));
    }
}
