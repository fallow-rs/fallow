//! Run each command and format on the LF form and on a CRLF form of one
//! generated project, and compare the outputs.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use super::project::{LineEndings, Project, Seeded, generate, isolate_git, materialize};
use crate::common::fallow_bin;

/// Each value of `--format` that the report commands accept.
const REPORT_FORMATS: [&str; 12] = [
    "human",
    "json",
    "sarif",
    "compact",
    "markdown",
    "codeclimate",
    "pr-comment-github",
    "pr-comment-gitlab",
    "review-github",
    "review-gitlab",
    "github-annotations",
    "github-summary",
];

/// The pairs whose output carries the CodeClimate fingerprint of a clone
/// instance. The review formats merge by that fingerprint, and the PR comment
/// formats sort by it when two issues share a path and a line. `audit` adds
/// the clone groups of the change to the same formats.
const CLONE_FINGERPRINT_PAIRS: [(&str, &str); 8] = [
    ("dupes", "codeclimate"),
    ("dupes", "pr-comment-github"),
    ("dupes", "pr-comment-gitlab"),
    ("dupes", "review-github"),
    ("dupes", "review-gitlab"),
    ("audit", "codeclimate"),
    ("audit", "review-github"),
    ("audit", "review-gitlab"),
];

/// The commands that save and read a baseline.
const BASELINE_COMMANDS: [&str; 3] = ["dead-code", "dupes", "health"];

/// JSON keys whose values change between two runs of the same analysis.
const VOLATILE_KEYS: [&str; 4] = [
    "elapsed_ms",
    "duration_ms",
    "phase_timings_ms",
    "analysis_run_id",
];
/// JSON keys whose values are verbatim source text. Source text keeps the
/// line endings of the file, so the oracle removes `\r` from these values
/// only. A `\r` in any other value is a difference.
const SOURCE_TEXT_KEYS: [&str; 1] = ["fragment"];
/// A byte offset into the file. A CRLF file has one more byte per line, so
/// this value must differ. Line and column stay the same.
const BYTE_OFFSET_KEY: &str = "span_start";

/// Narrows the formats, for example to `json` on an old commit that does not
/// have the other formats. A comma-separated list.
const FORMATS_ENV: &str = "FALLOW_PLATFORM_FORMATS";
const SEEDS_ENV: &str = "FALLOW_PLATFORM_SEEDS";
const SEED_START_ENV: &str = "FALLOW_PLATFORM_SEED_START";
const DEFAULT_SWEEP_SEEDS: u64 = 10;
const MAX_WORKERS: usize = 6;

/// One materialized form of the project.
struct Form {
    dir: tempfile::TempDir,
    base: String,
    head: String,
}

