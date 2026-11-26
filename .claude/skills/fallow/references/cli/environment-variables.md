# Environment Variables

| Variable | Description |
|----------|-------------|
| `FALLOW_FORMAT` | Default output format. CLI `--format` overrides. |
| `FALLOW_QUIET` | Set to `1` to suppress progress. CLI `--quiet` overrides. |
| `FALLOW_BIN` | Path to fallow binary (used by the MCP server). |
| `FALLOW_TIMEOUT_SECS` | MCP server subprocess timeout in seconds (default: `120`; similar-code discovery and inspection default to `900`; a call that runs as an MCP task defaults to `1800`). Increase or reduce it to override each bound. |
| `FALLOW_EXTENDS_TIMEOUT_SECS` | Timeout for fetching remote config inheritance in seconds (default: `5`). Do not raise this for untrusted sources. |
| `FALLOW_CACHE_DIR` | Override the persistent extraction cache directory. Wins over `cache.dir`. Useful for read-only checkouts or CI cache volumes. `--no-cache` disables this knob. |
| `FALLOW_CACHE_MAX_SIZE` | Maximum on-disk extraction cache (`.fallow/cache.bin`) size in megabytes (default: `256`). Triggers LRU eviction when crossed. Wins over `cache.maxSizeMb` config field. Intended for CI runners with disk quotas. `--no-cache` short-circuits this knob. |
| `FALLOW_PACKAGE_BASELINES` | Set to `false`, `0`, `no` or `off` to ignore `workspaces.changedSince` for every run of the process, like `--no-package-baselines`. Other values keep the map. |
| `FALLOW_COVERAGE` | Path to Istanbul or raw V8 coverage data for exact CRAP scoring in `health`, `audit`, and bare `fallow`. |
| `FALLOW_COVERAGE_ROOT` | Absolute coverage-data prefix to strip before matching Istanbul paths in `health`, `audit`, and bare `fallow`. |
| `FALLOW_TYPE_AWARE` | Enable or disable TypeScript semantic (type-aware) analysis for the run. Accepts `true`/`false`/`1`/`0`/`yes`/`no`/`on`/`off`; any other value is a hard error. Sits mid-chain in the precedence: the `--type-aware`/`--no-type-aware` CLI flags win over it, and it wins over the `audit.typeAware` config field, which wins over `typeAware.enabled`. |
| `FALLOW_AUDIT_BASE` | Pin the `fallow audit` comparison base when `--base` / `--changed-since` is unset (precedence: flag > env > auto-detect). Escape hatch for the agent gate and forks, e.g. `FALLOW_AUDIT_BASE=upstream/main`. When unset, audit auto-detects the `git merge-base` against the branch's upstream or the remote default. A malformed value exits 2. |
| `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS` | Max age (in days since last reuse or fresh create) of a persistent reusable `fallow audit` base-snapshot worktree cache. Older entries are reclaimed at the top of the next `fallow audit` invocation (default: `30`). Wins over `audit.cacheMaxAgeDays` config field. `0` disables the GC; invalid values log a warning and fall back to config / default. Each sweep also reclaims abandoned entries from other repo identities (deleted or moved repos, other git worktrees); entries whose recorded owner root still exists are left to that repo's own sweep and setting. |
| `FALLOW_UPDATE_CHECK` | Set to `off`, `0`, `false`, `disabled`, or `no` to disable the human-TTY upgrade nudge and its background latest-version check. `DO_NOT_TRACK`, `FALLOW_TELEMETRY_DISABLED`, and CI also suppress it. |
| `FALLOW_CLAUDE_CODE_HINT` | Set to `off`, `0`, `false`, `no`, or `disabled` to suppress the one-line Claude Code plugin hint. Fallow writes the hint to stderr only inside a Claude Code session, for human output without `--quiet`, outside CI, and when the project has no Fallow plugin or skill. |
| `FALLOW_SUGGESTIONS` | Set to `off`, `0`, `false`, `no`, or `disabled` to suppress the top-level `next_steps[]` array of read-only follow-up commands in JSON output (and the human `Next:` line on bare `fallow`). Default on. Inherited by the MCP-spawned CLI, so it disables `next_steps` on MCP responses too. Useful for CI consumers that snapshot-diff raw `--format json`. |
| `FALLOW_COMMAND` | GitLab CI: command to run (default: `dead-code`). |
| `FALLOW_FAIL_ON_ISSUES` | GitLab CI: set to `true` to exit 1 if issues found. For dead-code, check, architecture and the dead-code part of the bare run, only findings whose rule is `error` count, the same as the CLI exit code. |
| `FALLOW_CHANGED_SINCE` | GitLab CI: git ref for incremental analysis. Auto-detected in MR pipelines. |
| `FALLOW_COMMENT` | GitLab CI: set to `true` to post MR summary comments. |
| `FALLOW_REVIEW` | GitLab CI: set to `true` to post inline code review comments on MR diffs. |
| `FALLOW_REVIEW_GUIDANCE` | Add collapsed "What to do" guidance blocks to `review-github` / `review-gitlab` inline comments. |
| `FALLOW_SUMMARY_SCOPE` | Sticky PR/MR summary scope for `pr-comment-github` / `pr-comment-gitlab`: `all` (default) keeps project-level findings outside the diff; `diff` applies the diff filter to those findings too. Inline review comments are unaffected. |
| `FALLOW_PR_COMMENT_LAYOUT` | Sticky PR/MR summary layout for `pr-comment-github` / `pr-comment-gitlab`: `default`, `compact`, `gate-only`, or `details`. |
| `FALLOW_SCORE` | GitLab CI: set to `true` to compute health score in combined mode. Enables health delta header in MR comments. |
| `FALLOW_TREND` | GitLab CI: set to `true` to compare current health metrics against saved snapshot. Implies `FALLOW_SCORE`. |
| `FALLOW_EXTRA_ARGS` | GitLab CI: additional CLI flags passed through to fallow. |
| `FALLOW_VERSION` | GitLab CI: fallow version to install. Empty (default) reads the project's `package.json` `fallow` dependency, then falls back to `latest`; set explicitly to override the local pin. |
| `FALLOW_SKIP_BINARY_VERIFY` | Skip Ed25519 + SHA-256 verification of platform binaries on first invocation of `fallow`, `fallow-lsp`, or `fallow-mcp` (and during the GitHub Action installer). Set to `1`, `true`, or `yes` ONLY when deliberately replacing the published binary (source builds, airgapped mirrors, signed-repack registries). The skip is recorded in `fallow --version` output as `verified: skipped (FALLOW_SKIP_BINARY_VERIFY is set)` so it stays visible in CI logs and vendor audits. Never set in regular CI; use the published binary or the documented out-of-band verification recipe in [`SECURITY.md`](https://github.com/fallow-rs/fallow/blob/main/SECURITY.md) instead. |
| `FALLOW_VERIFY_CACHE_DIR` | Override where the lazy-verify sentinel file is written. Cascade is platform-pkg-dir, then this override, then `$XDG_CACHE_HOME/fallow/sentinels/` (Linux/macOS) or `%LOCALAPPDATA%\fallow\sentinels\` (Windows). Useful when the platform pkg dir is read-only (yarn PnP, Docker layered images, pnpm verify-store). |
| `FALLOW_VERIFY_LOG` | Set to `1`, `true`, or `yes` to emit one structured stderr line per verify outcome (`fallow-verify outcome=ok cache=hit sentinel=...`). Off by default so MCP stdout/stderr stay clean; enable for CI diagnostic logs. |
| `FALLOW_TELEMETRY` | Opt-in product telemetry mode, off by default: `off`/`on`/`inspect` (plus `0`/`1`/`true`/`false`/`disabled`/`enabled`/`debug`/`log`). `inspect` prints the exact payload to stderr without sending. Wins over the user telemetry config. |
| `FALLOW_TELEMETRY_DISABLED` | Admin/fleet telemetry kill switch (top precedence, with `DO_NOT_TRACK`). Any value except empty, `0`, `false`, `no` or `off` hard-disables telemetry and refuses `fallow telemetry enable`. |
| `FALLOW_TELEMETRY_DEBUG` | Forces inspect mode; outranks `FALLOW_TELEMETRY`. |
| `DO_NOT_TRACK` | Top-precedence telemetry kill switch (consoledonottrack.com convention). Any value except empty, `0`, `false`, `no` or `off` disables telemetry. |
| `FALLOW_AGENT_SOURCE` | Declare the calling agent for telemetry classification (only used when telemetry is enabled; never enables it): `codex`, `claude_code`, `cursor`, `copilot`, `opencode`, `aider`, `roo`, `windsurf`, `gemini` (aliases `gemini_cli`/`antigravity`), `cline`, `continue`, `zed`, `goose`, `other_known`, `unknown`, `none`. Unrecognized values are ignored. |
| `GITLAB_TOKEN` | GitLab CI: project access token with `api` scope (for MR comments/reviews; `CI_JOB_TOKEN` is read-only for MR notes in the official GitLab API). |

Set `FALLOW_FORMAT=json` and `FALLOW_QUIET=1` in your agent environment to avoid passing flags on every invocation.
