# Environment variables

Fallow works with zero configuration, but a handful of environment variables let
you and your CI operators override defaults without editing a config file or
passing flags. CLI flags always win over the matching environment variable, and
environment variables win over the corresponding config-file field unless noted
otherwise.

The same user-facing list is emitted as a machine-readable manifest by
`fallow schema` (under `environment_variables`), so agents and tooling can
discover these without parsing this page.

## Output

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_FORMAT` | Default output format (`json`, `human`, `sarif`, `compact`, `markdown`, `codeclimate`, `gitlab-codequality`, `pr-comment-github`, `pr-comment-gitlab`, `review-github`, `review-gitlab`, `badge`, `github-annotations`, `github-summary`). The `--format` flag overrides it. | `human` | `FALLOW_FORMAT=json` |
| `FALLOW_QUIET` | Set to `1` or `true` to suppress progress output. The `--quiet` flag overrides it. | unset (off) | `FALLOW_QUIET=1` |
| `FALLOW_SUGGESTIONS` | Set to `off`/`0`/`false`/`no`/`disabled` to suppress the `next_steps[]` array in JSON output and the human `Next:` line. Useful for CI consumers that snapshot-diff raw `--format json`. | `on` | `FALLOW_SUGGESTIONS=off` |
| `FALLOW_UPDATE_CHECK` | Set to `off`/`0`/`false`/`disabled`/`no` to disable the human-TTY upgrade nudge and its background version check. | unset (on) | `FALLOW_UPDATE_CHECK=off` |

## Caching

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_CACHE_DIR` | Directory for fallow's persistent analysis cache. Relative paths resolve from the project root and override the `cache.dir` config field. | `.fallow/cache` | `FALLOW_CACHE_DIR=.cache/fallow` |
| `FALLOW_CACHE_MAX_SIZE` | Extraction cache size cap in megabytes. Wins over the `cache.maxSizeMb` config field. | `256` | `FALLOW_CACHE_MAX_SIZE=512` |
| `FALLOW_MAX_FILE_SIZE` | Per-file size ceiling in megabytes for source discovery; `0` means no limit. The `--max-file-size` flag overrides it. | `5` | `FALLOW_MAX_FILE_SIZE=10` |
| `FALLOW_EXTENDS_TIMEOUT_SECS` | Timeout in seconds after a host explicitly permits `https://` config inheritance. This does not enable remote extends; use `--allow-remote-extends` or the typed library option. | `5` | `FALLOW_EXTENDS_TIMEOUT_SECS=15` |

## Production mode

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_PRODUCTION` | Override production mode for all analyses (`true`/`false`/`1`/`0`/`yes`/`no`/`on`/`off`). | unset | `FALLOW_PRODUCTION=true` |
| `FALLOW_PRODUCTION_DEAD_CODE` | Override production mode for dead-code analysis only (combined mode and `fallow audit`). | unset | `FALLOW_PRODUCTION_DEAD_CODE=false` |
| `FALLOW_PRODUCTION_HEALTH` | Override production mode for health analysis only. | unset | `FALLOW_PRODUCTION_HEALTH=true` |
| `FALLOW_PRODUCTION_DUPES` | Override production mode for duplication analysis only. | unset | `FALLOW_PRODUCTION_DUPES=false` |

## Licensing

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_LICENSE` | License JWT (full string) for the paid runtime intelligence layer; intended for shared CI runners. | unset | `FALLOW_LICENSE=eyJhbGci...` |
| `FALLOW_LICENSE_PATH` | File path containing the license JWT. | unset | `FALLOW_LICENSE_PATH=/etc/fallow/license.jwt` |
| `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS` | Clock-skew tolerance applied to the license JWT's `iat` claim. Unset/empty/invalid values fall back to the default. | `86400` | `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS=3600` |

## Audit & impact

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_AUDIT_BASE` | Pins the `fallow audit` comparison base ref when no `--base`/`--changed-since` is passed. A malformed value is a hard error. | auto-detected | `FALLOW_AUDIT_BASE=upstream/main` |
| `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` | GC threshold in days for reusable audit base-snapshot caches; `0` disables the sweep. Wins over the `audit.cacheMaxAgeDays` config field. | `30` | `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS=7` |
| `FALLOW_IMPACT_STORE_MAX_AGE_DAYS` | GC threshold in days for per-project `fallow impact` stores; unset/`0` keeps every store forever. | unset | `FALLOW_IMPACT_STORE_MAX_AGE_DAYS=90` |

## Runtime coverage

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_COVERAGE` | Path to Istanbul coverage data (`coverage-final.json`) for accurate per-function CRAP scores. The `--coverage` flag overrides it. | unset | `FALLOW_COVERAGE=coverage/coverage-final.json` |
| `FALLOW_COVERAGE_ROOT` | Absolute coverage-data path prefix for rebasing Istanbul paths in CI or containers. The `--coverage-root` flag overrides it. | unset | `FALLOW_COVERAGE_ROOT=/ci/workspace` |
| `FALLOW_COV_BIN` | Explicit path override for the `fallow-cov` runtime-coverage sidecar binary. | discovered | `FALLOW_COV_BIN=/usr/local/bin/fallow-cov` |
| `FALLOW_COV_BINARY_PATH` | Secondary path override for the sidecar, checked after `FALLOW_COV_BIN` (air-gapped installs, distro-packaged sidecars, shared Docker images). | discovered | `FALLOW_COV_BINARY_PATH=/opt/fallow/fallow-cov` |
| `FALLOW_RUNTIME_COVERAGE_SOURCE` | Set to `cloud` to select cloud runtime coverage in `fallow coverage analyze` without passing `--cloud`. | local | `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud` |

