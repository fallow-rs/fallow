use std::process::ExitCode;

use clap::CommandFactory;
#[cfg(test)]
use fallow_output::issue_output_contracts;
use fallow_output::{TsAliasMeta, issue_output_contract_by_code};
use fallow_types::issue_meta::{ISSUE_KIND_META, issue_meta_by_code};
use fallow_types::mcp_manifest::{MCP_TOOLS, RUNTIME_COVERAGE_LICENSE_NOTE};
use fallow_types::suppress::IssueKind;

use crate::Cli;
use crate::explain::{
    CHECK_RULES, DUPES_RULES, FLAGS_RULES, HEALTH_RULES, RuleDef, SECURITY_RULES, rule_docs_url,
};

pub fn run_schema(json_style: crate::json_style::JsonStyle) -> ExitCode {
    let cmd = Cli::command();
    let schema = build_cli_schema(&cmd);
    match json_style.serialize(&schema) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize schema: {e}");
            ExitCode::from(2)
        }
    }
}

pub fn build_cli_schema(cmd: &clap::Command) -> serde_json::Value {
    let mut global_flags = Vec::new();
    for arg in cmd.get_arguments() {
        if arg.get_id() == "help" || arg.get_id() == "version" {
            continue;
        }
        global_flags.push(build_arg_schema(arg));
    }

    let mut commands = Vec::new();
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let mut flags = Vec::new();
        for arg in sub.get_arguments() {
            if arg.get_id() == "help" || arg.get_id() == "version" {
                continue;
            }
            flags.push(build_arg_schema(arg));
        }
        commands.push(serde_json::json!({
            "name": sub.get_name(),
            "description": sub.get_about().map(std::string::ToString::to_string),
            "flags": flags,
        }));
    }

    serde_json::json!({
        "name": cmd.get_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "manifest_version": "1",
        "description": cmd.get_about().map(std::string::ToString::to_string),
        "global_flags": global_flags,
        "commands": commands,
        "default_command": null,
        "default_behavior": "Runs all analyses (check + dupes + health). Use --only/--skip to select.",
        "issue_types": issue_types_schema(),
        "suppression_comments": {
            "next_line": "// fallow-ignore-next-line [issue-type]",
            "file": "// fallow-ignore-file [issue-type]",
            "note": "Omit [issue-type] to suppress all issue types. Unknown tokens are silently ignored."
        },
        "output_formats": ["human", "json", "sarif", "compact", "markdown", "codeclimate", "gitlab-codequality", "pr-comment-github", "pr-comment-gitlab", "review-github", "review-gitlab", "badge"],
        "exit_codes": {
            "0": "Success (no error-severity issues found)",
            "1": "Error-severity issues found (per rules config, or --fail-on-issues promotes warn→error)",
            "2": "Error (invalid config, invalid input, etc.). When --format json is active, errors are emitted as structured JSON on stdout: {\"error\": true, \"message\": \"...\", \"exit_code\": 2}"
        },
        "environment_variables": environment_variables_schema(),
        "severity_levels": ["error", "warn", "off"],
        "field_notes": {
            "default_severity": "The rule's severity under zero config (error/warn/off). null means the finding is not gated by a rules.* severity (a metric or command-driven finding, or a security-catalogue row).",
            "opt_in": "true when the rule defaults to off and reports nothing until enabled. Enabling an opt_in rule blindly is the most common way to flood a repo with findings, so treat true as a deliberate decision.",
            "frameworks": "Non-empty ONLY when the rule's DETECTOR self-gates on that framework (it does nothing unless the framework is declared). An EMPTY array does NOT mean the rule is framework-agnostic: many rules are framework-relevant (see the description) yet fire on any matching syntax, so never disable a rule solely because frameworks is empty.",
            "config_key": "The canonical key under `rules` in the config file, i.e. rules.<config_key>. The `id` is an accepted alias. null for findings that are not configured via a rules.* severity."
        },
        "related_schemas": {
            "note": "This manifest lists RULES, capabilities, presets, and the taste catalog. To author a config FILE you also need its shape, which is a separate schema.",
            "config_schema_command": "fallow config-schema",
            "config_schema_note": "Full JSON Schema of the config file: every top-level key (rules, entry, ignorePatterns, workspaces, boundaries, duplicates, health, security, rulePacks, production, cache, ...) and its shape. entry and ignorePatterns are how you declare entry points and exclusions; fallow also auto-honors package.json exports/main/module for library public APIs.",
            "rule_pack_schema_command": "fallow rule-pack-schema",
            "rule_pack_schema_note": "JSON Schema for a declarative rule pack referenced from rulePacks.",
            "plugin_schema_command": "fallow plugin-schema",
            "plugin_schema_note": "JSON Schema for a user-authored external plugin (fallow-plugin-*.jsonc). Teach fallow about an unsupported framework declaratively: detection, entryPoints, alwaysUsed, usedExports, usedClassMembers, and manifestEntries (derive entry points from per-package manifest files).",
            "plugin_check_command": "fallow plugin-check",
            "plugin_check_note": "Read-only dry-run of your external plugins: reports which activated, which manifests each manifestEntries rule matched, what it seeded (with path-exists), and typed warnings (manifests-matched-none, when-excluded-all, field-path-unresolved, entries-empty, manifest-parse-failed, entry-outside-root, seeded-paths-missing). Run it after authoring a fallow-plugin-*.jsonc to verify it before a full analysis.",
            "config_files": [".fallowrc.json", ".fallowrc.jsonc", "fallow.toml", ".fallow.toml"]
        },
        "boundary_presets": crate::onboarding::boundary_presets_schema(),
        "taste_choices": crate::onboarding::taste_choices_schema(),
        "security_categories": security_categories_schema(),
        "mcp_tools": mcp_tools_schema(),
        "plugins": plugins_schema(),
        "task_matrix": task_matrix_schema(),
    })
}

/// Agent-discoverability task-to-command matrix (R2). One row per agent
/// intent; the `command` may contain `<placeholder>` tokens (docs context),
/// unlike the runnable-only `next_steps[]` contract. `note` is always present
/// (null when None) to honor the manifest's no-absent-key convention. Sourced
/// from the single `crate::task_matrix::TASK_MATRIX` slice that also drives the
/// `init --agents` template, the agent-hook managed block, and root `--help`.
fn task_matrix_schema() -> serde_json::Value {
    serde_json::Value::Array(
        crate::task_matrix::TASK_MATRIX
            .iter()
            .map(|row| {
                serde_json::json!({
                    "task": row.task,
                    "command": row.command,
                    "note": row.note,
                })
            })
            .collect(),
    )
}

/// Per-issue-type metadata that cannot be derived from the explain rule
/// registry: CLI filter flag, fixability, suppression-comment shape, and
/// caveats. A rule without an arm in the per-command meta functions below
/// gets safe defaults (no filter flag, not fixable, not suppressible);
/// add an arm when a new rule has any of those capabilities.
#[derive(Default)]
struct IssueTypeMeta {
    label: Option<&'static str>,
    config_key: Option<&'static str>,
    registry_index: Option<usize>,
    aliases: &'static [&'static str],
    lsp: bool,
    filter_flag: Option<&'static str>,
    result_key: Option<&'static str>,
    summary_label: Option<&'static str>,
    summary_docs_anchor: Option<&'static str>,
    sarif_rule_ids: Option<Vec<String>>,
    codeclimate_check_names: Option<Vec<String>>,
    ts_alias: Option<TsAliasMeta>,
    counts_in_total: bool,
    fixable: bool,
    /// `(suppression token, file_level)` when comment-suppressible. The
    /// token MUST round-trip through `IssueKind::parse`; a test below
    /// enforces it so agents never copy a no-op suppression comment.
    suppress: Option<(&'static str, bool)>,
    note: Option<&'static str>,
    freemium: bool,
}

