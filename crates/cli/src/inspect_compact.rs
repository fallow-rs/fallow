//! `fallow inspect --file <path> --format compact`: plain text for coding
//! agents and editor hooks, one line per item, no colors, about 1 KB.
//!
//! ```text
//! fallow src/services/user.ts
//! finding unused-export src/services/user.ts:42 `formatUser` is never imported
//! finding complexity src/services/user.ts:14 loadUser cyclomatic 21 cognitive 30
//! prod loadUser:14 called 4120 calls, 1 error group (7 events), 812.5 ms CPU/h
//! prod parseRow:60 never_called in 30 days
//! prod newHelper:80 no production data (new or moved since deploy)
//! cloud plan_required: production data needs a Fallow Cloud plan
//! ```
//!
//! Line kinds: the `fallow <path>` header, `finding <kind> <path:line> <text>`,
//! `prod <name:line> <text>`, `cloud <status>: <text>` (only for a status other
//! than `ok`) and `truncated <n> more` when the byte cap cuts lines.

use fallow_output::{
    InspectCloudEvidence, InspectCloudFunction, InspectCloudStatus, InspectEvidenceSection,
    InspectOutput, InspectTargetDescriptor,
};
use std::fmt::Write as _;

use serde_json::Value;

/// Upper limit on the bytes of the compact text.
pub const COMPACT_MAX_BYTES: usize = 1024;
/// Bytes kept free for the `truncated <n> more` line.
const TRUNCATED_LINE_RESERVE: usize = "truncated 99999 more\n".len();
/// Dead-code result keys that are context, not findings of the file.
const DEAD_CODE_CONTEXT_KEYS: &[&str] = &[
    "entry_points",
    "workspace_diagnostics",
    "next_steps",
    "gate_outcomes",
];
/// Fields that name the subject of a finding, in order of preference.
const SUBJECT_FIELDS: &[&str] = &[
    "export_name",
    "member_name",
    "name",
    "package_name",
    "specifier",
];

/// Render the compact text of an inspect bundle.
pub fn render(bundle: &InspectOutput) -> String {
    let file = match &bundle.target {
        InspectTargetDescriptor::File { file } | InspectTargetDescriptor::Symbol { file, .. } => {
            file.as_str()
        }
    };
    let mut lines = Vec::new();
    lines.extend(dead_code_lines(&bundle.evidence.dead_code, file));
    lines.extend(complexity_lines(&bundle.evidence.complexity, file));
    lines.extend(duplication_lines(&bundle.evidence.duplication, file));
    lines.extend(security_lines(&bundle.evidence.security, file));
    let status_line = bundle.evidence.cloud.as_ref().and_then(|cloud| {
        lines.extend(prod_lines(cloud));
        cloud_status_line(cloud)
    });
    fit(
        &format!("fallow {}", cut_text(file, HEADER_PATH_MAX_BYTES)),
        &lines,
        status_line.as_deref(),
    )
}

/// Join the lines under [`COMPACT_MAX_BYTES`]. The header and the cloud status
/// line always stay; item lines that do not fit become one `truncated` line.
fn fit(header: &str, items: &[String], status_line: Option<&str>) -> String {
    let status_bytes = status_line.map_or(0, |line| line.len() + 1);
    let full_bytes =
        header.len() + 1 + status_bytes + items.iter().map(|l| l.len() + 1).sum::<usize>();
    let budget = if full_bytes <= COMPACT_MAX_BYTES {
        COMPACT_MAX_BYTES
    } else {
        COMPACT_MAX_BYTES.saturating_sub(TRUNCATED_LINE_RESERVE)
    };
    let mut out = String::with_capacity(COMPACT_MAX_BYTES);
    out.push_str(header);
    out.push('\n');
    let mut written = 0;
    for line in items {
        if out.len() + line.len() + 1 + status_bytes > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
        written += 1;
    }
    if let Some(line) = status_line {
        out.push_str(line);
        out.push('\n');
    }
    if written < items.len() {
        let _ = writeln!(out, "truncated {} more", items.len() - written);
    }
    out
}

