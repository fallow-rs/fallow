//! `fallow report --mutation-report` and `--mutation-not-run`: the mutation
//! evidence of a CI job, joined to the saved analysis render.
//!
//! Most tests write the mutation report from the test. A test runner is not
//! necessary for a survived mutant, and each mutant keeps a known id and
//! line. The digests in `files[]` are the SHA-256 of the files on disk, the
//! same as in a report of `fallow mutation`.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::common::{commit_all, fallow_bin, git, git_capture, scrub_analysis_env};

const PACKAGE_JSON: &str =
    r#"{"name":"mutation-report-fixture","private":true,"main":"src/index.ts"}"#;
const INDEX: &str = "import { isAdult } from './age';\n\nexport const adult = isAdult(20);\n";
const AGE_BASE: &str = "export function isAdult(age: number): boolean {\n  return age >= 18;\n}\n";
/// `AGE_BASE` with `isSenior` on the added lines 4 to 7.
const AGE_HEAD: &str = "export function isAdult(age: number): boolean {\n  return age >= 18;\n}\n\nexport function isSenior(age: number): boolean {\n  return age > 65;\n}\n";
const UNUSED: &str = "export const unused = 1;\n";

/// A line of `src/age.ts` that the change did not touch.
const UNCHANGED_LINE: u32 = 2;
/// A line of `src/age.ts` that the change added.
const ADDED_LINE: u32 = 6;

const SURVIVED_RULE: &str = "fallow/mutation-survived";
const FORMATS_ONLY: &str = "--mutation-report and --mutation-not-run apply to the pr-comment, review, github-summary, github-annotations and codeclimate formats only";
const NOT_FOR_FIX: &str =
    "--mutation-report and --mutation-not-run do not apply to a saved fallow fix envelope";

/// The environment variables that change a CI render. Each run removes them,
/// so the shell of a developer cannot change the result.
const AMBIENT_CI_ENV: &[&str] = &[
    "FALLOW_DIFF_FILE",
    "FALLOW_DIFF_FILTER",
    "FALLOW_DIFF_CONTEXT",
    "FALLOW_SUMMARY_SCOPE",
    "FALLOW_MAX_COMMENTS",
    "FALLOW_COMMENT_ID",
    "FALLOW_PR_COMMENT_LAYOUT",
    "FALLOW_PR_COMMENT_ENVELOPE_FILE",
    "FALLOW_PR_DECISION_FILE",
    "FALLOW_PR_DETAILS_FILE",
    "FALLOW_REVIEW_ID",
    "FALLOW_REVIEW_GUIDANCE",
    "FALLOW_GITLAB_BASE_SHA",
    "FALLOW_GITLAB_START_SHA",
    "FALLOW_GITLAB_HEAD_SHA",
    "CI_MERGE_REQUEST_DIFF_BASE_SHA",
    "CI_COMMIT_SHA",
];

