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

A run that loads the `baseline` input reports how much of that baseline still matches. When entries have gone stale the action emits a `::warning::` and repeats it in the job summary, so a baseline cannot rot unnoticed the way it could before 3.27.0, when the same verdict existed on stderr only and `--quiet` removed it.

Set `fail-on-stale-baseline: true` to turn that into a failing job. It is independent of `fail-on-issues`: a baseline whose entries all match nothing while the project itself is clean reports zero issues, which is exactly the case the gate exists for. The verdict comes from the analysis envelope's `baseline_staleness.gate_trips`, not from the CLI exit code, so a findings exit and a gate exit cannot be confused.

On a pull request `auto-changed-since` narrows the analysis, and a narrowed run cannot judge a whole-project baseline. With the gate on, the action then re-runs the baseline comparison once over the whole project, on a warm cache, reading nothing but the staleness verdict from it: that run writes no baseline, no snapshot and no SARIF, and feeds no comment, annotation or summary. When the run is narrowed by something the action cannot remove, such as `production: true`, `workspace`, or scoping passed through `args`, the gate warns that it stood down instead of passing in silence.

The PR comment and inline review do not carry the advisory yet.

### Bot identity

The markdown comment keeps fallow branding intentionally light. Repository-visible identity such as avatar, bot name, checks, and richer app affordances should come from the GitHub App installation rather than from decorative markdown inside each comment.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