fn section_data(section: &InspectEvidenceSection) -> Option<&Value> {
    section.data.as_ref()
}

fn item_in_file(item: &Value, file: &str) -> bool {
    item.get("path")
        .and_then(Value::as_str)
        .is_some_and(|path| path == file)
}

fn item_line(item: &Value) -> u64 {
    item.get("line").and_then(Value::as_u64).unwrap_or(1)
}

/// Kind of a dead-code finding: the middle part of its `finding_id`
/// (`dc1:unused-export:<hash>`), else the result key in kebab case.
fn dead_code_kind(item: &Value, key: &str) -> String {
    item.get("finding_id")
        .and_then(Value::as_str)
        .and_then(|id| id.split(':').nth(1))
        .map_or_else(
            || key.trim_end_matches('s').replace('_', "-"),
            str::to_owned,
        )
}

fn dead_code_text(kind: &str, item: &Value) -> String {
    let subject = SUBJECT_FIELDS
        .iter()
        .find_map(|field| item.get(*field).and_then(Value::as_str));
    match (kind, subject) {
        ("unused-export" | "unused-type", Some(name)) => format!("`{name}` is never imported"),
        ("unused-file", _) => "file is not reachable from an entry point".to_owned(),
        (_, Some(name)) => format!("`{name}`"),
        (_, None) => kind.to_owned(),
    }
}

fn dead_code_lines(section: &InspectEvidenceSection, file: &str) -> Vec<String> {
    let Some(Value::Object(data)) = section_data(section) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for (key, value) in data {
        if DEAD_CODE_CONTEXT_KEYS.contains(&key.as_str()) {
            continue;
        }
        let Some(items) = value.as_array() else {
            continue;
        };
        for item in items.iter().filter(|item| item_in_file(item, file)) {
            let kind = dead_code_kind(item, key);
            let text = dead_code_text(&kind, item);
            lines.push(format!("finding {kind} {file}:{} {text}", item_line(item)));
        }
    }
    lines
}

