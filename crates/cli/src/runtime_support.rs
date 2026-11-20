use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{LazyLock, Mutex, OnceLock};

use fallow_config::{
    FallowConfig, OutputFormat, PartialRulesConfig, ProductionAnalysis, ResolvedConfig, RulesConfig,
};
use fallow_engine::project_config::{
    ConfigFile, ConfigFileOptions, ConfigLoadError, ProductionFlags, ResolveOptions,
};
use fallow_output::GroupByMode;
use rustc_hash::{FxHashMap, FxHashSet};

static CONFIG_LOADED_LOGGED: LazyLock<Mutex<FxHashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(FxHashSet::default()));

/// Process-wide dedup of `security.categories` typo warnings so combined mode's
/// repeated config loads emit each at most once.
static SECURITY_CATEGORY_WARNED: LazyLock<Mutex<FxHashSet<String>>> =
    LazyLock::new(|| Mutex::new(FxHashSet::default()));

/// An unknown `security.categories` id plus the closest valid suggestion.
pub struct UnknownSecurityCategory {
    /// `include` or `exclude`.
    pub field: &'static str,
    /// The unrecognized category id.
    pub id: String,
    /// The closest valid category id, when one is near enough to suggest.
    pub suggestion: Option<String>,
}

/// Find ids in `security.categories.{include,exclude}` that are not real
/// catalogue categories, each with a closest-match suggestion. Pure: no output.
///
/// The security config crate cannot depend on `fallow-security` (the catalogue
/// lives there), so this validation runs at the CLI layer where both are
/// available, mirroring the config crate's own unknown-rule-key check.
#[must_use]
pub fn find_unknown_security_categories(
    security: &fallow_config::SecurityConfig,
) -> Vec<UnknownSecurityCategory> {
    let Some(categories) = &security.categories else {
        return Vec::new();
    };
    let valid: Vec<String> = fallow_security::security_categories()
        .into_iter()
        .map(|category| category.id)
        .collect();
    let valid_set: FxHashSet<&str> = valid.iter().map(String::as_str).collect();
    let mut unknown = Vec::new();
    for (field, ids) in [
        ("include", categories.include.as_ref()),
        ("exclude", categories.exclude.as_ref()),
    ] {
        let Some(ids) = ids else { continue };
        for id in ids {
            if valid_set.contains(id.as_str()) {
                continue;
            }
            unknown.push(UnknownSecurityCategory {
                field,
                id: id.clone(),
                suggestion: fallow_config::levenshtein::closest_match(
                    id,
                    valid.iter().map(String::as_str),
                )
                .map(str::to_owned),
            });
        }
    }
    unknown
}

/// Emit one deduped `tracing::warn!` per unknown `security.categories` id.
///
/// Advisory only: an unknown id is silently ignored by the detector, so a typo
/// otherwise disables a category with no signal at all.
pub fn warn_unknown_security_categories(security: &fallow_config::SecurityConfig) {
    for unknown in find_unknown_security_categories(security) {
        let dedup_key = format!("{}:{}", unknown.field, unknown.id);
        if let Ok(mut seen) = SECURITY_CATEGORY_WARNED.lock()
            && !seen.insert(dedup_key)
        {
            continue;
        }
        if let Some(suggestion) = &unknown.suggestion {
            tracing::warn!(
                "unknown security category '{}' in security.categories.{} (did you mean '{}'?); it is ignored. Valid ids: fallow schema (security_categories) or fallow security --help",
                unknown.id,
                unknown.field,
                suggestion
            );
        } else {
            tracing::warn!(
                "unknown security category '{}' in security.categories.{}; it is ignored. Valid ids: fallow schema (security_categories) or fallow security --help",
                unknown.id,
                unknown.field
            );
        }
    }
}

/// The `--max-file-size` global flag value, set once from `main()` after clap
/// parse. `Some(Some(mb))` means the flag was passed; `Some(None)` / unset
/// means it was not. Held in a `OnceLock` rather than threaded through the ten
/// `load_config_for_analysis` callers (the skill-endorsed set-once-read-by-many
/// pattern; avoids `set_var`, which is unsafe under edition 2024).
static MAX_FILE_SIZE_OVERRIDE: OnceLock<Option<u32>> = OnceLock::new();

/// Record the `--max-file-size` flag value (megabytes; `Some(0)` = unlimited).
/// Called once from `main()` before dispatch. Subsequent calls are ignored.
pub fn set_max_file_size_override(max_file_size_mb: Option<u32>) {
    let _ = MAX_FILE_SIZE_OVERRIDE.set(max_file_size_mb);
}

