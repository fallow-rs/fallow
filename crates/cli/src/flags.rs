//! `fallow flags` subcommand: detect and report feature flag patterns.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_types::extract::{FlagUse, FlagUseKind, ModuleInfo};
use fallow_types::results::{FeatureFlag, FlagConfidence, FlagKind};

use crate::error::emit_error;

/// Convert an extraction-level `FlagUse` to a result-level `FeatureFlag`.
fn flag_use_to_feature_flag(
    flag_use: &FlagUse,
    module: &ModuleInfo,
    path: &std::path::Path,
) -> FeatureFlag {
    let (kind, confidence) = match flag_use.kind {
        FlagUseKind::EnvVar => (FlagKind::EnvironmentVariable, FlagConfidence::High),
        FlagUseKind::SdkCall => (FlagKind::SdkCall, FlagConfidence::High),
        FlagUseKind::ConfigObject => (FlagKind::ConfigObject, FlagConfidence::Low),
    };

    let (guard_line_start, guard_line_end) = if let (Some(start), Some(end)) =
        (flag_use.guard_span_start, flag_use.guard_span_end)
        && !module.line_offsets.is_empty()
    {
        let (sl, _) = fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, start);
        let (el, _) = fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, end);
        (Some(sl), Some(el))
    } else {
        (None, None)
    };

    FeatureFlag {
        path: path.to_path_buf(),
        flag_name: flag_use.flag_name.clone(),
        kind,
        confidence,
        line: flag_use.line,
        col: flag_use.col,
        guard_span_start: flag_use.guard_span_start,
        guard_span_end: flag_use.guard_span_end,
        sdk_name: flag_use.sdk_name.clone(),
        guard_line_start,
        guard_line_end,
        guarded_dead_exports: Vec::new(),
    }
}

/// Options for the `fallow flags` subcommand.
pub struct FlagsOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub production: bool,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub changed_since: Option<&'a str>,
    pub explain: bool,
    pub top: Option<usize>,
}

