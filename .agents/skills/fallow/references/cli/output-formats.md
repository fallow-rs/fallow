# Output Formats

| Format | Description | Use Case |
|--------|-------------|----------|
| `human` | Colored terminal output | Interactive use |
| `json` | Compact machine-readable JSON by default; add `--pretty` for indented manual inspection | Agent integration, CI pipelines |
| `sarif` | Static Analysis Results Interchange Format | GitHub Code Scanning, SARIF-compatible tools |
| `compact` | Grep-friendly: one issue per line. Dupes lines include `code-duplication:path:start-end:fingerprint=dup:<id>,...` | Quick filtering |
| `markdown` | Markdown tables | Documentation, PR comments |
| `codeclimate` / `gitlab-codequality` | CodeClimate JSON array | GitLab Code Quality, CodeClimate-compatible tools |
| `pr-comment-github` / `pr-comment-gitlab` | Sticky PR/MR comment markdown with HTML-comment marker for upsert | Posted by the action / template `comment.sh` scripts |
| `review-github` / `review-gitlab` | JSON envelope for `POST /pulls/.../reviews` (GH) or `POST /merge_requests/.../discussions` (GL) | Posted by the action / template `review.sh` scripts; reconciled by `fallow ci reconcile-review` |