/// The `--max-file-size` flag value in megabytes, when the flag was passed.
/// The engine applies `FALLOW_MAX_FILE_SIZE` when it resolves the config, and
/// the flag then replaces that value. `Some(0)` means unlimited.
fn max_file_size_flag_mb() -> Option<u32> {
    MAX_FILE_SIZE_OVERRIDE.get().copied().flatten()
}

/// The cache directories of the analysis roots in the temporary worktrees of
/// `fallow audit`: the staged index of `--staged`, and the base snapshot. The
/// config of such a root uses its directory here, also when
/// `FALLOW_CACHE_DIR` or `cache.dir` names another one. A shared directory
/// would get the caches of a temporary root, and the module graph cache of
/// the checkout would no longer match.
static ROOT_CACHE_DIR_OVERRIDES: LazyLock<Mutex<FxHashMap<PathBuf, PathBuf>>> =
    LazyLock::new(|| Mutex::new(FxHashMap::default()));

/// Give the config of `root` the cache directory `cache_dir` for the rest of
/// the process.
pub fn set_root_cache_dir_override(root: &Path, cache_dir: PathBuf) {
    ROOT_CACHE_DIR_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(root.to_path_buf(), cache_dir);
}

fn root_cache_dir_override(root: &Path) -> Option<PathBuf> {
    ROOT_CACHE_DIR_OVERRIDES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(root)
        .cloned()
}

/// Analysis types for --only/--skip selection.
#[derive(Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum AnalysisKind {
    #[value(alias = "check")]
    DeadCode,
    Dupes,
    Health,
    /// The architecture findings of the dead-code analysis: import cycles,
    /// boundary violations and rule-pack policy violations.
    Architecture,
}

/// Grouping mode for `--group-by`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GroupBy {
    /// Group by CODEOWNERS file ownership (first owner, last matching rule).
    #[value(alias = "team", alias = "codeowner")]
    Owner,
    /// Group by first directory component of the file path.
    Directory,
    /// Group by workspace package (monorepo).
    #[value(alias = "workspace", alias = "pkg")]
    Package,
    /// Group by GitLab CODEOWNERS section name (`[Section]` headers).
    /// Stable across reviewer rotation; produces distinct groups when
    /// multiple sections share a common default owner.
    #[value(alias = "gl-section")]
    Section,
}

impl From<GroupBy> for GroupByMode {
    fn from(value: GroupBy) -> Self {
        match value {
            GroupBy::Owner => Self::Owner,
            GroupBy::Directory => Self::Directory,
            GroupBy::Package => Self::Package,
            GroupBy::Section => Self::Section,
        }
    }
}

/// Build an `OwnershipResolver` from CLI `--group-by` and config settings.
///
/// Returns `None` when no grouping is requested. Returns `Err(ExitCode)` when
/// `--group-by owner` is requested but no CODEOWNERS file can be found.
///
/// `workspace_patterns` is the `workspaces.patterns` config value. `--group-by
/// package` adds these patterns to the manifest patterns.
pub fn build_ownership_resolver(
    group_by: Option<GroupBy>,
    root: &Path,
    codeowners_path: Option<&str>,
    workspace_patterns: &[String],
    output: OutputFormat,
) -> Result<Option<crate::report::OwnershipResolver>, ExitCode> {
    build_ownership_resolver_for_mode(
        group_by.map(Into::into),
        root,
        codeowners_path,
        workspace_patterns,
        output,
    )
}

