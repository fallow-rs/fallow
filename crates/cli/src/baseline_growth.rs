//! The opt-in `--fail-on-baseline-growth` gate (issue #2938).
//!
//! `--fail-on-stale-baseline` fails when a baseline keeps an entry that matches
//! nothing. It cannot see the other direction: a change that adds a finding and
//! re-saves the baseline in the same commit passes every gate. This gate
//! compares each baseline that a run loads with the same file at a base ref, and
//! it fails when the baseline has a key that the base file does not have.
//!
//! The comparison reads two file versions and runs no analysis, so the command
//! evaluates it before the analysis starts. A base ref that git cannot resolve
//! is then an exit 2 before any work, and the verdict is ready for the
//! `gate_outcomes` of the envelope that the command renders. The command records
//! the verdict in a slot for this process, the same way
//! [`crate::output_runtime`] records the loaded baseline, because the gate
//! applies to five commands and each of them builds its envelope in a
//! different place. The command that armed the gate is recorded with it, so a
//! section that one run renders as a part of another command does not repeat
//! the entry.
//!
//! The verdict line prints after the report and regardless of `--quiet`, for
//! the reason [`crate::baseline_gate`] documents: a gate that the repository
//! armed must say what it decided in every output format.

#![allow(
    clippy::print_stderr,
    reason = "the gate explains a non-zero exit on stderr, like the other CLI gates"
)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use fallow_engine::baseline::BaselineKind;
use fallow_engine::baseline_growth::{BaseBaselineError, BaselineGrowth, baseline_growth};
use fallow_output::GateOutcome;

/// The command that armed the gate, so only its envelope carries the entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrowthOwner {
    /// `fallow dead-code` or `fallow check`.
    DeadCode,
    /// `fallow dupes`.
    Dupes,
    /// `fallow health`.
    Health,
    /// The bare `fallow` run.
    Combined,
    /// `fallow audit`.
    Audit,
}

/// The two flags, as the command line gave them.
#[derive(Debug, Clone, Copy)]
pub struct GrowthFlags<'a> {
    /// `--fail-on-baseline-growth`.
    pub enabled: bool,
    /// `--baseline-base <ref>`.
    pub base: Option<&'a str>,
    /// `--changed-since` / `--base`, the explicit input of the audit base
    /// resolver when `--baseline-base` is not given.
    pub changed_since: Option<&'a str>,
}

/// One baseline that the run loads, and the command format it is in.
#[derive(Debug, Clone, Copy)]
pub struct GrowthTarget<'a> {
    pub path: &'a Path,
    pub kind: BaselineKind,
}

/// The targets of a run: each baseline path that the run loads, in order.
#[must_use]
pub fn targets<'a>(candidates: &[(Option<&'a Path>, BaselineKind)]) -> Vec<GrowthTarget<'a>> {
    candidates
        .iter()
        .filter_map(|&(path, kind)| path.map(|path| GrowthTarget { path, kind }))
        .collect()
}

/// What the gate found for one baseline.
#[derive(Debug, Clone)]
enum Verdict {
    /// The base ref has no file at this path: the change adds the baseline.
    New,
    /// The comparison ran. An empty growth is a pass.
    Compared(BaselineGrowth),
}

#[derive(Debug, Clone)]
struct JudgedBaseline {
    path: PathBuf,
    verdict: Verdict,
}

/// The verdict for every baseline of one run.
#[derive(Debug, Clone)]
pub struct GrowthGate {
    base: ResolvedBase,
    baselines: Vec<JudgedBaseline>,
}

/// The base ref and how the gate found it.
#[derive(Debug, Clone)]
struct ResolvedBase {
    git_ref: String,
    /// How an implicit base was found, for example `merge-base with
    /// origin/main`. `None` for `--baseline-base` and `--changed-since`.
    description: Option<String>,
}

impl ResolvedBase {
    fn label(&self) -> String {
        self.description.as_ref().map_or_else(
            || self.git_ref.clone(),
            |description| format!("{} ({description})", self.git_ref),
        )
    }
}

impl GrowthGate {
    fn added_entries(&self) -> usize {
        self.baselines
            .iter()
            .map(|judged| match &judged.verdict {
                Verdict::New => 0,
                Verdict::Compared(growth) => growth.added_entries(),
            })
            .sum()
    }

    fn failed(&self) -> bool {
        self.added_entries() > 0
    }

