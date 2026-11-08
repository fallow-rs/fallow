//! Run each command and format with and without a permutation seed, and
//! compare the output bytes.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fallow_config::permutation::PERMUTATION_SEED_ENV;

use super::project::{Rng, materialize};
use super::templates::{TEMPLATES, Template, generate};
use crate::common::{fallow_bin, scrub_analysis_env};

/// Each value of `--format` that a report command accepts. `badge` is valid
/// for `health` only.
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

/// The formats of `trace` and of the focused `--trace-dependency` query.
const TRACE_FORMATS: [&str; 2] = ["human", "json"];

/// The largest number of runs at the same time. Each run is a process that
/// starts its own thread pool, and the test runner runs the templates in
/// parallel, so each check uses half of the cores.
const MAX_WORKERS: usize = 8;

const SEEDS_ENV: &str = "FALLOW_PERMUTATION_SEEDS";
const SEED_START_ENV: &str = "FALLOW_PERMUTATION_SEED_START";
const DEFAULT_SWEEP_SEEDS: u64 = 20;

/// The output of one run that must not change with the permutation.
#[derive(Debug, PartialEq, Eq)]
struct RunOutput {
    code: Option<i32>,
    stdout: String,
}

fn run(root: &Path, args: &[String], format: &str, seed: Option<u64>) -> RunOutput {
    let mut cmd = Command::new(fallow_bin());
    scrub_analysis_env(&mut cmd);
    cmd.env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .env_remove(PERMUTATION_SEED_ENV)
        .env_remove("FALLOW_AUDIT_BASE")
        .current_dir(root)
        .args(args)
        .args(["--root", ".", "--no-cache", "--quiet", "--format", format]);
    if let Some(seed) = seed {
        cmd.env(PERMUTATION_SEED_ENV, seed.to_string());
    }
    let output = cmd.output().expect("run the fallow binary");
    RunOutput {
        code: output.status.code(),
        stdout: redact_volatile(&String::from_utf8_lossy(&output.stdout)),
    }
}

/// Replace the volatile values: the measured durations and the per-run
/// telemetry id. These are the only parts of a report that may change between
/// two runs of the same analysis (`docs/backwards-compatibility.md`).
fn redact_volatile(text: &str) -> String {
    let text = redact_json_value(text, "\"elapsed_ms\"");
    let text = redact_json_value(&text, "\"analysis_run_id\"");
    redact_durations(&text)
}

/// Replace the number or string value of each `key` with `0`. The key may be
/// in compact or pretty JSON.
fn redact_json_value(text: &str, key: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(key) {
        let (head, tail) = rest.split_at(start + key.len());
        out.push_str(head);
        let after_colon = tail.trim_start().strip_prefix(':').map(str::trim_start);
        let Some(value) = after_colon else {
            rest = tail;
            continue;
        };
        out.push_str(&tail[..tail.len() - value.len()]);
        let end = value.strip_prefix('"').map_or_else(
            || {
                value
                    .find(|c: char| !c.is_ascii_digit())
                    .unwrap_or(value.len())
            },
            |string| string.find('"').map_or(value.len(), |close| close + 2),
        );
        out.push('0');
        rest = &value[end..];
    }
    out.push_str(rest);
    out
}

/// Replace a token such as `9ms` or `1.2s` after a space or `(` with `[TIME]`.
fn redact_durations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < bytes.len() {
        let starts_token = bytes[index].is_ascii_digit()
            && (index == 0 || matches!(bytes[index - 1], b' ' | b'('));
        if starts_token {
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
            let ends_token = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
            if unit > 0 && ends_token {
                out.push_str(&text[copied..index]);
                out.push_str("[TIME]");
                copied = after;
                index = after;
                continue;
            }
            index = end;
            continue;
        }
        index += 1;
    }
    out.push_str(&text[copied..]);
    out
}