/// Build an `OwnershipResolver` from a typed output grouping mode.
pub fn build_ownership_resolver_for_mode(
    group_by: Option<GroupByMode>,
    root: &Path,
    codeowners_path: Option<&str>,
    workspace_patterns: &[String],
    output: OutputFormat,
) -> Result<Option<crate::report::OwnershipResolver>, ExitCode> {
    let Some(mode) = group_by else {
        return Ok(None);
    };
    match mode {
        GroupByMode::Owner => match crate::codeowners::CodeOwners::load(root, codeowners_path) {
            Ok(co) => Ok(Some(crate::report::OwnershipResolver::Owner(co))),
            Err(e) => Err(crate::error::emit_input_error(&e, output)),
        },
        GroupByMode::Section => match crate::codeowners::CodeOwners::load(root, codeowners_path) {
            Ok(co) => {
                if co.has_sections() {
                    Ok(Some(crate::report::OwnershipResolver::Section(co)))
                } else {
                    Err(crate::error::emit_input_error(
                        "--group-by section requires a GitLab-style CODEOWNERS file \
                         with `[Section]` headers. This CODEOWNERS has no sections; \
                         use --group-by owner instead.",
                        output,
                    ))
                }
            }
            Err(e) => Err(crate::error::emit_input_error(&e, output)),
        },
        GroupByMode::Directory => Ok(Some(crate::report::OwnershipResolver::Directory)),
        GroupByMode::Package => {
            let workspaces = fallow_engine::discover::discover_workspace_packages_with_patterns(
                root,
                workspace_patterns,
            );
            if workspaces.is_empty() {
                Err(crate::error::emit_input_error(
                    "--group-by package requires a monorepo with workspace packages \
                     (package.json workspaces, pnpm-workspace.yaml, tsconfig references, \
                     or \"workspaces.patterns\" in the fallow config). \
                     For single-package projects try --group-by directory instead.",
                    output,
                ))
            } else {
                Ok(Some(crate::report::OwnershipResolver::Package(
                    crate::report::grouping::PackageResolver::new(root, &workspaces),
                )))
            }
        }
    }
}

/// Emit a terse `"loaded config: <path>"` line on stderr so users can verify
/// which config was picked up. Suppressed for non-human output formats (so
/// JSON/SARIF/markdown consumers get clean machine-readable output) and when
/// `--quiet` is set.
fn log_config_loaded(path: &Path, output: OutputFormat, quiet: bool) {
    if quiet || !matches!(output, OutputFormat::Human) {
        return;
    }
    if !should_log_config_loaded(path) {
        return;
    }
    eprintln!("loaded config: {}", path.display());
}

fn should_log_config_loaded(path: &Path) -> bool {
    let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    CONFIG_LOADED_LOGGED
        .lock()
        .is_ok_and(|mut logged| logged.insert(key))
}

#[derive(Clone, Copy)]
pub struct ConfigLoadOptions {
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub production_override: Option<bool>,
    pub quiet: bool,
    pub allow_remote_extends: bool,
}

/// The scalar config-loading knobs for [`load_config`], bundled so the entry
/// point takes the root + config path plus one args struct instead of seven
/// positional parameters.
#[derive(Clone, Copy)]
pub struct LoadConfigArgs {
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub production: bool,
    pub quiet: bool,
    pub allow_remote_extends: bool,
}

#[expect(clippy::ref_option, reason = "&Option matches clap's field type")]
pub fn load_config(
    root: &Path,
    config_path: &Option<PathBuf>,
    args: LoadConfigArgs,
) -> Result<ResolvedConfig, ExitCode> {
    let LoadConfigArgs {
        output,
        no_cache,
        threads,
        production,
        quiet,
        allow_remote_extends,
    } = args;
    load_config_for_analysis(
        root,
        config_path,
        ConfigLoadOptions {
            output,
            no_cache,
            threads,
            production_override: production.then_some(true),
            quiet,
            allow_remote_extends,
        },
        ProductionAnalysis::DeadCode,
    )
}

#[expect(clippy::ref_option, reason = "&Option matches clap's field type")]
pub fn load_config_for_analysis(
    root: &Path,
    config_path: &Option<PathBuf>,
    options: ConfigLoadOptions,
    analysis: ProductionAnalysis,
) -> Result<ResolvedConfig, ExitCode> {
    let file = load_config_file(
        root,
        config_path.as_deref(),
        options.allow_remote_extends,
        options.output,
    )?;
    if let Some(path) = &file.path {
        log_config_loaded(path, options.output, options.quiet);
    }
    let loaded_user_config = file.path.is_some();
    let file = file.for_analysis(
        analysis,
        ProductionFlags {
            global: options.production_override,
            ..ProductionFlags::default()
        },
    );
    crate::telemetry::note_config_shape(config_shape_for(&file.config, loaded_user_config));

    // A rule pack that fails to load must fail the run: silently skipping
    // policy is the exact failure mode rule packs document themselves as
    // preventing.
    file.validate(root)
        .map_err(|err| crate::error::emit_input_error(err.message(), options.output))?;

    let mut resolved = file.resolve(
        root,
        ResolveOptions {
            output: options.output,
            threads: options.threads,
            no_cache: options.no_cache,
            quiet: options.quiet,
        },
    );
    if let Some(mb) = max_file_size_flag_mb() {
        resolved.max_file_size_bytes = fallow_config::resolve_max_file_size_bytes(Some(mb));
    }
    if let Some(cache_dir) = root_cache_dir_override(root) {
        resolved.cache_dir = cache_dir;
    }
    crate::cache_notice::record_candidate(
        root,
        &resolved.cache_dir,
        options.output,
        options.quiet,
        resolved.no_cache,
    );

    report_workspace_diagnostics(root, &resolved, &options)?;
    warn_unknown_security_categories(&resolved.security);
    crate::output_runtime::set_workspace_patterns(&resolved.workspace_patterns);

    Ok(resolved)
}