/// A git project with two commits: the second commit adds `isSenior` to
/// `src/age.ts`. `src/unused.ts` and the export `isSenior` are dead code, so
/// the analysis has findings.
struct Fixture {
    project: tempfile::TempDir,
    /// Files outside the project: the saved results, the reports, the diff
    /// and the sidecars. The worktree of the project stays clean.
    store: tempfile::TempDir,
    results: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let project = tempfile::tempdir().expect("project dir");
        write_project(project.path());
        let store = tempfile::tempdir().expect("store dir");
        let results = store.path().join("results.json");
        save_dead_code_results(project.path(), &results);
        Self {
            project,
            store,
            results,
        }
    }

    fn root(&self) -> &Path {
        self.project.path()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.store.path().join(name)
    }

    /// The diff of the second commit, with paths from the repository top.
    fn diff(&self) -> PathBuf {
        let diff = git_capture(self.root(), &["diff", "--unified=0", "HEAD~1", "HEAD"]);
        let path = self.path("pr.diff");
        fs::write(&path, format!("{diff}\n")).expect("write diff");
        path
    }

    fn write_report(&self, name: &str, report: &Value) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, report.to_string()).expect("write mutation report");
        path
    }

    /// `fallow report --from <results> --root <project> --quiet --format
    /// <format>` with `extra` and `env`.
    fn report(&self, format: &str, extra: &[&str], env: &[(&str, &Path)]) -> Output {
        let mut args = vec![
            "report".to_owned(),
            "--from".to_owned(),
            self.results.display().to_string(),
            "--root".to_owned(),
            self.root().display().to_string(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            format.to_owned(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        run(self.root(), &args, env)
    }

    /// The pr-comment render with the three sidecar files. `label` keeps the
    /// sidecars of two renders apart.
    fn pr_comment(&self, format: &str, label: &str, extra: &[&str]) -> Rendered {
        let envelope = self.path(&format!("{label}-envelope.json"));
        let decision = self.path(&format!("{label}-decision.json"));
        let details = self.path(&format!("{label}-details.json"));
        let output = self.report(
            format,
            extra,
            &[
                ("FALLOW_PR_COMMENT_ENVELOPE_FILE", &envelope),
                ("FALLOW_PR_DECISION_FILE", &decision),
                ("FALLOW_PR_DETAILS_FILE", &details),
            ],
        );
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        Rendered {
            body: stdout(&output),
            envelope: read_json(&envelope),
            decision: read_json(&decision),
        }
    }
}

struct Rendered {
    body: String,
    envelope: Value,
    decision: Value,
}

fn write_project(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("src dir");
    fs::write(root.join("package.json"), PACKAGE_JSON).expect("package.json");
    fs::write(root.join("src/index.ts"), INDEX).expect("index.ts");
    fs::write(root.join("src/age.ts"), AGE_BASE).expect("age.ts");
    fs::write(root.join("src/unused.ts"), UNUSED).expect("unused.ts");
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n").expect(".gitignore");
    git(root, &["init", "-q"]);
    commit_all(root, "base");
    fs::write(root.join("src/age.ts"), AGE_HEAD).expect("age.ts change");
    commit_all(root, "add isSenior");
}

fn save_dead_code_results(root: &Path, results: &Path) {
    let output = run(
        root,
        &[
            "dead-code".to_owned(),
            "--root".to_owned(),
            root.display().to_string(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ],
        &[],
    );
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "analysis failed: {}",
        stderr(&output)
    );
    fs::write(results, &output.stdout).expect("write saved results");
}

fn run(root: &Path, args: &[String], env: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(fallow_bin());
    command
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "")
        .args(args);
    scrub_analysis_env(&mut command);
    for name in AMBIENT_CI_ENV {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("run fallow")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn read_json(path: &Path) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).expect("sidecar JSON")
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// One mutant of `src/age.ts`.
fn mutant(id: &str, line: u32, status: &str) -> Value {
    json!({
        "mutant_id": id, "file": "src/age.ts", "line": line, "column": 14,
        "span": [62, 64], "operator": "relational-operator", "function": "isAdult",
        "change": { "from": ">=", "to": ">" }, "status": status,
        "covering_tests": [], "killing_tests": []
    })
}

/// A complete `kind: "mutation"` report of `mutants`, with the digest of each
/// file under `root`. With `min_score`, the report has the gate entry that
/// `fallow mutation --min-score` writes.
fn mutation_report(root: &Path, mutants: &[Value], min_score: Option<f64>) -> Value {
    let count = |status: &str| {
        mutants
            .iter()
            .filter(|mutant| mutant["status"] == status)
            .count()
    };
    let (killed, survived, no_coverage) =
        (count("killed"), count("survived"), count("no_coverage"));
    #[expect(
        clippy::cast_precision_loss,
        reason = "the mutant counts of a test are small"
    )]
    let score = (killed + survived > 0).then(|| killed as f64 / (killed + survived) as f64);
    let mut files: Vec<&str> = mutants
        .iter()
        .filter_map(|mutant| mutant["file"].as_str())
        .collect();
    files.sort_unstable();
    files.dedup();
    let files: Vec<Value> = files
        .into_iter()
        .map(|file| {
            let bytes = fs::read(root.join(file)).unwrap_or_default();
            json!({ "path": file, "source_sha256": sha256(&bytes) })
        })
        .collect();
    let mut report = json!({
        "kind": "mutation",
        "schema_version": "2",
        "version": "0.0.0",
        "elapsed_ms": 252_000,
        "completion": { "status": "complete", "reasons": [] },
        "scope": { "kind": "changed", "base_ref": "HEAD~1" },
        "summary": {
            "counts": {
                "total": mutants.len(), "killed": killed, "timeout": 0,
                "survived": survived, "no_coverage": no_coverage, "runtime_error": 0,
                "flaky": 0, "pruned": 0, "pending": 0, "equivalent": 0, "compile_error": 0
            },
            "score": score
        },
        "mutants": mutants,
        "files": files
    });
    if let Some(min) = min_score {
        let observed = score.map(|score| score * 100.0);
        let status = match observed {
            None => "skipped",
            Some(observed) if observed < min => "fail",
            Some(_) => "pass",
        };
        report["gate_outcomes"] = json!({ "mutation-min-score": {
            "status": status, "enforced": true, "observed": observed, "threshold": min
        } });
    }
    report
}

/// Two survivors (one on an unchanged line, one on an added line) and two
/// killed mutants: the score is 50%, below the minimum of 80%.
fn gate_failing_report(fixture: &Fixture) -> PathBuf {
    let report = mutation_report(
        fixture.root(),
        &[
            mutant("aaaa0001", UNCHANGED_LINE, "survived"),
            mutant("aaaa0002", ADDED_LINE, "survived"),
            mutant("aaaa0003", UNCHANGED_LINE, "killed"),
            mutant("aaaa0004", ADDED_LINE, "killed"),
        ],
        Some(80.0),
    );
    fixture.write_report("mutation.json", &report)
}

