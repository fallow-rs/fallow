//! User-facing environment variables listed in the `config-schema` output.

/// User-facing environment variables, in display order. A plain pair slice
/// (not a `json!` literal) because the map outgrew `json!`'s macro recursion
/// limit; insertion order is preserved by `serde_json`'s `preserve_order`
/// feature.
pub(super) const ENVIRONMENT_VARIABLES: &[(&str, &str)] = &[
    (
        "FALLOW_FORMAT",
        "Default output format (json/human/sarif/compact/markdown/codeclimate/gitlab-codequality/pr-comment-github/pr-comment-gitlab/review-github/review-gitlab/badge/github-annotations/github-summary). CLI --format flag overrides this.",
    ),
    (
        "FALLOW_QUIET",
        "Set to \"1\" or \"true\" to suppress progress output. CLI --quiet flag overrides this.",
    ),
    (
        "FALLOW_PRODUCTION",
        "Set to true/false to override production mode for all analyses.",
    ),
    (
        "FALLOW_PRODUCTION_DEAD_CODE",
        "Set to true/false to override production mode for dead-code analysis.",
    ),
    (
        "FALLOW_PRODUCTION_HEALTH",
        "Set to true/false to override production mode for health analysis.",
    ),
    (
        "FALLOW_PRODUCTION_DUPES",
        "Set to true/false to override production mode for duplication analysis.",
    ),
    (
        "FALLOW_REVIEW_GUIDANCE",
        "Set to true to append collapsed guidance blocks to review-github/review-gitlab inline comment bodies.",
    ),
    (
        "FALLOW_REVIEW_ID",
        "Stable 1-64 character identifier that isolates inline comments when multiple review jobs target the same PR/MR.",
    ),
    (
        "FALLOW_SUMMARY_SCOPE",
        "Summary scope for pr-comment-github/pr-comment-gitlab: all (default) keeps project-level dependency/catalog/override findings outside the diff filter; diff applies the diff filter to them too. Inline review comments are unaffected.",
    ),
    (
        "FALLOW_PR_COMMENT_LAYOUT",
        "Sticky PR comment layout: default, compact, gate-only, or details.",
    ),
    (
        "FALLOW_CONSOLIDATED_STATUS",
        "When split PR gate check runs are enabled, truthy values add one aggregate Fallow check alongside the per-gate checks.",
    ),
    (
        "FALLOW_DIFF_CONTEXT",
        "Line radius around changed diff lines when scoping findings to a diff in the review/PR-comment formats (default 3).",
    ),
    (
        "FALLOW_BOT_LOGIN",
        "Allowlisted posting username used to recognize fallow-owned finding roots and resolution replies. When set, it narrows ownership to that exact username (an empty value matches nobody); when unset, provider-native bot metadata is trusted. This is an authorship control, not a lifecycle or deduplication token.",
    ),
    (
        "FALLOW_API_RETRIES",
        "Maximum HTTP attempts for review-comment reconciliation API calls (default 3).",
    ),
    (
        "FALLOW_API_RETRY_DELAY",
        "Floor delay in seconds between HTTP retry attempts (default 2); a server-supplied Retry-After overrides it on 429 responses.",
    ),
    (
        "FALLOW_CACHE_DIR",
        "Directory for fallow's persistent analysis cache. Relative paths resolve from the project root and override cache.dir.",
    ),
    (
        "FALLOW_CACHE_MAX_SIZE",
        "Extraction cache size cap in megabytes (default 256). Wins over the cache.maxSizeMb config field.",
    ),
    (
        "FALLOW_PACKAGE_BASELINES",
        "Set to false, 0, no or off to ignore workspaces.changedSince for every run of the process, like --no-package-baselines.",
    ),
    (
        "FALLOW_EXTENDS_TIMEOUT_SECS",
        "Timeout in seconds for fetching https:// configs referenced via the extends field (default 5).",
    ),
    (
        "FALLOW_COVERAGE",
        "Path to Istanbul coverage data (coverage-final.json) or raw V8 coverage (a NODE_V8_COVERAGE directory or one V8 JSON file) for accurate per-function CRAP scores. CLI --coverage flag overrides this; it wins over the health.coverage config field. Honored by the health, bare fallow, audit, and viz CLI commands and by the MCP audit and check_health tools on both their typed route and their CLI fallback, where the explicit coverage parameter overrides it. Viz reads it through the same precedence and has no coverage flag of its own.",
    ),
    (
        "FALLOW_COVERAGE_ROOT",
        "Absolute coverage-data path prefix for rebasing Istanbul paths in CI or containers. CLI --coverage-root flag overrides this; it wins over the health.coverageRoot config field. Honored by the health, bare fallow, audit, and viz CLI commands and by the MCP audit and check_health tools on both their typed route and their CLI fallback, where the explicit coverage_root parameter overrides it. Viz reads it through the same precedence and has no coverage flag of its own.",
    ),
    (
        "FALLOW_MAX_FILE_SIZE",
        "Per-file size ceiling in megabytes for source discovery (default 5; 0 = no limit). CLI --max-file-size flag overrides this.",
    ),
    (
        "FALLOW_TYPE_AWARE",
        "Enable or disable TypeScript semantic (type-aware) analysis for the run (true/false/1/0/yes/no/on/off). Precedence: --type-aware/--no-type-aware CLI flags, then FALLOW_TYPE_AWARE, then the audit.typeAware config field, then typeAware.enabled.",
    ),
    (
        "FALLOW_TYPE_AWARE_TIMEOUT_SECS",
        "Wall clock in seconds allowed for one TypeScript semantic sidecar request (default 120; unset, 0, or a non-numeric value keeps the default). Raise it for very large TypeScript programs, where a semantic query scans the whole program.",
    ),
    (
        "FALLOW_TYPE_AWARE_BACKEND",
        "Experimental. Type-checker backend for the TypeScript semantic sidecar: typescript-go (default) or tsrs. The tsrs backend needs the exact pinned tsrs npm package where fallow-type-aware can resolve it. An unknown value makes the type-aware pass unavailable.",
    ),
    (
        "FALLOW_TYPE_AWARE_BIN",
        "Trusted executable override for the TypeScript semantic refinement sidecar used by dead-code --type-aware. Relative paths resolve from the caller's working directory before --root is applied; project node_modules and PATH are intentionally not searched. Default: sibling of the active Fallow executable.",
    ),
    (
        "FALLOW_AUDIT_BASE",
        "Pins the fallow audit comparison base ref when no --base/--changed-since is passed (e.g. upstream/main).",
    ),
    (
        "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS",
        "GC threshold in days for reusable audit base-snapshot caches (default 30; 0 disables the sweep).",
    ),
    (
        "FALLOW_IMPACT_STORE_MAX_AGE_DAYS",
        "GC threshold in days for per-project fallow impact stores; a recorded run reclaims stores older than this (unset/0 keeps every store forever).",
    ),
    (
        "FALLOW_ROOT",
        "Project root used by the review-github/review-gitlab renderers to read source for suggestion blocks. Set it alongside --root when rendering review formats outside the bundled CI integrations.",
    ),
    (
        "FALLOW_LICENSE",
        "License JWT (full string) for Fallow Cloud, the paid product; intended for shared CI runners.",
    ),
    (
        "FALLOW_LICENSE_PATH",
        "File path containing the license JWT.",
    ),
    (
        "FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS",
        "Clock-skew tolerance applied to the license JWT's iat claim (default 86400).",
    ),
    (
        "FALLOW_COV_BIN",
        "Explicit path override for the fallow-cov runtime-coverage sidecar binary.",
    ),
    (
        "FALLOW_COV_BINARY_PATH",
        "Secondary explicit path override for the fallow-cov sidecar, checked after FALLOW_COV_BIN (air-gapped installs, distro-packaged sidecars, shared Docker images).",
    ),
    (
        "FALLOW_RUNTIME_COVERAGE_SOURCE",
        "Set to cloud to select cloud runtime coverage in fallow coverage analyze without passing --cloud.",
    ),
    (
        "FALLOW_REPO",
        "owner/repo fallback for fallow coverage analyze --cloud when --repo is not passed (otherwise parsed from the git origin remote).",
    ),
    (
        "FALLOW_API_URL",
        "Base URL override for Fallow Cloud API calls (license refresh, trial, coverage uploads).",
    ),
    (
        "FALLOW_API_KEY",
        "Fallow Cloud bearer token for coverage upload commands, and the fallback bearer for fallow license refresh when the stored license JWT is missing or too stale.",
    ),
    (
        "FALLOW_CA_BUNDLE",
        "Path to a PEM certificate bundle for Fallow Cloud and provider HTTP calls (replaces the default WebPKI roots).",
    ),
    (
        "FALLOW_UPDATE_CHECK",
        "Set to off/0/false to disable the human-TTY upgrade nudge and its background version check.",
    ),
    (
        "FALLOW_CLAUDE_CODE_HINT",
        "Set to off/0/false/no/disabled to suppress the one-line Claude Code plugin hint on stderr. Fallow writes the hint only inside a Claude Code session (CLAUDECODE or CLAUDE_CODE_CHILD_SESSION), for human output without --quiet, outside CI, and when no Fallow plugin or skill is set up for the project. Default on.",
    ),
    (
        "FALLOW_SUGGESTIONS",
        "Set to off/0/false/no/disabled to suppress the next_steps[] array of read-only follow-up commands in JSON output (and the human Next: line). Useful for CI consumers that snapshot-diff raw --format json output. Default on.",
    ),
    (
        "FALLOW_TELEMETRY",
        "Opt-in telemetry mode: off, on, or inspect (print the payload to stderr without sending). Telemetry is off by default.",
    ),
    (
        "FALLOW_TELEMETRY_DISABLED",
        "Admin/fleet kill switch: truthy values hard-disable telemetry and refuse fallow telemetry enable.",
    ),
    (
        "FALLOW_TELEMETRY_DEBUG",
        "Truthy values alias FALLOW_TELEMETRY=inspect.",
    ),
    (
        "FALLOW_AGENT_SOURCE",
        "Normalized agent vendor for telemetry classification (e.g. claude_code, codex, cursor). Only read when telemetry is on.",
    ),
    (
        "DO_NOT_TRACK",
        "Honored as a top-precedence telemetry kill switch (consoledonottrack.com convention).",
    ),
    (
        "FALLOW_BIN",
        "Path to the fallow binary (used by the fallow-mcp server to spawn the CLI).",
    ),
    (
        "FALLOW_TIMEOUT_SECS",
        "MCP server: per-tool-call CLI subprocess timeout in seconds (default 120). Raise for long runs like production coverage on large dumps.",
    ),
    (
        "FALLOW_DIFF_FILE",
        "MCP server: path to a unified diff that scopes all findings by changed line.",
    ),
    (
        "FALLOW_CHANGED_SINCE",
        "MCP server: git ref that scopes file discovery for analysis tools.",
    ),
    (
        "FALLOW_MCP_WARM_SESSION",
        "MCP server: set to 0, false, off or no to stop typed tool calls from keeping parsed modules in memory between calls (default on).",
    ),
    (
        "FALLOW_INTEGRATION_SURFACE",
        "Telemetry integration_surface override for non-CLI surfaces (mcp/lsp/vscode/napi/programmatic). Set by the MCP server on the CLI it spawns.",
    ),
    (
        "FALLOW_MCP_TOOL",
        "Telemetry mcp_tool dimension, validated against the MCP tool-name allowlist. Set by the MCP server alongside FALLOW_INTEGRATION_SURFACE=mcp.",
    ),
    (
        "FALLOW_LSP_REUSE_SESSION",
        "Language server: set to 0/false/off/no to load a new project session on each analysis run. By default the server keeps one session per project root between saves and parses only the changed files.",
    ),
];