/// The side effects of a config load for a resolved run: the `loaded config`
/// line, the config shape of the telemetry, the cache notice candidate, the
/// stash and the notice of the workspace diagnostics, the notes of the run
/// notices, the warning for an unknown security category and the workspace
/// patterns of the output.
///
/// `given_config` is the `--config` path as the user wrote it, so the line
/// names an explicit file as given.
pub fn on_run_resolved(
    run: &fallow_engine::resolved_run::ResolvedRun,
    given_config: Option<&Path>,
    output: OutputFormat,
    quiet: bool,
) {
    let config_file = run.config_file().value().as_deref();
    if let Some(path) = given_config.or(config_file) {
        log_config_loaded(path, output, quiet);
    }
    crate::telemetry::note_config_shape(config_shape_for(run.config(), config_file.is_some()));
    let root = run.root().value();
    let settings = run.settings();
    crate::cache_notice::record_candidate(
        root,
        settings.cache_dir.value(),
        output,
        quiet,
        *settings.no_cache.value(),
    );
    let runnable = [
        fallow_config::PlannedAnalysis::DeadCode,
        fallow_config::PlannedAnalysis::Health,
        fallow_config::PlannedAnalysis::Duplication,
    ]
    .into_iter()
    .find_map(|analysis| match run.analysis(analysis) {
        fallow_engine::resolved_run::AnalysisSlot::Runs(runnable) => Some(runnable),
        fallow_engine::resolved_run::AnalysisSlot::Skipped { .. } => None,
    });
    if let Some(runnable) = runnable {
        // The notice and the stash hold the diagnostics of workspace
        // discovery only. The session adds the undeclared workspaces to its
        // own list, so they reach the reports without the notice.
        let diagnostics: Vec<_> = runnable
            .workspace_diagnostics()
            .iter()
            .filter(|diagnostic| {
                !matches!(
                    diagnostic.kind,
                    fallow_config::WorkspaceDiagnosticKind::UndeclaredWorkspace
                )
            })
            .cloned()
            .collect();
        let discovered = diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.kind.is_run_notice())
            .count();
        if discovered > 0 && matches!(output, OutputFormat::Human) && !quiet {
            eprintln!("{}", workspace_diagnostics_notice(discovered));
        }
        fallow_config::stash_workspace_diagnostics(root, diagnostics);
        crate::output_runtime::set_workspace_patterns(&runnable.config().workspace_patterns);
    }
    crate::report::notices::print_run_notices(run.notices(), output, quiet);
    warn_unknown_security_categories(&run.config().security);
}

/// Load the config file through the engine loader: the file at `config_path`,
/// else the discovered file. A load error prints on the error channel of
/// `output` and gives exit code 2.
pub fn load_config_file(
    root: &Path,
    config_path: Option<&Path>,
    allow_remote_extends: bool,
    output: OutputFormat,
) -> Result<ConfigFile, ExitCode> {
    fallow_engine::project_config::load_config_file(
        root,
        config_path,
        ConfigFileOptions::new(allow_remote_extends, &fallow_config::EnvSnapshot::capture()),
    )
    .map_err(|err| crate::error::emit_input_error(&config_load_error_message(&err), output))
}

/// The CLI text of a config load error. An explicit file adds its path to the
/// top error. The top error of a discovered file names the path itself.
pub fn config_load_error_message(err: &ConfigLoadError) -> String {
    match err {
        ConfigLoadError::Parse {
            path: Some(path),
            message,
            ..
        } => format!("failed to load config '{}': {message}", path.display()),
        _ => err.message().to_owned(),
    }
}

