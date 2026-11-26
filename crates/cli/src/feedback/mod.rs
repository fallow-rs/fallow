//! `fallow feedback`: file a feedback report as a GitHub issue on the Fallow
//! repository.
//!
//! A run without `--yes` sends nothing. It prints the issue preview and a
//! prefilled `issues/new` link. With `--yes`, the GitHub CLI (`gh`) creates the
//! issue under the account of the person who runs the command. When `gh` is
//! not available, the run prints the link and exits 3.
//!
//! The issue is public. The report holds only the text that the caller gives,
//! plus the Fallow version and the operating system. Home directory paths
//! become `~`, and token-shaped strings become `[redacted]`.

mod gh;
mod redact;

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, ValueEnum};
use fallow_config::OutputFormat;
use serde::Deserialize;

use crate::exit_codes::RESOURCE_UNAVAILABLE_EXIT_CODE;
use crate::json_style::JsonStyle;

/// The repository that receives the issues.
pub const FEEDBACK_REPOSITORY: &str = "fallow-rs/fallow";
const NEW_ISSUE_URL: &str = "https://github.com/fallow-rs/fallow/issues/new";
/// The marker that lets maintainers find these issues.
const BODY_MARKER: &str = "<!-- fallow-feedback v1 -->";
/// Per-field cap. GitHub limits an issue body to 65,536 characters.
const MAX_FIELD_CHARS: usize = 4_000;
const MAX_TITLE_SUMMARY_CHARS: usize = 80;
const DIGEST_BYTES: usize = 8;
/// A larger report is not a feedback note.
const MAX_INPUT_BYTES: u64 = 1024 * 1024;
/// Browsers and GitHub refuse longer `issues/new` links.
const MAX_URL_BYTES: usize = 8_000;
const FENCE_RESERVE_BYTES: usize = 64;
const URL_TRUNCATION_NOTE: &str =
    "\n\n_The link holds a shortened report. The full text is in the output of `fallow feedback`._";
const REPORT_HEADING: &str = "Input";
const SUBMIT_HEADING: &str = "Run";

/// What kind of feedback the report gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackKind {
    /// A command failed, crashed or gave wrong output.
    Bug,
    /// Fallow reported code, a dependency or a file that is in use.
    #[serde(alias = "false_positive")]
    FalsePositive,
    /// Fallow missed a problem that is in the code.
    #[serde(alias = "false_negative")]
    FalseNegative,
    /// A task took more steps than it should, or the output was unclear.
    Friction,
    /// A capability that Fallow does not have.
    Feature,
    /// The docs or the help text were wrong or missing.
    Docs,
}

impl FeedbackKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Bug => "bug",
            Self::FalsePositive => "false positive",
            Self::FalseNegative => "false negative",
            Self::Friction => "friction",
            Self::Feature => "feature request",
            Self::Docs => "docs",
        }
    }
}

/// How much the problem cost the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeedbackImpact {
    /// The task could not be completed.
    Blocked,
    /// The task was completed with a workaround.
    #[serde(alias = "worked_around")]
    WorkedAround,
    /// The problem cost little time.
    Minor,
}

impl FeedbackImpact {
    const fn label(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::WorkedAround => "worked around",
            Self::Minor => "minor",
        }
    }
}

/// Arguments of `fallow feedback`.
#[derive(Args, Debug, Default, Clone)]
pub struct FeedbackArgs {
    /// A short note. The first line becomes the issue title.
    pub message: Option<String>,

    /// What kind of feedback this is (default: friction).
    #[arg(long = "type", value_enum, help_heading = REPORT_HEADING)]
    pub kind: Option<FeedbackKind>,

    /// How much the problem cost.
    #[arg(long, value_enum, help_heading = REPORT_HEADING)]
    pub impact: Option<FeedbackImpact>,

    /// The task that you tried to do.
    #[arg(long, allow_hyphen_values = true, help_heading = REPORT_HEADING)]
    pub task: Option<String>,

    /// The Fallow command that you ran.
    #[arg(long, allow_hyphen_values = true, help_heading = REPORT_HEADING)]
    pub command: Option<String>,

    /// The result that you expected.
    #[arg(long, allow_hyphen_values = true, help_heading = REPORT_HEADING)]
    pub expected: Option<String>,

    /// The result that you got.
    #[arg(long, allow_hyphen_values = true, help_heading = REPORT_HEADING)]
    pub actual: Option<String>,