impl Form {
    fn new(project: &Project, endings: LineEndings) -> Self {
        let dir = tempfile::tempdir().expect("project directory");
        let (base, head) = materialize(dir.path(), project, endings);
        Self { dir, base, head }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Run `fallow <args> --format <format>` and return the exit code and the
    /// normalized stdout.
    fn run(&self, args: &[&str], format: &str) -> (Option<i32>, String) {
        let output = isolate_git(&mut Command::new(fallow_bin()))
            .current_dir(self.root())
            .args(args)
            .args(["--root", ".", "--no-cache", "--quiet", "--format", format])
            .env("NO_COLOR", "1")
            .env("RUST_LOG", "")
            .env("FALLOW_CREDENTIAL_STORE", "none")
            .env_remove("FALLOW_COVERAGE")
            .env_remove("FALLOW_COVERAGE_ROOT")
            .env_remove("FALLOW_PACKAGE_BASELINES")
            .env_remove("FALLOW_AUDIT_BASE")
            .env_remove("CLAUDECODE")
            .env_remove("CLAUDE_CODE_CHILD_SESSION")
            .output()
            .expect("run the fallow binary");
        let stdout = String::from_utf8_lossy(&output.stdout);
        (output.status.code(), self.normalize(&stdout))
    }

    /// Replace the values that differ between two checkouts of the same
    /// project: the directory, the commit SHAs, the durations, and the byte
    /// offsets and verbatim source text that keep the line endings.
    fn normalize(&self, text: &str) -> String {
        let mut text = text.to_owned();
        for root in [self.root().to_path_buf(), canonical(self.root())] {
            let root = root.display().to_string();
            // JSON doubles each backslash of a Windows root.
            text = text
                .replace(&root.replace('\\', "\\\\"), "<root>")
                .replace(&root, "<root>");
        }
        for (sha, label) in [(&self.base, "<base>"), (&self.head, "<head>")] {
            for length in [40, 12, 10, 9, 8, 7] {
                text = text.replace(&sha[..length], label);
            }
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(mut value) => {
                normalize_json(&mut value);
                serde_json::to_string_pretty(&value).expect("serialize JSON")
            }
            Err(_) => redact_durations(&text),
        }
    }
}

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Drop the volatile keys and the byte offsets, remove `\r` from verbatim
/// source text, and redact the durations in text.
fn normalize_json(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in VOLATILE_KEYS
                .iter()
                .chain(std::iter::once(&BYTE_OFFSET_KEY))
            {
                map.remove(*key);
            }
            for key in SOURCE_TEXT_KEYS {
                if let Some(Value::String(text)) = map.get_mut(key) {
                    text.retain(|character| character != '\r');
                }
            }
            map.values_mut().for_each(normalize_json);
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_json),
        Value::String(text) => *text = redact_durations(text),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// Replace a duration token such as `9ms` or `1.2s` with `[TIME]`. A token
/// starts after a character that is not a letter or a digit, and ends before
/// one, so `v2s` and hex digests stay as they are.
fn redact_durations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut index = 0;
    while index < bytes.len() {
        let token_start = bytes[index].is_ascii_digit()
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric());
        if !token_start {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
            end += 1;
        }
        let unit = if text[end..].starts_with("ms") {
            2
        } else {
            usize::from(text[end..].starts_with('s'))
        };
        let after = end + unit;
        if unit > 0 && (after == bytes.len() || !bytes[after].is_ascii_alphanumeric()) {
            out.push_str(&text[copied..index]);
            out.push_str("[TIME]");
            copied = after;
        }
        index = after.max(end);
    }
    out.push_str(&text[copied..]);
    out
}

/// The first line where two outputs differ.
fn first_difference(expected: &str, actual: &str) -> String {
    expected
        .lines()
        .zip(actual.lines())
        .enumerate()
        .find(|(_, (left, right))| left != right)
        .map_or_else(
            || {
                format!(
                    "the outputs have {} and {} lines",
                    expected.lines().count(),
                    actual.lines().count()
                )
            },
            |(line, (left, right))| {
                let clip = |text: &str| text.chars().take(240).collect::<String>();
                format!(
                    "line {}:\n    LF:   {}\n    CRLF: {}",
                    line + 1,
                    clip(left),
                    clip(right)
                )
            },
        )
}

/// The formats to check: each report format, or the list in `FORMATS_ENV`.
fn formats() -> Vec<String> {
    std::env::var(FORMATS_ENV).map_or_else(
        |_| {
            REPORT_FORMATS
                .iter()
                .map(|format| (*format).to_owned())
                .collect()
        },
        |list| {
            list.split(',')
                .map(str::trim)
                .filter(|format| !format.is_empty())
                .map(str::to_owned)
                .collect()
        },
    )
}

/// The arguments of `command` for a form. `audit` reviews the change commit
/// against the base commit of that form.
fn command_args<'a>(command: &'a str, form: &'a Form) -> Vec<&'a str> {
    if command == "audit" {
        vec!["audit", "--base", form.base.as_str()]
    } else {
        vec![command]
    }
}

/// Compare one command and format on the two forms. Returns a failure
/// message, or `None` when the outputs are equal.
fn compare(command: &str, format: &str, lf: &Form, crlf: &Form) -> Option<String> {
    let (lf_code, lf_out) = lf.run(&command_args(command, lf), format);
    let (crlf_code, crlf_out) = crlf.run(&command_args(command, crlf), format);
    if !matches!(lf_code, Some(0 | 1)) {
        return Some(format!(
            "fallow {command} --format {format}: the LF run exits {lf_code:?}, not 0 or 1"
        ));
    }
    if format == "json" && !has_findings(command, lf_code, &lf_out) {
        return Some(format!(
            "fallow {command} --format json: the LF run has no findings, so the comparison shows nothing"
        ));
    }
    (lf_code != crlf_code || lf_out != crlf_out).then(|| {
        format!(
            "fallow {command} --format {format}: exit codes {lf_code:?} and {crlf_code:?}\n  {}",
            first_difference(&lf_out, &crlf_out)
        )
    })
}