impl IssueTypeMeta {
    fn from_shared(bare_id: &str) -> Self {
        let mut meta = Self::default();
        if let Some(shared) = issue_meta_by_code(bare_id) {
            meta.label = Some(shared.label);
            meta.config_key = shared.config_key;
            meta.registry_index = ISSUE_KIND_META
                .iter()
                .position(|candidate| candidate.code == shared.code);
            meta.aliases = shared.aliases;
            meta.lsp = shared.lsp;
            meta.filter_flag = shared.filter_flag;
            if let Some(token) = shared.suppress_token {
                meta.suppress = Some((token, shared.suppress_file_level));
            }
        }
        if let Some(contract) = issue_output_contract_by_code(bare_id) {
            meta.result_key = Some(contract.result_key);
            meta.summary_label = Some(contract.summary_label);
            meta.summary_docs_anchor = Some(contract.summary_docs_anchor);
            meta.sarif_rule_ids = Some(contract.sarif_rule_ids);
            if !contract.codeclimate_check_names.is_empty() {
                meta.codeclimate_check_names = Some(contract.codeclimate_check_names);
            }
            meta.ts_alias = contract.ts_alias;
            meta.counts_in_total = contract.counts_in_total;
        }
        meta
    }
}

/// The machine-readable vocabulary for `security.categories.include` /
/// `exclude`: every valid category id, its title, CWE (when it maps to one),
/// and whether it is include-required (runs only when explicitly named). This
/// is the id list the config file's `security` key needs and the config-schema
/// itself does not carry (it only describes the mechanism).
fn security_categories_schema() -> serde_json::Value {
    let categories: Vec<serde_json::Value> = fallow_security::security_categories()
        .into_iter()
        .map(|category| {
            serde_json::json!({
                "id": category.id,
                "title": category.title,
                "cwe": category.cwe,
                "include_required": category.include_required,
            })
        })
        .collect();
    serde_json::json!({
        "note": "Valid ids for security.categories.include / exclude. include-required categories (hardcoded-secret, secret-to-network) run only when explicitly listed in categories.include; all others are admitted by default unless an include list restricts to a whitelist.",
        "categories": categories,
    })
}

fn issue_types_schema() -> serde_json::Value {
    // A flat map of config_key -> default severity string, serialized ONCE from
    // RulesConfig::default(). This is the single source of default severities
    // and covers every rule that has a `rules.*` config field, including rules
    // that carry no distinct IssueKind (e.g. unused-optional-dependency, whose
    // finding folds into another kind but which is independently configurable).
    // Infallible in practice (a flat struct of Severity enums); the empty-map
    // fallback keeps this panic-free and simply yields null default_severity if
    // serialization ever changed shape.
    let default_severities = serde_json::to_value(fallow_config::RulesConfig::default())
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
    let mut rows = Vec::new();
    for rule in CHECK_RULES {
        rows.push(issue_type_row(rule, "dead-code", &default_severities));
    }
    for rule in HEALTH_RULES {
        rows.push(issue_type_row(rule, "health", &default_severities));
    }
    for rule in DUPES_RULES {
        rows.push(issue_type_row(rule, "dupes", &default_severities));
    }
    for rule in FLAGS_RULES {
        rows.push(issue_type_row(rule, "flags", &default_severities));
    }
    for rule in SECURITY_RULES {
        rows.push(issue_type_row(rule, "security", &default_severities));
    }
    serde_json::Value::Array(rows)
}

/// The frameworks whose DETECTOR self-gates on the framework being declared, so
/// the rule genuinely does nothing unless the project uses that framework.
///
/// Only a rule whose detector short-circuits on a framework dependency belongs
/// here. "Commonly relevant to framework X" is the OPPOSITE semantics and must
/// NOT be encoded: labeling a framework-agnostic rule (e.g. `unused-server-action`,
/// which fires on any `use server` export) as framework-specific would tell an
/// agent to disable a real safeguard. When in doubt, return the empty slice.
/// Verified against the detectors: `route_collision` / `dynamic_segment_name_conflict`
/// / `invalid_client_export` gate on `declared_deps.contains("next")`;
/// `misplaced_directive` / `mixed_client_server_barrel` gate on the broader
/// framework-agnostic RSC-directive predicate, so they stay empty.
fn frameworks_for_kind(kind: IssueKind) -> &'static [&'static str] {
    match kind {
        IssueKind::RouteCollision
        | IssueKind::DynamicSegmentNameConflict
        | IssueKind::InvalidClientExport => &["next"],
        _ => &[],
    }
}

fn issue_type_row(
    rule: &RuleDef,
    command: &str,
    default_severities: &serde_json::Value,
) -> serde_json::Value {
    let bare_id = rule.id.split_once('/').map_or(rule.id, |(_, bare)| bare);
    let meta = issue_type_meta(bare_id, command);
    let kind = meta
        .registry_index
        .and_then(|i| ISSUE_KIND_META.get(i))
        .and_then(|m| m.kind);
    // Default severity comes from the rule that GATES this finding: its own
    // `config_key` when it is a 1:1 rule, else the suppression token's rule when
    // the finding is gated by a shared rule (every tainted-sink category is
    // gated by `security-sink`; coverage findings by `coverage-gaps`). This lets
    // opt-in command families (security, coverage) expose default_severity/opt_in
    // like any other rule instead of reading as null. Stays null only for
    // findings with no `rules.*` gate at all (complexity, duplication metrics).
    let severity_key = meta
        .config_key
        .or_else(|| meta.suppress.map(|(token, _)| token));
    let default_severity = severity_key
        .and_then(|key| default_severities.get(key))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let opt_in = default_severity.as_deref().map(|sev| sev == "off");
    let frameworks = kind.map_or(&[][..], frameworks_for_kind);
    let suppress_comment = meta.suppress.map(|(token, file_level)| {
        if file_level {
            format!("// fallow-ignore-file {token}")
        } else {
            format!("// fallow-ignore-next-line {token}")
        }
    });
    serde_json::json!({
        "id": bare_id,
        "rule_id": rule.id,
        "command": command,
        "category": rule.category,
        "description": rule.short,
        "label": meta.label,
        "config_key": meta.config_key,
        "registry_index": meta.registry_index,
        "aliases": meta.aliases,
        "lsp": meta.lsp,
        "filter_flag": meta.filter_flag,
        "result_key": meta.result_key,
        "summary_label": meta.summary_label,
        "summary_docs_anchor": meta.summary_docs_anchor,
        "sarif_rule_ids": meta.sarif_rule_ids,
        "codeclimate_check_names": meta.codeclimate_check_names,
        "ts_alias": meta.ts_alias.map(|alias| serde_json::json!({
            "name": alias.name,
            "parent": alias.parent,
        })),
        "counts_in_total": meta.counts_in_total,
        "fixable": meta.fixable,
        "suppressible": meta.suppress.is_some(),
        "suppress_comment": suppress_comment,
        // Zero-config default severity for this rule (error/warn/off). Null only
        // for synthetic sub-rows that have no distinct IssueKind of their own.
        "default_severity": default_severity,
        // True when the rule defaults to off and detects nothing until enabled.
        // The single most load-bearing signal for avoiding a findings flood.
        "opt_in": opt_in,
        // Frameworks whose detector self-gates on the framework (empty for the
        // framework-agnostic majority). "detector self-gates on X", NOT
        // "commonly relevant to X"; see frameworks_for_kind.
        "frameworks": frameworks,
        "note": meta.note,
        "license": if meta.freemium { "freemium" } else { "free" },
        "license_note": meta.freemium.then_some(RUNTIME_COVERAGE_LICENSE_NOTE),
        "docs_url": rule_docs_url(rule),
    })
}