fn gate_ids(decision: &Value) -> Vec<&str> {
    decision["gates"]
        .as_array()
        .expect("decision gates")
        .iter()
        .map(|gate| gate["id"].as_str().expect("gate id"))
        .collect()
}

fn gate<'a>(decision: &'a Value, id: &str) -> &'a Value {
    decision["gates"]
        .as_array()
        .expect("decision gates")
        .iter()
        .find(|gate| gate["id"] == id)
        .unwrap_or_else(|| panic!("no `{id}` gate row in {decision}"))
}

fn annotation_titles(decision: &Value) -> Vec<&str> {
    decision["annotations"]
        .as_array()
        .expect("decision annotations")
        .iter()
        .map(|annotation| annotation["title"].as_str().expect("annotation title"))
        .collect()
}

#[test]
fn pr_comments_join_the_survivors_the_section_and_the_decision_rows() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let report_arg = report.display().to_string();
    for format in ["pr-comment-github", "pr-comment-gitlab"] {
        let plain = fixture.pr_comment(format, &format!("{format}-plain"), &[]);
        let joined = fixture.pr_comment(
            format,
            &format!("{format}-joined"),
            &["--mutation-report", &report_arg],
        );
        let body = &joined.body;

        let footer = body.find("Generated by fallow.").expect("footer");
        let survivor = body.find(SURVIVED_RULE).expect("survivor row");
        let section = body.find("#### Mutation testing").expect("section");
        assert!(survivor < footer, "{format}: survivor after footer\n{body}");
        assert!(section < footer, "{format}: section after footer\n{body}");
        assert!(body.contains("Tests"), "{format}: no Tests table\n{body}");
        assert!(
            body.contains(
                "**Score below the minimum.** The score is 50.0%, and the minimum is 80%."
            ),
            "{format}: no gate headline\n{body}"
        );

        let ids = gate_ids(&joined.decision);
        assert_eq!(ids[0], "dead-code", "{format}: the command row stays first");
        assert_eq!(ids[1], "mutation", "{format}: {ids:?}");
        assert!(ids.contains(&"mutation-min-score"), "{format}: {ids:?}");
        assert_eq!(
            gate(&joined.decision, "mutation-min-score")["status"],
            "failure"
        );
        assert_eq!(gate(&joined.decision, "mutation")["scope"], "changed lines");

        let titles = annotation_titles(&joined.decision);
        let first_survivor = titles
            .iter()
            .position(|title| *title == SURVIVED_RULE)
            .expect("survivor annotations");
        assert!(
            titles[first_survivor..]
                .iter()
                .all(|title| *title == SURVIVED_RULE),
            "{format}: the survivors come after the analysis annotations: {titles:?}"
        );
        assert_eq!(
            titles[..first_survivor],
            annotation_titles(&plain.decision)[..],
            "{format}: the analysis annotations keep their order"
        );
        assert_eq!(titles.len() - first_survivor, 2, "{format}: {titles:?}");

        assert_eq!(
            joined.decision["conclusion"], plain.decision["conclusion"],
            "{format}: a failed mutation gate does not change the conclusion"
        );
        assert_eq!(joined.envelope["is_clean"], false);
        let total = |rendered: &Rendered| {
            rendered.envelope["truncation"]["total_findings"]
                .as_u64()
                .expect("total findings")
        };
        assert_eq!(total(&joined), total(&plain) + 2, "{format}");
        assert_eq!(
            joined.envelope["truncation"]["shown_findings"],
            total(&joined),
            "{format}"
        );
    }
}

#[test]
fn a_clean_analysis_with_only_a_failed_gate_keeps_the_success_conclusion() {
    let fixture = Fixture::new();
    let report = mutation_report(
        fixture.root(),
        &[
            mutant("bbbb0001", UNCHANGED_LINE, "killed"),
            mutant("bbbb0002", ADDED_LINE, "no_coverage"),
        ],
        Some(101.0),
    );
    let report = fixture.write_report("gate-only.json", &report);
    let clean = fixture.path("clean.json");
    fs::write(&clean, clean_results(&fixture.results).to_string()).expect("clean results");
    let plain = clean_pr_comment(&fixture, &clean, "clean-plain", &[]);
    assert_eq!(plain.decision["conclusion"], "success", "{}", plain.body);
    let joined = clean_pr_comment(
        &fixture,
        &clean,
        "clean-joined",
        &["--mutation-report", &report.display().to_string()],
    );
    assert_eq!(joined.decision["conclusion"], "success");
    assert_eq!(joined.envelope["is_clean"], true);
    assert_eq!(
        gate(&joined.decision, "mutation-min-score")["status"],
        "failure"
    );
    assert!(
        joined.body.contains("mutation-min-score (100 against 101)"),
        "the status note names the gate\n{}",
        joined.body
    );
}