/// True when the JSON output of `command` holds the findings that the
/// generated project must give: an unused file, a clone group, a complexity
/// finding, or a failed `audit` gate on the change.
fn has_findings(command: &str, code: Option<i32>, json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let non_empty = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
    };
    match command {
        "dead-code" => non_empty("unused_files"),
        "dupes" => non_empty("clone_groups"),
        "health" => non_empty("findings"),
        "audit" => code == Some(1),
        _ => true,
    }
}

/// Run `jobs` on `workers` threads and collect the failure messages.
fn run_jobs(jobs: &[(&str, String)], workers: usize, lf: &Form, crlf: &Form) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some((command, format)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    if let Some(failure) = compare(command, format, lf, crlf) {
                        failures.lock().expect("failure list").push(failure);
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().expect("failure list");
    failures.sort();
    failures
}

fn workers() -> usize {
    std::thread::available_parallelism().map_or(2, |count| (count.get() / 2).clamp(1, MAX_WORKERS))
}

/// Check that each `(command, format)` pair gives the same output for the LF
/// form and for `endings` of the project of `seed`.
fn check_pairs(seed: u64, endings: LineEndings, pairs: &[(&str, String)]) {
    assert!(
        !pairs.is_empty(),
        "no command and format to compare; check {FORMATS_ENV}"
    );
    let project = generate(seed);
    let lf = Form::new(&project, LineEndings::Lf);
    let crlf = Form::new(&project, endings);
    // The audit runs share one repository and its base worktree, so each
    // form runs them one at a time.
    let (audit, other): (Vec<_>, Vec<_>) = pairs
        .iter()
        .cloned()
        .partition(|(command, _)| *command == "audit");
    let mut failures = run_jobs(&other, workers(), &lf, &crlf);
    failures.extend(run_jobs(&audit, 1, &lf, &crlf));
    assert!(
        failures.is_empty(),
        "the output changes with the line endings\n  project seed: {seed}\n  form: {endings:?}\n{} of {} pairs differ:\n  {}\nRun this seed again with {SEED_START_ENV}={seed} {SEEDS_ENV}=1 in the seed sweep.",
        failures.len(),
        pairs.len(),
        failures.join("\n  "),
    );
}

/// The pairs of `commands` and each format.
fn pairs<'a>(commands: &[&'a str]) -> Vec<(&'a str, String)> {
    commands
        .iter()
        .flat_map(|command| formats().into_iter().map(move |format| (*command, format)))
        .collect()
}