fn issue_type_meta(bare_id: &str, command: &str) -> IssueTypeMeta {
    let mut meta = IssueTypeMeta::from_shared(bare_id);
    match command {
        "dead-code" => apply_dead_code_issue_meta(bare_id, &mut meta),
        "health" => apply_health_issue_meta(bare_id, &mut meta),
        "security" => apply_security_issue_meta(bare_id, &mut meta),
        _ => apply_standalone_issue_meta(bare_id, &mut meta),
    }
    meta
}

fn apply_dead_code_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) {
    apply_source_issue_meta(bare_id, m);
    apply_dependency_issue_meta(bare_id, m);
    apply_architecture_issue_meta(bare_id, m);
    apply_catalog_issue_meta(bare_id, m);
}

fn apply_source_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) -> bool {
    match bare_id {
        "unused-export" | "unused-enum-member" => {
            m.fixable = true;
        }
        "private-type-leak" => {
            m.note = Some("Opt-in API hygiene check; the rule defaults to off");
        }
        "unused-file"
        | "unused-type"
        | "unresolved-import"
        | "missing-suppression-reason"
        | "unused-class-member"
        | "unused-store-member"
        | "unprovided-inject"
        | "unrendered-component"
        | "unused-component-prop"
        | "unused-component-emit"
        | "unused-component-input"
        | "unused-component-output"
        | "unused-svelte-event"
        | "unused-server-action"
        | "unused-load-data-key" => {}
        "prop-drilling" => {
            m.note = Some(
                "Opt-in: set rules.prop-drilling to warn or error to enable. Defaults to off.",
            );
        }
        "thin-wrapper" => {
            m.note =
                Some("Opt-in: set rules.thin-wrapper to warn or error to enable. Defaults to off.");
        }
        "duplicate-prop-shape" => {
            m.note = Some(
                "Opt-in: set rules.duplicate-prop-shape to warn or error to enable. Defaults to off.",
            );
        }
        "duplicate-export" => {
            m.note = Some(
                "fallow fix can add an ignoreExports rule to the fallow config instead of editing source",
            );
        }
        "stale-suppression" => {
            m.note = Some("Fix by removing the stale suppression marker itself");
        }
        _ => return false,
    }
    true
}

fn apply_dependency_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) -> bool {
    match bare_id {
        "unused-dependency" | "unused-dev-dependency" | "unused-optional-dependency" => {
            m.fixable = true;
            m.note = Some(
                "--unused-deps controls unused-dependency, unused-dev-dependency, unused-optional-dependency, type-only-dependency, and test-only-dependency",
            );
        }
        "type-only-dependency" => {
            m.note = Some(
                "Only reported in --production mode; --unused-deps scopes it together with the other dependency kinds",
            );
        }
        "test-only-dependency" => {
            m.note = Some(
                "Not reported in --production mode (test files are excluded there); --unused-deps scopes it together with the other dependency kinds",
            );
        }
        "unlisted-dependency" => {}
        _ => return false,
    }
    true
}

fn apply_architecture_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) -> bool {
    match bare_id {
        "circular-dependency" | "re-export-cycle" => {}
        "boundary-violation" => {
            m.note = Some("Requires configured boundary zones (boundaries config)");
        }
        "boundary-coverage" => {
            m.note = Some("Requires boundaries.coverage.requireAllFiles");
        }
        "boundary-call-violation" => {
            m.note = Some("Requires boundaries.calls.forbidden patterns");
        }
        "policy-violation" => {
            m.note = Some("Requires a configured rule pack (rulePacks config)");
        }
        "invalid-client-export" | "mixed-client-server-barrel" | "misplaced-directive" => {
            m.note = Some("Requires the project to declare next");
        }
        _ => return false,
    }
    true
}

fn apply_catalog_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) -> bool {
    match bare_id {
        "unused-catalog-entry" => {
            m.fixable = true;
        }
        "empty-catalog-group"
        | "unresolved-catalog-reference"
        | "unused-dependency-override"
        | "misconfigured-dependency-override" => {}
        _ => return false,
    }
    true
}

fn apply_health_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) {
    match bare_id {
        "high-cyclomatic-complexity"
        | "high-cognitive-complexity"
        | "high-complexity"
        | "high-crap-score" => {
            m.filter_flag = Some("--complexity");
            m.suppress = Some(("complexity", false));
        }
        "refactoring-target" => {
            m.filter_flag = Some("--targets");
        }
        "untested-file" | "untested-export" => {
            m.filter_flag = Some("--coverage-gaps");
            m.suppress = Some(("coverage-gaps", true));
        }
        "runtime-safe-to-delete"
        | "runtime-review-required"
        | "runtime-low-traffic"
        | "runtime-coverage-unavailable"
        | "runtime-coverage" => {
            m.freemium = true;
            m.note =
                Some("Requires --runtime-coverage input (V8 directory, V8 JSON, or Istanbul map)");
        }
        "coverage-intelligence-risky-change"
        | "coverage-intelligence-delete"
        | "coverage-intelligence-review"
        | "coverage-intelligence-refactor" => {
            m.freemium = true;
            m.note = Some("Produced by fallow coverage analyze");
        }
        _ => {}
    }
}

fn apply_standalone_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) {
    match bare_id {
        "code-duplication" => {
            m.suppress = Some(("code-duplication", false));
            m.note = Some("Reported by fallow dupes (and bare fallow / fallow audit)");
        }
        "feature-flag" => {
            m.suppress = Some(("feature-flag", false));
            m.note = Some("Reported by fallow flags");
        }
        _ => {}
    }
}

fn apply_security_issue_meta(bare_id: &str, m: &mut IssueTypeMeta) {
    match bare_id {
        "client-server-leak" => {
            m.suppress = Some(("security-client-server-leak", true));
        }
        "hardcoded-secret" => {
            m.suppress = Some(("security-sink", false));
            m.note = Some("Include-required category: enable via security.categories.include");
        }
        "tainted-sink" => {
            m.suppress = Some(("security-sink", false));
        }
        // Every other id is a tainted-sink catalogue category; ONE
        // suppression token (security-sink) covers them all.
        _ => {
            m.suppress = Some(("security-sink", false));
            m.note = Some(
                "Tainted-sink catalogue category; the security-sink suppression token covers every category",
            );
        }
    }
}