## Cloud API

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_API_URL` | Base URL override for fallow cloud API calls (license refresh, trial, coverage uploads). Trailing slashes are trimmed. | `https://api.fallow.cloud` | `FALLOW_API_URL=https://staging.fallow.cloud` |
| `FALLOW_API_KEY` | fallow cloud bearer token for coverage upload commands. | unset | `FALLOW_API_KEY=fk_live_...` |
| `FALLOW_API_RETRIES` | Maximum HTTP attempts for review-comment reconciliation API calls. | `3` | `FALLOW_API_RETRIES=5` |
| `FALLOW_API_RETRY_DELAY` | Floor delay in seconds between HTTP retry attempts; a server-supplied `Retry-After` overrides it on 429 responses. | `2` | `FALLOW_API_RETRY_DELAY=5` |
| `FALLOW_CA_BUNDLE` | Path to a PEM certificate bundle for fallow cloud and provider HTTP calls; replaces the default WebPKI roots. Relative paths resolve from the process cwd. | unset | `FALLOW_CA_BUNDLE=/etc/ssl/corp-bundle.pem` |
| `FALLOW_REPO` | `owner/repo` fallback for `fallow coverage analyze --cloud` when `--repo` is not passed (otherwise parsed from the git origin remote). | git origin | `FALLOW_REPO=acme/widgets` |

## Change-scope & diff

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_CHANGED_SINCE` | git ref that scopes file discovery for analysis tools (MCP server). | unset | `FALLOW_CHANGED_SINCE=origin/main` |
| `FALLOW_DIFF_FILE` | Path to a unified diff that scopes all findings by changed line (MCP server). | unset | `FALLOW_DIFF_FILE=/tmp/pr.diff` |
| `FALLOW_DIFF_CONTEXT` | Line radius around changed diff lines when scoping findings to a diff in the review/PR-comment formats. | `3` | `FALLOW_DIFF_CONTEXT=5` |
| `FALLOW_ROOT` | Project root used by the `review-github`/`review-gitlab` renderers to read source for suggestion blocks. Set it alongside `--root` when rendering review formats outside the bundled CI integrations. | `--root` value | `FALLOW_ROOT=/workspace/repo` |
| `FALLOW_MAX_COMMENTS` | Maximum number of inline review comments emitted by `review-github`/`review-gitlab`. The GitHub Action sets this from `max-comments`; the GitLab template exposes the same variable directly. Sticky summary comments are not counted against this limit. | `50` | `FALLOW_MAX_COMMENTS=30` |
| `FALLOW_SUMMARY_SCOPE` | Summary scope for `pr-comment-github`/`pr-comment-gitlab`: `all` keeps project-level dependency/catalog/override findings outside the diff filter; `diff` applies the diff filter to them too. | `all` | `FALLOW_SUMMARY_SCOPE=diff` |
| `FALLOW_PR_COMMENT_LAYOUT` | Sticky PR comment layout: `default`, `compact`, `gate-only`, or `details`. Useful when teams prefer provider-native checks and want less Markdown in the PR conversation. | `default` | `FALLOW_PR_COMMENT_LAYOUT=gate-only` |
| `FALLOW_CONSOLIDATED_STATUS` | When `fallow ci post-check-run --split-gates` is used, truthy values add one aggregate `Fallow` check alongside the per-gate checks. | unset (off) | `FALLOW_CONSOLIDATED_STATUS=1` |
| `FALLOW_REVIEW_GUIDANCE` | Set to a truthy value (`1`/`true`/`yes`/`on`) to append collapsed guidance blocks to `review-github`/`review-gitlab` inline comment bodies. | unset (off) | `FALLOW_REVIEW_GUIDANCE=true` |
| `FALLOW_BOT_LOGIN` | Bot or token username treated as fallow's own when reconciling existing PR/MR comments in `review-github`/`review-gitlab`. Required when posting with a personal access token. | unset | `FALLOW_BOT_LOGIN=fallow-bot` |

## Agent / MCP

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_BIN` | Path to the fallow binary; used by the `fallow-mcp` server to spawn the CLI. | discovered | `FALLOW_BIN=/usr/local/bin/fallow` |
| `FALLOW_TIMEOUT_SECS` | MCP server per-tool-call CLI subprocess timeout in seconds. Raise it for long runs like production coverage on large dumps. | `120` | `FALLOW_TIMEOUT_SECS=300` |
| `FALLOW_AGENT_SOURCE` | Normalized agent vendor for telemetry classification (e.g. `claude_code`, `codex`, `cursor`). Only read when telemetry is on. | unset | `FALLOW_AGENT_SOURCE=claude_code` |
| `FALLOW_INTEGRATION_SURFACE` | Telemetry `integration_surface` override for non-CLI surfaces (`mcp`/`lsp`/`vscode`/`napi`/`programmatic`). Set by the MCP server on the CLI it spawns. | auto-derived | `FALLOW_INTEGRATION_SURFACE=mcp` |
| `FALLOW_MCP_TOOL` | Telemetry `mcp_tool` dimension, validated against the MCP tool-name allowlist. Set by the MCP server alongside `FALLOW_INTEGRATION_SURFACE=mcp`. | unset | `FALLOW_MCP_TOOL=check_health` |