/// The saved results at `path` with no finding and no gate outcome.
fn clean_results(path: &Path) -> Value {
    let mut results = read_json(path);
    let object = results.as_object_mut().expect("results object");
    object.remove("gate_outcomes");
    object.insert("total_issues".to_owned(), json!(0));
    for value in object.values_mut() {
        if value.is_array() {
            *value = json!([]);
        }
    }
    results
}

fn clean_pr_comment(fixture: &Fixture, results: &Path, label: &str, extra: &[&str]) -> Rendered {
    let envelope = fixture.path(&format!("{label}-envelope.json"));
    let decision = fixture.path(&format!("{label}-decision.json"));
    let mut args = vec![
        "report".to_owned(),
        "--from".to_owned(),
        results.display().to_string(),
        "--root".to_owned(),
        fixture.root().display().to_string(),
        "--quiet".to_owned(),
        "--format".to_owned(),
        "pr-comment-github".to_owned(),
    ];
    args.extend(extra.iter().map(|arg| (*arg).to_owned()));
    let output = run(
        fixture.root(),
        &args,
        &[
            ("FALLOW_PR_COMMENT_ENVELOPE_FILE", &envelope),
            ("FALLOW_PR_DECISION_FILE", &decision),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    Rendered {
        body: stdout(&output),
        envelope: read_json(&envelope),
        decision: read_json(&decision),
    }
}

/// The new-side line of a review comment of either provider.
fn comment_line(comment: &Value) -> u64 {
    comment["line"]
        .as_u64()
        .or_else(|| comment["position"]["new_line"].as_u64())
        .unwrap_or_else(|| panic!("no line in {comment}"))
}

fn survivor_lines(review: &Value) -> Vec<u64> {
    review["comments"]
        .as_array()
        .expect("review comments")
        .iter()
        .filter(|comment| {
            comment["body"]
                .as_str()
                .is_some_and(|body| body.contains(SURVIVED_RULE))
        })
        .map(comment_line)
        .collect()
}

#[test]
fn reviews_keep_the_survivor_on_an_added_line_and_drop_the_unchanged_one() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let diff = fixture.diff();
    for format in ["review-github", "review-gitlab"] {
        let output = fixture.report(
            format,
            &["--mutation-report", &report.display().to_string()],
            &[("FALLOW_DIFF_FILE", &diff)],
        );
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let review: Value = serde_json::from_slice(&output.stdout).expect("review JSON");
        assert_eq!(
            survivor_lines(&review),
            [u64::from(ADDED_LINE)],
            "{format}: {review:#}"
        );
        let survivor = review["comments"]
            .as_array()
            .expect("comments")
            .iter()
            .find(|comment| comment_line(comment) == u64::from(ADDED_LINE))
            .expect("survivor comment");
        let body = survivor["body"].as_str().expect("comment body");
        assert!(
            body.contains("made no test fail. Mutant aaaa0002."),
            "{format}: {body}"
        );
        assert!(
            !body.contains("```suggestion"),
            "{format}: no suggestion\n{body}"
        );
        let summary = review_summary_text(&review);
        assert!(
            summary.contains("Mutation testing: score 50.0%, 2 survivors (1 inline)."),
            "{format}: summary line\n{summary}"
        );

        let guided = fixture.report(
            format,
            &["--mutation-report", &report.display().to_string()],
            &[
                ("FALLOW_DIFF_FILE", &diff),
                ("FALLOW_REVIEW_GUIDANCE", Path::new("1")),
            ],
        );
        let guided: Value = serde_json::from_slice(&guided.stdout).expect("review JSON");
        let bodies = guided["comments"].to_string();
        assert!(
            bodies.contains(
                "Add or change a test so that it fails on this change. To see the code, run `fallow mutation inspect` with the mutant ID that the finding names."
            ),
            "{format}: the opt-in guidance of the rule\n{bodies}"
        );
    }
}

/// The text of the review body and of the sticky summary comment.
fn review_summary_text(review: &Value) -> String {
    format!(
        "{}\n{}",
        review["body"].as_str().unwrap_or_default(),
        review["summary"]["body"].as_str().unwrap_or_default()
    )
}

#[test]
fn github_summary_renders_the_section_for_each_flag() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let plain = stdout(&fixture.report("github-summary", &[], &[]));

    let joined = fixture.report(
        "github-summary",
        &["--mutation-report", &report.display().to_string()],
        &[],
    );
    assert_eq!(joined.status.code(), Some(0), "{}", stderr(&joined));
    let body = stdout(&joined);
    assert!(body.contains("## Mutation testing"), "{body}");
    assert!(
        body.contains("The run took 4m 12s."),
        "the job summary has the elapsed time\n{body}"
    );
    let section = body.find("## Mutation testing").expect("section");
    let analysis = body.find("Unused exports").expect("analysis body");
    assert!(
        analysis < section,
        "the section follows the analysis\n{body}"
    );
    assert!(
        body.contains("mutation-min-score (50 against 80)"),
        "the gate line names the mutation gate\n{body}"
    );

    let not_run = fixture.report(
        "github-summary",
        &["--mutation-not-run", "event-refused"],
        &[],
    );
    assert_eq!(not_run.status.code(), Some(0), "{}", stderr(&not_run));
    let body = stdout(&not_run);
    assert!(
        body.contains("**Mutation testing did not run.** This event can run the code of a fork"),
        "{body}"
    );
    assert!(
        body.starts_with(
            plain
                .trim_end_matches('\n')
                .lines()
                .next()
                .unwrap_or_default()
        ),
        "the analysis summary stays first\n{body}"
    );
}