fn mcp_tools_schema() -> serde_json::Value {
    let tools: Vec<serde_json::Value> = MCP_TOOLS
        .iter()
        .map(|tool| {
            serde_json::json!({
                "name": tool.name,
                "kind": tool.kind,
                "description": tool.description,
                "cli_command": tool.cli_command,
                "key_params": tool.key_params,
                "license": tool.license.as_str(),
                "license_note": tool.license_note,
                "read_only": tool.read_only,
            })
        })
        .collect();
    serde_json::json!({
        "server": "fallow-mcp",
        "note": "key_params is a curated subset; the live MCP input schemas (list_tools) are authoritative for the full parameter list. cli_command is the nearest CLI fallback, not a full MCP input-schema projection",
        "tools": tools,
    })
}

fn plugins_schema() -> serde_json::Value {
    let names = fallow_engine::plugins::registry::builtin_plugin_names();
    serde_json::json!({
        "count": names.len(),
        "note": "Built-in framework plugins, auto-activated when their enabler dependency is present; run fallow list --plugins for the set active in a specific project",
        "names": names,
    })
}

/// User-facing environment variables, in display order. A plain pair slice
/// (not a `json!` literal) because the map outgrew `json!`'s macro recursion
/// limit; insertion order is preserved by `serde_json`'s `preserve_order`
/// feature.
const ENVIRONMENT_VARIABLES: &[(&str, &str)] = &[
    (
        "FALLOW_FORMAT",
        "Default output format (json/human/sarif/compact/markdown/codeclimate/gitlab-codequality/pr-comment-github/pr-comment-gitlab/review-github/review-gitlab/badge). CLI --format flag overrides this.",
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
        "Bot or token username treated as fallow's own when reconciling existing PR/MR comments in review-github/review-gitlab. Required when posting with a personal access token (the author then carries a human identity).",
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
        "FALLOW_EXTENDS_TIMEOUT_SECS",
        "Timeout in seconds for fetching https:// configs referenced via the extends field (default 5).",
    ),
    (
        "FALLOW_COVERAGE",
        "Path to Istanbul coverage data (coverage-final.json) for accurate per-function CRAP scores. CLI --coverage flag overrides this.",
    ),
    (
        "FALLOW_MAX_FILE_SIZE",
        "Per-file size ceiling in megabytes for source discovery (default 5; 0 = no limit). CLI --max-file-size flag overrides this.",
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
        "License JWT (full string) for the paid runtime intelligence layer; intended for shared CI runners.",
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
        "Base URL override for fallow cloud API calls (license refresh, trial, coverage uploads).",
    ),
    (
        "FALLOW_API_KEY",
        "fallow cloud bearer token for coverage upload commands.",
    ),
    (
        "FALLOW_CA_BUNDLE",
        "Path to a PEM certificate bundle for fallow cloud and provider HTTP calls (replaces the default WebPKI roots).",
    ),
    (
        "FALLOW_UPDATE_CHECK",
        "Set to off/0/false to disable the human-TTY upgrade nudge and its background version check.",
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
        "FALLOW_INTEGRATION_SURFACE",
        "Telemetry integration_surface override for non-CLI surfaces (mcp/lsp/vscode/napi/programmatic). Set by the MCP server on the CLI it spawns.",
    ),
    (
        "FALLOW_MCP_TOOL",
        "Telemetry mcp_tool dimension, validated against the MCP tool-name allowlist. Set by the MCP server alongside FALLOW_INTEGRATION_SURFACE=mcp.",
    ),
];

fn environment_variables_schema() -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = ENVIRONMENT_VARIABLES
        .iter()
        .map(|(name, description)| ((*name).to_string(), serde_json::Value::from(*description)))
        .collect();
    serde_json::Value::Object(map)
}

fn build_arg_schema(arg: &clap::Arg) -> serde_json::Value {
    let name = arg
        .get_long()
        .map_or_else(|| arg.get_id().to_string(), |l| format!("--{l}"));

    let arg_type = match arg.get_action() {
        clap::ArgAction::SetTrue | clap::ArgAction::SetFalse => "bool",
        clap::ArgAction::Count => "count",
        _ => "string",
    };

    let possible: Vec<String> = arg
        .get_possible_values()
        .iter()
        .map(|v| v.get_name().to_string())
        .collect();

    let mut schema = serde_json::json!({
        "name": name,
        "type": arg_type,
        "required": arg.is_required_set(),
        "description": arg.get_help().map(std::string::ToString::to_string),
    });

    if let Some(short) = arg.get_short() {
        schema["short"] = serde_json::json!(format!("-{short}"));
    }

    if let Some(default) = arg.get_default_values().first() {
        schema["default"] = serde_json::json!(default.to_str());
    }

    if !possible.is_empty() {
        schema["possible_values"] = serde_json::json!(possible);
    }

    schema
}

#[cfg(test)]
mod tests {
    use fallow_types::results::TOTAL_ISSUE_RESULT_KEYS;
    use fallow_types::suppress::{DEAD_CODE_FILTER_FLAGS, IssueKind, KNOWN_ISSUE_KIND_NAMES};
    use rustc_hash::FxHashSet;

    use super::*;

    fn schema() -> serde_json::Value {
        let cmd = Cli::command();
        build_cli_schema(&cmd)
    }

    /// Collect every `--long` flag of a subcommand from live clap state.
    fn subcommand_flags(name: &str) -> FxHashSet<String> {
        let cmd = Cli::command();
        let sub = cmd
            .get_subcommands()
            .find(|s| s.get_name() == name)
            .unwrap_or_else(|| panic!("no subcommand named {name}"));
        sub.get_arguments()
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .collect()
    }

    #[test]
    fn related_schemas_points_at_plugin_authoring() {
        let schema = schema();
        let related = &schema["related_schemas"];
        assert_eq!(related["plugin_schema_command"], "fallow plugin-schema");
        assert_eq!(related["plugin_check_command"], "fallow plugin-check");
        assert!(
            related["plugin_schema_note"].is_string() && related["plugin_check_note"].is_string(),
            "plugin schema/check pointers must carry an agent-facing note"
        );
    }

    #[test]
    fn schema_includes_environment_variables() {
        let schema = schema();
        let env_vars = &schema["environment_variables"];
        assert!(env_vars["FALLOW_FORMAT"].is_string());
        assert!(env_vars["FALLOW_QUIET"].is_string());
        assert!(env_vars["FALLOW_CACHE_DIR"].is_string());
        assert!(env_vars["FALLOW_BIN"].is_string());
        assert!(env_vars["FALLOW_CACHE_MAX_SIZE"].is_string());
        assert!(env_vars["FALLOW_TELEMETRY"].is_string());
        assert!(env_vars["FALLOW_AUDIT_BASE"].is_string());
        assert!(env_vars["FALLOW_IMPACT_STORE_MAX_AGE_DAYS"].is_string());
        assert!(env_vars["FALLOW_TIMEOUT_SECS"].is_string());
        assert!(env_vars["FALLOW_SUGGESTIONS"].is_string());
        assert!(env_vars["DO_NOT_TRACK"].is_string());
    }

