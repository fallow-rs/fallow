# `ci`: Provider-Aware Review Automation

`fallow ci reconcile-review` reads a typed review envelope (`--format review-github` / `review-gitlab`), looks up existing fingerprints on the PR/MR, and resolves stale review threads when their finding is no longer present in the new envelope. Posts an idempotent "Resolved in `<sha>`" follow-up comment per stale finding (skipped if a marker for the same fingerprint at the current SHA already exists).

Provider mutations are isolated per fingerprint. A failed mutation blocks only the remaining operations of that same fingerprint, which is retried whole on the next run, while every other stale fingerprint is still applied. (A preflight failure is different: preflight runs before any mutation, and a failure there abandons the whole plan because the state snapshot is untrustworthy.) If a preflight check, permission error, or provider mutation fails, JSON output keeps `apply_errors` and can add `apply_hint`, `failed_fingerprints`, and `unapplied_fingerprints` so agents and CI wrappers can report what was not fully applied. `fallow ci post-review` reports those same three fields for the reconcile pass it runs after posting new inline comments.

Review comments end with a `<!-- fallow-fingerprint:v3: <fp> -->` marker. A dead-code fingerprint comes from the `finding_id`, so a line shift above the finding keeps the thread. When a comment had another fingerprint in an older release, the envelope comment carries that value as `legacy_fingerprint`. For one release, both commands match an open thread with the older `v2` marker through `legacy_fingerprint`: they do not post the finding again and do not resolve the thread as stale.

## Flags

| Flag | Type | Description |
|------|------|-------------|
| `--provider` | `github\|gitlab` | Required. Selects the provider API. |
| `--pr` | `<number>` | GitHub PR number. Required when `--provider github`. |
| `--mr` | `<iid>` | GitLab MR internal id. Required when `--provider gitlab`. |
| `--repo` | `owner/name` | GitHub repo. Defaults to `$GH_REPO` / `$GITHUB_REPOSITORY`. |
| `--project-id` | `<id>` | GitLab project id (numeric or `group/project`). Defaults to `$CI_PROJECT_ID`. |
| `--api-url` | `<url>` | Override the API base URL (GitHub Enterprise, self-hosted GitLab). |
| `--envelope` | `<path>` | Path to the review envelope JSON written by `--format review-{github,gitlab}`. |
| `--dry-run` | `bool` | Compute the new/stale plan without posting / resolving. |

The HTTP layer mirrors the bash `gh_api_retry` / `curl_retry` helpers: `FALLOW_API_RETRIES` (default 3) caps attempts; `FALLOW_API_RETRY_DELAY` (default 2) sets the floor delay; server-supplied `Retry-After` overrides the floor on 429 responses.
