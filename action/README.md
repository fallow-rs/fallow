# fallow GitHub Action

The action runs fallow in GitHub Actions and can publish job summaries, workflow annotations, sticky PR comments, inline review comments, and SARIF.

SARIF upload uses GitHub Code Scanning. Code Scanning is available for public repositories (free, no GitHub Advanced Security needed) and for private or internal repositories with GitHub Advanced Security enabled. On a public repository the action always attempts the upload (the first upload initializes Code Scanning); on a private or internal repository without Advanced Security it warns and skips, and the job summary and primary fallow output still run.

The upload requires the job to grant `permissions: security-events: write`. Without it, `github/codeql-action/upload-sarif` fails the step. On public repositories this surfaces as a job failure rather than a silent skip, so add the permission alongside `sarif: true`.

Inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

Sticky PR comments are posted through `fallow ci post-pr-comment`, so lookup, retry, create/update, and clean-run skip policy live in Rust instead of the shell wrapper.

Clean pull requests do not create a new sticky PR comment. If a previous fallow sticky comment exists, the action updates it to the clean result so stale warnings disappear.

GitHub Check Runs are posted from the typed PR decision sidecar against the PR head SHA, falling back to `GITHUB_SHA` when no PR head is available. Grant `permissions: checks: write` to let the Fallow check appear as a native PR gate; without that permission the action keeps the comment flow and emits a warning. The same render step also writes `fallow-pr-details.json` as a CI artifact for full finding drilldown.

Set `comment-layout: gate-only` when the native Check Run is the primary review surface and the PR timeline should stay compact.

### Bot identity

The markdown comment keeps fallow branding intentionally light. Repository-visible identity such as avatar, bot name, checks, and richer app affordances should come from the GitHub App installation rather than from decorative markdown inside each comment.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
