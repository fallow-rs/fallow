# fallow GitHub Action

The action runs fallow in GitHub Actions and can publish job summaries, workflow annotations, sticky PR comments, inline review comments, and SARIF.

SARIF upload uses GitHub Code Scanning. Code Scanning is available for public repositories (free, no GitHub Advanced Security needed) and for private or internal repositories with GitHub Advanced Security enabled. On a public repository the action always attempts the upload (the first upload initializes Code Scanning); on a private or internal repository without Advanced Security it warns and skips, and the job summary and primary fallow output still run.

The upload requires the job to grant `permissions: security-events: write`. Without it, `github/codeql-action/upload-sarif` fails the step. On public repositories this surfaces as a job failure rather than a silent skip, so add the permission alongside `sarif: true`.

Inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

Sticky PR comments are posted through `fallow ci post-pr-comment`, so lookup, retry, create/update, and clean-run skip policy live in Rust instead of the shell wrapper.

Clean pull requests do not create a new sticky PR comment. If a previous fallow sticky comment exists, the action updates it to the clean result so stale warnings disappear.

GitHub Check Runs are posted from the typed PR decision sidecar against the PR head SHA, falling back to `GITHUB_SHA` when no PR head is available. Grant `permissions: checks: write` to let the Fallow check appear as a native PR gate; without that permission the action keeps the comment flow and emits a warning. The same render step also writes `fallow-pr-details.json` as a CI artifact for full finding drilldown.

Set `comment-layout: gate-only` when the native Check Run is the primary review surface and the PR timeline should stay compact.

### Baselines

A run that loads the `baseline` input reports how much of that baseline still matches. When entries have gone stale the action emits a `::warning::` and repeats it in the job summary, so a stale baseline stops being invisible the way it was before this release, when the same verdict existed on stderr only and `--quiet` removed it.

A run scoped to changed files cannot judge a whole-project baseline: it compares the baseline against a slice and would report every entry outside that slice as unmatched. On a pull request the action therefore re-reads the baseline once over the whole project before reporting on it. That re-read is not behind an input, because a repository that never asked for a gate still wants to know its baseline has rotted. It carries no narrowing flag and no writing flag, so it writes no baseline, no snapshot and no SARIF, and it feeds no comment, annotation or summary; on a warm cache it costs about as much as the run before it. The step log names it and its wall time.

Set `fail-on-stale-baseline: true` to turn that verdict into a failing job. It is independent of `fail-on-issues`: a baseline whose entries all went stale on a project that is now clean reports zero issues, which is exactly the case the gate exists for. The verdict comes from the analysis envelope's `baseline_staleness.gate_trips`, not from the CLI exit code, so a findings exit and a gate exit cannot be confused.

When the run still cannot judge the baseline, because of `production: true`, `workspace`, `changed-workspaces`, or a positional path passed through `args`, the action says so instead of passing in silence: a `::warning::` when the gate was asked for and did not get one, a `::notice::` otherwise, since a repository that asked for nothing should not get an unsuppressible warning on every pull request. What it cannot read at runtime fails open: a fallow that predates this feature, or a command that reports no staleness, produces a warning and a green job. Combinations that cannot work at all are rejected up front with exit 2 instead: the gate with no `baseline` set, or the gate on `command: fix` or `command: security`.

Two configurations defeat this. Pointing `baseline` and `save-baseline` at the same file means the run saves before it compares, so the baseline is rewritten from the run that was supposed to be judged against it and can never report a stale entry; the action warns when it sees that. And on a pull request the six `baseline-*` outputs and the job-summary line describe the unscoped re-read, not the scoped analysis, so `baseline-change-scoped` reads `false` there even though the analysis itself was narrowed.

The PR comment and inline review do not carry the advisory yet.

### Gates

Every gate the run armed publishes a verdict in the analysis envelope, and the
action reads that rather than the CLI's exit code, which it discards whenever
stdout parses as JSON. A gate fails the job when three things hold: the input
that owns it asked for it, the CLI concluded `fail`, and the CLI marked the
verdict enforced.

| Gate | Input that owns it |
|---|---|
| `regression` | `fail-on-regression` |
| `duplication-threshold` | `threshold` |
| `health-min-score` | `min-score` |
| `health-min-severity` | `min-severity` |
| `security` | `security-gate` |
| `stale-baseline` | `fail-on-stale-baseline` |
| `type-aware-require` | `type-aware-require` |

Each is independent of `fail-on-issues`, which keeps its own job: it gates on
the issue count and nothing else. `command: audit` is the exception and still
gates on its verdict through `fail-on-issues`, so an audit job with
`fail-on-issues: false` stays a reporting configuration.

A gate that concluded `fail` without its input being set produces a
`::warning::` and never fails the job, so a flag passed through `args:` cannot
override `fail-on-issues: false`. A gate that stood down without judging the run
produces a `::warning::` when its input asked for it and a `::notice::`
otherwise. Every failing gate prints its own `::error::` and the step exits once
at the end, after the outputs and artifacts are written, so the comment,
annotation and summary steps still run. The security gate keeps its documented
exit 8 and outranks the generic 1.

The outputs `gates-failed`, `gates-warned`, `gates-skipped` and `gates-passed`
carry the comma-separated names, so a downstream step can report on a gate
without failing on it.

`min-score` and `min-severity` apply to `command: health` only and are rejected
with exit 2 elsewhere. `--min-score` implies `--score`, which is a section
selector, so the action adds `--complexity` unless you selected a health section
yourself; without that the annotations, the SARIF upload and the pull-request
comment would all render empty. `target_thresholds` and `hotspot_summary` are
not restored by that, and the `fail-on-issues` count gate still counts every
finding, so set `fail-on-issues: false` to gate on the score alone.

In combined mode (no `command`) the CLI does not enforce the duplication
threshold, and says so in the envelope. The action honours that and warns rather
than failing; run `command: dupes` to gate on it.

On a fallow older than 3.27.0 the envelope carries no gate verdicts. The action
falls back to the fields those releases already published for `regression`,
`security`, `stale-baseline` and `type-aware-require`, and fails open with one
warning for `threshold`, `min-score` and `min-severity`, which had no field to
read. That warning appears only when the matching input is set.

### Degraded and empty analysis

A run whose findings were computed over less than the whole project reports one
`::warning::` listing the diagnostic kinds and their counts, and sets the
`analysis-degraded` output. A run that analyzed no source file at all gets its
own sentence, because its clean result means nothing was measured rather than
that nothing was found. It warns and passes by default; set
`fail-on-empty-analysis: true` to fail instead.

### Bot identity

The markdown comment keeps fallow branding intentionally light. Repository-visible identity such as avatar, bot name, checks, and richer app affordances should come from the GitHub App installation rather than from decorative markdown inside each comment.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
