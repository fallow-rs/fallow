//! User-facing environment variables listed in the `fallow schema` output.

/// One user-facing environment variable.
pub(super) struct EnvironmentVariable {
    pub name: &'static str,
    /// A short stable label, one of the group constants of this module.
    pub group: &'static str,
    /// The value that the code uses when the variable is unset.
    pub default: Option<&'static str>,
    pub description: &'static str,
}

const fn var(
    name: &'static str,
    group: &'static str,
    default: Option<&'static str>,
    description: &'static str,
) -> EnvironmentVariable {
    EnvironmentVariable {
        name,
        group,
        default,
        description,
    }
}

const ANALYSIS: &str = "analysis";
const CACHE: &str = "cache";
const CI: &str = "ci";
const CLOUD: &str = "cloud";
const COMMIT_GATE: &str = "commit-gate";
const COVERAGE: &str = "coverage";
const LSP: &str = "lsp";
const MCP: &str = "mcp";
const OUTPUT: &str = "output";
const TELEMETRY: &str = "telemetry";
const TYPE_AWARE: &str = "type-aware";

/// Every group label. A test checks that each variable uses one of them and
/// that each label is in use.
#[cfg(test)]
pub(super) const ENVIRONMENT_GROUPS: &[&str] = &[
    ANALYSIS,
    CACHE,
    CI,
    CLOUD,
    COMMIT_GATE,
    COVERAGE,
    LSP,
    MCP,
    OUTPUT,
    TELEMETRY,
    TYPE_AWARE,
];