/// The config inputs of a command line: the `--config` path and the trust
/// policy of `--allow-remote-extends`. A command that reads optional settings
/// passes both to each config load, so every load reads the same file with
/// the same policy.
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigFlags<'a> {
    /// The `--config` path. `None` lets discovery find the config.
    pub path: Option<&'a Path>,
    /// Permit `https://` entries in the `extends` chain.
    pub allow_remote_extends: bool,
}

impl ConfigFlags<'_> {
    /// The load options of these flags.
    #[must_use]
    pub fn file_options(self) -> ConfigFileOptions {
        ConfigFileOptions::new(
            self.allow_remote_extends,
            &fallow_config::EnvSnapshot::capture(),
        )
    }

    /// The `--config` file, when it loads.
    #[must_use]
    pub fn explicit(self, root: &Path) -> Option<FallowConfig> {
        self.path
            .and_then(|path| loaded_config(root, Some(path), self.file_options()))
    }

    /// The config that discovery finds from `root`, when it loads.
    #[must_use]
    pub fn discovered(self, root: &Path) -> Option<FallowConfig> {
        loaded_config(root, None, self.file_options())
    }
}

impl<'a> ConfigFlags<'a> {
    /// The config flags of `cli`.
    #[must_use]
    pub fn from_cli(cli: &'a crate::cli_args::Cli) -> Self {
        Self {
            path: cli.config.as_deref(),
            allow_remote_extends: cli.allow_remote_extends,
        }
    }
}

/// The config file at `config_path`, else the discovered config file. `None`
/// when the file does not load or when no file exists. A caller that reads
/// one optional setting uses this, so a broken config never stops it.
pub fn loaded_config(
    root: &Path,
    config_path: Option<&Path>,
    options: ConfigFileOptions,
) -> Option<FallowConfig> {
    let file = fallow_engine::project_config::load_config_file(root, config_path, options).ok()?;
    file.path.map(|_| file.config)
}

/// Discover and stash workspace diagnostics, surfacing a one-line stderr notice
/// in human mode when any are present.
fn report_workspace_diagnostics(
    root: &Path,
    resolved: &ResolvedConfig,
    options: &ConfigLoadOptions,
) -> Result<(), ExitCode> {
    match fallow_engine::discover::discover_workspace_packages_for_config(
        root,
        &resolved.ignore_patterns,
        &resolved.workspace_patterns,
    ) {
        Ok((_, diagnostics)) => {
            fallow_config::stash_workspace_diagnostics(root, diagnostics.clone());
            if !diagnostics.is_empty()
                && matches!(options.output, OutputFormat::Human)
                && !options.quiet
            {
                eprintln!("{}", workspace_diagnostics_notice(diagnostics.len()));
            }
            Ok(())
        }
        Err(err) => Err(crate::error::emit_input_error(
            err.message(),
            options.output,
        )),
    }
}

/// Render the one-line stderr notice that workspace discovery produced
/// diagnostics.
///
/// Built by name so the wording is unit testable rather than only observable
/// through a subprocess. The notice names the dedicated `fallow workspaces`
/// command rather than the equivalent `fallow list --workspaces`: both print
/// the same per-entry block, and the dedicated command is the shorter, more
/// direct spelling: at the four-digit count this treats as the realistic
/// ceiling it leaves ten columns of the eighty-column terminal budget, against
/// three for `fallow list --workspaces`. The pinning test measures the chosen
/// spelling; the alternative's figure is stated here because nothing measures
/// it.
fn workspace_diagnostics_notice(count: usize) -> String {
    format!(
        "fallow: {count} workspace discovery diagnostic{}. Run `fallow workspaces`.",
        crate::report::plural(count)
    )
}

fn config_shape_for(
    config: &FallowConfig,
    loaded_user_config: bool,
) -> crate::telemetry::ConfigShape {
    if !config.plugins.is_empty() || !config.framework.is_empty() {
        return crate::telemetry::ConfigShape::PluginsEnabled;
    }
    if config.rules != RulesConfig::default()
        || config
            .overrides
            .iter()
            .any(|entry| partial_rules_config_has_values(&entry.rules))
    {
        return crate::telemetry::ConfigShape::CustomRules;
    }
    if loaded_user_config {
        return crate::telemetry::ConfigShape::CustomConfig;
    }
    crate::telemetry::ConfigShape::Default
}