#[test]
fn github_annotations_append_the_run_state_and_the_survivors_for_each_flag() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let plain = stdout(&fixture.report("github-annotations", &[], &[]));
    assert!(!plain.contains("Mutation testing"), "{plain}");

    let joined = fixture.report(
        "github-annotations",
        &["--mutation-report", &report.display().to_string()],
        &[],
    );
    assert_eq!(joined.status.code(), Some(0), "{}", stderr(&joined));
    let lines: Vec<String> = stdout(&joined).lines().map(str::to_owned).collect();
    let survivors: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            line.starts_with("::warning file=src/age.ts,line=")
                && line.contains("title=fallow/mutation-survived::")
        })
        .map(|(index, _)| index)
        .collect();
    assert_eq!(survivors.len(), 2, "{lines:#?}");
    let last_analysis = lines
        .iter()
        .rposition(|line| {
            line.starts_with("::") && line.contains("file=") && !line.contains(SURVIVED_RULE)
        })
        .expect("analysis annotations");
    assert!(
        survivors[0] > last_analysis,
        "the survivors come after the analysis annotations: {lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("mutation-min-score (50 against 80)")),
        "the gate line names the mutation gate: {lines:#?}"
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains("title=Mutation testing")),
        "a complete run has no run-state line: {lines:#?}"
    );

    let interrupted = fixture.report(
        "github-annotations",
        &["--mutation-not-run", "interrupted"],
        &[],
    );
    assert_eq!(
        interrupted.status.code(),
        Some(0),
        "{}",
        stderr(&interrupted)
    );
    let body = stdout(&interrupted);
    assert!(
        body.lines()
            .any(|line| line.starts_with("::error title=Mutation testing::")),
        "{body}"
    );

    let refused = fixture.report(
        "github-annotations",
        &["--mutation-not-run", "fork-refused"],
        &[],
    );
    assert_eq!(refused.status.code(), Some(0), "{}", stderr(&refused));
    assert_eq!(stdout(&refused), plain, "a refused run adds no annotation");
}

#[test]
fn a_failed_run_envelope_gives_an_error_annotation_and_a_failure_row() {
    let fixture = Fixture::new();
    let failed = fixture.write_report(
        "failed.json",
        &json!({
            "error": true, "exit_code": 2, "code": "FALLOW_MUTATION_NO_RUNNER",
            "message": "no test runner was found\n::error::forged", "help": "install vitest"
        }),
    );
    let failed_arg = failed.display().to_string();
    let annotations = fixture.report(
        "github-annotations",
        &["--mutation-report", &failed_arg],
        &[],
    );
    assert_eq!(
        annotations.status.code(),
        Some(0),
        "{}",
        stderr(&annotations)
    );
    let body = stdout(&annotations);
    let error_lines: Vec<&str> = body
        .lines()
        .filter(|line| line.starts_with("::error title=Mutation testing::"))
        .collect();
    assert_eq!(error_lines.len(), 1, "{body}");
    assert!(
        error_lines[0].contains("FALLOW_MUTATION_NO_RUNNER"),
        "{body}"
    );
    assert!(
        !body.lines().any(|line| line.starts_with("::error::forged")),
        "the message cannot start a workflow command\n{body}"
    );

    let comment = fixture.pr_comment(
        "pr-comment-github",
        "failed",
        &["--mutation-report", &failed_arg],
    );
    assert_eq!(gate(&comment.decision, "mutation")["status"], "failure");
    assert!(
        comment
            .body
            .contains("**Mutation testing failed** (exit code 2, `FALLOW_MUTATION_NO_RUNNER`)."),
        "{}",
        comment.body
    );
}

