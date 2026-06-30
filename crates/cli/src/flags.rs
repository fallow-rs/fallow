//! `fallow flags` subcommand: detect and report feature flag patterns.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use fallow_config::{OutputFormat, ResolvedConfig};
use fallow_types::results::{FeatureFlag, FlagKind};

use crate::error::emit_error;

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

    let config = match load_flags_config(opts) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let analysis = fallow_engine::analyze_feature_flags(&config);
    if analysis.files_scanned == 0 {
        return emit_error("no files discovered", 2, opts.output);
    }

    let mut flags = analysis.flags;
    if let Err(code) = apply_flag_scopes(&mut flags, opts) {
        return code;
    }
    // Note find-state for telemetry before any exit (issue #1650 follow-up): the
    // flags command emits a `code_quality_review` workflow event (the same label
    // as combined `fallow`), so without this its findings_present serialized as
    // null. Count the scope-filtered flags BEFORE `--top` truncation so the
    // bucket reflects the full set, not the displayed head.
    crate::telemetry::note_result_count(flags.len());
    sort_and_limit_flags(&mut flags, opts.top);

    let elapsed = start.elapsed();
    if let Err(code) = validate_flags_output(opts.output) {
        return code;
    }

    print_flags_result(&flags, &config, opts, elapsed, analysis.files_scanned);

    ExitCode::SUCCESS
}

fn load_flags_config(opts: &FlagsOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    crate::runtime_support::load_config(
        opts.root,
        opts.config_path,
        crate::runtime_support::LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: opts.quiet,
        },
    )
}

fn apply_flag_scopes(
    flags: &mut Vec<FeatureFlag>,
    opts: &FlagsOptions<'_>,
) -> Result<(), ExitCode> {
    if let Some(git_ref) = opts.changed_since
        && let Some(changed) = crate::check::get_changed_files(opts.root, git_ref)
    {
        flags.retain(|f| changed.contains(&f.path));
    }

    let ws_scope = crate::check::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;
    if let Some(ref ws_roots) = ws_scope {
        flags.retain(|f| ws_roots.iter().any(|r| f.path.starts_with(r)));
    }
    Ok(())
}

fn sort_and_limit_flags(flags: &mut Vec<FeatureFlag>, top: Option<usize>) {
    flags.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.flag_name.cmp(&b.flag_name))
    });

    if let Some(top) = top {
        flags.truncate(top);
    }
}

fn validate_flags_output(output: OutputFormat) -> Result<(), ExitCode> {
    if matches!(
        output,
        OutputFormat::PrCommentGithub
            | OutputFormat::PrCommentGitlab
            | OutputFormat::ReviewGithub
            | OutputFormat::ReviewGitlab
            | OutputFormat::Badge
    ) {
        return Err(emit_error(
            "flags supports human, json, compact, sarif, markdown, and codeclimate output",
            2,
            output,
        ));
    }
    Ok(())
}