/// The commands that the oracle runs, without the shared flags.
/// The commands that the oracle runs, without the shared flags, each with
/// the formats that it accepts.
fn commands(base: &str, dependency: &str, symbol: &str) -> Vec<(Vec<String>, Vec<&'static str>)> {
    let owned = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect();
    let mut health_formats = REPORT_FORMATS.to_vec();
    health_formats.push("badge");
    vec![
        (owned(&["dead-code"]), REPORT_FORMATS.to_vec()),
        (owned(&["dupes"]), REPORT_FORMATS.to_vec()),
        (owned(&["health"]), health_formats),
        (owned(&["audit", "--base", base]), REPORT_FORMATS.to_vec()),
        (
            owned(&["trace", "--dependency", dependency]),
            TRACE_FORMATS.to_vec(),
        ),
        (
            owned(&["dead-code", "--trace-dependency", dependency]),
            TRACE_FORMATS.to_vec(),
        ),
        (owned(&["trace", symbol]), TRACE_FORMATS.to_vec()),
    ]
}

/// The first line where two outputs differ, for the failure message.
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
                let offset = left
                    .char_indices()
                    .zip(right.chars())
                    .find(|((_, a), b)| a != b)
                    .map_or(0, |((offset, _), _)| offset);
                let start = left.floor_char_boundary(offset.saturating_sub(120));
                let window = |text: &str| {
                    text[text.floor_char_boundary(start)..]
                        .chars()
                        .take(240)
                        .collect::<String>()
                };
                format!(
                    "line {}, from byte {start}:\n  without the seed: {}\n  with the seed:    {}",
                    line + 1,
                    window(left),
                    window(right)
                )
            },
        )
}

/// One command and format, run without a seed and with each permutation seed.
struct Job {
    args: Vec<String>,
    format: &'static str,
}

impl Job {
    fn label(&self) -> String {
        format!("fallow {} --format {}", self.args.join(" "), self.format)
    }

    /// Run the job, and return a failure message for each difference. A run
    /// that fails as a tool (exit 2) is a failure too, because two equal tool
    /// errors would hide the output that the oracle must compare.
    fn check(&self, root: &Path, permutation_seeds: &[u64]) -> Vec<String> {
        let expected = run(root, &self.args, self.format, None);
        if !matches!(expected.code, Some(0 | 1)) {
            return vec![format!(
                "{}\n  the run without the seed exits {:?}, not 0 or 1",
                self.label(),
                expected.code
            )];
        }
        permutation_seeds
            .iter()
            .filter_map(|&seed| {
                let actual = run(root, &self.args, self.format, Some(seed));
                (expected != actual).then(|| {
                    format!(
                        "{} (permutation seed {seed})\n  exit codes: {:?} and {:?}\n  {}",
                        self.label(),
                        expected.code,
                        actual.code,
                        first_difference(&expected.stdout, &actual.stdout),
                    )
                })
            })
            .collect()
    }
}

/// Run `jobs` on `workers` threads, and collect the failure messages.
fn run_jobs(jobs: &[Job], workers: usize, root: &Path, permutation_seeds: &[u64]) -> Vec<String> {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let found = job.check(root, permutation_seeds);
                    failures.lock().expect("failure list").extend(found);
                }
            });
        }
    });
    failures.into_inner().expect("failure list")
}