/// User-facing environment variables, in display order. Insertion order is
/// preserved by `serde_json`'s `preserve_order` feature.
pub(super) const ENVIRONMENT_VARIABLES: &[EnvironmentVariable] = &[
    var(
        "FALLOW_FORMAT",
        OUTPUT,
        Some("human"),
        "Default output format (json/human/sarif/compact/markdown/codeclimate/gitlab-codequality/pr-comment-github/pr-comment-gitlab/review-github/review-gitlab/badge/github-annotations/github-summary). CLI --format flag overrides this.",
    ),
    var(
        "FALLOW_QUIET",
        OUTPUT,
        None,
        "Set to \"1\" or \"true\" to suppress progress output. CLI --quiet flag overrides this.",
    ),
    var(
        "FALLOW_PRODUCTION",
        ANALYSIS,
        None,
        "Set to true/false to override production mode for all analyses.",
    ),
    var(
        "FALLOW_PRODUCTION_DEAD_CODE",
        ANALYSIS,
        None,
        "Set to true/false to override production mode for dead-code analysis.",
    ),
    var(
        "FALLOW_PRODUCTION_HEALTH",
        ANALYSIS,
        None,
        "Set to true/false to override production mode for health analysis.",
    ),
    var(
        "FALLOW_PRODUCTION_DUPES",
        ANALYSIS,
        None,
        "Set to true/false to override production mode for duplication analysis.",
    ),
    var(
        "FALLOW_REVIEW_GUIDANCE",
        CI,
        None,
        "Set to true to append collapsed guidance blocks to review-github/review-gitlab inline comment bodies.",
    ),
    var(
        "FALLOW_REVIEW_ID",
        CI,
        None,
        "Stable 1-64 character identifier that isolates inline comments when multiple review jobs target the same PR/MR.",
    ),
    var(
        "FALLOW_SUMMARY_SCOPE",
        CI,
        Some("all"),
        "Summary scope for pr-comment-github/pr-comment-gitlab: all (default) keeps project-level dependency/catalog/override findings outside the diff filter; diff applies the diff filter to them too. Inline review comments are unaffected.",
    ),
    var(
        "FALLOW_PR_COMMENT_LAYOUT",
        CI,
        Some("default"),
        "Sticky PR comment layout: default, compact, gate-only, or details.",
    ),
    var(
        "FALLOW_CONSOLIDATED_STATUS",
        CI,
        None,
        "When split PR gate check runs are enabled, truthy values add one aggregate Fallow check alongside the per-gate checks.",
    ),
    var(
        "FALLOW_DIFF_CONTEXT",
        CI,
        Some("3"),
        "Line radius around changed diff lines when scoping findings to a diff in the review/PR-comment formats (default 3).",
    ),
    var(
        "FALLOW_BOT_LOGIN",
        CI,
        None,
        "Allowlisted posting username used to recognize fallow-owned finding roots and resolution replies. When set, it narrows ownership to that exact username (an empty value matches nobody); when unset, provider-native bot metadata is trusted. This is an authorship control, not a lifecycle or deduplication token.",
    ),
    var(
        "FALLOW_API_RETRIES",
        CI,
        Some("3"),
        "Maximum HTTP attempts for review-comment reconciliation API calls (default 3).",
    ),
    var(
        "FALLOW_API_RETRY_DELAY",
        CI,
        Some("2"),
        "Floor delay in seconds between HTTP retry attempts (default 2); a server-supplied Retry-After overrides it on 429 responses.",
    ),
    var(
        "FALLOW_CACHE_DIR",
        CACHE,
        None,
        "Directory for fallow's persistent analysis cache. Relative paths resolve from the project root and override cache.dir.",
    ),
    var(
        "FALLOW_CACHE_MAX_SIZE",
        CACHE,
        Some("256"),
        "Extraction cache size cap in megabytes (default 256). Wins over the cache.maxSizeMb config field. When the cache file grows past 80 percent of the cap, fallow drops the least recently used entries until the file is below 60 percent.",
    ),
    var(
        "FALLOW_PACKAGE_BASELINES",
        ANALYSIS,
        None,
        "Set to false, 0, no or off to ignore workspaces.changedSince for every run of the process, like --no-package-baselines.",
    ),
    var(
        "FALLOW_EXTENDS_TIMEOUT_SECS",
        ANALYSIS,
        Some("5"),
        "Timeout in seconds for fetching https:// configs referenced via the extends field (default 5).",
    ),
    var(
        "FALLOW_COVERAGE",
        COVERAGE,
        None,
        "Path to Istanbul coverage data (coverage-final.json) or raw V8 coverage (a NODE_V8_COVERAGE directory or one V8 JSON file) for accurate per-function CRAP scores. CLI --coverage flag overrides this; it wins over the health.coverage config field. Honored by the health, bare fallow, audit, and viz CLI commands and by the MCP audit and check_health tools on both their typed route and their CLI fallback, where the explicit coverage parameter overrides it. Viz reads it through the same precedence and has no coverage flag of its own.",
    ),
    var(
        "FALLOW_COVERAGE_ROOT",
        COVERAGE,
        None,
        "Absolute coverage-data path prefix for rebasing Istanbul paths in CI or containers. CLI --coverage-root flag overrides this; it wins over the health.coverageRoot config field. Honored by the health, bare fallow, audit, and viz CLI commands and by the MCP audit and check_health tools on both their typed route and their CLI fallback, where the explicit coverage_root parameter overrides it. Viz reads it through the same precedence and has no coverage flag of its own.",
    ),
    var(
        "FALLOW_MAX_FILE_SIZE",
        ANALYSIS,
        Some("5"),
        "Per-file size ceiling in megabytes for source discovery (default 5; 0 = no limit). CLI --max-file-size flag overrides this.",
    ),
    var(
        "FALLOW_TYPE_AWARE",
        TYPE_AWARE,
        None,
        "Enable or disable TypeScript semantic (type-aware) analysis for the run (true/false/1/0/yes/no/on/off). Precedence: --type-aware/--no-type-aware CLI flags, then FALLOW_TYPE_AWARE, then the audit.typeAware config field, then typeAware.enabled.",
    ),
    var(
        "FALLOW_TYPE_AWARE_PROJECTS",
        TYPE_AWARE,
        None,
        "The TypeScript project configs of type-aware analysis, separated by the path separator of the platform. The --type-aware-project flag wins over it, and it wins over typeAware.projects.",
    ),
    var(
        "FALLOW_TYPE_AWARE_REQUIRE",
        TYPE_AWARE,
        None,
        "The completeness policy of type-aware analysis: best-effort or complete. The --type-aware-require flag wins over it, and it wins over typeAware.require.",
    ),
    var(
        "FALLOW_TYPE_AWARE_TIMEOUT_SECS",
        TYPE_AWARE,
        Some("120"),
        "Wall clock in seconds allowed for one request to the type-aware companion (default 120; unset, 0, or a non-numeric value keeps the default). Raise it for very large TypeScript programs, where a semantic query scans the whole program.",
    ),
    var(
        "FALLOW_TYPE_AWARE_BACKEND",
        TYPE_AWARE,
        Some("typescript-go"),
        "Experimental. Type-checker backend for the type-aware companion: typescript-go (default) or tsrs. The tsrs backend needs a tsrs npm package in the supported range of the type-aware protocol manifest, where fallow-type-aware can resolve it. An unknown value makes the type-aware pass unavailable.",
    ),
    var(
        "FALLOW_TYPE_AWARE_BIN",
        TYPE_AWARE,
        None,
        "Trusted executable override for the type-aware companion that --type-aware runs. Relative paths resolve from the caller's working directory before --root is applied; project node_modules and PATH are intentionally not searched. Default: sibling of the active Fallow executable.",
    ),
    var(
        "FALLOW_AUDIT_BASE",
        ANALYSIS,
        None,
        "Pins the fallow audit comparison base ref when no --base/--changed-since is passed (e.g. upstream/main).",
    ),
    var(
        "FALLOW_AUDIT_CACHE_MAX_AGE_DAYS",
        CACHE,
        Some("30"),
        "GC threshold in days for reusable audit base-snapshot caches (default 30; 0 disables the sweep). Each cache version keeps only its 32 newest snapshot files.",
    ),
    var(
        "FALLOW_IMPACT_STORE_MAX_AGE_DAYS",
        CACHE,
        None,
        "GC threshold in days for per-project fallow impact stores; a recorded run reclaims stores older than this (unset/0 keeps every store forever).",
    ),
    var(
        "FALLOW_CLOCK_EPOCH",
        ANALYSIS,
        None,
        "Reference instant in unix seconds for commit recency and ownership staleness. Default: the committer timestamp of HEAD. Set it to compare two checkouts against one fixed instant. An empty or non-numeric value is ignored with a warning.",
    ),
    var(
        "FALLOW_ROOT",
        CI,
        None,
        "Project root used by the review-github/review-gitlab renderers to read source for suggestion blocks. Set it alongside --root when rendering review formats outside the bundled CI integrations.",
    ),
    var(
        "FALLOW_LICENSE",
        CLOUD,
        None,
        "License JWT (full string) for Fallow Cloud; intended for shared CI runners.",
    ),
    var(
        "FALLOW_LICENSE_PATH",
        CLOUD,
        None,
        "File path containing the license JWT.",
    ),
    var(
        "FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS",
        CLOUD,
        Some("86400"),
        "Clock-skew tolerance applied to the license JWT's iat claim (default 86400).",
    ),
    var(
        "FALLOW_COV_BIN",
        COVERAGE,
        None,
        "Path to the runtime coverage add-on (`fallow-cov`). Fallow checks it before every other location.",
    ),
    var(
        "FALLOW_COV_BINARY_PATH",
        COVERAGE,
        None,
        "Second path to the runtime coverage add-on (`fallow-cov`), checked after FALLOW_COV_BIN (air-gapped installs, distro packages, shared Docker images).",
    ),
    var(
        "FALLOW_RUNTIME_COVERAGE_SOURCE",
        COVERAGE,
        None,
        "Set to cloud to select cloud runtime coverage in fallow coverage analyze without passing --cloud.",
    ),
    var(
        "FALLOW_REPO",
        CLOUD,
        None,
        "owner/repo fallback for fallow coverage analyze --cloud and fallow inspect --cloud when --repo is not passed (otherwise parsed from the git origin remote).",
    ),
    var(
        "FALLOW_API_URL",
        CLOUD,
        Some("https://api.fallow.cloud"),
        "Base URL override for Fallow Cloud API calls (license refresh, trial, coverage uploads).",
    ),
    var(
        "FALLOW_API_KEY",
        CLOUD,
        None,
        "Fallow Cloud bearer token for coverage upload and cloud read commands (wins over the key that fallow cloud login stored), and the fallback bearer for fallow license refresh when the stored license JWT is missing or too stale.",
    ),
    var(
        "FALLOW_CREDENTIAL_STORE",
        CLOUD,
        Some("auto"),
        "Credential store for the key that fallow cloud login writes: auto (default: OS keychain, else ~/.config/fallow/credentials), file (credentials file only) or none (no stored key is read or written).",
    ),
    var(
        "FALLOW_CA_BUNDLE",
        CLOUD,
        None,
        "Path to a PEM certificate bundle for Fallow Cloud and provider HTTP calls (replaces the default WebPKI roots).",
    ),
    var(
        "FALLOW_UPDATE_CHECK",
        OUTPUT,
        Some("on"),
        "Set to off/0/false to disable the human-TTY upgrade nudge and its background version check.",
    ),
    var(
        "FALLOW_CLAUDE_CODE_HINT",
        OUTPUT,
        Some("on"),
        "Set to off/0/false/no/disabled to suppress the one-line Claude Code plugin hint on stderr. Fallow writes the hint only inside a Claude Code session (CLAUDECODE or CLAUDE_CODE_CHILD_SESSION), for human output without --quiet, outside CI, and when no Fallow plugin or skill is set up for the project. Default on.",
    ),
    var(
        "FALLOW_SUGGESTIONS",
        OUTPUT,
        Some("on"),
        "Set to off/0/false/no/disabled to suppress the next_steps[] array of read-only follow-up commands in JSON output (and the human Next: line). Useful for CI consumers that snapshot-diff raw --format json output. Default on.",
    ),
    var(
        "FALLOW_TELEMETRY",
        TELEMETRY,
        Some("off"),
        "Opt-in telemetry mode: off, on, or inspect (print the payload to stderr without sending). Telemetry is off by default.",
    ),
    var(
        "FALLOW_TELEMETRY_DISABLED",
        TELEMETRY,
        None,
        "Admin/fleet kill switch: truthy values hard-disable telemetry and refuse fallow telemetry enable.",
    ),
    var(
        "FALLOW_TELEMETRY_DEBUG",
        TELEMETRY,
        None,
        "Truthy values alias FALLOW_TELEMETRY=inspect.",
    ),
    var(
        "FALLOW_AGENT_SOURCE",
        TELEMETRY,
        None,
        "Normalized agent vendor for telemetry classification (e.g. claude_code, codex, cursor). Only read when telemetry is on.",
    ),
    var(
        "DO_NOT_TRACK",
        TELEMETRY,
        None,
        "Honored as a top-precedence telemetry kill switch (consoledonottrack.com convention).",
    ),
    var(
        "FALLOW_BIN",
        MCP,
        None,
        "Path to the fallow binary that the fallow-mcp server runs. When it is unset, the server runs the `fallow` file next to its own executable, else `fallow` on PATH.",
    ),
    var(
        "FALLOW_TIMEOUT_SECS",
        MCP,
        Some("120"),
        "MCP server: per-tool-call CLI subprocess timeout in seconds (default 120). Raise for long runs like production coverage on large dumps.",
    ),
    var(
        "FALLOW_MCP_MAX_OUTPUT_BYTES",
        MCP,
        Some("1000000"),
        "MCP server: response cap in bytes for each tool call (default 1000000, highest 16777216). A larger response is refused with FALLOW_MCP_SUBPROCESS_OUTPUT_LIMIT and a narrow list of parameters. The max_output_bytes parameter of a call can only lower this cap.",
    ),
    var(
        "FALLOW_DIFF_FILE",
        MCP,
        None,
        "MCP server: path to a unified diff that scopes all findings by changed line.",
    ),
    var(
        "FALLOW_CHANGED_SINCE",
        MCP,
        None,
        "MCP server: git ref that scopes file discovery for analysis tools.",
    ),
    var(
        "FALLOW_MCP_WARM_SESSION",
        MCP,
        Some("on"),
        "MCP server: set to 0, false, off or no to stop typed tool calls from keeping parsed modules in memory between calls (default on).",
    ),
    var(
        "FALLOW_INTEGRATION_SURFACE",
        TELEMETRY,
        None,
        "Telemetry integration_surface override for non-CLI surfaces (mcp/lsp/vscode/napi/programmatic). Set by the MCP server on the CLI it spawns.",
    ),
    var(
        "FALLOW_MCP_TOOL",
        TELEMETRY,
        None,
        "Telemetry mcp_tool dimension, validated against the MCP tool-name allowlist. Set by the MCP server alongside FALLOW_INTEGRATION_SURFACE=mcp.",
    ),
    var(
        "FALLOW_LSP_REUSE_SESSION",
        LSP,
        Some("on"),
        "Language server: set to 0/false/off/no to load a new project session on each analysis run. By default the server keeps one session per project root between saves and parses only the changed files.",
    ),
    var(
        "FALLOW_GATE_REPORT",
        COMMIT_GATE,
        Some(".fallow/gate-report.json"),
        "Agent gate script of fallow agent install: path of the full JSON report that the gate writes on a block (default .fallow/gate-report.json). The fallow binary does not read it.",
    ),
    var(
        "FALLOW_GATE_OUTPUT",
        COMMIT_GATE,
        None,
        "Agent gate script of fallow agent install: set to json to print the full JSON report on stderr instead of the compact block message. The fallow binary does not read it.",
    ),
    var(
        "FALLOW_GATE_MIN_VERSION",
        COMMIT_GATE,
        Some("2.85.0"),
        "Agent gate script of fallow agent install: lowest fallow version that the gate accepts. Set it to the empty string to disable the check. The fallow binary does not read it.",
    ),
    var(
        "FALLOW_GATE_DEBUG",
        COMMIT_GATE,
        None,
        "Agent gate script of fallow agent install: set to a non-empty value to log each command that the gate skips because it is not a git commit or push. The fallow binary does not read it.",
    ),
];

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::{ENVIRONMENT_GROUPS, ENVIRONMENT_VARIABLES};

    /// The maintainer list of environment variables and the registry that
    /// `fallow schema` prints must agree. The variables that the bundled CI
    /// scripts set stay out of the registry.
    #[test]
    fn environment_registry_matches_the_maintainer_list() {
        const SET_BY_CI_SCRIPTS: &[&str] = &["FALLOW_MAX_COMMENTS"];
        let doc = include_str!("../../../../docs/environment-variables.md");
        let registry: FxHashSet<&str> = ENVIRONMENT_VARIABLES
            .iter()
            .map(|variable| variable.name)
            .collect();
        for line in doc.lines() {
            let Some(rest) = line.strip_prefix("| `") else {
                continue;
            };
            let Some((name, _)) = rest.split_once('`') else {
                continue;
            };
            if name == "Variable" || SET_BY_CI_SCRIPTS.contains(&name) {
                continue;
            }
            assert!(
                registry.contains(name),
                "{name} is in docs/environment-variables.md but not in the schema registry"
            );
        }
        for name in ["FALLOW_CLOCK_EPOCH", "FALLOW_MCP_MAX_OUTPUT_BYTES"] {
            assert!(
                registry.contains(name),
                "{name} is missing from the registry"
            );
        }
    }

    #[test]
    fn every_variable_has_one_known_group_and_every_group_is_used() {
        for variable in ENVIRONMENT_VARIABLES {
            assert!(
                ENVIRONMENT_GROUPS.contains(&variable.group),
                "{} has the unknown group {}",
                variable.name,
                variable.group
            );
            assert!(
                variable.default.is_none_or(|value| !value.is_empty()),
                "{} has an empty default",
                variable.name
            );
        }
        for group in ENVIRONMENT_GROUPS {
            assert!(
                ENVIRONMENT_VARIABLES
                    .iter()
                    .any(|variable| variable.group == *group),
                "no variable uses the group {group}"
            );
        }
    }

    /// The gate script owns the version floor; the registry repeats it.
    #[test]
    fn gate_min_version_default_matches_the_gate_script() {
        let floor = crate::setup_hooks::FALLOW_GATE_SCRIPT
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("MIN_VERSION=\"${FALLOW_GATE_MIN_VERSION-")
                    .and_then(|value| value.strip_suffix("}\""))
            })
            .expect("the gate script sets a version floor");
        let default = ENVIRONMENT_VARIABLES
            .iter()
            .find(|variable| variable.name == "FALLOW_GATE_MIN_VERSION")
            .and_then(|variable| variable.default);
        assert_eq!(default, Some(floor));
    }
}