fn complexity_lines(section: &InspectEvidenceSection, file: &str) -> Vec<String> {
    let Some(findings) = section_data(section)
        .and_then(|data| data.get("findings"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    findings
        .iter()
        .filter(|item| item_in_file(item, file))
        .map(|item| {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("?");
            let mut text = name.to_owned();
            if let Some(cyclomatic) = item.get("cyclomatic").and_then(Value::as_u64) {
                let _ = write!(text, " cyclomatic {cyclomatic}");
            }
            if let Some(cognitive) = item.get("cognitive").and_then(Value::as_u64) {
                let _ = write!(text, " cognitive {cognitive}");
            }
            format!("finding complexity {file}:{} {text}", item_line(item))
        })
        .collect()
}

fn duplication_lines(section: &InspectEvidenceSection, file: &str) -> Vec<String> {
    let Some(groups) = section_data(section)
        .and_then(|data| data.get("clone_groups"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    for group in groups {
        let Some(instances) = group.get("instances").and_then(Value::as_array) else {
            continue;
        };
        let file_of = |instance: &Value| {
            instance
                .get("file")
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let start = |instance: &Value| instance.get("start_line").and_then(Value::as_u64);
        for own in instances
            .iter()
            .filter(|instance| file_of(instance).as_deref() == Some(file))
        {
            let other = instances
                .iter()
                .find(|instance| !std::ptr::eq(*instance, own))
                .and_then(|instance| Some(format!("{}:{}", file_of(instance)?, start(instance)?)));
            let line = start(own).unwrap_or(1);
            let span = own
                .get("end_line")
                .and_then(Value::as_u64)
                .map_or(0, |end| end.saturating_sub(line) + 1);
            let text = match other {
                Some(other) => format!("{span} lines duplicated at {other}"),
                None => format!("{span} lines duplicated"),
            };
            lines.push(format!("finding duplication {file}:{line} {text}"));
        }
    }
    lines
}

fn security_lines(section: &InspectEvidenceSection, file: &str) -> Vec<String> {
    let Some(findings) = section_data(section)
        .and_then(|data| data.get("security_findings"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    findings
        .iter()
        .filter(|item| item_in_file(item, file))
        .map(|item| {
            let text = item
                .get("category")
                .or_else(|| item.get("kind"))
                .and_then(Value::as_str)
                .unwrap_or("security candidate");
            format!("finding security {file}:{} {text}", item_line(item))
        })
        .collect()
}

/// Production lines, most signal first, so the byte cap cuts the least
/// useful lines: functions with errors, then never_called functions, then
/// local functions without production data, then the rest. Each group is in
/// line order.
fn prod_lines(cloud: &InspectCloudEvidence) -> Vec<String> {
    if cloud.status != InspectCloudStatus::Ok {
        return Vec::new();
    }
    let period = cloud.period_days.unwrap_or(DEFAULT_PERIOD_DAYS);
    let mut entries: Vec<(u8, u32, String)> = cloud
        .functions
        .iter()
        .map(|function| {
            (
                prod_rank(function),
                function.line,
                format!(
                    "prod {}:{} {}",
                    function.name,
                    function.line,
                    prod_text(function, period)
                ),
            )
        })
        .chain(cloud.unpaired_local.iter().map(|function| {
            (
                RANK_UNPAIRED,
                function.line,
                format!("prod {}:{} {}", function.name, function.line, function.note),
            )
        }))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    entries.into_iter().map(|(_, _, line)| line).collect()
}

const RANK_ERRORS: u8 = 0;
const RANK_NEVER_CALLED: u8 = 1;
const RANK_UNPAIRED: u8 = 2;
const RANK_OTHER: u8 = 3;
/// Period of the cloud read when the answer names none.
const DEFAULT_PERIOD_DAYS: u16 = 30;

fn prod_rank(function: &InspectCloudFunction) -> u8 {
    if function.errors.is_some_and(|errors| errors.groups > 0) {
        RANK_ERRORS
    } else if function.state == "never_called" {
        RANK_NEVER_CALLED
    } else {
        RANK_OTHER
    }
}

fn prod_text(function: &InspectCloudFunction, period: u16) -> String {
    let mut parts = Vec::new();
    match (function.state.as_str(), function.calls) {
        ("called", Some(calls)) => parts.push(format!("called {calls} {}", plural(calls, "call"))),
        ("never_called", _) if function.period_state.as_deref() == Some("called") => parts.push(
            format!("not called in this deployment, called earlier in {period} days"),
        ),
        ("never_called", _) => parts.push(format!("never_called in {period} days")),
        ("untracked", _) => parts.push("untracked (no runtime data)".to_owned()),
        (state, _) => parts.push(state.to_owned()),
    }
    if let Some(errors) = function.errors
        && errors.groups > 0
    {
        parts.push(format!(
            "{} {} ({} {})",
            errors.groups,
            plural(errors.groups, "error group"),
            errors.events,
            plural(errors.events, "event")
        ));
    }
    if let Some(cpu) = function.cpu_ms_per_hour {
        parts.push(format!("{cpu:.1} ms CPU/h"));
    }
    if let Some(ranges) = function.never_run_ranges
        && ranges > 0
    {
        parts.push(format!("{ranges} never-run {}", plural(ranges, "branch")));
    }
    parts.join(", ")
}

fn plural(count: u64, word: &str) -> String {
    if count == 1 {
        word.to_owned()
    } else if word.ends_with("ch") {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

fn cloud_status_line(cloud: &InspectCloudEvidence) -> Option<String> {
    if cloud.status == InspectCloudStatus::Ok {
        return None;
    }
    let message = cloud
        .message
        .as_deref()
        .unwrap_or("no production data")
        .lines()
        .next()
        .unwrap_or_default()
        .trim();
    Some(format!(
        "cloud {}: {}",
        cloud.status.as_str(),
        cut_text(message, STATUS_MESSAGE_MAX_BYTES)
    ))
}

/// Longest file path in the header line. Together with the status message cap
/// and the truncated line, the output stays under [`COMPACT_MAX_BYTES`] for
/// any path.
const HEADER_PATH_MAX_BYTES: usize = 500;

/// Longest status message in the compact text. A cloud error body can be
/// long, and the status line always stays in the output.
const STATUS_MESSAGE_MAX_BYTES: usize = 200;

/// `text` cut to at most `max` bytes on a character boundary, with `...`
/// at the end when cut.
fn cut_text(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max.saturating_sub(3);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_header_and_status_and_counts_the_cut() {
        let items: Vec<String> = (0..100)
            .map(|i| format!("finding x src/a.ts:{i} {}", "y".repeat(30)))
            .collect();
        let text = fit("fallow src/a.ts", &items, Some("cloud no_key: run login"));
        assert!(text.len() <= COMPACT_MAX_BYTES, "{}", text.len());
        assert!(text.starts_with("fallow src/a.ts\n"));
        assert!(text.contains("cloud no_key: run login\n"));
        let last = text.lines().last().unwrap();
        assert!(last.starts_with("truncated "), "{last}");
        let shown = text.lines().filter(|l| l.starts_with("finding ")).count();
        assert_eq!(last, format!("truncated {} more", 100 - shown));
    }

    #[test]
    fn a_long_path_and_a_long_status_stay_under_the_cap() {
        let path = "d/".repeat(600);
        let status = format!(
            "cloud no_key: {}",
            cut_text(&"x".repeat(5000), STATUS_MESSAGE_MAX_BYTES)
        );
        let text = fit(
            &format!("fallow {}", cut_text(&path, HEADER_PATH_MAX_BYTES)),
            &["finding x".to_owned()],
            Some(&status),
        );
        assert!(text.len() <= COMPACT_MAX_BYTES, "{}", text.len());
    }

    #[test]
    fn fit_shows_every_line_when_all_fit() {
        let items = vec!["finding a src/a.ts:1 x".to_owned()];
        assert_eq!(
            fit("fallow src/a.ts", &items, None),
            "fallow src/a.ts\nfinding a src/a.ts:1 x\n"
        );
    }

    fn never_called(period_state: Option<&str>) -> InspectCloudFunction {
        InspectCloudFunction {
            name: "a".to_owned(),
            line: 1,
            stable_id: None,
            match_kind: fallow_output::InspectCloudMatch::StableId,
            state: "never_called".to_owned(),
            period_state: period_state.map(str::to_owned),
            calls: Some(0),
            errors: None,
            cpu_ms_per_hour: None,
            never_run_ranges: None,
            summary: String::new(),
        }
    }

    #[test]
    fn a_function_that_ran_in_an_earlier_deployment_is_not_never_called() {
        let text = prod_text(&never_called(Some("called")), 30);
        assert_eq!(
            text,
            "not called in this deployment, called earlier in 30 days"
        );
        assert!(!text.contains("never_called"));
        assert_eq!(
            prod_text(&never_called(Some("never_called")), 30),
            "never_called in 30 days"
        );
        assert_eq!(prod_text(&never_called(None), 7), "never_called in 7 days");
    }

    #[test]
    fn a_long_status_message_stays_under_the_byte_cap() {
        let cloud = InspectCloudEvidence::status_only(
            InspectCloudStatus::Unavailable,
            format!(
                "file-context failed with HTTP 418: {}",
                "\u{e9}".repeat(3000)
            ),
            None,
        );
        let line = cloud_status_line(&cloud).unwrap();
        assert!(
            line.len() <= STATUS_MESSAGE_MAX_BYTES + 40,
            "{}",
            line.len()
        );
        assert!(line.ends_with("..."));
        let text = fit("fallow src/a.ts", &[], Some(&line));
        assert!(text.len() <= COMPACT_MAX_BYTES, "{}", text.len());
    }

    #[test]
    fn plural_handles_branch() {
        assert_eq!(plural(2, "branch"), "branches");
        assert_eq!(plural(1, "event"), "event");
    }
}
