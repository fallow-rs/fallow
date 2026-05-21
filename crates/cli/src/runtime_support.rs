use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat, ProductionAnalysis, ResolvedConfig};

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
    eprintln!("loaded config: {}", path.display());
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
        output,
        no_cache,
        threads,
        production.then_some(true),
        quiet,
        ProductionAnalysis::DeadCode,
    )
}

#[expect(clippy::ref_option, reason = "&Option matches clap's field type")]
#[expect(
    clippy::too_many_arguments,
    reason = "central config loader mirrors CLI dispatch options"
)]
pub fn load_config_for_analysis(
    root: &Path,
    config_path: &Option<PathBuf>,
    output: OutputFormat,
    no_cache: bool,
    threads: usize,
    production_override: Option<bool>,
    quiet: bool,
    analysis: ProductionAnalysis,
) -> Result<ResolvedConfig, ExitCode> {
    let user_config = if let Some(path) = config_path {
        match FallowConfig::load(path) {
            Ok(c) => {
                log_config_loaded(path, output, quiet);
                Some(c)
            }
            Err(e) => {
                let msg = format!("failed to load config '{}': {e}", path.display());
                return Err(crate::error::emit_error(&msg, 2, output));
            }
        }
    } else {
        match FallowConfig::find_and_load(root) {
            Ok(Some((config, found_path))) => {
                log_config_loaded(&found_path, output, quiet);
                Some(config)
            }
            Ok(None) => None,
            Err(e) => {
                return Err(crate::error::emit_error(&e, 2, output));
            }
        }
    };

    let final_config = match user_config {
        Some(mut config) => {
            let production =
                production_override.unwrap_or_else(|| config.production.for_analysis(analysis));
            config.production = production.into();
            config
        }
        None => FallowConfig {
            production: production_override.unwrap_or(false).into(),
            ..FallowConfig::default()
        },
    };

    // Issue #463: validate user-supplied glob patterns on EXTERNAL plugin files
    // loaded from `.fallow/plugins/` / `fallow-plugin-*` / config-listed paths.
    // Inline `framework[]` blocks are already validated by `FallowConfig::load`.
    // The external-plugin step runs here because plugins are root-dependent and
    // `load` does not know the project root.
    if let Err(errors) =
        fallow_config::discover_and_validate_external_plugins(root, &final_config.plugins)
    {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        let msg = format!("invalid external plugin definition:\n  - {joined}");
        return Err(crate::error::emit_error(&msg, 2, output));
    }

    // Issue #468: validate boundary zone references and root-prefix conflicts
    // AFTER preset and auto-discover expansion. Mirrors the upstream
    // `discover_and_validate_external_plugins` pattern: both checks need the
    // project root, both surface every offending entry in one rendered run.
    if let Err(errors) = final_config.validate_resolved_boundaries(root) {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        let msg = format!("invalid boundary configuration:\n  - {joined}");
        return Err(crate::error::emit_error(&msg, 2, output));
    }

    let cache_max_size_mb = resolve_cache_max_size_env();
    let resolved = final_config.resolve(
        root.to_path_buf(),
        output,
        threads,
        no_cache,
        quiet,
        cache_max_size_mb,
    );

    // Issue #473: discover workspaces here so any silent-fail in
    // crates/config/src/workspace/ surfaces with a typed diagnostic (and a
    // tracing::warn! per (root, kind, path)). A malformed ROOT package.json
    // is unrecoverable; promote to exit 2 to match the boundary-validation
    // exit-code policy above. The diagnostics that come back from this call
    // stay in a process-wide registry keyed by canonical root so downstream
    // renderers (check.rs, audit.rs, combined.rs, list.rs) can fold them
    // into their JSON envelope and stderr summary without re-walking the
    // workspace tree.
    match fallow_config::discover_workspaces_with_diagnostics(root, &resolved.ignore_patterns) {
        Ok((_, diagnostics)) => {
            // Stash diagnostics so downstream JSON-envelope builders
            // (`report::json::build_json*`, audit, combined) and the analyze
            // pipeline's later `find_undeclared_workspaces_with_ignores`
            // pass can fold their results into the same registry without
            // re-walking the workspace tree. The registry lives in
            // `fallow-config` so both crates can populate it without a
            // cyclic dep.
            fallow_config::stash_workspace_diagnostics(root, diagnostics.clone());
            if !diagnostics.is_empty() && matches!(output, OutputFormat::Human) && !quiet {
                eprintln!(
                    "fallow: {} workspace discovery diagnostic{}. \
                     Run `fallow list --workspaces` for detail.",
                    diagnostics.len(),
                    if diagnostics.len() == 1 { "" } else { "s" }
                );
            }
        }
        Err(err) => {
            return Err(crate::error::emit_error(&err.to_string(), 2, output));
        }
    }

    Ok(resolved)
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
