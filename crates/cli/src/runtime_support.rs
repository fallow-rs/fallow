use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{LazyLock, Mutex, OnceLock};

use fallow_config::{
    FallowConfig, OutputFormat, PartialRulesConfig, ProductionAnalysis, ResolvedConfig, RulesConfig,
};
use rustc_hash::FxHashSet;

static CONFIG_LOADED_LOGGED: LazyLock<Mutex<FxHashSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(FxHashSet::default()));

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

/// Resolve the effective per-file size ceiling override (in megabytes): the
/// `--max-file-size` flag wins, then `FALLOW_MAX_FILE_SIZE`, else `None` (the
/// built-in default applies). `Some(0)` from either source means unlimited.
fn resolve_max_file_size_mb() -> Option<u32> {
    if let Some(Some(mb)) = MAX_FILE_SIZE_OVERRIDE.get() {
        return Some(*mb);
    }
    std::env::var("FALLOW_MAX_FILE_SIZE")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
}

/// Analysis types for --only/--skip selection.
#[derive(Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum AnalysisKind {
    #[value(alias = "check")]
    DeadCode,
    Dupes,
    Health,
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

/// Build an `OwnershipResolver` from CLI `--group-by` and config settings.
///
/// Returns `None` when no grouping is requested. Returns `Err(ExitCode)` when
/// `--group-by owner` is requested but no CODEOWNERS file can be found.
pub fn build_ownership_resolver(
    group_by: Option<GroupBy>,
    root: &Path,
    codeowners_path: Option<&str>,
    output: OutputFormat,
) -> Result<Option<crate::report::OwnershipResolver>, ExitCode> {
    let Some(mode) = group_by else {
        return Ok(None);
    };
    match mode {
        GroupBy::Owner => match crate::codeowners::CodeOwners::load(root, codeowners_path) {
            Ok(co) => Ok(Some(crate::report::OwnershipResolver::Owner(co))),
            Err(e) => Err(crate::error::emit_error(&e, 2, output)),
        },
        GroupBy::Section => match crate::codeowners::CodeOwners::load(root, codeowners_path) {
            Ok(co) => {
                if co.has_sections() {
                    Ok(Some(crate::report::OwnershipResolver::Section(co)))
                } else {
                    Err(crate::error::emit_error(
                        "--group-by section requires a GitLab-style CODEOWNERS file \
                         with `[Section]` headers. This CODEOWNERS has no sections; \
                         use --group-by owner instead.",
                        2,
                        output,
                    ))
                }
            }
            Err(e) => Err(crate::error::emit_error(&e, 2, output)),
        },
        GroupBy::Directory => Ok(Some(crate::report::OwnershipResolver::Directory)),
        GroupBy::Package => {
            let workspaces = fallow_config::discover_workspaces(root);
            if workspaces.is_empty() {
                Err(crate::error::emit_error(
                    "--group-by package requires a monorepo with workspace packages \
                     (package.json workspaces, pnpm-workspace.yaml, or tsconfig references). \
                     For single-package projects try --group-by directory instead.",
                    2,
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
}

#[expect(clippy::ref_option, reason = "&Option matches clap's field type")]
pub fn load_config(
    root: &Path,
    config_path: &Option<PathBuf>,
    output: OutputFormat,
    no_cache: bool,
    threads: usize,
    production: bool,
    quiet: bool,
) -> Result<ResolvedConfig, ExitCode> {
    load_config_for_analysis(
        root,
        config_path,
        ConfigLoadOptions {
            output,
            no_cache,
            threads,
            production_override: production.then_some(true),
            quiet,
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
    let user_config = if let Some(path) = config_path {
        match FallowConfig::load(path) {
            Ok(c) => {
                log_config_loaded(path, options.output, options.quiet);
                Some(c)
            }
            Err(e) => {
                let msg = format!("failed to load config '{}': {e}", path.display());
                return Err(crate::error::emit_error(&msg, 2, options.output));
            }
        }
    } else {
        match FallowConfig::find_and_load(root) {
            Ok(Some((config, found_path))) => {
                log_config_loaded(&found_path, options.output, options.quiet);
                Some(config)
            }
            Ok(None) => None,
            Err(e) => {
                return Err(crate::error::emit_error(&e, 2, options.output));
            }
        }
    };

    let loaded_user_config = user_config.is_some();
    let final_config = match user_config {
        Some(mut config) => {
            let production = options
                .production_override
                .unwrap_or_else(|| config.production.for_analysis(analysis));
            config.production = production.into();
            config
        }
        None => FallowConfig {
            production: options.production_override.unwrap_or(false).into(),
            ..FallowConfig::default()
        },
    };
    crate::telemetry::note_config_shape(config_shape_for(&final_config, loaded_user_config));

    if let Err(errors) =
        fallow_config::discover_and_validate_external_plugins(root, &final_config.plugins)
    {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        let msg = format!("invalid external plugin definition:\n  - {joined}");
        return Err(crate::error::emit_error(&msg, 2, options.output));
    }

    if let Err(errors) = final_config.validate_resolved_boundaries(root) {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        let msg = format!("invalid boundary configuration:\n  - {joined}");
        return Err(crate::error::emit_error(&msg, 2, options.output));
    }

    // A pack that fails to load must fail the run: silently skipping policy
    // is the exact failure mode rule packs document themselves as preventing.
    if let Err(errors) = fallow_config::load_rule_packs(root, &final_config.rule_packs) {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        let msg = format!("invalid rule pack:\n  - {joined}");
        return Err(crate::error::emit_error(&msg, 2, options.output));
    }

    let cache_max_size_mb = resolve_cache_max_size_env();
    let mut resolved = final_config.resolve(
        root.to_path_buf(),
        options.output,
        options.threads,
        options.no_cache,
        options.quiet,
        cache_max_size_mb,
    );
    if let Some(mb) = resolve_max_file_size_mb() {
        resolved.max_file_size_bytes = fallow_config::resolve_max_file_size_bytes(Some(mb));
    }
    apply_cache_dir_env_override(root, &mut resolved, resolve_cache_dir_env());
    crate::cache_notice::record_candidate(
        root,
        &resolved.cache_dir,
        options.output,
        options.quiet,
        resolved.no_cache,
    );

    match fallow_config::discover_workspaces_with_diagnostics(root, &resolved.ignore_patterns) {
        Ok((_, diagnostics)) => {
            fallow_config::stash_workspace_diagnostics(root, diagnostics.clone());
            if !diagnostics.is_empty()
                && matches!(options.output, OutputFormat::Human)
                && !options.quiet
            {
                eprintln!(
                    "fallow: {} workspace discovery diagnostic{}. \
                     Run `fallow list --workspaces` for detail.",
                    diagnostics.len(),
                    if diagnostics.len() == 1 { "" } else { "s" }
                );
            }
        }
        Err(err) => {
            return Err(crate::error::emit_error(
                &err.to_string(),
                2,
                options.output,
            ));
        }
    }

    Ok(resolved)
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

/// Read the workspace-discovery diagnostics produced by the most recent
/// `load_config_for_analysis` call for `root`. Thin re-export over
/// [`fallow_config::workspace_diagnostics_for`] so call sites inside the
/// CLI crate (`report::json::build_json*`) keep a stable module-local path.
#[must_use]
pub fn workspace_diagnostics_for(root: &Path) -> Vec<fallow_config::WorkspaceDiagnostic> {
    fallow_config::workspace_diagnostics_for(root)
}

/// Read `FALLOW_CACHE_MAX_SIZE` (megabytes) into `Option<u32>`, returning
/// `None` when the env var is unset or fails to parse as a positive integer.
/// Resolved here rather than as a clap flag because the cache cap is a
/// platform/CI ergonomic concern, not an analysis input; an env var keeps
/// it out of the `--help` surface (see ADR-009).
fn resolve_cache_max_size_env() -> Option<u32> {
    std::env::var("FALLOW_CACHE_MAX_SIZE")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .filter(|mb| *mb > 0)
}

/// Read `FALLOW_CACHE_DIR` into an optional project-root-resolved cache path.
/// Relative values use the same project-root base as `cache.dir`.
fn resolve_cache_dir_env() -> Option<PathBuf> {
    std::env::var_os("FALLOW_CACHE_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn resolve_cache_dir_value(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn apply_cache_dir_env_override(
    root: &Path,
    resolved: &mut ResolvedConfig,
    env_value: Option<PathBuf>,
) {
    if let Some(path) = env_value {
        resolved.cache_dir = resolve_cache_dir_value(root, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn cache_dir_env_value_resolves_relative_to_project_root() {
        assert_eq!(
            resolve_cache_dir_value(Path::new("/repo"), PathBuf::from(".cache/fallow")),
            PathBuf::from("/repo/.cache/fallow")
        );
        assert_eq!(
            resolve_cache_dir_value(Path::new("/repo"), PathBuf::from("/tmp/fallow-cache")),
            PathBuf::from("/tmp/fallow-cache")
        );
    }

    #[test]
    fn cache_dir_env_value_wins_over_configured_cache_dir() {
        let mut resolved = FallowConfig {
            cache: fallow_config::CacheConfig {
                dir: Some(PathBuf::from(".cache/from-config")),
                ..Default::default()
            },
            ..Default::default()
        }
        .resolve(
            PathBuf::from("/repo"),
            OutputFormat::Human,
            1,
            false,
            true,
            None,
        );

        apply_cache_dir_env_override(
            Path::new("/repo"),
            &mut resolved,
            Some(PathBuf::from(".cache/from-env")),
        );

        assert_eq!(resolved.cache_dir, PathBuf::from("/repo/.cache/from-env"));
    }
}
