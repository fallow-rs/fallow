# fallow GitHub Action

The action runs fallow in GitHub Actions and can publish job summaries, workflow annotations, sticky PR comments, inline review comments, and SARIF.

SARIF upload uses GitHub Code Scanning. Code Scanning is available for public repositories and for private repositories with GitHub Advanced Security enabled. When Code Scanning is unavailable, the action warns and skips the SARIF upload; the job summary and primary fallow output still run.

Inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

For full setup and input reference, see the main repository README and the hosted CI integration docs.