/// Run the `fallow flags` subcommand.
pub fn run_flags(opts: &FlagsOptions<'_>) -> ExitCode {
    let start = Instant::now();

    let config = match crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production,
        opts.quiet,
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    // Discover files
    let files = fallow_core::discover::discover_files_with_plugin_scopes(&config);
    if files.is_empty() {
        return emit_error("no files discovered", 2, opts.output);
    }

    // Parse all files (flag extraction happens automatically during parse)
    let cache_store = if config.no_cache {
        None
    } else {
        fallow_core::cache::CacheStore::load(&config.cache_dir)
    };
    let parse_result = fallow_core::extract::parse_all_files(&files, cache_store.as_ref(), false);

    // Build file_id -> path lookup from discovered files
    let file_paths: rustc_hash::FxHashMap<_, _> = files.iter().map(|f| (f.id, &f.path)).collect();

    // Prepare user-configured flag patterns for supplementary extraction
    let extra_sdk: Vec<(String, usize, String)> = config
        .flags
        .sdk_patterns
        .iter()
        .map(|p| {
            (
                p.function.clone(),
                p.name_arg,
                p.provider.clone().unwrap_or_default(),
            )
        })
        .collect();
    let has_custom_config = !extra_sdk.is_empty()
        || !config.flags.env_prefixes.is_empty()
        || config.flags.config_object_heuristics;

    // Collect feature flags from parsed modules (built-in patterns from cache/parse)
    let mut flags: Vec<FeatureFlag> = Vec::new();
    for module in &parse_result.modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };

        // Built-in flag results from parse/cache
        let file_suppressed = fallow_core::suppress::is_file_suppressed(
            &module.suppressions,
            fallow_core::suppress::IssueKind::FeatureFlag,
        );
        for flag_use in &module.flag_uses {
            if file_suppressed
                || fallow_core::suppress::is_suppressed(
                    &module.suppressions,
                    flag_use.line,
                    fallow_core::suppress::IssueKind::FeatureFlag,
                )
            {
                continue;
            }
            flags.push(flag_use_to_feature_flag(flag_use, module, path));
        }

        // Supplementary extraction pass for user-configured patterns.
        // Built-in patterns are already in module.flag_uses (cached).
        // Custom SDK patterns, env prefixes, and config object heuristics
        // require re-reading source because they weren't applied at parse time.
        if has_custom_config && let Ok(source) = std::fs::read_to_string(path) {
            let custom_flags = fallow_core::extract::flags::extract_flags_from_source(
                &source,
                path,
                &extra_sdk,
                &config.flags.env_prefixes,
                config.flags.config_object_heuristics,
            );
            // Only add flags not already found by built-in extraction (dedup by line+name)
            for flag_use in &custom_flags {
                let already_found = module.flag_uses.iter().any(|existing| {
                    existing.line == flag_use.line && existing.flag_name == flag_use.flag_name
                });
                if !already_found
                    && !fallow_core::suppress::is_suppressed(
                        &module.suppressions,
                        flag_use.line,
                        fallow_core::suppress::IssueKind::FeatureFlag,
                    )
                {
                    flags.push(flag_use_to_feature_flag(flag_use, module, path));
                }
            }
        }
    }

    // Run dead code analysis for cross-reference (flags guarding dead code).
    // Uses pre-parsed modules to avoid re-parsing.
    if let Ok(analysis_output) =
        fallow_core::analyze_with_parse_result(&config, &parse_result.modules)
    {
        fallow_core::analyze::feature_flags::correlate_with_dead_code(
            &mut flags,
            &analysis_output.results,
        );
    }

    // Filter to changed files if --changed-since is active
    if let Some(git_ref) = opts.changed_since
        && let Some(changed) = crate::check::get_changed_files(opts.root, git_ref)
    {
        flags.retain(|f| changed.contains(&f.path));
    }

    // Filter to workspace(s) if specified (either --workspace or --changed-workspaces)
    let ws_scope = match crate::check::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    ) {
        Ok(scope) => scope,
        Err(code) => return code,
    };
    if let Some(ref ws_roots) = ws_scope {
        flags.retain(|f| ws_roots.iter().any(|r| f.path.starts_with(r)));
    }

    // Sort for deterministic output
    flags.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.flag_name.cmp(&b.flag_name))
    });

    // Apply top N limit
    if let Some(top) = opts.top {
        flags.truncate(top);
    }

    let elapsed = start.elapsed();

    // Badge format is health-only
    if matches!(
        opts.output,
        OutputFormat::PrCommentGithub
            | OutputFormat::PrCommentGitlab
            | OutputFormat::ReviewGithub
            | OutputFormat::ReviewGitlab
            | OutputFormat::Badge
    ) {
        return emit_error(
            "flags supports human, json, compact, sarif, markdown, and codeclimate output",
            2,
            opts.output,
        );
    }

    // Render output
    print_flags_result(&flags, &config, opts, elapsed);

    ExitCode::SUCCESS
}

/// Print feature flag results in the requested format.
fn print_flags_result(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    opts: &FlagsOptions<'_>,
    elapsed: std::time::Duration,
) {
    match opts.output {
        OutputFormat::Human => print_flags_human(flags, config, elapsed, opts.quiet),
        OutputFormat::Json => print_flags_json(flags, config, elapsed, opts.explain),
        OutputFormat::Compact => print_flags_compact(flags, config),
        OutputFormat::Sarif => print_flags_sarif(flags, config),
        OutputFormat::Markdown => print_flags_markdown(flags, config),
        OutputFormat::CodeClimate => print_flags_codeclimate(flags, config),
        OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge => unreachable!("handled above"),
    }
}

/// Format a kind tag for a feature flag.
fn kind_tag(flag: &FeatureFlag) -> String {
    use colored::Colorize;
    match flag.kind {
        FlagKind::EnvironmentVariable => "(env)".dimmed().to_string(),
        FlagKind::SdkCall => {
            if let Some(ref sdk) = flag.sdk_name {
                format!("(SDK: {sdk})").dimmed().to_string()
            } else {
                "(SDK)".dimmed().to_string()
            }
        }
        FlagKind::ConfigObject => "(config, heuristic)".dimmed().to_string(),
    }
}