fn partial_rules_config_has_values(rules: &PartialRulesConfig) -> bool {
    serde_json::to_value(rules)
        .ok()
        .and_then(|value| value.as_object().map(|object| !object.is_empty()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_flags_read_the_explicit_file_and_keep_the_discovered_file_apart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir(root.join(".git")).expect("git marker");
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"entry":["discovered.ts"]}"#,
        )
        .expect("discovered config");
        let explicit = root.join("ci.json");
        std::fs::write(&explicit, r#"{"entry":["explicit.ts"]}"#).expect("explicit config");
        let flags = ConfigFlags {
            path: Some(&explicit),
            allow_remote_extends: true,
        };

        assert!(flags.file_options().allow_remote_extends);
        assert_eq!(
            flags.explicit(root).expect("explicit").entry,
            ["explicit.ts"]
        );
        assert_eq!(
            flags.discovered(root).expect("discovered").entry,
            ["discovered.ts"]
        );
        assert!(ConfigFlags::default().explicit(root).is_none());
    }

    #[test]
    fn find_unknown_security_categories_flags_typos_with_suggestion() {
        let security = fallow_config::SecurityConfig {
            categories: Some(fallow_config::SecurityCategories {
                include: Some(vec!["sql-injection".to_owned(), "sql-injektion".to_owned()]),
                exclude: Some(vec!["hardcoded-secret".to_owned()]),
            }),
            ..fallow_config::SecurityConfig::default()
        };
        let unknown = find_unknown_security_categories(&security);
        assert_eq!(unknown.len(), 1, "only the typo is unknown");
        assert_eq!(unknown[0].id, "sql-injektion");
        assert_eq!(unknown[0].field, "include");
        assert_eq!(unknown[0].suggestion.as_deref(), Some("sql-injection"));
    }

    #[test]
    fn find_unknown_security_categories_empty_when_all_valid_or_unset() {
        assert!(
            find_unknown_security_categories(&fallow_config::SecurityConfig::default()).is_empty()
        );
        let security = fallow_config::SecurityConfig {
            categories: Some(fallow_config::SecurityCategories {
                include: Some(vec!["secret-to-network".to_owned()]),
                exclude: None,
            }),
            ..fallow_config::SecurityConfig::default()
        };
        assert!(find_unknown_security_categories(&security).is_empty());
    }

    /// The notice promises a route, so pin the exact sentence in both
    /// spellings: `fallow workspaces` prints the per-entry block this line
    /// summarises, and a rename of that command must fail here rather than
    /// leave the warning pointing at nothing.
    ///
    /// The count is the only part that grows, so these two strings are also
    /// the narrowest and widest plausible renderings: 66 columns at one
    /// diagnostic, 70 once a monorepo drives the count to four digits. That
    /// leaves ten columns of headroom under the eighty-column terminal
    /// budget, so pinning the strings byte for byte is the width guard as
    /// well; a rewording that spends the headroom fails here.
    #[test]
    fn workspace_diagnostics_notice_pins_wording_and_route() {
        assert_eq!(
            workspace_diagnostics_notice(1),
            "fallow: 1 workspace discovery diagnostic. Run `fallow workspaces`."
        );
        assert_eq!(
            workspace_diagnostics_notice(1234),
            "fallow: 1234 workspace discovery diagnostics. Run `fallow workspaces`."
        );
    }

    #[test]
    fn config_load_error_message_names_only_an_explicit_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let path = dir.path().join(".fallowrc.json");
        std::fs::write(&path, r#"{ "production": "yes" }"#).unwrap();

        let cause = FallowConfig::load(&path).expect_err("wrong type");
        let explicit = fallow_engine::project_config::load_config_file(
            dir.path(),
            Some(&path),
            ConfigFileOptions::default(),
        )
        .expect_err("wrong type");
        assert_eq!(
            config_load_error_message(&explicit),
            format!("failed to load config '{}': {cause}", path.display())
        );

        let cause = FallowConfig::find_and_load(dir.path()).expect_err("wrong type");
        let discovered = fallow_engine::project_config::load_config_file(
            dir.path(),
            None,
            ConfigFileOptions::default(),
        )
        .expect_err("wrong type");
        assert_eq!(config_load_error_message(&discovered), cause);
    }

    #[test]
    fn config_loaded_notice_dedupes_by_config_path() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.fallow.json");
        let second = dir.path().join("second.fallow.json");
        std::fs::write(&first, "{}").unwrap();
        std::fs::write(&second, "{}").unwrap();

        assert!(should_log_config_loaded(&first));
        assert!(!should_log_config_loaded(&first));
        assert!(should_log_config_loaded(&second));
    }
}