    /// Internal plumbing vars must NOT leak into the agent-facing manifest.
    /// Each excluded var carries the reason it stays internal.
    #[test]
    fn environment_variables_exclude_internal_plumbing() {
        const EXCLUDED: &[(&str, &str)] = &[
            ("FALLOW_TEST_SIGNAL_HELPER", "test harness only"),
            ("FALLOW_STUB_MODE", "test harness only"),
            (
                "FALLOW_RAYON_STACK_PROBE_CHILD",
                "internal child-process marker",
            ),
            (
                "FALLOW_PROGRAMMATIC_SHARED_DIFF_CHILD",
                "internal child-process marker",
            ),
            (
                "FALLOW_GITLAB_BASE_SHA",
                "set by the bundled GitLab CI template, not user-configured",
            ),
            (
                "FALLOW_GITLAB_START_SHA",
                "set by the bundled GitLab CI template, not user-configured",
            ),
            (
                "FALLOW_GITLAB_HEAD_SHA",
                "set by the bundled GitLab CI template, not user-configured",
            ),
            (
                "FALLOW_COMMENT_ID",
                "set by the bundled Action/CI scripts, not user-configured",
            ),
            (
                "FALLOW_MAX_COMMENTS",
                "set by the bundled Action/CI scripts, not user-configured",
            ),
            (
                "FALLOW_DIFF_FILTER",
                "set by the bundled Action/CI scripts, not user-configured",
            ),
        ];
        let schema = schema();
        let env_vars = env_var_map(&schema);
        for (var, reason) in EXCLUDED {
            assert!(
                !env_vars.contains_key(*var),
                "{var} is internal plumbing ({reason}) and must not be documented in the manifest"
            );
        }
    }

    fn env_var_map(schema: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        schema["environment_variables"].as_object().unwrap().clone()
    }

    #[test]
    fn schema_exit_code_2_mentions_json_errors() {
        let schema = schema();
        let exit_2 = schema["exit_codes"]["2"].as_str().unwrap();
        assert!(exit_2.contains("JSON"));
    }

    #[test]
    fn schema_has_name_and_version() {
        let schema = schema();
        assert_eq!(schema["name"], "fallow");
        assert!(schema["version"].is_string());
        assert_eq!(schema["manifest_version"], "1");
    }

    #[test]
    fn schema_has_commands_array() {
        let schema = schema();
        let commands = schema["commands"].as_array().unwrap();
        assert!(!commands.is_empty());
        assert!(
            !commands
                .iter()
                .any(|c| c["name"].as_str().unwrap() == "help")
        );
    }

    #[test]
    fn schema_has_global_flags() {
        let schema = schema();
        let flags = schema["global_flags"].as_array().unwrap();
        assert!(!flags.iter().any(|f| f["name"].as_str().unwrap() == "help"));
        assert!(
            !flags
                .iter()
                .any(|f| f["name"].as_str().unwrap() == "version")
        );
    }

    #[test]
    fn schema_has_issue_types() {
        let schema = schema();
        let issue_types = schema["issue_types"].as_array().unwrap();
        assert!(!issue_types.is_empty());
        for issue_type in issue_types {
            assert!(issue_type["id"].is_string());
            assert!(issue_type["description"].is_string());
        }
    }

    /// Row-source completeness: every rule in every explain slice gets
    /// exactly one issue_types row, so the manifest cannot drift behind
    /// the rule registry again.
    #[test]
    fn issue_types_cover_every_explain_rule() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();
        let expected = CHECK_RULES.len()
            + HEALTH_RULES.len()
            + DUPES_RULES.len()
            + FLAGS_RULES.len()
            + SECURITY_RULES.len();
        assert_eq!(rows.len(), expected, "one issue_types row per explain rule");