/// Print a file path with dimmed directory and bold filename.
fn print_file_path(display: &str) {
    use colored::Colorize;
    if let Some(parent) = std::path::Path::new(display).parent() {
        let parent_str = parent.to_string_lossy();
        let file_name = std::path::Path::new(display)
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().to_string());
        if parent_str.is_empty() {
            println!("  {}", file_name.bold());
        } else {
            println!(
                "  {}{}{}",
                parent_str.dimmed(),
                "/".dimmed(),
                file_name.bold()
            );
        }
    } else {
        println!("  {}", display.bold());
    }
}

/// Human-readable output for `fallow flags`.
fn print_flags_human(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    elapsed: std::time::Duration,
    quiet: bool,
) {
    use colored::Colorize;

    if flags.is_empty() {
        if !quiet {
            eprintln!(
                "{} No feature flags detected ({:.2}s)",
                "\u{2713}".green().bold(),
                elapsed.as_secs_f64()
            );
        }
        return;
    }

    // Separate flags guarding dead code (cross-reference) from inventory
    let dead_code_flags: Vec<&FeatureFlag> = flags
        .iter()
        .filter(|f| !f.guarded_dead_exports.is_empty())
        .collect();

    // Cross-reference section first (the primary value)
    if !dead_code_flags.is_empty() {
        let label = format!("Flags guarding dead code ({})", dead_code_flags.len());
        println!("{} {}", "\u{25cf}".yellow(), label.yellow().bold());

        for flag in &dead_code_flags {
            let relative = flag
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&flag.path)
                .to_string_lossy()
                .replace('\\', "/");
            print_file_path(&relative);

            let dead_count = flag.guarded_dead_exports.len();
            let guard_lines = flag
                .guard_line_start
                .and_then(|s| flag.guard_line_end.map(|e| e.saturating_sub(s) + 1))
                .unwrap_or(0);

            let detail = if guard_lines > 0 {
                format!("guards {guard_lines} lines, {dead_count} statically dead")
            } else {
                format!("{dead_count} dead exports in guarded block")
            };

            println!(
                "    {} {} {} {}",
                format!(":{}", flag.line).dimmed(),
                flag.flag_name.bold(),
                kind_tag(flag),
                format!("({detail})").dimmed(),
            );
        }
        println!();
    }

    // Full inventory section
    let mut by_file: Vec<(&std::path::Path, Vec<&FeatureFlag>)> = Vec::new();
    for flag in flags {
        if let Some(entry) = by_file.iter_mut().find(|(p, _)| *p == flag.path.as_path()) {
            entry.1.push(flag);
        } else {
            by_file.push((flag.path.as_path(), vec![flag]));
        }
    }

    let label = format!("Feature flags ({})", flags.len());
    println!("{} {}", "\u{25cf}".cyan(), label.cyan().bold());

    for (file_path, file_flags) in &by_file {
        let relative = file_path.strip_prefix(&config.root).unwrap_or(file_path);
        let display = relative.to_string_lossy().replace('\\', "/");
        print_file_path(&display);

        for flag in file_flags {
            println!(
                "    {} {} {}",
                format!(":{}", flag.line).dimmed(),
                flag.flag_name.bold(),
                kind_tag(flag),
            );
        }
    }

    // Footer
    if !quiet {
        let elapsed_str = format!("{:.2}s", elapsed.as_secs_f64());
        eprintln!(
            "\n{} {} flags detected ({})",
            "\u{2713}".green().bold(),
            flags.len(),
            elapsed_str.dimmed(),
        );
    }
}

/// Compact output (one line per finding) for `fallow flags`.
///
/// Follows the established `tag:path:line:detail` convention from `compact.rs`.
fn print_flags_compact(flags: &[FeatureFlag], config: &ResolvedConfig) {
    for flag in flags {
        let relative = flag
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&flag.path)
            .to_string_lossy()
            .replace('\\', "/");
        let tag = match flag.kind {
            FlagKind::EnvironmentVariable => "feature-flag-env",
            FlagKind::SdkCall => "feature-flag-sdk",
            FlagKind::ConfigObject => "feature-flag-config",
        };
        println!("{tag}:{relative}:{}:{}", flag.line, flag.flag_name);
    }
}