/// Print feature flag results in the requested format.
fn print_flags_result(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    opts: &FlagsOptions<'_>,
    elapsed: std::time::Duration,
    files_scanned: usize,
) {
    match opts.output {
        OutputFormat::Human => print_flags_human(flags, config, elapsed, opts.quiet, files_scanned),
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

/// When `fallow flags` finds nothing, surface the configuration surface so the
/// user can distinguish a true negative from "fallow does not recognize my SDK
/// yet". On full defaults the hint enumerates the built-in detectors (sourced
/// from `fallow-engine`, never hardcoded) and points at the config knobs. When
/// custom `flags.*` config is present, it collapses to a single terse
/// acknowledgement so users who already found the surface are not nagged. All
/// lines go to stderr, mirroring the empty-result line they follow.
fn print_empty_flags_hint(config: &ResolvedConfig, files_scanned: usize) {
    let custom_sdk = config.flags.sdk_patterns.len();
    let custom_env = config.flags.env_prefixes.len();
    let heuristics = config.flags.config_object_heuristics;
    let has_custom = custom_sdk > 0 || custom_env > 0 || heuristics;

    let files_label = if files_scanned == 1 { "file" } else { "files" };

    if has_custom {
        print_empty_flags_custom_hint(
            custom_sdk,
            custom_env,
            heuristics,
            files_scanned,
            files_label,
        );
    } else {
        print_empty_flags_default_hint(files_scanned, files_label);
    }
}

/// Terse one-line acknowledgement of an empty result when custom `flags.*`
/// config is present.
fn print_empty_flags_custom_hint(
    custom_sdk: usize,
    custom_env: usize,
    heuristics: bool,
    files_scanned: usize,
    files_label: &str,
) {
    use colored::Colorize;

    let mut parts: Vec<String> = Vec::new();
    if custom_sdk > 0 {
        parts.push(format!(
            "{custom_sdk} custom SDK pattern{}",
            if custom_sdk == 1 { "" } else { "s" }
        ));
    }
    if custom_env > 0 {
        parts.push(format!(
            "{custom_env} custom env prefix{}",
            if custom_env == 1 { "" } else { "es" }
        ));
    }
    if heuristics {
        parts.push("config-object heuristics enabled".to_string());
    }
    eprintln!(
        "  {}",
        format!(
            "Scanned {files_scanned} {files_label} with your custom flag config: {}.",
            parts.join(", ")
        )
        .dimmed()
    );
}

/// Enumerate the built-in detectors and config knobs on an empty result with a
/// full-defaults configuration.
fn print_empty_flags_default_hint(files_scanned: usize, files_label: &str) {
    use colored::Colorize;

    let env_prefixes = fallow_engine::builtin_env_prefixes()
        .iter()
        .map(|p| format!("{p}*"))
        .collect::<Vec<_>>()
        .join(", ");
    let providers = fallow_engine::builtin_sdk_providers().join(", ");

    eprintln!(
        "  {}",
        format!("Scanned {files_scanned} {files_label} for:").dimmed()
    );
    eprintln!(
        "    {} Env prefixes: {}",
        "\u{00b7}".dimmed(),
        env_prefixes.dimmed()
    );
    eprintln!("    {} SDKs: {}", "\u{00b7}".dimmed(), providers.dimmed());
    eprintln!(
        "  {}",
        "Using a different SDK (in-house, or one not listed)? Add it via `flags.sdkPatterns` in your config.".dimmed()
    );
    eprintln!(
        "  {}",
        "For property-access patterns (config.featureX), enable `flags.configObjectHeuristics`."
            .dimmed()
    );
    eprintln!(
        "  {}",
        "Docs: https://docs.fallow.tools/cli/flags#configuration".dimmed()
    );
}

/// Print the "Flags guarding dead code" section (human format). No-op when no
/// flag guards a statically dead export.
fn print_dead_code_flags_section(flags: &[FeatureFlag], config: &ResolvedConfig) {
    use colored::Colorize;

    let dead_code_flags: Vec<&FeatureFlag> = flags
        .iter()
        .filter(|f| !f.guarded_dead_exports.is_empty())
        .collect();
    if dead_code_flags.is_empty() {
        return;
    }

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

/// Print the per-file "Feature flags" listing (human format), preserving the
/// order in which files first appear in `flags`.
fn print_flags_by_file_section(flags: &[FeatureFlag], config: &ResolvedConfig) {
    use colored::Colorize;

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
}

/// Human-readable output for `fallow flags`.
fn print_flags_human(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    elapsed: std::time::Duration,
    quiet: bool,
    files_scanned: usize,
) {
    use colored::Colorize;

    if flags.is_empty() {
        if !quiet {
            eprintln!(
                "{} No feature flags detected ({:.2}s)",
                "\u{2713}".green().bold(),
                elapsed.as_secs_f64()
            );
            print_empty_flags_hint(config, files_scanned);
        }
        return;
    }

    print_dead_code_flags_section(flags, config);
    print_flags_by_file_section(flags, config);

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
#[expect(
    clippy::expect_used,
    reason = "feature flag SARIF JSON is built from serializable literals"
)]
fn print_flags_sarif(flags: &[FeatureFlag], config: &ResolvedConfig) {
    let rules = vec![serde_json::json!({
        "id": "fallow/feature-flag",
        "shortDescription": { "text": "Feature flag pattern detected" },
        "helpUri": "https://docs.fallow.tools/cli/flags",
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

    println!("## Feature flags: {} found\n", flags.len());

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
#[expect(
    clippy::expect_used,
    reason = "feature flag CodeClimate JSON is built from serializable literals"
)]
fn print_flags_codeclimate(flags: &[FeatureFlag], config: &ResolvedConfig) {
    let issues: Vec<serde_json::Value> = flags
        .iter()
        .map(|f| {
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
#[expect(
    clippy::expect_used,
    reason = "feature flag JSON output is built from serializable literals"
)]
fn print_flags_json(
    flags: &[FeatureFlag],
    config: &ResolvedConfig,
    elapsed: std::time::Duration,
    explain: bool,
) {
    let output =
        fallow_output::build_feature_flags_output(fallow_output::FeatureFlagsOutputInput {
            schema_version: crate::report::SCHEMA_VERSION,
            version: env!("CARGO_PKG_VERSION").to_string(),
            elapsed,
            flags,
            root: &config.root,
            meta: explain.then(fallow_output::feature_flags_meta),
        });
    let output = fallow_output::serialize_feature_flags_json_output(
        output,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .expect("JSON serialization should not fail");

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("JSON serialization should not fail")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::FlagConfidence;
    use std::path::PathBuf;

    /// No explicit `--config`; static so the `&Option<PathBuf>` field borrows it.
    const NO_CONFIG: Option<PathBuf> = None;

    fn flags_fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/feature-flag-suppression")
    }

    fn flag(kind: FlagKind, name: &str, path: &str) -> FeatureFlag {
        FeatureFlag {
            path: PathBuf::from(path),
            flag_name: name.to_owned(),
            kind,
            confidence: FlagConfidence::High,
            line: 3,
            col: 2,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: None,
            guard_line_end: None,
            guarded_dead_exports: Vec::new(),
        }
    }

    fn flags_opts(root: &Path, output: OutputFormat) -> FlagsOptions<'_> {
        FlagsOptions {
            root,
            config_path: &NO_CONFIG,
            output,
            no_cache: true,
            threads: 1,
            quiet: true,
            production: false,
            workspace: None,
            changed_workspaces: None,
            changed_since: None,
            explain: false,
            top: None,
        }
    }

    #[test]
    fn fnv_fingerprint_is_deterministic_16_hex() {
        let a = fnv_fingerprint(&["src/index.ts", "FEATURE_X", "3"]);
        let b = fnv_fingerprint(&["src/index.ts", "FEATURE_X", "3"]);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        // Per-part separation means reordering parts changes the digest.
        assert_ne!(a, fnv_fingerprint(&["FEATURE_X", "src/index.ts", "3"]));
    }

    #[test]
    fn escape_backticks_escapes_only_backticks() {
        assert_eq!(escape_backticks("a`b`c"), "a\\`b\\`c");
        assert_eq!(escape_backticks("no ticks"), "no ticks");
    }

    #[test]
    fn kind_label_covers_all_kinds() {
        assert_eq!(
            kind_label(&flag(FlagKind::EnvironmentVariable, "X", "a.ts")),
            "environment variable"
        );
        assert_eq!(
            kind_label(&flag(FlagKind::SdkCall, "X", "a.ts")),
            "SDK call"
        );
        assert_eq!(
            kind_label(&flag(FlagKind::ConfigObject, "X", "a.ts")),
            "config object"
        );
    }

    #[test]
    fn relative_path_strips_root_and_normalizes_separators() {
        let root = Path::new("/proj");
        let f = flag(FlagKind::EnvironmentVariable, "X", "/proj/src/index.ts");
        assert_eq!(relative_path(&f, root), "src/index.ts");
        // A path outside the root is returned as-is (normalized).
        let outside = flag(FlagKind::EnvironmentVariable, "X", "/other/file.ts");
        assert_eq!(relative_path(&outside, root), "/other/file.ts");
    }

    #[test]
    fn kind_tag_labels_sdk_with_and_without_name() {
        colored::control::set_override(false);
        let mut sdk = flag(FlagKind::SdkCall, "X", "a.ts");
        sdk.sdk_name = Some("LaunchDarkly".to_owned());
        assert_eq!(kind_tag(&sdk), "(SDK: LaunchDarkly)");
        sdk.sdk_name = None;
        assert_eq!(kind_tag(&sdk), "(SDK)");
        assert_eq!(
            kind_tag(&flag(FlagKind::EnvironmentVariable, "X", "a.ts")),
            "(env)"
        );
        assert_eq!(
            kind_tag(&flag(FlagKind::ConfigObject, "X", "a.ts")),
            "(config, heuristic)"
        );
    }

    #[test]
    fn run_flags_renders_every_supported_format() {
        colored::control::set_override(false);
        let root = flags_fixture_root();
        for output in [
            OutputFormat::Human,
            OutputFormat::Json,
            OutputFormat::Compact,
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
        ] {
            assert_eq!(
                run_flags(&flags_opts(&root, output)),
                ExitCode::SUCCESS,
                "format {output:?} should render and exit 0"
            );
        }
    }

    #[test]
    fn run_flags_with_explain_emits_json_meta() {
        let root = flags_fixture_root();
        let opts = FlagsOptions {
            explain: true,
            ..flags_opts(&root, OutputFormat::Json)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_rejects_unsupported_format() {
        let root = flags_fixture_root();
        // Badge / PR-comment / review formats are not supported by `flags`.
        assert_eq!(
            run_flags(&flags_opts(&root, OutputFormat::Badge)),
            ExitCode::from(2)
        );
    }

    #[test]
    fn run_flags_empty_default_config_surfaces_detectors_hint() {
        colored::control::set_override(false);
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/flags-none-default");
        // Non-quiet so the built-in detectors hint renders on an empty result.
        let opts = FlagsOptions {
            quiet: false,
            ..flags_opts(&root, OutputFormat::Human)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_empty_custom_config_surfaces_terse_hint() {
        colored::control::set_override(false);
        let root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/flags-none-custom");
        let opts = FlagsOptions {
            quiet: false,
            ..flags_opts(&root, OutputFormat::Human)
        };
        assert_eq!(run_flags(&opts), ExitCode::SUCCESS);
    }

    #[test]
    fn run_flags_renders_sdk_call_flag_across_formats() {
        colored::control::set_override(false);
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"flags-sdk","main":"src/index.ts"}"#,
        )
        .unwrap();
        // `variation('name', ...)` is a built-in LaunchDarkly SDK flag pattern,
        // so the SDK-name branches of every renderer are exercised.
        std::fs::write(
            root.join("src/index.ts"),
            "export function boot() {\n  if (variation('checkout-flag', false)) {\n    console.log('on');\n  }\n}\n",
        )
        .unwrap();
        for output in [
            OutputFormat::Human,
            OutputFormat::Compact,
            OutputFormat::Sarif,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
            OutputFormat::Json,
        ] {
            assert_eq!(
                run_flags(&flags_opts(root, output)),
                ExitCode::SUCCESS,
                "SDK-flag render for {output:?} should exit 0"
            );
        }
    }
}