/// Save a baseline in each form and apply it in the other form. The two
/// baseline files must be equal, and the result with the baseline of the
/// other form must equal the result with the own baseline.
fn check_baselines(seed: u64, endings: LineEndings) {
    let project = generate(seed);
    let lf = Form::new(&project, LineEndings::Lf);
    let crlf = Form::new(&project, endings);
    let mut failures = Vec::new();
    for command in BASELINE_COMMANDS {
        let own = format!("baselines/{command}.json");
        let other = format!("baselines/{command}-other.json");
        for form in [&lf, &crlf] {
            std::fs::create_dir_all(form.root().join("baselines")).expect("baseline dir");
            let (code, _) = form.run(&[command, "--save-baseline", &own], "json");
            assert!(
                matches!(code, Some(0 | 1)),
                "fallow {command} --save-baseline exits {code:?}"
            );
        }
        let lf_saved = std::fs::read_to_string(lf.root().join(&own)).expect("LF baseline");
        let crlf_saved = std::fs::read_to_string(crlf.root().join(&own)).expect("CRLF baseline");
        if lf_saved != crlf_saved {
            failures.push(format!(
                "fallow {command} --save-baseline: the files differ\n  {}",
                first_difference(&lf_saved, &crlf_saved)
            ));
        }
        std::fs::write(lf.root().join(&other), &crlf_saved).expect("copy baseline");
        std::fs::write(crlf.root().join(&other), &lf_saved).expect("copy baseline");
        for (name, form) in [("LF", &lf), ("CRLF", &crlf)] {
            let without = form.run(&[command], "json");
            let with_own = form.run(&[command, "--baseline", &own], "json");
            if without == with_own {
                failures.push(format!(
                    "fallow {command} --baseline in the {name} form: the own baseline removes no finding, so the round trip shows nothing"
                ));
            }
            let with_other = form.run(&[command, "--baseline", &other], "json");
            if with_own != with_other {
                failures.push(format!(
                    "fallow {command} --baseline in the {name} form: the baseline of the other form gives other findings\n  {}",
                    first_difference(&with_own.1, &with_other.1)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "a baseline changes with the line endings\n  project seed: {seed}\n  form: {endings:?}\n  {}",
        failures.join("\n  ")
    );
}

fn env_number(name: &str) -> Option<u64> {
    let value = std::env::var(name).ok()?;
    Some(
        value
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a whole number, got {value:?}")),
    )
}

#[test]
fn dead_code_matches_across_line_endings() {
    check_pairs(1, LineEndings::Crlf, &pairs(&["dead-code"]));
    check_pairs(2, LineEndings::Mixed(21), &pairs(&["dead-code"]));
}

#[test]
fn dupes_match_across_line_endings() {
    check_pairs(3, LineEndings::Crlf, &pairs(&["dupes"]));
    check_pairs(4, LineEndings::Mixed(41), &pairs(&["dupes"]));
}

#[test]
fn health_matches_across_line_endings() {
    check_pairs(5, LineEndings::Crlf, &pairs(&["health"]));
    check_pairs(6, LineEndings::Mixed(61), &pairs(&["health"]));
}

#[test]
fn audit_matches_across_line_endings() {
    check_pairs(7, LineEndings::Crlf, &pairs(&["audit"]));
}

#[test]
fn baselines_round_trip_across_line_endings() {
    check_baselines(8, LineEndings::Crlf);
    check_baselines(9, LineEndings::Mixed(91));
}

/// The CodeClimate fingerprint of a clone instance hashes the first 64
/// characters of the fragment without `\r`. When a `\r` stays in the hash,
/// GitLab Code Quality reports each clone as new and fixed when a Windows
/// runner and a Linux runner share a pipeline. The PR comment and review
/// formats sort and merge by that fingerprint, so their order and their
/// `merged:` fingerprints change too, in `dupes` and in `audit`.
/// `seams::codeclimate_clone_fingerprints_ignore_line_endings` shows the
/// same contract without the CLI.
#[test]
fn ci_clone_fingerprints_keep_crlf_invariance() {
    let pairs: Vec<(&str, String)> = CLONE_FINGERPRINT_PAIRS
        .iter()
        .map(|(command, format)| (*command, (*format).to_owned()))
        .collect();
    check_pairs(7, LineEndings::Crlf, &pairs);
}

/// The wide seed range of the daily `Oracle Seeds` workflow. Each project
/// seed checks every command in the CRLF form or in a mixed form, and the
/// baseline round trip.
#[test]
#[ignore = "wide seed range; the Oracle Seeds workflow runs it"]
fn seed_sweep_keeps_findings_identical() {
    let count = env_number(SEEDS_ENV).unwrap_or(DEFAULT_SWEEP_SEEDS);
    assert!(count >= 1, "{SEEDS_ENV} must be 1 or more");
    let start = env_number(SEED_START_ENV).unwrap_or(1);
    let all = pairs(&["dead-code", "dupes", "health", "audit"]);
    for seed in start..start + count {
        let endings = if seed % 2 == 0 {
            LineEndings::Crlf
        } else {
            LineEndings::Mixed(Seeded::new(seed).next_u64())
        };
        check_pairs(seed, endings, &all);
        check_baselines(seed, endings);
    }
}

#[test]
fn normalization_keeps_findings_and_drops_volatile_values() {
    let mut value: Value = serde_json::from_str(
        r#"{"elapsed_ms":14,"analysis_run_id":"run_1","span_start":67,"line":3,"fragment":"a\r\nb","message":"leak\r","fingerprint":"dup:12ab34s0","summary":"took 9ms (1.5s) in v2s"}"#,
    )
    .expect("JSON");
    normalize_json(&mut value);
    assert_eq!(
        value,
        serde_json::json!({
            "line": 3,
            "fragment": "a\nb",
            "message": "leak\r",
            "fingerprint": "dup:12ab34s0",
            "summary": "took [TIME] ([TIME]) in v2s"
        })
    );
}