#[test]
fn codeclimate_appends_the_survivors_for_each_flag() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let plain_output = fixture.report("codeclimate", &[], &[]);
    let plain: Value = serde_json::from_slice(&plain_output.stdout).expect("plain CodeClimate");
    let plain = plain.as_array().expect("CodeClimate issues").clone();

    let joined = fixture.report(
        "codeclimate",
        &["--mutation-report", &report.display().to_string()],
        &[],
    );
    assert_eq!(joined.status.code(), Some(0), "{}", stderr(&joined));
    let issues: Value = serde_json::from_slice(&joined.stdout).expect("CodeClimate JSON");
    let issues = issues.as_array().expect("CodeClimate issues");
    assert_eq!(
        issues[..plain.len()],
        plain[..],
        "the analysis issues stay first"
    );
    let survivors = &issues[plain.len()..];
    assert_eq!(survivors.len(), 2, "{issues:#?}");
    for issue in survivors {
        assert_eq!(issue["check_name"], SURVIVED_RULE);
        assert_eq!(issue["severity"], "minor");
        assert_eq!(issue["location"]["path"], "src/age.ts");
    }

    let diff = fixture.diff();
    let filtered = fixture.report(
        "codeclimate",
        &["--mutation-report", &report.display().to_string()],
        &[("FALLOW_DIFF_FILE", &diff)],
    );
    let filtered: Value = serde_json::from_slice(&filtered.stdout).expect("CodeClimate JSON");
    let lines: Vec<&Value> = filtered
        .as_array()
        .expect("issues")
        .iter()
        .filter(|issue| issue["check_name"] == SURVIVED_RULE)
        .map(|issue| &issue["location"]["lines"]["begin"])
        .collect();
    assert_eq!(lines, [&json!(ADDED_LINE)], "the diff filter applies");

    let not_run = fixture.report(
        "codeclimate",
        &["--mutation-not-run", "no-change-scope"],
        &[],
    );
    assert_eq!(not_run.status.code(), Some(0), "{}", stderr(&not_run));
    assert_eq!(
        not_run.stdout, plain_output.stdout,
        "a job that did not run adds no CodeClimate issue"
    );
}

#[test]
fn sarif_and_markdown_refuse_each_flag() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let report_arg = report.display().to_string();
    for format in ["sarif", "markdown"] {
        for flag in [
            ["--mutation-report", report_arg.as_str()],
            ["--mutation-not-run", "event-refused"],
        ] {
            let output = fixture.report(format, &flag, &[]);
            assert_eq!(output.status.code(), Some(2), "{format} {flag:?}");
            assert!(
                stderr(&output).contains(FORMATS_ONLY),
                "{format} {flag:?}: {}",
                stderr(&output)
            );
            assert!(output.stdout.is_empty(), "{format} {flag:?}");
        }
    }
}

