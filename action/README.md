# fallow GitHub Action

The action runs fallow in GitHub Actions and can publish job summaries, workflow annotations, sticky PR comments, inline review comments, and SARIF.

SARIF upload uses GitHub Code Scanning. Code Scanning is available for public repositories (free, no GitHub Advanced Security needed) and for private or internal repositories with GitHub Advanced Security enabled. On a public repository the action always attempts the upload (the first upload initializes Code Scanning); on a private or internal repository without Advanced Security it warns and skips, and the job summary and primary fallow output still run.

The upload requires the job to grant `permissions: security-events: write`. Without it, `github/codeql-action/upload-sarif` fails the step. On public repositories, a missing permission fails the job. Add the permission alongside `sarif: true`.

Inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

Sticky PR comments are posted through `fallow ci post-pr-comment`, so lookup, retry, create/update, and clean-run skip policy live in Rust instead of the shell wrapper.

Clean pull requests do not create a new sticky PR comment. If a previous fallow sticky comment exists, the action updates it to the clean result so stale warnings disappear.

GitHub Check Runs are posted from the typed PR decision sidecar against the PR head SHA, falling back to `GITHUB_SHA` when no PR head is available. Grant `permissions: checks: write` to let the Fallow check appear as a native PR gate; without that permission the action keeps the comment flow and emits a warning. The same render step also writes `fallow-pr-details.json` as a CI artifact for full finding drilldown.

Set `comment-layout: gate-only` to use the native Check Run for the full result and keep the PR timeline compact.

### Baselines

A run that loads the `baseline` input reports how much of that baseline still matches. For stale entries, the action emits a `::warning::` and repeats it in the job summary. The warning remains visible with `--quiet`.

A run scoped to changed files cannot judge a whole-project baseline. Entries outside the analyzed files would appear unmatched. On a pull request, the action re-reads the baseline over the whole project before reporting its staleness, even when the stale-baseline gate is off.

The re-read removes narrowing and writing flags. It writes no baseline, snapshot, or SARIF, and its findings do not feed comments, annotations, or summaries. The step log reports the re-read and its wall time.

Set `fail-on-stale-baseline: true` to fail the job on a stale baseline. This gate is independent of `fail-on-issues`, so it can fail even when a clean project has no remaining issues. The gate reads `baseline_staleness.gate_trips` from the analysis output. It does not use the CLI exit code.

The action can still be unable to judge the baseline with `production: true`, `workspace`, `changed-workspaces`, or a positional path in `args`. It emits a `::warning::` when the gate was requested, or a `::notice::` otherwise.

If the installed fallow predates this feature or a command reports no staleness, the action warns without failing on the stale-baseline gate. Other gates can still fail the job. Invalid input combinations exit 2 before analysis: the gate without `baseline`, or the gate with `command: fix` or `command: security`.

Do not point `baseline` and `save-baseline` at the same file. The run saves before it compares, so it replaces the reference baseline and cannot report stale entries. The action warns about this configuration.

On a pull request, the `baseline-*` outputs and job-summary line describe the whole-project re-read. As a result, `baseline-change-scoped` is `false` even when the primary analysis is scoped to changed files.

The PR comment and inline review do not include the stale-baseline warning yet.

### Gates

Each enabled gate reports its result in the analysis output. The action reads
that result and discards the CLI exit code when stdout parses as JSON. A gate
fails the job when its input is enabled, the CLI reports `fail`, and the result
is marked as enforced.

| Gate | Input that owns it |
|---|---|
| `regression` | `fail-on-regression` |
| `duplication-threshold` | `threshold` |
| `health-min-score` | `min-score` |
| `health-min-severity` | `min-severity` |
| `security` | `security-gate` |
| `stale-baseline` | `fail-on-stale-baseline` |
| `type-aware-require` | `type-aware-require` |
| `parse-error` | none: the entry exists only when `failOnParseError` in config or `--fail-on-parse-error` in `args` armed it, so an enforced failure always fails the job |
| `error-severity-findings` | none: it is the CLI's own severity rule, reported in the outputs and never in the log |
| `audit-verdict` | `fail-on-issues`, through the count gate |