## Telemetry

Telemetry is opt-in and off by default. See [telemetry.md](telemetry.md) for the full payload contract.

| Variable | Description | Default | Example |
| --- | --- | --- | --- |
| `FALLOW_TELEMETRY` | Telemetry mode: `off`, `on`, or `inspect` (print the payload to stderr without sending). Wins over the user config file. | `off` | `FALLOW_TELEMETRY=inspect` |
| `FALLOW_TELEMETRY_DISABLED` | Admin/fleet kill switch: truthy values hard-disable telemetry and refuse `fallow telemetry enable`. | unset | `FALLOW_TELEMETRY_DISABLED=1` |
| `FALLOW_TELEMETRY_DEBUG` | Truthy values alias `FALLOW_TELEMETRY=inspect`. | unset | `FALLOW_TELEMETRY_DEBUG=1` |
| `DO_NOT_TRACK` | Honored as a top-precedence telemetry kill switch ([consoledonottrack.com](https://consoledonottrack.com) convention). | unset | `DO_NOT_TRACK=1` |

## Internal markers

Fallow sets the following variables itself (telemetry sentinels, error markers, test/probe gates, and bundled-CI plumbing); they are not user knobs and you should not set them: `FALLOW_GITLAB_BASE_SHA`, `FALLOW_GITLAB_START_SHA`, `FALLOW_GITLAB_HEAD_SHA`, `FALLOW_COMMENT_ID`, `FALLOW_DIFF_FILTER` (set by the bundled Action/CI scripts), `FALLOW_PR_COMMENT_ENVELOPE_FILE`, `FALLOW_PR_DECISION_FILE`, `FALLOW_PR_DETAILS_FILE`, `FALLOW_GATE_MIN_VERSION`, `FALLOW_GATE_SCRIPT`, `FALLOW_GATE_POSIX_SUFFIX`, `FALLOW_GATE_WINDOWS_SUFFIX`, `FALLOW_MUTUALLY_EXCLUSIVE_SCOPE`, `FALLOW_NODE_ERROR`, `FALLOW_CONFIG_LOAD_FAILED`, `FALLOW_CWD_UNAVAILABLE`, `FALLOW_CHANGED_FILES_FAILED`, `FALLOW_CHANGED_WORKSPACES_FAILED`, `FALLOW_DEAD_CODE_FAILED`, `FALLOW_THREAD_POOL_INIT_FAILED`, `FALLOW_INVALID_CONFIG_PATH`, `FALLOW_INVALID_COVERAGE_PATH`, `FALLOW_INVALID_COVERAGE_ROOT`, `FALLOW_INVALID_DIFF_FILE`, `FALLOW_INVALID_ROOT`, `FALLOW_INVALID_THREADS`, `FALLOW_INVALID_WORKSPACE_PATTERN`, `FALLOW_WORKSPACE_PATTERN_UNMATCHED`, `FALLOW_WORKSPACE_SCOPE_EMPTY`, `FALLOW_WORKSPACES_NOT_FOUND`, `FALLOW_GENERIC_ATTR_PROBE`, `FALLOW_DUPES_ROLLING`, `FALLOW_OUTPUT_VARIANTS`, `FALLOW_STUB_MODE`, `FALLOW_TEST_SIGNAL_HELPER`, `FALLOW_RAYON_STACK_PROBE_CHILD`, and `FALLOW_PROGRAMMATIC_SHARED_DIFF_CHILD`.