#[test]
fn a_saved_fix_envelope_refuses_the_flags() {
    let fixture = Fixture::new();
    let fix = fixture.path("fix.json");
    let output = run(
        fixture.root(),
        &[
            "fix".to_owned(),
            "--root".to_owned(),
            fixture.root().display().to_string(),
            "--dry-run".to_owned(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            "json".to_owned(),
        ],
        &[],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    fs::write(&fix, &output.stdout).expect("save fix envelope");
    let rendered = run(
        fixture.root(),
        &[
            "report".to_owned(),
            "--from".to_owned(),
            fix.display().to_string(),
            "--root".to_owned(),
            fixture.root().display().to_string(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            "github-annotations".to_owned(),
            "--mutation-not-run".to_owned(),
            "event-refused".to_owned(),
        ],
        &[],
    );
    assert_eq!(rendered.status.code(), Some(2), "{}", stderr(&rendered));
    assert!(
        stderr(&rendered).contains(NOT_FOR_FIX),
        "{}",
        stderr(&rendered)
    );
}

#[test]
fn the_two_flags_together_are_a_usage_error() {
    let fixture = Fixture::new();
    let report = gate_failing_report(&fixture);
    let output = fixture.report(
        "pr-comment-github",
        &[
            "--mutation-report",
            &report.display().to_string(),
            "--mutation-not-run",
            "interrupted",
        ],
        &[],
    );
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(text.contains("cannot be used with"), "{text}");
    assert!(output.stdout.is_empty());
}

#[test]
fn an_unknown_not_run_reason_is_a_usage_error() {
    let fixture = Fixture::new();
    let output = fixture.report("pr-comment-github", &["--mutation-not-run", "maybe"], &[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("event-refused"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn input_errors_of_the_mutation_report_exit_2() {
    let fixture = Fixture::new();
    let plan = fixture.write_report(
        "plan.json",
        &json!({ "kind": "mutation-plan", "mutants": [] }),
    );
    let outside = fixture.write_report(
        "outside.json",
        &mutation_report(
            fixture.root(),
            &[json!({
                "mutant_id": "cccc0001", "file": "../outside.ts", "line": 1, "column": 1,
                "span": [0, 1], "operator": "relational-operator", "status": "survived"
            })],
            None,
        ),
    );
    let missing = fixture.path("missing.json");
    for path in [plan, outside, missing] {
        let output = fixture.report(
            "pr-comment-github",
            &["--mutation-report", &path.display().to_string()],
            &[],
        );
        assert_eq!(output.status.code(), Some(2), "{}", path.display());
        assert!(
            stderr(&output).contains("Error: FALLOW_MUTATION_REPORT_INVALID: "),
            "{}: {}",
            path.display(),
            stderr(&output)
        );
        assert!(output.stdout.is_empty(), "{}", path.display());
    }
}

#[test]
fn an_unreadable_report_keeps_the_analysis_body_and_exits_0() {
    let fixture = Fixture::new();
    let mut report = mutation_report(
        fixture.root(),
        &[mutant("dddd0001", ADDED_LINE, "survived")],
        Some(80.0),
    );
    report["mutants"][0]["line"] = json!("six");
    let report = fixture.write_report("unreadable.json", &report);
    let plain = fixture.pr_comment("pr-comment-github", "unreadable-plain", &[]);

    let envelope = fixture.path("unreadable-envelope.json");
    let decision = fixture.path("unreadable-decision.json");
    let output = fixture.report(
        "pr-comment-github",
        &["--mutation-report", &report.display().to_string()],
        &[
            ("FALLOW_PR_COMMENT_ENVELOPE_FILE", &envelope),
            ("FALLOW_PR_DECISION_FILE", &decision),
        ],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("warning: FALLOW_MUTATION_REPORT_INVALID: "),
        "{}",
        stderr(&output)
    );
    let decision = read_json(&decision);
    assert_eq!(gate(&decision, "mutation")["status"], "failure");
    let body = stdout(&output);
    assert!(
        body.contains("**Fallow could not read the mutation report**"),
        "{body}"
    );
    for row in plain.body.lines().filter(|line| line.starts_with("| ")) {
        assert!(body.contains(row), "the analysis row stays: {row}\n{body}");
    }
    assert!(!body.contains(SURVIVED_RULE), "{body}");
}

#[test]
fn ten_thousand_survivors_keep_each_body_under_the_provider_limit() {
    const PROVIDER_LIMIT: usize = 65_536;
    let fixture = Fixture::new();
    let mutants: Vec<Value> = (1..=10_000_u32)
        .map(|index| mutant(&format!("{index:08x}"), index, "survived"))
        .collect();
    let report = fixture.write_report(
        "many.json",
        &mutation_report(fixture.root(), &mutants, Some(80.0)),
    );
    let report_arg = report.display().to_string();
    for format in ["pr-comment-github", "pr-comment-gitlab"] {
        let rendered = fixture.pr_comment(
            format,
            &format!("{format}-many"),
            &["--mutation-report", &report_arg],
        );
        let chars = rendered.body.chars().count();
        assert!(chars < PROVIDER_LIMIT, "{format}: {chars} characters");
        assert!(
            rendered
                .body
                .contains("9980 more survived mutants are not shown inline."),
            "{format}: the overflow line\n{}",
            rendered.body
        );
        let survivors = annotation_titles(&rendered.decision)
            .iter()
            .filter(|title| **title == SURVIVED_RULE)
            .count();
        assert_eq!(survivors, 20, "{format}: the inline cap");
    }
    for format in ["review-github", "review-gitlab"] {
        let output = fixture.report(format, &["--mutation-report", &report_arg], &[]);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        let review: Value = serde_json::from_slice(&output.stdout).expect("review JSON");
        for text in [&review["body"], &review["summary"]["body"]] {
            let chars = text.as_str().unwrap_or_default().chars().count();
            assert!(chars < PROVIDER_LIMIT, "{format}: {chars} characters");
        }
    }
    let summary = fixture.report("github-summary", &["--mutation-report", &report_arg], &[]);
    let chars = stdout(&summary).chars().count();
    assert!(chars < PROVIDER_LIMIT, "github-summary: {chars} characters");
}

/// A repository with the project in `packages/app`, and a second commit that
/// adds `isSenior`.
fn monorepo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("repo dir");
    let app = repo.path().join("packages/app");
    fs::create_dir_all(app.join("src")).expect("app src");
    fs::write(app.join("package.json"), PACKAGE_JSON).expect("package.json");
    fs::write(app.join("src/index.ts"), INDEX).expect("index.ts");
    fs::write(app.join("src/age.ts"), AGE_BASE).expect("age.ts");
    fs::write(repo.path().join(".gitignore"), "node_modules\n.fallow\n").expect(".gitignore");
    git(repo.path(), &["init", "-q"]);
    commit_all(repo.path(), "base");
    fs::write(app.join("src/age.ts"), AGE_HEAD).expect("age.ts change");
    commit_all(repo.path(), "add isSenior");
    repo
}

/// The lines of the mutants that `fallow mutation --diff-file <diff>` plans
/// for the app, and the lines of the survivor comments that the review keeps
/// with the same diff, when each planned mutant survives.
fn mutation_and_review_lines(repo: &Path, store: &Path, diff: &Path) -> (Vec<u64>, Vec<u64>) {
    let app = repo.join("packages/app");
    let report_path = store.join("mutation.json");
    let output = run(
        &app,
        &[
            "mutation".to_owned(),
            "--root".to_owned(),
            app.display().to_string(),
            "--diff-file".to_owned(),
            diff.display().to_string(),
            "--format".to_owned(),
            "json".to_owned(),
            "--quiet".to_owned(),
            "-o".to_owned(),
            report_path.display().to_string(),
        ],
        &[],
    );
    assert!(
        matches!(output.status.code(), Some(0 | 1)),
        "mutation failed: {}",
        stderr(&output)
    );
    let mut report = read_json(&report_path);
    assert_eq!(report["kind"], "mutation", "{report:#}");
    let mut mutation_lines: Vec<u64> = Vec::new();
    for mutant in report["mutants"].as_array_mut().expect("mutants") {
        mutation_lines.push(mutant["line"].as_u64().expect("mutant line"));
        mutant["status"] = json!("survived");
    }
    mutation_lines.sort_unstable();
    mutation_lines.dedup();
    let survived = report["mutants"].as_array().expect("mutants").len();
    report["summary"]["counts"]["survived"] = json!(survived);
    report["summary"]["counts"]["no_coverage"] = json!(0);
    report["summary"]["counts"]["pending"] = json!(0);
    report["summary"]["score"] = json!(0.0);
    let survived_report = store.join("survived.json");
    fs::write(&survived_report, report.to_string()).expect("write survived report");

    let results = store.join("results.json");
    save_dead_code_results(&app, &results);
    let review = run(
        &app,
        &[
            "report".to_owned(),
            "--from".to_owned(),
            results.display().to_string(),
            "--root".to_owned(),
            app.display().to_string(),
            "--quiet".to_owned(),
            "--format".to_owned(),
            "review-github".to_owned(),
            "--mutation-report".to_owned(),
            survived_report.display().to_string(),
        ],
        &[("FALLOW_DIFF_FILE", diff)],
    );
    assert_eq!(review.status.code(), Some(0), "{}", stderr(&review));
    let review: Value = serde_json::from_slice(&review.stdout).expect("review JSON");
    let mut review_lines = survivor_lines(&review);
    review_lines.sort_unstable();
    review_lines.dedup();
    (mutation_lines, review_lines)
}

#[test]
fn a_repo_top_and_a_root_relative_diff_scope_the_run_and_the_review_to_the_same_lines() {
    let repo = monorepo();
    let store = tempfile::tempdir().expect("store dir");
    let top_diff = store.path().join("top.diff");
    let top = git_capture(repo.path(), &["diff", "--unified=0", "HEAD~1", "HEAD"]);
    assert!(top.contains("+++ b/packages/app/src/age.ts"), "{top}");
    fs::write(&top_diff, format!("{top}\n")).expect("write top diff");
    let relative_diff = store.path().join("relative.diff");
    let relative = git_capture(
        &repo.path().join("packages/app"),
        &["diff", "--unified=0", "--relative", "HEAD~1", "HEAD"],
    );
    assert!(relative.contains("+++ b/src/age.ts"), "{relative}");
    fs::write(&relative_diff, format!("{relative}\n")).expect("write relative diff");

    let added: Vec<u64> = (4..=7).collect();
    let mut seen = Vec::new();
    for diff in [&top_diff, &relative_diff] {
        let label = diff.display().to_string();
        let run_store = tempfile::tempdir().expect("run store");
        let (mutation_lines, review_lines) =
            mutation_and_review_lines(repo.path(), run_store.path(), diff);
        assert!(
            !mutation_lines.is_empty(),
            "{label}: the run planned no mutant"
        );
        assert!(
            mutation_lines.iter().all(|line| added.contains(line)),
            "{label}: the run mutates only added lines: {mutation_lines:?}"
        );
        assert_eq!(
            review_lines, mutation_lines,
            "{label}: the review keeps the survivor of each planned line"
        );
        seen.push(mutation_lines);
    }
    assert_eq!(seen[0], seen[1], "both diffs give the same lines");
}

#[test]
fn without_the_flags_each_body_is_unchanged() {
    let fixture = Fixture::new();
    let diff = fixture.diff();
    for format in [
        "pr-comment-github",
        "pr-comment-gitlab",
        "review-github",
        "review-gitlab",
        "github-summary",
        "github-annotations",
        "codeclimate",
    ] {
        let output = fixture.report(format, &[], &[("FALLOW_DIFF_FILE", &diff)]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{format}: {}",
            stderr(&output)
        );
        let body = stdout(&output);
        assert!(!body.contains("Mutation"), "{format}: {body}");
        let body = mask_elapsed(&body);
        insta::assert_snapshot!(format!("without_flags_{format}"), body);
    }
}

/// Mask the elapsed time of the analysis run in a job summary, which differs
/// for each fixture run.
fn mask_elapsed(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(index) = rest.find("ms") {
        let head = &rest[..index];
        let digits = head.len() - head.bytes().rev().take_while(u8::is_ascii_digit).count();
        if digits < head.len() {
            out.push_str(&head[..digits]);
            out.push_str("<n>ms");
        } else {
            out.push_str(head);
            out.push_str("ms");
        }
        rest = &rest[index + 2..];
    }
    out.push_str(rest);
    out
}
