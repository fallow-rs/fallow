# fallow GitHub Action

The action runs fallow in GitHub Actions and can publish job summaries, workflow annotations, sticky PR comments, inline review comments, and SARIF.

SARIF upload uses GitHub Code Scanning. Code Scanning is available for public repositories (free, no GitHub Advanced Security needed) and for private or internal repositories with GitHub Advanced Security enabled. On a public repository the action always attempts the upload (the first upload initializes Code Scanning); on a private or internal repository without Advanced Security it warns and skips, and the job summary and primary fallow output still run.

The upload requires the job to grant `permissions: security-events: write`. Without it, `github/codeql-action/upload-sarif` fails the step. On public repositories this surfaces as a job failure rather than a silent skip, so add the permission alongside `sarif: true`.

Inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