/// FNV-1a (64-bit) fingerprint for deterministic CodeClimate fingerprints.
/// Matches the algorithm used in `report/codeclimate.rs`.
fn fnv_fingerprint(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Helper: get relative path string for a flag.
fn relative_path(flag: &FeatureFlag, root: &std::path::Path) -> String {
    flag.path
        .strip_prefix(root)
        .unwrap_or(&flag.path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Helper: human-readable kind label.
fn kind_label(flag: &FeatureFlag) -> &'static str {
    match flag.kind {
        FlagKind::EnvironmentVariable => "environment variable",
        FlagKind::SdkCall => "SDK call",
        FlagKind::ConfigObject => "config object",
    }
}

/// SARIF output for `fallow flags`.
fn print_flags_sarif(flags: &[FeatureFlag], config: &ResolvedConfig) {
    let rules = vec![serde_json::json!({
        "id": "fallow/feature-flag",
        "shortDescription": { "text": "Feature flag pattern detected" },
        "helpUri": "https://docs.fallow.tools/explanations/feature-flags",
        "defaultConfiguration": { "level": "note" },
    })];

    let results: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
            let path = crate::report::normalize_uri(&relative_path(f, &config.root));
            let mut msg = format!("Feature flag '{}' ({})", f.flag_name, kind_label(f));
            if !f.guarded_dead_exports.is_empty() {
                use std::fmt::Write;
                let _ = write!(
                    msg,
                    " guards {} dead exports: {}",
                    f.guarded_dead_exports.len(),
                    f.guarded_dead_exports.join(", ")
                );
            }
            serde_json::json!({
                "ruleId": "fallow/feature-flag",
                "level": "note",
                "message": { "text": msg },
                "locations": [{
                    "physicalLocation": {
                        "artifactLocation": { "uri": path },
                        "region": { "startLine": f.line, "startColumn": f.col + 1 },
                    }
                }],
            })
        })
        .collect();

    let sarif = serde_json::json!({
        "$schema": "https://raw.githubusercontent.com/oasis-tcs/sarif-spec/main/sarif-2.1/schema/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                    "rules": rules,
                }
            },
            "results": results,
        }]
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&sarif).expect("JSON serialization should not fail")
    );
}

/// Escape backticks in a string for safe embedding in markdown code spans.
fn escape_backticks(s: &str) -> String {
    s.replace('`', "\\`")
}

/// Markdown output for `fallow flags` (PR comments).
fn print_flags_markdown(flags: &[FeatureFlag], config: &ResolvedConfig) {
    if flags.is_empty() {
        println!("## Feature flags: no flags detected");
        return;
    }

    // Summary heading
    println!("## Feature flags: {} found\n", flags.len());

    // Cross-reference section first
    let dead_flags: Vec<&FeatureFlag> = flags
        .iter()
        .filter(|f| !f.guarded_dead_exports.is_empty())
        .collect();

    if !dead_flags.is_empty() {
        println!("### Flags guarding dead code ({})\n", dead_flags.len());
        println!("| File | Line | Flag | Dead exports |");
        println!("|------|------|------|-------------|");
        for f in &dead_flags {
            let path = escape_backticks(&relative_path(f, &config.root));
            let name = escape_backticks(&f.flag_name);
            println!(
                "| `{path}` | {} | `{name}` | `{}` |",
                f.line,
                f.guarded_dead_exports.join("`, `")
            );
        }
        println!();
    }

    // Full inventory
    println!("### Feature flags ({})\n", flags.len());
    println!("| File | Line | Flag | Kind |");
    println!("|------|------|------|------|");
    for f in flags {
        let path = escape_backticks(&relative_path(f, &config.root));
        let name = escape_backticks(&f.flag_name);
        let kind = match f.kind {
            FlagKind::EnvironmentVariable => "env".to_string(),
            FlagKind::SdkCall => f
                .sdk_name
                .as_ref()
                .map_or_else(|| "SDK".to_string(), |sdk| format!("SDK: {sdk}")),
            FlagKind::ConfigObject => "config".to_string(),
        };
        println!("| `{path}` | {} | `{name}` | {kind} |", f.line);
    }
}