    /// The name of the coding agent that writes the report, such as `claude-code`.
    #[arg(long, allow_hyphen_values = true, help_heading = REPORT_HEADING)]
    pub agent: Option<String>,

    /// Read the report from a JSON file, or from stdin with `-`. A flag
    /// overrides the same field in the file.
    #[arg(long, value_name = "FILE", help_heading = REPORT_HEADING)]
    pub input_json: Option<PathBuf>,

    /// Create the issue with the GitHub CLI (`gh`). Without this flag the
    /// command prints a preview and a prefilled link, and sends nothing.
    #[arg(long, conflicts_with = "dry_run", help_heading = SUBMIT_HEADING)]
    pub yes: bool,

    /// Create the issue only when the report still has this digest, the
    /// `feedback.digest` of the preview. A changed report exits 2.
    #[arg(long, value_name = "DIGEST", requires = "yes", help_heading = SUBMIT_HEADING)]
    pub confirm: Option<String>,

    /// Print the preview and the prefilled link only. This is the default.
    #[arg(long, help_heading = SUBMIT_HEADING)]
    pub dry_run: bool,

    /// Leave out the Fallow version and the operating system.
    #[arg(long, help_heading = SUBMIT_HEADING)]
    pub no_diagnostics: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct JsonReport {
    message: Option<String>,
    #[serde(rename = "type")]
    kind: Option<FeedbackKind>,
    impact: Option<FeedbackImpact>,
    task: Option<String>,
    command: Option<String>,
    expected: Option<String>,
    actual: Option<String>,
    agent: Option<JsonAgent>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonAgent {
    Name(String),
    Detail(AgentDetail),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentDetail {
    name: Option<String>,
    #[serde(default)]
    commands: Vec<String>,
}

/// A complete report after the flags and the JSON input merge.
#[derive(Debug, Default, PartialEq, Eq)]
struct Report {
    message: String,
    kind: Option<FeedbackKind>,
    impact: Option<FeedbackImpact>,
    task: Option<String>,
    command: Option<String>,
    expected: Option<String>,
    actual: Option<String>,
    agent: Option<String>,
    agent_commands: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Issue {
    title: String,
    body: String,
    digest: String,
    url: String,
    url_truncated: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Preview,
    Created { issue_url: String },
    Manual { reason: String },
}

/// Run `fallow feedback`.
pub fn run(args: &FeedbackArgs, output: OutputFormat, json_style: JsonStyle) -> ExitCode {
    let report = match read_report(args) {
        Ok(report) => report,
        Err(message) => {
            return crate::error::emit_input_error_with_style(&message, output, json_style);
        }
    };
    let diagnostics = diagnostics_enabled(args.no_diagnostics, &|name| std::env::var(name).ok());
    let home = home_text(&fallow_process::HostPaths::from_env());
    let issue = build_issue(&report, diagnostics, home.as_deref());
    if let Some(expected) = &args.confirm
        && expected.trim() != issue.digest
    {
        let message = format!(
            "the report changed since the preview (digest {}, expected {}); show the new preview to the user",
            issue.digest,
            expected.trim()
        );
        return crate::error::emit_input_error_with_style(&message, output, json_style);
    }
    let outcome = if args.yes {
        match gh::create_issue(&issue.title, &issue.body) {
            Ok(issue_url) => Outcome::Created { issue_url },
            Err(reason) => Outcome::Manual { reason },
        }
    } else {
        Outcome::Preview
    };
    let code = match outcome {
        Outcome::Manual { .. } => ExitCode::from(RESOURCE_UNAVAILABLE_EXIT_CODE),
        Outcome::Preview | Outcome::Created { .. } => ExitCode::SUCCESS,
    };
    if matches!(output, OutputFormat::Json) {
        let value = to_json(&issue, &outcome, diagnostics);
        let emitted = crate::report::emit_report_json(&value, "feedback", json_style);
        return if emitted == ExitCode::SUCCESS {
            code
        } else {
            emitted
        };
    }
    print_human(&issue, &outcome);
    code
}

fn read_report(args: &FeedbackArgs) -> Result<Report, String> {
    let json = match &args.input_json {
        Some(path) => parse_json_input(&read_input(path)?)?,
        None => JsonReport::default(),
    };
    merge(args, json)
}

fn read_input(path: &Path) -> Result<String, String> {
    if path.as_os_str() == "-" {
        return read_capped(std::io::stdin(), "stdin");
    }
    let file = std::fs::File::open(path).map_err(|err| {
        format!(
            "cannot read the feedback report from {}: {err}",
            path.display()
        )
    })?;
    read_capped(file, &path.display().to_string())
}

fn read_capped(reader: impl std::io::Read, source: &str) -> Result<String, String> {
    let mut text = String::new();
    reader
        .take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| format!("cannot read the feedback report from {source}: {err}"))?;
    if text.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!(
            "the feedback report from {source} is larger than {MAX_INPUT_BYTES} bytes"
        ));
    }
    Ok(text)
}

fn parse_json_input(text: &str) -> Result<JsonReport, String> {
    serde_json::from_str(text).map_err(|err| format!("the feedback report is not valid: {err}"))
}

fn merge(args: &FeedbackArgs, json: JsonReport) -> Result<Report, String> {
    let (json_agent, agent_commands) = match json.agent {
        Some(JsonAgent::Name(name)) => (Some(name), Vec::new()),
        Some(JsonAgent::Detail(detail)) => (detail.name, detail.commands),
        None => (None, Vec::new()),
    };
    let message = non_empty(args.message.clone())
        .or_else(|| non_empty(json.message))
        .ok_or_else(|| {
            "give a feedback message: `fallow feedback \"<note>\"`, or `message` in --input-json"
                .to_owned()
        })?;
    Ok(Report {
        message,
        kind: args.kind.or(json.kind),
        impact: args.impact.or(json.impact),
        task: non_empty(args.task.clone()).or_else(|| non_empty(json.task)),
        command: non_empty(args.command.clone()).or_else(|| non_empty(json.command)),
        expected: non_empty(args.expected.clone()).or_else(|| non_empty(json.expected)),
        actual: non_empty(args.actual.clone()).or_else(|| non_empty(json.actual)),
        agent: non_empty(args.agent.clone()).or_else(|| non_empty(json_agent)),
        agent_commands: agent_commands
            .into_iter()
            .filter(|command| !command.trim().is_empty())
            .collect(),
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// The report leaves out the diagnostics when the caller asks, and under the
/// `DO_NOT_TRACK` and `FALLOW_TELEMETRY_DISABLED` switches.
fn diagnostics_enabled(no_diagnostics: bool, env: &dyn Fn(&str) -> Option<String>) -> bool {
    if no_diagnostics {
        return false;
    }
    !["DO_NOT_TRACK", "FALLOW_TELEMETRY_DISABLED"]
        .iter()
        .any(|name| env(name).is_some_and(|value| env_value_is_truthy(&value)))
}

fn env_value_is_truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

/// The home folder as text, for the `~` replacement. A root home such as `/`
/// would turn every path into `~`, so it does not count.
fn home_text(host: &fallow_process::HostPaths) -> Option<String> {
    host.home
        .as_deref()
        .and_then(std::path::Path::to_str)
        .map(|home| home.trim_end_matches(['/', '\\']).to_owned())
        .filter(|home| home.len() > 1)
}

fn build_issue(report: &Report, diagnostics: bool, home: Option<&str>) -> Issue {
    let clean = |text: &str| clean_text(text, home);
    let title = build_title(report, &clean(&report.message));
    let body = build_body(report, diagnostics, &clean);
    let (url, url_truncated) = new_issue_url(&title, &body);
    let digest = digest(&title, &body);
    Issue {
        title,
        body,
        digest,
        url,
        url_truncated,
    }
}

/// A short digest of the issue text. The MCP tool passes the digest of the
/// preview back, so the issue is the text that the user saw.
fn digest(title: &str, body: &str) -> String {
    use sha2::Digest as _;
    let hash = sha2::Sha256::new()
        .chain_update(title.as_bytes())
        .chain_update([0])
        .chain_update(body.as_bytes())
        .finalize();
    hash[..DIGEST_BYTES]
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

fn build_title(report: &Report, message: &str) -> String {
    let summary: String = message.lines().next().unwrap_or_default().trim().to_owned();
    let summary = truncate_chars(&summary, MAX_TITLE_SUMMARY_CHARS);
    let source = if report.agent.is_some() {
        "Agent feedback"
    } else {
        "Feedback"
    };
    let kind = report.kind.unwrap_or(FeedbackKind::Friction).label();
    format!("{source} ({kind}): {summary}")
}

fn build_body(report: &Report, diagnostics: bool, clean: &dyn Fn(&str) -> String) -> String {
    let mut body = String::from(BODY_MARKER);
    body.push_str("\n\n");
    let _ = writeln!(
        body,
        "**Type:** {}",
        report.kind.unwrap_or(FeedbackKind::Friction).label()
    );
    if let Some(impact) = report.impact {
        let _ = writeln!(body, "**Impact:** {}", impact.label());
    }
    push_section(&mut body, "Summary", &clean(&report.message), false);
    let sections = [
        ("Task", &report.task, false),
        ("Command", &report.command, true),
        ("Expected", &report.expected, true),
        ("Actual", &report.actual, true),
    ];
    for (heading, value, code) in sections {
        if let Some(text) = value {
            push_section(&mut body, heading, &clean(text), code);
        }
    }
    if report.agent.is_some() || !report.agent_commands.is_empty() {
        body.push_str("\n### Agent\n\n");
        if let Some(agent) = &report.agent {
            let _ = writeln!(body, "{}", clean(agent));
        }
        if !report.agent_commands.is_empty() {
            let commands: Vec<String> = report
                .agent_commands
                .iter()
                .map(|command| clean(command))
                .collect();
            body.push('\n');
            body.push_str(&code_block(&commands.join("\n")));
        }
    }
    if diagnostics {
        let _ = writeln!(
            body,
            "\n### Environment\n\n- fallow {}\n- OS: {} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
    body.push_str("\n---\n_Filed with `fallow feedback`._\n");
    body
}

fn push_section(body: &mut String, heading: &str, text: &str, code: bool) {
    let _ = writeln!(body, "\n### {heading}\n");
    if code {
        body.push_str(&code_block(text));
    } else {
        body.push_str(text);
        body.push('\n');
    }
}

/// A fenced block whose fence is longer than any backtick run in the text, so
/// the text cannot close the block.
fn code_block(text: &str) -> String {
    let longest = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}text\n{text}\n{fence}\n")
}

/// Redact before the cut, so the cut cannot leave a token too short to find.
fn clean_text(text: &str, home: Option<&str>) -> String {
    truncate_chars(&redact::redact(text.trim(), home), MAX_FIELD_CHARS)
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((index, _)) => format!("{}...", &text[..index]),
        None => text.to_owned(),
    }
}

fn new_issue_url(title: &str, body: &str) -> (String, bool) {
    let prefix = format!("{NEW_ISSUE_URL}?title={}&body=", percent_encode(title));
    let full = percent_encode(body);
    if prefix.len() + full.len() <= MAX_URL_BYTES {
        return (format!("{prefix}{full}"), false);
    }
    let note = percent_encode(URL_TRUNCATION_NOTE);
    // Room for a closing fence of a long backtick run.
    let budget = MAX_URL_BYTES.saturating_sub(prefix.len() + note.len() + FENCE_RESERVE_BYTES);
    let mut encoded = String::new();
    let mut kept = String::new();
    for ch in body.chars() {
        let piece = percent_encode(ch.encode_utf8(&mut [0; 4]));
        if encoded.len() + piece.len() > budget {
            break;
        }
        encoded.push_str(&piece);
        kept.push(ch);
    }
    let close =
        open_fence(&kept).map_or_else(String::new, |fence| percent_encode(&format!("\n{fence}")));
    (format!("{prefix}{encoded}{close}{note}"), true)
}

/// The fence of a code block that `text` opens and does not close.
fn open_fence(text: &str) -> Option<String> {
    let mut open: Option<String> = None;
    for line in text.lines() {
        let ticks = line.len() - line.trim_start_matches('`').len();
        if ticks < 3 {
            continue;
        }
        match &open {
            Some(fence) if ticks >= fence.len() && line[ticks..].trim().is_empty() => open = None,
            Some(_) => {}
            None => open = Some("`".repeat(ticks)),
        }
    }
    open
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn to_json(issue: &Issue, outcome: &Outcome, diagnostics: bool) -> serde_json::Value {
    let (status, issue_url, reason) = match outcome {
        Outcome::Preview => ("preview", None, None),
        Outcome::Created { issue_url } => ("created", Some(issue_url.as_str()), None),
        Outcome::Manual { reason } => ("manual", None, Some(reason.as_str())),
    };
    serde_json::json!({
        "feedback": {
            "status": status,
            "repository": FEEDBACK_REPOSITORY,
            "public": true,
            "title": issue.title,
            "body": issue.body,
            "digest": issue.digest,
            "diagnostics_included": diagnostics,
            "issue_url": issue_url,
            "new_issue_url": issue.url,
            "new_issue_url_truncated": issue.url_truncated,
            "reason": reason,
            "submit_command": matches!(outcome, Outcome::Preview)
                .then_some("rerun the same command with --yes to create the issue with the GitHub CLI"),
        }
    })
}

fn print_human(issue: &Issue, outcome: &Outcome) {
    match outcome {
        Outcome::Created { issue_url } => {
            println!("Created issue: {issue_url}");
        }
        Outcome::Preview => {
            println!(
                "Feedback preview. Nothing was sent. The issue will be public on github.com/{FEEDBACK_REPOSITORY}.\n"
            );
            print_issue(issue);
            println!(
                "\nTo create the issue, run the same command with --yes (uses the GitHub CLI)."
            );
            println!("Or open this link: {}", issue.url);
        }
        Outcome::Manual { reason } => {
            eprintln!("The issue was not created: {reason}.");
            println!("Open this link to file the issue: {}", issue.url);
            if issue.url_truncated {
                println!("The link holds a shortened report. The full report:\n");
                print_issue(issue);
            }
        }
    }
}

fn print_issue(issue: &Issue) {
    println!("Title: {}\n", issue.title);
    println!("{}", issue.body.trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(message: &str) -> Report {
        Report {
            message: message.to_owned(),
            ..Report::default()
        }
    }

    #[test]
    fn json_input_fills_fields_and_flags_win() {
        let json = parse_json_input(
            r#"{"message":"from json","type":"false_positive","impact":"worked_around",
                "task":"t","agent":{"name":"claude-code","commands":["fallow dead-code"]}}"#,
        )
        .unwrap();
        let args = FeedbackArgs {
            kind: Some(FeedbackKind::Bug),
            ..FeedbackArgs::default()
        };
        let merged = merge(&args, json).unwrap();
        assert_eq!(merged.message, "from json");
        assert_eq!(merged.kind, Some(FeedbackKind::Bug));
        assert_eq!(merged.impact, Some(FeedbackImpact::WorkedAround));
        assert_eq!(merged.agent.as_deref(), Some("claude-code"));
        assert_eq!(merged.agent_commands, vec!["fallow dead-code"]);
    }

    #[test]
    fn json_input_rejects_unknown_fields() {
        let err = parse_json_input(r#"{"message":"x","severity":"high"}"#).unwrap_err();
        assert!(err.contains("severity"), "{err}");
    }

    #[test]
    fn a_report_needs_a_message() {
        let err = merge(&FeedbackArgs::default(), JsonReport::default()).unwrap_err();
        assert!(err.contains("feedback message"), "{err}");
        let blank = FeedbackArgs {
            message: Some("   ".to_owned()),
            ..FeedbackArgs::default()
        };
        assert!(merge(&blank, JsonReport::default()).is_err());
    }

    #[test]
    fn title_uses_the_first_line_and_the_kind() {
        let mut report = report("audit flags a used export\nmore detail");
        report.kind = Some(FeedbackKind::FalsePositive);
        report.agent = Some("codex".to_owned());
        let issue = build_issue(&report, false, None);
        assert_eq!(
            issue.title,
            "Agent feedback (false positive): audit flags a used export"
        );
        assert!(issue.body.starts_with(BODY_MARKER));
        assert!(
            issue
                .body
                .contains("### Summary\n\naudit flags a used export\nmore detail")
        );
        assert!(!issue.body.contains("### Environment"));
    }

    #[test]
    fn body_has_the_sections_and_diagnostics() {
        let mut report = report("slow");
        report.impact = Some(FeedbackImpact::Blocked);
        report.command = Some("fallow audit".to_owned());
        report.expected = Some("fast".to_owned());
        report.actual = Some("slow".to_owned());
        let body = build_issue(&report, true, None).body;
        assert!(body.contains("**Type:** friction\n**Impact:** blocked\n"));
        assert!(body.contains("### Command\n\n```text\nfallow audit\n```\n"));
        assert!(body.contains("### Expected\n\n```text\nfast\n```\n"));
        assert!(body.contains(&format!("- fallow {}", env!("CARGO_PKG_VERSION"))));
        assert!(body.contains(&format!("- OS: {}", std::env::consts::OS)));
    }

    #[test]
    fn code_block_fence_outlasts_backticks_in_the_text() {
        let block = code_block("a ```` b");
        assert!(block.starts_with("`````text\n"), "{block}");
        assert!(block.ends_with("\n`````\n"), "{block}");
    }

    #[test]
    fn a_secret_cut_by_the_field_cap_is_still_redacted() {
        let token = format!("ghp_{}", "a1B2".repeat(6));
        let text = format!("{}{token}", "x ".repeat((MAX_FIELD_CHARS - 10) / 2));
        let cleaned = clean_text(&text, None);
        assert!(!cleaned.contains("ghp_"), "{cleaned}");
    }

    #[test]
    fn diagnostics_follow_the_switches() {
        let none = |_: &str| None;
        assert!(diagnostics_enabled(false, &none));
        assert!(!diagnostics_enabled(true, &none));
        let dnt = |name: &str| (name == "DO_NOT_TRACK").then(|| "1".to_owned());
        assert!(!diagnostics_enabled(false, &dnt));
        let off = |name: &str| (name == "DO_NOT_TRACK").then(|| "0".to_owned());
        assert!(diagnostics_enabled(false, &off));
        let disabled =
            |name: &str| (name == "FALLOW_TELEMETRY_DISABLED").then(|| "true".to_owned());
        assert!(!diagnostics_enabled(false, &disabled));
    }

    #[test]
    fn url_is_prefilled_and_bounded() {
        let issue = build_issue(&report("a b&c"), false, None);
        assert!(
            issue.url.starts_with(
                "https://github.com/fallow-rs/fallow/issues/new?title=Feedback%20%28friction%29%3A%20a%20b%26c&body="
            ),
            "{}",
            issue.url
        );
        assert!(!issue.url_truncated);

        let long = report(&"é".repeat(MAX_FIELD_CHARS));
        let issue = build_issue(&long, false, None);
        assert!(issue.url_truncated);
        assert!(issue.url.len() <= MAX_URL_BYTES);
        assert!(issue.body.chars().count() > MAX_FIELD_CHARS);
    }

    #[test]
    fn a_truncated_link_closes_an_open_fence() {
        let mut long = report("x");
        long.actual = Some("é\n".repeat(1_999));
        let issue = build_issue(&long, false, None);
        assert!(issue.url_truncated);
        assert!(issue.url.len() <= MAX_URL_BYTES, "{}", issue.url.len());
        assert!(
            issue.url.contains("%0A%60%60%60%0A%0A_The%20link"),
            "{}",
            &issue.url[issue.url.len() - 200..]
        );
    }

    #[test]
    fn open_fence_tracks_the_fence_length() {
        assert_eq!(open_fence("a\n```text\nb"), Some("```".to_owned()));
        assert_eq!(open_fence("````text\n```\nb"), Some("````".to_owned()));
        assert_eq!(open_fence("```text\nb\n```\n"), None);
    }

    #[test]
    fn an_empty_flag_does_not_hide_a_json_value() {
        let json = parse_json_input(r#"{"message":"from json","task":"json task"}"#).unwrap();
        let args = FeedbackArgs {
            message: Some("  ".to_owned()),
            task: Some(String::new()),
            ..FeedbackArgs::default()
        };
        let merged = merge(&args, json).unwrap();
        assert_eq!(merged.message, "from json");
        assert_eq!(merged.task.as_deref(), Some("json task"));
    }

    #[test]
    fn digest_follows_the_issue_text() {
        let one = build_issue(&report("a"), false, None);
        assert_eq!(one.digest.len(), DIGEST_BYTES * 2);
        assert_eq!(one.digest, build_issue(&report("a"), false, None).digest);
        assert_ne!(one.digest, build_issue(&report("b"), false, None).digest);
    }

    #[test]
    fn json_output_names_the_status() {
        let issue = build_issue(&report("x"), false, None);
        let value = to_json(&issue, &Outcome::Preview, false);
        assert_eq!(value["feedback"]["status"], "preview");
        assert_eq!(value["feedback"]["public"], true);
        assert!(value["feedback"]["issue_url"].is_null());
        let created = to_json(
            &issue,
            &Outcome::Created {
                issue_url: "https://example.test/1".to_owned(),
            },
            false,
        );
        assert_eq!(created["feedback"]["issue_url"], "https://example.test/1");
        assert!(created["feedback"]["submit_command"].is_null());
    }
}