        let row_rule_ids: FxHashSet<&str> = rows
            .iter()
            .map(|r| r["rule_id"].as_str().unwrap())
            .collect();
        assert_eq!(
            row_rule_ids.len(),
            rows.len(),
            "duplicate rule_id in issue_types"
        );
        for rule in CHECK_RULES
            .iter()
            .chain(HEALTH_RULES)
            .chain(DUPES_RULES)
            .chain(FLAGS_RULES)
            .chain(SECURITY_RULES)
        {
            assert!(
                row_rule_ids.contains(rule.id),
                "explain rule {} has no issue_types row",
                rule.id
            );
        }
    }

    /// Subset cross-check (NOT a bijection): every suppressible/filterable
    /// `IssueKind` must be represented by at least one row, either via its
    /// own id or via a suppression-comment token. Complexity is one kind
    /// covered by several rule rows; that is expected.
    #[test]
    fn every_issue_kind_is_covered_by_a_row() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();

        let mut covered: FxHashSet<u8> = FxHashSet::default();
        for row in rows {
            if let Some(kind) = IssueKind::parse(row["id"].as_str().unwrap()) {
                covered.insert(kind.to_discriminant());
            }
            if let Some(comment) = row["suppress_comment"].as_str() {
                let token = comment.split_whitespace().last().unwrap();
                if let Some(kind) = IssueKind::parse(token) {
                    covered.insert(kind.to_discriminant());
                }
            }
        }

        for name in KNOWN_ISSUE_KIND_NAMES.iter() {
            let kind = IssueKind::parse(name).unwrap();
            assert!(
                covered.contains(&kind.to_discriminant()),
                "IssueKind for token '{name}' has no issue_types row (neither id nor suppress token)"
            );
        }
    }

    /// The highest-value guard: every emitted suppress_comment must carry a
    /// token `IssueKind::parse` accepts, otherwise agents copy a silent
    /// no-op suppression.
    #[test]
    fn suppress_comments_round_trip_through_issue_kind_parse() {
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            let suppressible = row["suppressible"].as_bool().unwrap();
            let comment = &row["suppress_comment"];
            assert_eq!(
                comment.is_string(),
                suppressible,
                "suppress_comment must be a string iff suppressible ({})",
                row["id"]
            );
            if let Some(comment) = comment.as_str() {
                assert!(
                    comment.starts_with("// fallow-ignore-next-line ")
                        || comment.starts_with("// fallow-ignore-file "),
                    "unexpected suppress_comment shape: {comment}"
                );
                let token = comment.split_whitespace().last().unwrap();
                assert!(
                    IssueKind::parse(token).is_some(),
                    "suppress_comment token '{token}' on row {} does not parse; agents would copy a no-op suppression",
                    row["id"]
                );
            }
        }
    }

    #[test]
    fn dead_code_schema_issue_contracts_follow_issue_kind_meta() {
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            if row["command"].as_str().unwrap() != "dead-code" {
                continue;
            }

            let id = row["id"].as_str().unwrap();
            let shared = issue_meta_by_code(id)
                .unwrap_or_else(|| panic!("dead-code row {id} has no IssueKindMeta"));

            assert_eq!(
                row["filter_flag"].as_str(),
                shared.filter_flag,
                "dead-code row {id} must derive filter_flag from IssueKindMeta"
            );

            let expected_comment = shared.suppress_token.map(|token| {
                if shared.suppress_file_level {
                    format!("// fallow-ignore-file {token}")
                } else {
                    format!("// fallow-ignore-next-line {token}")
                }
            });
            assert_eq!(
                row["suppressible"].as_bool().unwrap(),
                expected_comment.is_some(),
                "dead-code row {id} must derive suppressible from IssueKindMeta"
            );
            assert_eq!(
                row["suppress_comment"].as_str(),
                expected_comment.as_deref(),
                "dead-code row {id} must derive suppress_comment from IssueKindMeta"
            );
        }
    }

    #[test]
    fn issue_type_registry_fields_follow_issue_kind_meta() {
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            let id = row["id"].as_str().unwrap();
            let Some(shared) = issue_meta_by_code(id) else {
                assert!(
                    row["label"].is_null(),
                    "unregistered row {id} must not invent a label"
                );
                assert!(
                    row["config_key"].is_null(),
                    "unregistered row {id} must not invent a config key"
                );
                assert!(
                    row["registry_index"].is_null(),
                    "unregistered row {id} must not invent a registry index"
                );
                assert_eq!(
                    row["aliases"].as_array().map(Vec::len),
                    Some(0),
                    "unregistered row {id} must not invent aliases"
                );
                assert_eq!(
                    row["lsp"].as_bool(),
                    Some(false),
                    "unregistered row {id} must not be exposed as an LSP issue type"
                );
                continue;
            };

            assert_eq!(
                row["label"].as_str(),
                Some(shared.label),
                "row {id} must derive label from IssueKindMeta"
            );
            assert_eq!(
                row["config_key"].as_str(),
                shared.config_key,
                "row {id} must derive config_key from IssueKindMeta"
            );
            assert!(
                row["registry_index"].as_u64().is_some(),
                "row {id} must derive registry_index from IssueKindMeta"
            );
            let aliases: Vec<&str> = row["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|alias| alias.as_str().unwrap())
                .collect();
            assert_eq!(
                aliases, shared.aliases,
                "row {id} must derive aliases from IssueKindMeta"
            );
            assert_eq!(
                row["lsp"].as_bool(),
                Some(shared.lsp),
                "row {id} must derive lsp from IssueKindMeta"
            );
        }
    }

    /// `default_severity` / `opt_in` are derived from the rule that GATES a
    /// finding via the SINGLE serialized `RulesConfig::default()` (no second copy
    /// of the default table): the row's own `config_key` for a 1:1 rule, else the
    /// rule named by its suppression token for a shared-gate finding (every
    /// tainted-sink category is gated by `security-sink`, coverage findings by
    /// `coverage-gaps`). Null only for findings with no `rules.*` gate at all
    /// (complexity, duplication metrics).
    #[test]
    fn issue_type_default_severity_and_opt_in_track_the_gating_rule() {
        let defaults = serde_json::to_value(fallow_config::RulesConfig::default()).unwrap();
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            let id = row["id"].as_str().unwrap();
            // The gating rule: own config_key, else the suppression token's rule.
            let suppress_rule = row["suppress_comment"]
                .as_str()
                .and_then(|comment| comment.rsplit(' ').next());
            let severity_key = row["config_key"].as_str().or(suppress_rule);
            let expected = severity_key
                .and_then(|key| defaults.get(key))
                .and_then(serde_json::Value::as_str);
            assert_eq!(
                row["default_severity"].as_str(),
                expected,
                "row {id} default_severity must equal the loader default for its gating rule ({severity_key:?})"
            );
            match row["default_severity"].as_str() {
                Some(sev) => {
                    assert!(
                        ["error", "warn", "off"].contains(&sev),
                        "row {id} default_severity {sev} is not a severity level"
                    );
                    assert_eq!(
                        row["opt_in"].as_bool(),
                        Some(sev == "off"),
                        "row {id} opt_in must mean default severity off"
                    );
                }
                None => assert!(
                    row["opt_in"].is_null(),
                    "row {id} with no gating rule must carry null opt_in"
                ),
            }
            assert!(
                row["frameworks"].is_array(),
                "row {id} frameworks must always be an array"
            );
        }
    }

    /// The opt-in command families (security, coverage) now expose opt_in=true
    /// on their gated findings, so an agent scanning the manifest sees they are
    /// off by default like any other opt-in rule.
    #[test]
    fn opt_in_command_findings_expose_opt_in() {
        let schema = schema();
        let opt_in = |id: &str| {
            schema["issue_types"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["id"] == id)
                .and_then(|row| row["opt_in"].as_bool())
        };
        for id in ["tainted-sink", "client-server-leak", "hardcoded-secret"] {
            assert_eq!(
                opt_in(id),
                Some(true),
                "security finding {id} must read as opt-in (off by default)"
            );
        }
    }

    /// The known opt-in rules (default off) are exactly the rows flagged
    /// `opt_in: true`, so an agent can trust the manifest to avoid enabling a
    /// findings-flooding rule blindly.
    #[test]
    fn opt_in_rows_are_the_off_default_rules() {
        let schema = schema();
        let mut opt_in_ids: Vec<&str> = schema["issue_types"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["opt_in"].as_bool() == Some(true))
            .map(|row| row["id"].as_str().unwrap())
            .collect();
        opt_in_ids.sort_unstable();
        opt_in_ids.dedup();
        for expected in [
            "private-type-leak",
            "prop-drilling",
            "thin-wrapper",
            "duplicate-prop-shape",
            "feature-flag",
        ] {
            assert!(
                opt_in_ids.contains(&expected),
                "expected opt-in rule {expected} to be flagged opt_in in the manifest, got {opt_in_ids:?}"
            );
        }
    }

    /// The only rules whose detector self-gates on a framework carry a
    /// non-empty `frameworks`; the framework-agnostic majority stays empty so
    /// an agent never disables a real safeguard.
    #[test]
    fn only_self_gating_rules_carry_frameworks() {
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            let id = row["id"].as_str().unwrap();
            let frameworks: Vec<&str> = row["frameworks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f.as_str().unwrap())
                .collect();
            if frameworks.is_empty() {
                continue;
            }
            assert!(
                matches!(
                    id,
                    "route-collision" | "dynamic-segment-name-conflict" | "invalid-client-export"
                ),
                "row {id} carries frameworks {frameworks:?} but its detector is not known to self-gate"
            );
            assert_eq!(frameworks, ["next"], "row {id} unexpected frameworks");
        }
    }

    /// The manifest carries the security-category vocabulary an agent needs to
    /// author `security.categories.include` / `exclude`, since the config-schema
    /// only describes the mechanism, not the valid ids.
    #[test]
    fn schema_has_security_categories_with_include_required_flags() {
        let schema = schema();
        let cats = schema["security_categories"]["categories"]
            .as_array()
            .expect("security_categories.categories is an array");
        assert!(!cats.is_empty(), "security categories must be listed");
        for c in cats {
            assert!(c["id"].as_str().is_some(), "each category has an id");
            assert!(c["include_required"].as_bool().is_some());
        }
        let flagged: Vec<&str> = cats
            .iter()
            .filter(|c| c["include_required"].as_bool() == Some(true))
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert!(flagged.contains(&"hardcoded-secret"));
        assert!(flagged.contains(&"secret-to-network"));
    }

    /// Nullable fields are ALWAYS present (null when not applicable), so
    /// consumers never face absent-vs-null ambiguity.
    #[test]
    fn issue_type_nullable_fields_are_always_present() {
        let schema = schema();
        for row in schema["issue_types"].as_array().unwrap() {
            let obj = row.as_object().unwrap();
            for key in [
                "filter_flag",
                "result_key",
                "summary_label",
                "summary_docs_anchor",
                "sarif_rule_ids",
                "codeclimate_check_names",
                "ts_alias",
                "suppress_comment",
                "default_severity",
                "opt_in",
                "frameworks",
                "note",
                "license_note",
                "rule_id",
                "command",
                "category",
                "label",
                "config_key",
                "registry_index",
                "aliases",
                "lsp",
                "counts_in_total",
                "fixable",
                "suppressible",
                "license",
                "docs_url",
            ] {
                assert!(
                    obj.contains_key(key),
                    "row {} is missing key {key}",
                    row["id"]
                );
            }
        }
    }

    #[test]
    fn dead_code_result_keys_match_total_issue_contract() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();

        let expected: FxHashSet<&str> = TOTAL_ISSUE_RESULT_KEYS.iter().copied().collect();
        let mut counted = FxHashSet::default();
        let mut advisory = FxHashSet::default();

        for row in rows {
            if row["command"].as_str() != Some("dead-code") {
                assert!(
                    row["result_key"].is_null(),
                    "non dead-code row {} must not expose a dead-code result_key",
                    row["id"]
                );
                assert!(
                    row["summary_label"].is_null(),
                    "non dead-code row {} must not expose a dead-code summary_label",
                    row["id"]
                );
                assert!(
                    row["summary_docs_anchor"].is_null(),
                    "non dead-code row {} must not expose a dead-code summary_docs_anchor",
                    row["id"]
                );
                assert!(
                    row["sarif_rule_ids"].is_null(),
                    "non dead-code row {} must not expose dead-code sarif_rule_ids",
                    row["id"]
                );
                assert!(
                    row["codeclimate_check_names"].is_null(),
                    "non dead-code row {} must not expose dead-code codeclimate_check_names",
                    row["id"]
                );
                assert!(
                    row["ts_alias"].is_null(),
                    "non dead-code row {} must not expose a dead-code ts_alias",
                    row["id"]
                );
                assert_eq!(
                    row["counts_in_total"].as_bool(),
                    Some(false),
                    "non dead-code row {} must not count in total_issues",
                    row["id"]
                );
                continue;
            }

            let counts = row["counts_in_total"].as_bool().unwrap();
            if let Some(result_key) = row["result_key"].as_str() {
                assert!(
                    row["summary_label"]
                        .as_str()
                        .is_some_and(|label| !label.is_empty()),
                    "dead-code row {} has result_key {result_key} but no summary_label",
                    row["id"]
                );
                assert!(
                    row["summary_docs_anchor"]
                        .as_str()
                        .is_some_and(|anchor| !anchor.is_empty()),
                    "dead-code row {} has result_key {result_key} but no summary_docs_anchor",
                    row["id"]
                );
                let sarif_rule_ids: FxHashSet<&str> = row["sarif_rule_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert!(
                    sarif_rule_ids.contains(row["rule_id"].as_str().unwrap()),
                    "dead-code row {} must include the rule id in SARIF rule ids",
                    row["id"]
                );
                if row["id"].as_str() == Some("stale-suppression") {
                    assert!(
                        sarif_rule_ids.contains("fallow/missing-suppression-reason"),
                        "stale-suppression must expose its missing-reason SARIF variant"
                    );
                }
                if counts {
                    counted.insert(result_key);
                } else {
                    advisory.insert(result_key);
                }
            } else {
                assert!(
                    !counts,
                    "dead-code row {} counts in total_issues but has no result_key",
                    row["id"]
                );
            }
        }

        assert_eq!(expected, counted);
        assert_eq!(
            FxHashSet::from_iter([
                "duplicate_prop_shapes",
                "prop_drilling_chains",
                "thin_wrappers",
            ]),
            advisory
        );
    }

    #[test]
    fn dead_code_codeclimate_contract_is_explicit() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();

        let missing: FxHashSet<&str> = rows
            .iter()
            .filter(|row| row["command"].as_str() == Some("dead-code"))
            .filter(|row| row["result_key"].as_str().is_some())
            .filter(|row| row["codeclimate_check_names"].is_null())
            .map(|row| row["id"].as_str().unwrap())
            .collect();

        assert_eq!(
            FxHashSet::from_iter(["duplicate-prop-shape", "prop-drilling", "thin-wrapper"]),
            missing
        );

        for row in rows {
            if row["command"].as_str() == Some("dead-code")
                && row["codeclimate_check_names"].as_array().is_some()
            {
                let codeclimate_check_names: FxHashSet<&str> = row["codeclimate_check_names"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_str().unwrap())
                    .collect();
                assert!(
                    codeclimate_check_names.contains(row["rule_id"].as_str().unwrap()),
                    "dead-code row {} must include the rule id in CodeClimate check names",
                    row["id"]
                );
                if row["id"].as_str() == Some("stale-suppression") {
                    assert!(
                        codeclimate_check_names.contains("fallow/missing-suppression-reason"),
                        "stale-suppression must expose its missing-reason CodeClimate variant"
                    );
                }
            }
        }
    }

    #[test]
    fn dead_code_ts_alias_contract_is_explicit() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();
        let aliases: FxHashSet<(String, String, String)> = rows
            .iter()
            .filter(|row| row["command"].as_str() == Some("dead-code"))
            .filter_map(|row| {
                let alias = row["ts_alias"].as_object()?;
                Some((
                    row["id"].as_str().unwrap().to_string(),
                    alias["name"].as_str().unwrap().to_string(),
                    alias["parent"].as_str().unwrap().to_string(),
                ))
            })
            .collect();
        let expected: FxHashSet<(String, String, String)> = issue_output_contracts()
            .filter_map(|contract| {
                let alias = contract.ts_alias?;
                Some((
                    contract.code.to_string(),
                    alias.name.to_string(),
                    alias.parent.to_string(),
                ))
            })
            .collect();

        assert!(aliases.contains(&(
            "unused-dependency".to_string(),
            "UnusedDependency".to_string(),
            "UnusedDependencyFinding".to_string()
        )));
        assert!(aliases.contains(&(
            "unused-dev-dependency".to_string(),
            "UnusedDependency".to_string(),
            "UnusedDevDependencyFinding".to_string()
        )));
        assert!(aliases.contains(&(
            "unused-optional-dependency".to_string(),
            "UnusedDependency".to_string(),
            "UnusedOptionalDependencyFinding".to_string()
        )));
        assert!(aliases.contains(&(
            "unused-class-member".to_string(),
            "UnusedMember".to_string(),
            "UnusedClassMemberFinding".to_string()
        )));
        assert!(aliases.contains(&(
            "unused-enum-member".to_string(),
            "UnusedMember".to_string(),
            "UnusedEnumMemberFinding".to_string()
        )));
        assert!(aliases.contains(&(
            "unused-store-member".to_string(),
            "UnusedMember".to_string(),
            "UnusedStoreMemberFinding".to_string()
        )));
        assert_eq!(
            expected, aliases,
            "schema ts_alias rows must exactly mirror fallow-output contracts"
        );

        for (_, name, parent) in aliases {
            assert!(
                name.chars().next().is_some_and(char::is_uppercase),
                "TS alias name must be PascalCase: {name}"
            );
            assert!(
                parent.ends_with("Finding"),
                "TS alias parent must target a finding wrapper: {parent}"
            );
        }
    }

    /// Filter flags in the manifest must exist on the live clap command,
    /// and the shared dead-code filter-flag list must be fully represented.
    #[test]
    fn filter_flags_exist_on_live_clap_commands() {
        let schema = schema();
        let rows = schema["issue_types"].as_array().unwrap();
        let dead_code_flags = subcommand_flags("dead-code");
        let health_flags = subcommand_flags("health");

        let mut seen_dead_code_filters: FxHashSet<&str> = FxHashSet::default();
        for row in rows {
            let Some(flag) = row["filter_flag"].as_str() else {
                continue;
            };
            match row["command"].as_str().unwrap() {
                "dead-code" => {
                    assert!(
                        DEAD_CODE_FILTER_FLAGS.contains(&flag),
                        "row {} filter_flag {flag} is not in the shared DEAD_CODE_FILTER_FLAGS list",
                        row["id"]
                    );
                    assert!(
                        dead_code_flags.contains(flag),
                        "row {} filter_flag {flag} does not exist on the dead-code subcommand",
                        row["id"]
                    );
                    seen_dead_code_filters
                        .insert(DEAD_CODE_FILTER_FLAGS.iter().find(|f| **f == flag).unwrap());
                }
                "health" => {
                    assert!(
                        health_flags.contains(flag),
                        "row {} filter_flag {flag} does not exist on the health subcommand",
                        row["id"]
                    );
                }
                other => panic!("unexpected filter_flag on command {other}"),
            }
        }
        for flag in DEAD_CODE_FILTER_FLAGS.iter() {
            assert!(
                seen_dead_code_filters.contains(flag),
                "shared filter flag {flag} is not represented by any issue_types row"
            );
        }
    }

    #[test]
    fn mcp_tools_block_lists_every_manifest_tool() {
        let schema = schema();
        let block = &schema["mcp_tools"];
        assert_eq!(block["server"], "fallow-mcp");
        let tools = block["tools"].as_array().unwrap();
        assert_eq!(tools.len(), MCP_TOOLS.len());
        for tool in tools {
            let obj = tool.as_object().unwrap();
            for key in [
                "name",
                "kind",
                "description",
                "cli_command",
                "key_params",
                "license",
                "license_note",
                "read_only",
            ] {
                assert!(
                    obj.contains_key(key),
                    "mcp tool {} missing key {key}",
                    tool["name"]
                );
            }
            if tool["license"] == "freemium" {
                assert!(
                    tool["license_note"].is_string(),
                    "freemium tool {} must carry a license_note",
                    tool["name"]
                );
            }
        }
        let code_execute = tools
            .iter()
            .find(|t| t["name"] == "code_execute")
            .expect("code_execute in mcp_tools");
        assert_eq!(code_execute["kind"], "composition");
        assert!(code_execute["cli_command"].is_null());
        let analyze = tools
            .iter()
            .find(|t| t["name"] == "analyze")
            .expect("analyze in mcp_tools");
        assert_eq!(
            analyze["cli_command"],
            "fallow dead-code --format json --quiet"
        );
    }

    #[test]
    fn plugins_block_reflects_live_registry() {
        let schema = schema();
        let block = &schema["plugins"];
        let names = block["names"].as_array().unwrap();
        let count = usize::try_from(block["count"].as_u64().unwrap()).unwrap();
        assert_eq!(names.len(), count);
        assert_eq!(
            count,
            fallow_engine::plugins::registry::builtin_plugin_names().len()
        );
        assert!(count >= 110, "plugin registry shrank unexpectedly");
    }

    #[test]
    fn schema_output_formats_include_all_formats() {
        let schema = schema();
        let formats = schema["output_formats"].as_array().unwrap();
        for expected in [
            "human",
            "json",
            "sarif",
            "compact",
            "markdown",
            "codeclimate",
            "gitlab-codequality",
            "pr-comment-github",
            "pr-comment-gitlab",
            "review-github",
            "review-gitlab",
            "badge",
        ] {
            assert!(
                formats.iter().any(|f| f.as_str().unwrap() == expected),
                "missing format: {expected}"
            );
        }
    }

    #[test]
    fn schema_severity_levels() {
        let schema = schema();
        let levels = schema["severity_levels"].as_array().unwrap();
        for expected in ["error", "warn", "off"] {
            assert!(
                levels.iter().any(|l| l.as_str().unwrap() == expected),
                "missing severity level: {expected}"
            );
        }
    }

    #[test]
    fn build_arg_schema_bool_type() {
        let cmd = Cli::command();
        let quiet_arg = cmd.get_arguments().find(|a| a.get_id() == "quiet").unwrap();
        let schema = build_arg_schema(quiet_arg);
        assert_eq!(schema["type"], "bool");
    }

    #[test]
    fn build_arg_schema_includes_short_flag() {
        let cmd = Cli::command();
        let quiet_arg = cmd.get_arguments().find(|a| a.get_id() == "quiet").unwrap();
        let schema = build_arg_schema(quiet_arg);
        if quiet_arg.get_short().is_some() {
            assert!(schema["short"].is_string());
        }
    }

    /// Every long flag (`--name`) declared as a global argument on the root.
    fn global_flag_longs() -> FxHashSet<String> {
        Cli::command()
            .get_arguments()
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .collect()
    }

    #[test]
    fn schema_has_task_matrix() {
        let schema = schema();
        let rows = schema["task_matrix"].as_array().unwrap();
        assert!(!rows.is_empty(), "task_matrix must have at least one row");
        for row in rows {
            let obj = row.as_object().unwrap();
            for key in ["task", "command", "note"] {
                assert!(obj.contains_key(key), "task_matrix row missing key {key}");
            }
            assert!(obj["task"].is_string());
            assert!(obj["command"].is_string());
        }
    }

    /// The highest-value guard: every row with a runnable `probe` must parse
    /// through the live clap command tree, so a row can never name a flag or
    /// subcommand that does not exist.
    #[test]
    fn task_matrix_commands_parse_through_clap() {
        use clap::Parser;
        for row in crate::task_matrix::TASK_MATRIX {
            if row.probe.is_empty() {
                continue;
            }
            let argv = std::iter::once("fallow").chain(row.probe.iter().copied());
            Cli::try_parse_from(argv).unwrap_or_else(|e| {
                panic!(
                    "task matrix probe {:?} for command '{}' does not parse: {e}",
                    row.probe, row.command
                )
            });
        }
    }

    /// Read-only-evidence contract (R1): no matrix command may name a mutating
    /// command (`fix`/`init`/`hooks`/`migrate`/`setup-hooks`/`watch`), mirroring
    /// the `next_steps[]` exclusion in `report/suggestions.rs`.
    #[test]
    fn task_matrix_excludes_mutating_commands() {
        for row in crate::task_matrix::TASK_MATRIX {
            let after_fallow = row.command.strip_prefix("fallow ").unwrap_or(row.command);
            let first_token = after_fallow.split_whitespace().next().unwrap_or("");
            assert!(
                !crate::task_matrix::MUTATING_COMMANDS.contains(&first_token),
                "task matrix command '{}' names mutating token '{first_token}'",
                row.command
            );
        }
    }

    /// The flag-fragment "scope a monorepo" row carries an empty probe, so the
    /// parse test skips it; assert its global flags exist on the live root
    /// command instead so the row can never reference a phantom flag.
    #[test]
    fn task_matrix_workspace_flags_are_global() {
        let longs = global_flag_longs();
        for flag in ["--workspace", "--changed-workspaces"] {
            assert!(
                longs.contains(flag),
                "{flag} is not a global flag on the root command"
            );
        }
    }
}