/// CodeClimate output for `fallow flags` (GitLab Code Quality).
fn print_flags_codeclimate(flags: &[FeatureFlag], config: &ResolvedConfig) {
    let issues: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
            // Use crate::report::n for bracket encoding (Next.js dynamic routes)
            let path = crate::report::normalize_uri(&relative_path(f, &config.root));
            let mut description = format!(
                "Feature flag '{}' detected ({})",
                f.flag_name,
                kind_label(f)
            );
            if !f.guarded_dead_exports.is_empty() {
                use std::fmt::Write;
                let _ = write!(
                    description,
                    ". Guards {} dead exports",
                    f.guarded_dead_exports.len()
                );
            }
            let fingerprint =
                fnv_fingerprint(&["feature-flag", &path, &f.line.to_string(), &f.flag_name]);
            serde_json::json!({
                "type": "issue",
                "check_name": "fallow/feature-flag",
                "description": description,
                "categories": ["Clarity"],
                "severity": "info",
                "fingerprint": fingerprint,
                "location": {
                    "path": path,
                    "lines": { "begin": f.line },
                }
            })
        })
        .collect();

    println!(
        "{}",
        serde_json::to_string_pretty(&issues).expect("JSON serialization should not fail")
    );
}

/// JSON output for `fallow flags`.
fn print_flags_json(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    elapsed: std::time::Duration,
    explain: bool,
) {
    let flags_json: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
            let path = f
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&f.path)
                .to_string_lossy()
                .replace('\\', "/");

            let confidence = match f.confidence {
                FlagConfidence::High => "high",
                FlagConfidence::Medium => "medium",
                FlagConfidence::Low => "low",
            };

            let kind = match f.kind {
                FlagKind::EnvironmentVariable => "environment_variable",
                FlagKind::SdkCall => "sdk_call",
                FlagKind::ConfigObject => "config_object",
            };

            let mut obj = serde_json::json!({
                "path": path,
                "flag_name": f.flag_name,
                "kind": kind,
                "confidence": confidence,
                "line": f.line,
                "col": f.col,
                "actions": [
                    {
                        "type": "investigate-flag",
                        "auto_fixable": false,
                        "description": format!("Verify whether feature flag '{}' is still active", f.flag_name),
                    },
                    {
                        "type": "suppress-line",
                        "auto_fixable": false,
                        "description": "Suppress with an inline comment",
                        "comment": "// fallow-ignore-next-line feature-flag",
                    },
                ],
            });

            if let Some(ref sdk) = f.sdk_name {
                obj["sdk_name"] = serde_json::json!(sdk);
            }

            if !f.guarded_dead_exports.is_empty() {
                let guard_lines = f
                    .guard_line_start
                    .and_then(|s| f.guard_line_end.map(|e| e.saturating_sub(s) + 1))
                    .unwrap_or(0);
                obj["dead_code_overlap"] = serde_json::json!({
                    "guarded_lines": guard_lines,
                    "dead_export_count": f.guarded_dead_exports.len(),
                    "dead_exports": f.guarded_dead_exports,
                });
            }

            obj
        })
        .collect();

    // Schema version must match SCHEMA_VERSION in report/json.rs
    let mut output = serde_json::json!({
        "schema_version": 3,
        "version": env!("CARGO_PKG_VERSION"),
        "elapsed_ms": elapsed.as_millis(),
        "feature_flags": flags_json,
        "total_flags": flags.len(),
    });

    if explain {
        output["_meta"] = serde_json::json!({
            "feature_flags": {
                "description": "Feature flag patterns detected via AST analysis",
                "kinds": {
                    "environment_variable": "process.env.FEATURE_* pattern (high confidence)",
                    "sdk_call": "Feature flag SDK function call (high confidence)",
                    "config_object": "Config object property access matching flag keywords (low confidence, heuristic)",
                },
                "confidence": {
                    "high": "Unambiguous pattern match (env vars, direct SDK calls)",
                    "medium": "Pattern match with some ambiguity",
                    "low": "Heuristic match (config objects), may produce false positives",
                },
                "docs": "https://docs.fallow.tools/explanations/feature-flags",
            }
        });
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization should not fail")
    );
}