/// Generate the project of `template` and `project_seed`, and check that no
/// permutation seed changes any output.
fn check(template: Template, project_seed: u64, permutation_seeds: &[u64]) {
    let project = generate(template, project_seed);
    let dir = tempfile::tempdir().expect("project directory");
    let root = dir.path();
    let base = materialize(root, &project, &mut Rng::new(project_seed));
    let (audit, other): (Vec<Job>, Vec<Job>) = commands(&base, project.dependency, project.symbol)
        .into_iter()
        .flat_map(|(args, formats)| {
            formats.into_iter().map(move |format| Job {
                args: args.clone(),
                format,
            })
        })
        .partition(|job| job.args[0] == "audit");
    let workers = std::thread::available_parallelism()
        .map_or(2, |count| (count.get() / 2).clamp(2, MAX_WORKERS));
    // The audit runs share one git repository and its base worktree, so they
    // run one at a time, next to the other runs.
    let (mut failures, other_failures) = std::thread::scope(|scope| {
        let serial = scope.spawn(|| run_jobs(&audit, 1, root, permutation_seeds));
        let parallel = run_jobs(&other, workers - 1, root, permutation_seeds);
        (serial.join().expect("audit runs"), parallel)
    });
    failures.extend(other_failures);
    failures.sort();
    assert!(
        failures.is_empty(),
        "the output changes with the discovery order\n  template: {template:?}\n  project seed: {project_seed}\n  permutation seeds: {permutation_seeds:?}\n{} of {} jobs fail. The first:\n  {}\nThe seed sweep runs this project seed again with {SEED_START_ENV}={project_seed} {SEEDS_ENV}=1.",
        failures.len(),
        audit.len() + other.len(),
        failures[0],
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
fn packages_keep_output_identical() {
    check(Template::Packages, 1, &[11, 12]);
}

#[test]
fn sveltekit_apps_keep_output_identical() {
    check(Template::SvelteKit, 2, &[29, 30]);
}

#[test]
fn nuxt_apps_keep_output_identical() {
    check(Template::Nuxt, 3, &[47, 48]);
}

/// The oracle passes without the hook, so this test shows that the binary
/// under test has it: a seed changes the order of the workspace list, which
/// `list --workspaces` prints as discovered.
#[test]
fn test_binary_has_the_permutation_hook() {
    let project = generate(Template::Packages, 1);
    let dir = tempfile::tempdir().expect("project directory");
    let root = dir.path();
    materialize(root, &project, &mut Rng::new(1));
    let list = |seed: Option<u64>| {
        let mut cmd = Command::new(fallow_bin());
        scrub_analysis_env(&mut cmd);
        cmd.env_remove(PERMUTATION_SEED_ENV)
            .args([
                "list",
                "--workspaces",
                "--root",
                ".",
                "--quiet",
                "--format",
                "json",
            ])
            .current_dir(root);
        if let Some(seed) = seed {
            cmd.env(PERMUTATION_SEED_ENV, seed.to_string());
        }
        let output = cmd.output().expect("run the fallow binary");
        assert!(output.status.success(), "list --workspaces fails");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let unpermuted = list(None);
    assert!(
        (1..=4).any(|seed| list(Some(seed)) != unpermuted),
        "{PERMUTATION_SEED_ENV} does not change the workspace order; build the binary with the permutation-hook feature of fallow-config"
    );
}

/// The wide seed range of the daily `Oracle Seeds` workflow. Each project
/// seed rotates over the templates and uses two permutation seeds derived
/// from it.
#[test]
#[ignore = "wide seed range; the Oracle Seeds workflow runs it"]
fn seed_sweep_keeps_output_identical() {
    let count = env_number(SEEDS_ENV).unwrap_or(DEFAULT_SWEEP_SEEDS);
    let start = env_number(SEED_START_ENV).unwrap_or(1);
    for project_seed in start..start + count {
        let template = TEMPLATES[usize::try_from(project_seed % 3).expect("index")];
        let mut rng = Rng::new(project_seed);
        check(template, project_seed, &[rng.next(), rng.next()]);
    }
}

/// A pair that the CLI rejects exits 2 with and without the seed. The
/// outputs are equal, so the check must report the tool error itself.
#[test]
fn rejected_format_fails_the_check() {
    let dir = tempfile::tempdir().expect("project directory");
    std::fs::write(dir.path().join("package.json"), r#"{"name":"empty"}"#).expect("manifest");
    let job = Job {
        args: vec!["dead-code".to_owned()],
        format: "badge",
    };
    let failures = job.check(dir.path(), &[1]);
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("exits Some(2)"), "{failures:?}");
}

#[test]
fn redaction_keeps_other_values() {
    assert_eq!(
        redact_volatile(
            r#"{"elapsed_ms":140,"total":4,"analysis_run_id":"run_1a"} took 9ms (1.5s) in 12 files, v2s"#
        ),
        r#"{"elapsed_ms":0,"total":4,"analysis_run_id":0} took [TIME] ([TIME]) in 12 files, v2s"#
    );
    assert_eq!(
        redact_volatile(
            "{\n  \"elapsed_ms\": 140,\n  \"analysis_run_id\": \"run_1a\",\n  \"total\": 4\n}"
        ),
        "{\n  \"elapsed_ms\": 0,\n  \"analysis_run_id\": 0,\n  \"total\": 4\n}"
    );
}