Each is independent of `fail-on-issues`, which keeps its own job: it gates on
the issue count and nothing else. `command: audit` is the exception and still
gates on its verdict through `fail-on-issues`, so an audit job with
`fail-on-issues: false` stays a reporting configuration.

A gate that concluded `fail` without its input being set produces a
`::warning::` and never fails the job, so a flag passed through `args:` cannot
override `fail-on-issues: false`. A gate that stood down without judging the run
produces a `::warning::` when its input asked for it and a `::notice::`
otherwise. Each failing gate prints its own `::error::`. The step exits after
writing outputs and artifacts, so comments, annotations, and summaries still
run. The security gate keeps its documented exit 8 and outranks the generic 1.

The outputs `gates-failed`, `gates-warned`, `gates-skipped` and `gates-passed`
contain comma-separated gate names. A downstream step can report a result
without failing the job.

The CLI reports the default rule of the command (`error-severity-findings`,
`health-findings`, `audit-verdict`) on every run. A run with findings therefore
names that rule in `gates-failed`, also when `fail-on-issues: false` keeps the
job green. To act on one gate, read its name in these outputs. Do not treat a
non-empty `gates-failed` as a failed job.

`min-score` and `min-severity` apply to `command: health` only and are rejected
with exit 2 elsewhere. `--min-score` implies `--score`, which is a section
selector, so the action adds `--complexity` unless you selected a health section
yourself; without that the annotations, the SARIF upload and the pull-request
comment would all render empty. `target_thresholds` and `hotspot_summary` are
not restored by that. When `min-score` is set the CLI turns its own findings
rule off, and the action follows: the `fail-on-issues` count gate stands down
for that run, so the score is the only thing that decides it. That is what
`--min-score` means by "complexity findings become informational".

By default, combined mode (no `command`) leaves the duplication threshold
unenforced and reports that in the analysis output. The action warns about an
unenforced duplication gate without failing on that gate. Other gates can still
fail the job. Passing `--fail-on-issues` through `args:` can enforce the combined
threshold; the action fails on it only when its `fail-on-issues` input is true.
Use `command: dupes` to enforce the threshold directly.

On a fallow older than 3.27.0 the envelope carries no gate verdicts. The action
falls back to the fields those releases already published for `regression`,
`security`, `stale-baseline` and `type-aware-require`, and fails open with one
warning for `threshold`, `min-score` and `min-severity`, which had no field to
read. That warning appears only when the matching input is set.

### Degraded and empty analysis

A run whose findings were computed over less than the whole project reports one
`::warning::` listing the diagnostic kinds and their counts, and sets the
`analysis-degraded` output, read from the envelope root or, on `audit`, from its
`dead_code` section. A run that analyzed no source files gets a separate warning
because its clean result has no analysis evidence. It passes by default. Set
`fail-on-empty-analysis: true` to fail instead.

### Requests the run could not apply

A run asked to narrow its report (`changed-since`, or a supplied diff) and
unable to, widens instead of failing, and the report that follows is complete
and covers more than was asked for. That is reported once as a `::warning::` and
published as the `requests-unapplied` output, so a workflow can branch on it
rather than trusting a scoped review that was never scoped. Only requests that
narrow the report are listed. A request that writes a file beside the report,
such as the SARIF document, changes nothing about the report's scope, and a
failure there is reported by the SARIF warning instead.

### Upgrading from an earlier action

The inline `Check threshold` step moved into the analyze step. Its gates are
independent of `fail-on-issues`. Update workflows that referenced the old step
through `continue-on-error` or `steps.*.outcome`. The result is now on the
analyze step, and `gates-failed` lists the failed gates.

One stale baseline now produces two lines: the action's own advisory, from the
unscoped re-read it performs on a pull request, and the neutral gate line
`fallow report` renders into the summary and the comment from the primary
envelope. They describe different runs, which is why both exist: the advisory
judges the whole project, and the gate line reports what the scoped run
concluded, which on a pull request is that it stood down.

### Bot identity

The markdown comment keeps fallow branding intentionally light. Repository-visible identity such as avatar, bot name, checks, and richer app affordances should come from the GitHub App installation rather than from decorative markdown inside each comment.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