    /// The envelope entry: `observed` is the number of new keys and
    /// `threshold` is zero, the number of new keys the gate allows.
    #[must_use]
    pub fn outcome(&self) -> GateOutcome {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a baseline key count never approaches the f64 integer limit"
        )]
        let observed = self.added_entries() as f64;
        GateOutcome::measured(crate::gates::status_of(self.failed()), true, observed, 0.0)
    }

    /// Print the verdict for each baseline and report whether the gate failed.
    fn report(&self) -> bool {
        for judged in &self.baselines {
            match &judged.verdict {
                Verdict::New => eprintln!(
                    "Note: --fail-on-baseline-growth found no {} at {}, so it is a new \
                     baseline. The gate passes. The next change compares against it.",
                    judged.path.display(),
                    self.base.label(),
                ),
                Verdict::Compared(growth) if !growth.is_empty() => {
                    eprintln!(
                        "Baseline growth gate failed: {} has {} {} that {} does not have. \
                         Fix the new findings, or get a review for each new key:",
                        judged.path.display(),
                        growth.added_entries(),
                        if growth.added_entries() == 1 {
                            "key"
                        } else {
                            "keys"
                        },
                        self.base.label(),
                    );
                    for grown in &growth.categories {
                        for key in &grown.keys {
                            eprintln!("  {}: {key}", grown.category);
                        }
                    }
                }
                Verdict::Compared(_) => {}
            }
        }
        self.failed()
    }
}

static RECORDED: Mutex<Option<(GrowthOwner, GateOutcome)>> = Mutex::new(None);

fn record(owner: GrowthOwner, outcome: Option<GateOutcome>) {
    if let Ok(mut slot) = RECORDED.lock() {
        *slot = outcome.map(|outcome| (owner, outcome));
    }
}

/// Clear the verdict that a previous command run left in this process.
pub fn reset() {
    record(GrowthOwner::DeadCode, None);
}

/// The recorded verdict, when `owner` is the command that armed the gate.
#[must_use]
pub fn recorded_outcome(owner: GrowthOwner) -> Option<GateOutcome> {
    RECORDED
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .filter(|(armed_by, _)| *armed_by == owner)
        .map(|(_, outcome)| outcome)
}

/// Run a command under the gate.
///
/// Without `--fail-on-baseline-growth` this only runs the command, after it
/// refuses a `--baseline-base` that has no gate to feed. With the flag, it
/// judges every target before the command runs, records the verdict for the
/// envelope, runs the command, prints the verdict, and makes a passing run
/// exit 1 when the gate failed. A run that already exits non-zero keeps its
/// exit code.
pub fn run_gated(
    ctx: &GrowthContext<'_>,
    targets: &[GrowthTarget<'_>],
    run: impl FnOnce() -> ExitCode,
) -> ExitCode {
    let flags = ctx.flags;
    if !flags.enabled {
        if flags.base.is_some() {
            return usage_error(
                ctx,
                "--baseline-base needs --fail-on-baseline-growth. Add the flag, or remove \
                 --baseline-base.",
            );
        }
        return run();
    }
    if targets.is_empty() {
        return usage_error(
            ctx,
            "--fail-on-baseline-growth needs a baseline to compare. Pass --baseline (on bare \
             `fallow`: --baseline, --dupes-baseline or --health-baseline; on `fallow audit`: \
             --dead-code-baseline, --health-baseline or --dupes-baseline), or remove the flag.",
        );
    }
    let gate = match judge(ctx.root, flags, targets) {
        Ok(gate) => gate,
        Err(message) => return usage_error(ctx, &message),
    };
    record(ctx.owner, Some(gate.outcome()));
    let code = run();
    let failed = gate.report();
    record(ctx.owner, None);
    if failed && code == ExitCode::SUCCESS {
        return ExitCode::from(crate::exit_codes::gate_failed_exit_code(
            fallow_output::GateName::BaselineGrowth,
            true,
        ));
    }
    code
}

/// Say that a run which asked for the gate deliberately did not apply it.
pub fn note_stood_down(flags: GrowthFlags<'_>, reason: &str) {
    if flags.enabled {
        eprintln!("Note: --fail-on-baseline-growth did not run: {reason}.");
    }
}

/// Where the gate runs and who armed it.
#[derive(Debug, Clone, Copy)]
pub struct GrowthContext<'a> {
    pub root: &'a Path,
    pub flags: GrowthFlags<'a>,
    pub owner: GrowthOwner,
    pub output: fallow_config::OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
}

