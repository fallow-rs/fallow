# CI Integration

- **GitHub Actions**: `uses: fallow-rs/fallow@v3` - supports SARIF upload to Code Scanning, inline PR annotations (`annotations: true`), PR comments, all commands. `fail-on-issues` counts dead-code findings only when their rule is `error`, the same as the CLI exit code. An audit that compared no changed file (no pull request base) prints a `::warning::`. Annotations use workflow commands (no Advanced Security required); limit with `max-annotations` (default 50). Set `score: true` to compute health score and enable the health delta header in PR comments
- **GitLab CI**: include `ci/gitlab-ci.yml` template and extend `.fallow` - generates Code Quality reports via `--format codeclimate` / `--format gitlab-codequality` (inline MR annotations), rich MR comments, code review comments, all commands. Use `fallow ci-template gitlab --vendor` when runners cannot reach `raw.githubusercontent.com`; commit the generated `ci/` and `action/` files and use GitLab's local include syntax. Variables use `FALLOW_` prefix (e.g., `FALLOW_COMMAND`, `FALLOW_FAIL_ON_ISSUES`). Set `FALLOW_SCORE: "true"` to compute health score; `FALLOW_TREND: "true"` to compare against saved snapshots
- **Any CI**: `npx fallow --ci` - equivalent to `--format sarif --fail-on-issues --quiet`

## GitLab CI Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `FALLOW_COMMAND` | `dead-code` | Command to run (`dead-code`, `dupes`, `health`, or default combined) |
| `FALLOW_FAIL_ON_ISSUES` | `true` | Exit 1 if issues found. Dead-code findings count only when their rule is `error`. An audit with no changed file prints a warning. |
| `FALLOW_CHANGED_SINCE` | auto | Git ref for incremental analysis. Auto-detected in MR pipelines (`origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME`) |
| `FALLOW_COMMENT` | `false` | Post a summary comment on the MR with findings |
| `FALLOW_REVIEW` | `false` | Post inline code review comments on MR diff lines where issues were found |
| `FALLOW_REVIEW_GUIDANCE` | `false` | Add collapsed "What to do" guidance blocks to inline review comments |
| `FALLOW_SUMMARY_SCOPE` | `all` | Sticky summary scope: `all` keeps project-level findings outside the diff; `diff` applies the diff filter to those findings too |
| `FALLOW_PR_COMMENT_LAYOUT` | `default` | Sticky summary layout: `default`, `compact`, `gate-only`, or `details` |
| `FALLOW_SCORE` | `false` | Compute health score (0-100 with letter grade) in combined mode. Enables the health delta header in MR comments |
| `FALLOW_TREND` | `false` | Compare current health metrics against saved snapshot. Implies `FALLOW_SCORE`. Shows per-metric deltas |
| `FALLOW_EXTRA_ARGS` | - | Additional CLI flags passed through to fallow |
| `GITLAB_TOKEN` | - | Project access token with `api` scope (required for `FALLOW_COMMENT` and `FALLOW_REVIEW`). Alternatively, enable job token API access |

**Package manager detection**: The GitLab template auto-detects the project's package manager (npm, pnpm, or yarn) from lockfiles. MR comments and review comments show the correct install/run commands for the detected manager (e.g., `pnpm add -D` vs `npm install --save-dev`).

**Auto `--changed-since` in MR pipelines**: When running in a merge request pipeline, the template automatically sets `--changed-since origin/$CI_MERGE_REQUEST_TARGET_BRANCH_NAME` unless `FALLOW_CHANGED_SINCE` is explicitly set. This scopes analysis to files changed in the MR without manual configuration.