fn usage_error(ctx: &GrowthContext<'_>, message: &str) -> ExitCode {
    crate::error::emit_error_with_style(message, 2, ctx.output, ctx.json_style)
}

fn judge(
    root: &Path,
    flags: GrowthFlags<'_>,
    targets: &[GrowthTarget<'_>],
) -> Result<GrowthGate, String> {
    let base = resolve_base(root, flags)?;
    if flags.base.is_none() && fallow_engine::baseline_growth::ref_is_head(root, &base.git_ref) {
        // The audit resolver prefers the merge-base with the upstream. On a
        // branch that tracks its own pushed copy, that is HEAD. A comparison
        // with HEAD always passes, and a gate that cannot fail is fail-open.
        return Err(format!(
            "--fail-on-baseline-growth resolved its base to {} (HEAD), so the gate cannot \
             detect growth. Pass --baseline-base <ref>, for example \
             --baseline-base origin/main.",
            base.label(),
        ));
    }
    let mut baselines = Vec::with_capacity(targets.len());
    for target in targets {
        baselines.push(JudgedBaseline {
            path: target.path.to_path_buf(),
            verdict: judge_one(target, &base.git_ref)?,
        });
    }
    Ok(GrowthGate { base, baselines })
}

/// `--baseline-base` when given, else the audit base resolver: the explicit
/// `--changed-since` / `--base`, then `FALLOW_AUDIT_BASE`, then the merge-base
/// with the upstream or the remote default branch.
fn resolve_base(root: &Path, flags: GrowthFlags<'_>) -> Result<ResolvedBase, String> {
    if let Some(base) = flags.base {
        fallow_engine::validate::validate_git_ref(base).map_err(|reason| {
            format!("--baseline-base '{base}' is not a valid git ref: {reason}")
        })?;
        return Ok(ResolvedBase {
            git_ref: base.to_owned(),
            description: None,
        });
    }
    fallow_api::audit_run::resolve_audit_base(root, flags.changed_since)
        .map(|resolved| ResolvedBase {
            git_ref: resolved.git_ref,
            description: resolved.description,
        })
        .map_err(|_| {
            "--fail-on-baseline-growth could not find a base ref to compare the baseline with. \
             Pass --baseline-base <ref>, for example --baseline-base origin/main."
                .to_owned()
        })
}

fn judge_one(target: &GrowthTarget<'_>, git_ref: &str) -> Result<Verdict, String> {
    let path = target.path;
    let head_text = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "--fail-on-baseline-growth could not read the baseline {}: {error}",
            path.display()
        )
    })?;
    let base_text = fallow_engine::baseline_growth::read_baseline_at_ref(path, git_ref)
        .map_err(|error| base_error_message(&error, path, git_ref))?;
    let Some(base_text) = base_text else {
        return Ok(Verdict::New);
    };
    let head = parse(&head_text, path, "")?;
    let base = parse(&base_text, path, &format!(" at {git_ref}"))?;
    Ok(Verdict::Compared(baseline_growth(
        target.kind,
        &base,
        &head,
    )))
}

fn parse(text: &str, path: &Path, at: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(text).map_err(|error| {
        format!(
            "--fail-on-baseline-growth could not parse the baseline {}{at}: {error}",
            path.display()
        )
    })
}

fn base_error_message(error: &BaseBaselineError, path: &Path, git_ref: &str) -> String {
    match error {
        BaseBaselineError::RefUnavailable => {
            let (remote, branch) = git_ref.split_once('/').unwrap_or(("origin", git_ref));
            format!(
                "--fail-on-baseline-growth cannot resolve the base ref '{git_ref}', so it cannot \
                 compare the baseline {}. Fetch it with `git fetch {remote} {branch}`, or check \
                 out with full history (`fetch-depth: 0` on actions/checkout, `GIT_DEPTH: 0` in \
                 GitLab CI).",
                path.display()
            )
        }
        BaseBaselineError::NotARepository => format!(
            "--fail-on-baseline-growth needs git history, and the baseline {} is not in a git \
             work tree. Run fallow in a git checkout, or remove the flag.",
            path.display()
        ),
        BaseBaselineError::GitMissing(reason) => format!(
            "--fail-on-baseline-growth needs git, and git could not start: {reason}. Install git \
             and make it available on PATH, or remove the flag."
        ),
        BaseBaselineError::GitFailed(reason) => format!(
            "--fail-on-baseline-growth could not read the baseline {} at '{git_ref}': {reason}",
            path.display()
        ),
    }
}
