//! Read-only project readiness inspection.

use std::path::{Path, PathBuf};

use fallow_engine::project_config::{
    ProjectConfig, ProjectConfigOptions, config_for_project_readiness,
};
use fallow_output::{
    DOCTOR_SCHEMA_VERSION, DoctorCheck, DoctorCheckCategory, DoctorCheckId, DoctorCheckStatus,
    DoctorOutput, DoctorRemediation, DoctorStatus, DoctorSummary,
};
use fallow_types::envelope::{SchemaVersion, ToolVersion};

/// Inputs for a deterministic doctor run.
pub struct DoctorOptions<'a> {
    /// Project root, before canonical validation.
    pub root: &'a Path,
    /// Optional explicit fallow config path.
    pub config_path: Option<&'a Path>,
}

/// Inspect project-local readiness without analysis, cache writes, telemetry,
/// network access, or third-party execution.
#[must_use]
pub fn run_doctor(options: &DoctorOptions<'_>) -> DoctorOutput {
    run_doctor_with_discovery(options, &crate::type_aware::discover_companion)
}

/// Inspect readiness using an explicit cache-directory override.
///
/// The override takes precedence over `FALLOW_CACHE_DIR` and `cache.dir`;
/// relative paths resolve from the validated project root. Empty paths are
/// ignored. [`run_doctor`] already honors `FALLOW_CACHE_DIR`, so use this
/// variant only for a directory that the host selects itself. Inspection
/// never writes caches.
#[must_use]
pub fn run_doctor_with_cache_dir(
    options: &DoctorOptions<'_>,
    cache_dir: Option<&Path>,
) -> DoctorOutput {
    run_doctor_with_cache_dir_and_discovery(
        options,
        cache_dir,
        &crate::type_aware::discover_companion,
    )
}

fn run_doctor_with_discovery<F>(options: &DoctorOptions<'_>, discover_companion: &F) -> DoctorOutput
where
    F: Fn(&Path) -> Result<(), String>,
{
    run_doctor_with_cache_dir_and_discovery(options, None, discover_companion)
}

fn run_doctor_with_cache_dir_and_discovery<F>(
    options: &DoctorOptions<'_>,
    cache_dir: Option<&Path>,
    discover_companion: &F,
) -> DoctorOutput
where
    F: Fn(&Path) -> Result<(), String>,
{
    let mut checks = Vec::with_capacity(7);
    let root = match fallow_engine::validate::validate_root(options.root) {
        Ok(root) => {
            checks.push(check(
                DoctorCheckId::Root,
                DoctorCheckCategory::Project,
                DoctorCheckStatus::Pass,
                true,
                "Project root is an accessible directory.",
                None,
            ));
            root
        }
        Err(_) => {
            checks.push(check(
                DoctorCheckId::Root,
                DoctorCheckCategory::Project,
                DoctorCheckStatus::Fail,
                true,
                "Project root is not accessible. Set --root to an existing, readable directory.",
                None,
            ));
            push_prerequisite_skips(&mut checks, "Project root readiness failed.");
            return build_output(checks);
        }
    };

    let project = config_for_project_readiness(
        &root,
        options.config_path,
        ProjectConfigOptions {
            output: fallow_config::OutputFormat::Json,
            no_cache: true,
            threads: 1,
            production_override: None,
            quiet: true,
            analysis: fallow_config::ProductionAnalysis::DeadCode,
            allow_remote_extends: false,
        },
    );

    match project {
        Ok(mut readiness) => {
            if let Some(path) = cache_dir.filter(|path| !path.as_os_str().is_empty()) {
                readiness
                    .project
                    .config
                    .override_cache_dir(path.to_path_buf());
            }
            push_ready_project_checks(
                &mut checks,
                &root,
                &readiness.project,
                &readiness.configured_plugin_diagnostics,
                discover_companion,
            );
        }
        Err(error) => {
            push_project_failure_checks(&mut checks, error.message(), &root, options.config_path);
        }
    }

    build_output(checks)
}

fn push_ready_project_checks<F>(
    checks: &mut Vec<DoctorCheck>,
    root: &Path,
    project: &ProjectConfig,
    configured_plugin_diagnostics: &[fallow_config::ConfiguredPluginDiagnostic],
    discover_companion: &F,
) where
    F: Fn(&Path) -> Result<(), String>,
{
    let config_message = project.path.as_ref().map_or_else(
        || "Zero-config defaults resolved successfully.".to_string(),
        |path| match safe_config_argument(root, path) {
            Some(path) => format!("Configuration resolved from {path}."),
            None => "The explicitly selected configuration resolved successfully.".to_string(),
        },
    );
    checks.push(check(
        DoctorCheckId::Config,
        DoctorCheckCategory::Configuration,
        DoctorCheckStatus::Pass,
        true,
        config_message,
        None,
    ));

    let workspace_status = if project.workspace_diagnostics.is_empty() {
        DoctorCheckStatus::Pass
    } else {
        DoctorCheckStatus::Warn
    };
    let workspace_count = project.workspaces.len();
    let workspace_noun = if workspace_count == 1 {
        "workspace package"
    } else {
        "workspace packages"
    };
    let mut workspace_message = if project.workspace_diagnostics.is_empty() {
        format!("Workspace discovery completed ({workspace_count} {workspace_noun}).")
    } else {
        let diagnostic_count = project.workspace_diagnostics.len();
        let diagnostic_noun = if diagnostic_count == 1 {
            "diagnostic"
        } else {
            "diagnostics"
        };
        format!(
            "Workspace discovery completed with {diagnostic_count} {diagnostic_noun}; {workspace_count} {workspace_noun} retained."
        )
    };
    if workspace_status == DoctorCheckStatus::Warn {
        append_external_config_note(&mut workspace_message, root, project.path.as_deref());
    }
    checks.push(check(
        DoctorCheckId::Workspaces,
        DoctorCheckCategory::Workspace,
        workspace_status,
        false,
        workspace_message,
        (workspace_status == DoctorCheckStatus::Warn)
            .then(|| {
                remediation_with_config(
                    "fallow workspaces --format json --quiet",
                    root,
                    project.path.as_deref(),
                )
            })
            .flatten(),
    ));

    checks.push(plugin_check(root, project, configured_plugin_diagnostics));

    checks.push(type_aware_check(
        root,
        &project.config.type_aware,
        discover_companion,
    ));

    checks.push(dependencies_check(root));
    checks.push(cache_check(&project.config));
    checks.push(graph_cache_check(&project.config));
}

/// Report whether the project has an installed dependency tree.
///
/// Advisory, never required: analysis runs without `node_modules`, it just
/// runs blind to package `exports`, to plugins that activate on an installed
/// package, and to a dependency's installed shape. Doctor used to report
/// `pass` on a tree that had never been installed, which is the one state this
/// command exists to catch.
fn dependencies_check(root: &Path) -> DoctorCheck {
    if !fallow_config::node_modules_missing(root) {
        return check(
            DoctorCheckId::Dependencies,
            DoctorCheckCategory::Project,
            DoctorCheckStatus::Pass,
            false,
            "Dependencies are installed, or the project runs without a node_modules directory.",
            None,
        );
    }
    check(
        DoctorCheckId::Dependencies,
        DoctorCheckCategory::Project,
        DoctorCheckStatus::Warn,
        false,
        "No node_modules directory. Package exports, plugin activation, and dependency \
         classification degrade until dependencies are installed.",
        Some(remediation("npm install", true)),
    )
}

/// Report whether a persisted extraction cache would be reused.
///
/// Advisory, never required, and never a `fail`: a refused cache costs time,
/// not correctness. A missing cache passes, because a first run legitimately
/// has none; a cache that exists and would be discarded warns, because the
/// project is paying for a blob it never gets back.
fn cache_check(config: &fallow_config::ResolvedConfig) -> DoctorCheck {
    let status = fallow_engine::cache_status::inspect_parse_cache(config);
    let size = status
        .size_bytes
        .map_or_else(String::new, |bytes| format!(" ({})", format_size_mb(bytes)));
    match status.rejection {
        None => check(
            DoctorCheckId::Cache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Pass,
            false,
            format!("Extraction cache is reusable{size}."),
            None,
        ),
        Some(fallow_types::cache_rejection::CacheRejection::Absent) => check(
            DoctorCheckId::Cache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Pass,
            false,
            "No extraction cache yet; the next run writes one.",
            None,
        ),
        Some(rejection) => check(
            DoctorCheckId::Cache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Warn,
            false,
            format!(
                "Extraction cache{size} would not be reused: {}. The next run parses every file.",
                rejection.describe()
            ),
            Some(remediation("fallow dead-code --quiet", false)),
        ),
    }
}

/// Report whether the persisted module graph would load.
///
/// Advisory for the same reason as the extraction-cache check, and reported
/// separately because the two blobs are reused independently: a project whose
/// extraction cache is perfectly healthy can still rebuild the whole graph on
/// every run, and the graph is the larger file of the two.
///
/// This answers whether the blob LOADS, not whether a run would reuse it. The
/// reuse decision also compares resolver options, entry points, and per-file
/// content hashes, and computing those means running discovery and extraction,
/// which doctor deliberately does not do.
fn graph_cache_check(config: &fallow_config::ResolvedConfig) -> DoctorCheck {
    let status = fallow_engine::cache_status::inspect_graph_cache(config);
    let size = status
        .size_bytes
        .map_or_else(String::new, |bytes| format!(" ({})", format_size_mb(bytes)));
    match status.rejection {
        None => check(
            DoctorCheckId::GraphCache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Pass,
            false,
            format!(
                "Module-graph cache{size} loads; a run reuses it when the analysed files and \
                 options are unchanged."
            ),
            None,
        ),
        Some(fallow_types::cache_rejection::CacheRejection::Absent) => check(
            DoctorCheckId::GraphCache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Pass,
            false,
            "No module-graph cache yet; the next run writes one.",
            None,
        ),
        Some(rejection) => check(
            DoctorCheckId::GraphCache,
            DoctorCheckCategory::Cache,
            DoctorCheckStatus::Warn,
            false,
            format!(
                "Module-graph cache{size} would not be reused: {}. The next run resolves imports \
                 and rebuilds the graph.",
                rejection.describe()
            ),
            Some(remediation("fallow dead-code --quiet", false)),
        ),
    }
}

/// Render a byte count at a unit that shows it.
///
/// A fixed megabyte figure reported every small blob as `0.0 MB`, which reads
/// as "empty" next to a message about a cache that exists and was refused: a
/// corrupt 4 KB file and a truncated 40-byte one printed the same size.
fn format_size_mb(bytes: u64) -> String {
    #[expect(
        clippy::cast_precision_loss,
        reason = "display-only size figure; precision loss past 2^53 bytes is irrelevant"
    )]
    let scaled = bytes as f64;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", scaled / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", scaled / 1024.0)
    } else {
        format!("{bytes} bytes")
    }
}

fn plugin_check(
    root: &Path,
    project: &ProjectConfig,
    configured_plugin_diagnostics: &[fallow_config::ConfiguredPluginDiagnostic],
) -> DoctorCheck {
    if !configured_plugin_diagnostics.is_empty() {
        let diagnostic_count = configured_plugin_diagnostics.len();
        let resource_noun = if diagnostic_count == 1 {
            "resource"
        } else {
            "resources"
        };
        let mut message = format!(
            "External plugin configuration contains {diagnostic_count} unresolved configured {resource_noun}."
        );
        append_external_config_note(&mut message, root, project.path.as_deref());
        return check(
            DoctorCheckId::Plugins,
            DoctorCheckCategory::Plugin,
            DoctorCheckStatus::Fail,
            true,
            message,
            remediation_with_config(
                "fallow plugin-check --format json --quiet",
                root,
                project.path.as_deref(),
            ),
        );
    }

    let configured = &project.config.external_plugins;
    if configured.is_empty() {
        return check(
            DoctorCheckId::Plugins,
            DoctorCheckCategory::Plugin,
            DoctorCheckStatus::Pass,
            true,
            "No external plugins are configured; built-in detection remains available.",
            None,
        );
    }

    let active = configured
        .iter()
        .filter(|plugin| external_plugin_is_active(plugin, root, &project.workspaces))
        .count();
    let configured_count = configured.len();
    let status = if active == configured_count {
        DoctorCheckStatus::Pass
    } else {
        DoctorCheckStatus::Warn
    };
    let mut message = format!(
        "External plugin activation evaluated ({active} active of {configured_count} configured)."
    );
    if status == DoctorCheckStatus::Warn {
        append_external_config_note(&mut message, root, project.path.as_deref());
    }
    check(
        DoctorCheckId::Plugins,
        DoctorCheckCategory::Plugin,
        status,
        false,
        message,
        (status == DoctorCheckStatus::Warn)
            .then(|| {
                remediation_with_config(
                    "fallow plugin-check --format json --quiet",
                    root,
                    project.path.as_deref(),
                )
            })
            .flatten(),
    )
}

fn external_plugin_is_active(
    plugin: &fallow_config::ExternalPluginDef,
    root: &Path,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> bool {
    std::iter::once(root)
        .chain(workspaces.iter().map(|workspace| workspace.root.as_path()))
        .any(|package_root| {
            let Some(package) = fallow_config::load_dir_package_json(package_root) else {
                return false;
            };
            fallow_engine::plugins::is_external_plugin_active(
                plugin,
                &package.all_dependency_names(),
                package_root,
                &[],
            )
        })
}

fn push_project_failure_checks(
    checks: &mut Vec<DoctorCheck>,
    error: &str,
    root: &Path,
    config_path: Option<&Path>,
) {
    if error.starts_with("invalid external plugin definition") {
        checks.push(check(
            DoctorCheckId::Config,
            DoctorCheckCategory::Configuration,
            DoctorCheckStatus::Pass,
            true,
            "Configuration parsed, but external plugin validation failed.",
            None,
        ));
        checks.push(skipped(
            DoctorCheckId::Workspaces,
            DoctorCheckCategory::Workspace,
            "Plugin validation failed before workspace discovery.",
        ));
        let mut message = "External plugin configuration is invalid.".to_string();
        append_external_config_note(&mut message, root, config_path);
        checks.push(check(
            DoctorCheckId::Plugins,
            DoctorCheckCategory::Plugin,
            DoctorCheckStatus::Fail,
            true,
            message,
            remediation_with_config(
                "fallow plugin-check --format json --quiet",
                root,
                config_path,
            ),
        ));
    } else if error.starts_with("root package.json") || error.starts_with("root Deno config") {
        checks.push(check(
            DoctorCheckId::Config,
            DoctorCheckCategory::Configuration,
            DoctorCheckStatus::Pass,
            true,
            "Fallow configuration resolved successfully.",
            None,
        ));
        let mut message = "Root workspace manifest discovery failed.".to_string();
        append_external_config_note(&mut message, root, config_path);
        checks.push(check(
            DoctorCheckId::Workspaces,
            DoctorCheckCategory::Workspace,
            DoctorCheckStatus::Fail,
            true,
            message,
            remediation_with_config("fallow workspaces --format json --quiet", root, config_path),
        ));
        checks.push(skipped(
            DoctorCheckId::Plugins,
            DoctorCheckCategory::Plugin,
            "Workspace discovery failed before readiness collection completed.",
        ));
    } else {
        let mut message = "Fallow configuration could not be resolved.".to_string();
        append_external_config_note(&mut message, root, config_path);
        checks.push(check(
            DoctorCheckId::Config,
            DoctorCheckCategory::Configuration,
            DoctorCheckStatus::Fail,
            true,
            message,
            remediation_with_config("fallow config", root, config_path),
        ));
        checks.push(skipped(
            DoctorCheckId::Workspaces,
            DoctorCheckCategory::Workspace,
            "Configuration readiness failed.",
        ));
        checks.push(skipped(
            DoctorCheckId::Plugins,
            DoctorCheckCategory::Plugin,
            "Configuration readiness failed.",
        ));
    }
    checks.push(skipped(
        DoctorCheckId::TypeAware,
        DoctorCheckCategory::Companion,
        "Configuration readiness did not establish whether type-aware analysis is enabled.",
    ));
    checks.push(dependencies_check(root));
    checks.push(skipped(
        DoctorCheckId::Cache,
        DoctorCheckCategory::Cache,
        "Configuration readiness did not establish which cache this project uses.",
    ));
    checks.push(skipped(
        DoctorCheckId::GraphCache,
        DoctorCheckCategory::Cache,
        "Configuration readiness did not establish which cache this project uses.",
    ));
}

fn type_aware_check<F>(
    root: &Path,
    config: &fallow_config::TypeAwareConfig,
    discover_companion: &F,
) -> DoctorCheck
where
    F: Fn(&Path) -> Result<(), String>,
{
    let (enabled, require) = match effective_type_aware_config(config) {
        Ok(effective) => effective,
        Err(message) => {
            return check(
                DoctorCheckId::TypeAware,
                DoctorCheckCategory::Companion,
                DoctorCheckStatus::Fail,
                true,
                message,
                None,
            );
        }
    };
    if !enabled {
        return skipped(
            DoctorCheckId::TypeAware,
            DoctorCheckCategory::Companion,
            "Type-aware analysis is not enabled.",
        );
    }

    match discover_companion(root) {
        Ok(()) => check(
            DoctorCheckId::TypeAware,
            DoctorCheckCategory::Companion,
            DoctorCheckStatus::Pass,
            require == fallow_config::TypeAwareRequire::Complete,
            "A trusted type-aware companion is discoverable without starting it.",
            None,
        ),
        Err(_) => {
            let required = require == fallow_config::TypeAwareRequire::Complete;
            check(
                DoctorCheckId::TypeAware,
                DoctorCheckCategory::Companion,
                if required {
                    DoctorCheckStatus::Fail
                } else {
                    DoctorCheckStatus::Warn
                },
                required,
                "Type-aware analysis is enabled, but no trusted companion is discoverable.",
                Some(remediation(
                    &format!(
                        "npm install --save-dev fallow-type-aware@{}",
                        env!("CARGO_PKG_VERSION")
                    ),
                    true,
                )),
            )
        }
    }
}

fn effective_type_aware_config(
    config: &fallow_config::TypeAwareConfig,
) -> Result<(bool, fallow_config::TypeAwareRequire), &'static str> {
    let enabled = match std::env::var("FALLOW_TYPE_AWARE") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => return Err("FALLOW_TYPE_AWARE must contain a supported boolean value."),
        },
        Err(std::env::VarError::NotPresent) => config.enabled,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("FALLOW_TYPE_AWARE must contain valid UTF-8.");
        }
    };
    let require = match std::env::var("FALLOW_TYPE_AWARE_REQUIRE") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "best-effort" => fallow_config::TypeAwareRequire::BestEffort,
            "complete" => fallow_config::TypeAwareRequire::Complete,
            _ => return Err("FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete."),
        },
        Err(std::env::VarError::NotPresent) => config.require,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("FALLOW_TYPE_AWARE_REQUIRE must contain valid UTF-8.");
        }
    };
    Ok((enabled, require))
}

fn remediation(command: &str, mutating: bool) -> DoctorRemediation {
    DoctorRemediation {
        command: command.to_string(),
        cwd: ".".to_string(),
        mutating,
    }
}

fn remediation_with_config(
    command: &str,
    root: &Path,
    config_path: Option<&Path>,
) -> Option<DoctorRemediation> {
    let command = match config_path {
        None => command.to_string(),
        Some(path) => format!("{command} --config={}", safe_config_argument(root, path)?),
    };
    Some(remediation(&command, false))
}

fn safe_config_argument(root: &Path, config_path: &Path) -> Option<String> {
    let canonical_root = dunce::canonicalize(root).ok()?;
    let canonical = canonicalize_with_missing_suffix(config_path)?;
    let relative = relative_path(&canonical_root, &canonical)?;
    relative
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/'))
        .then_some(relative)
}

fn canonicalize_with_missing_suffix(path: &Path) -> Option<PathBuf> {
    let mut ancestor = path;
    let mut suffix = Vec::new();

    loop {
        match dunce::canonicalize(ancestor) {
            Ok(mut canonical) => {
                if !suffix.is_empty() && !canonical.is_dir() {
                    return None;
                }
                for component in suffix.into_iter().rev() {
                    canonical.push(component);
                }
                return Some(canonical);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::symlink_metadata(ancestor) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return None,
                }
                let std::path::Component::Normal(component) = ancestor.components().next_back()?
                else {
                    return None;
                };
                suffix.push(component.to_os_string());
                ancestor = ancestor.parent()?;
            }
            Err(_) => return None,
        }
    }
}

fn append_external_config_note(message: &mut String, root: &Path, config_path: Option<&Path>) {
    if config_path.is_some_and(|path| safe_config_argument(root, path).is_none()) {
        message.push_str(" Repeat this diagnostic with the same explicit --config value.");
    }
}

fn push_prerequisite_skips(checks: &mut Vec<DoctorCheck>, message: &str) {
    for (id, category) in [
        (DoctorCheckId::Config, DoctorCheckCategory::Configuration),
        (DoctorCheckId::Workspaces, DoctorCheckCategory::Workspace),
        (DoctorCheckId::Plugins, DoctorCheckCategory::Plugin),
        (DoctorCheckId::TypeAware, DoctorCheckCategory::Companion),
        (DoctorCheckId::Dependencies, DoctorCheckCategory::Project),
        (DoctorCheckId::Cache, DoctorCheckCategory::Cache),
        (DoctorCheckId::GraphCache, DoctorCheckCategory::Cache),
    ] {
        checks.push(skipped(id, category, message));
    }
}

fn skipped(id: DoctorCheckId, category: DoctorCheckCategory, message: &str) -> DoctorCheck {
    check(
        id,
        category,
        DoctorCheckStatus::Skipped,
        false,
        message,
        None,
    )
}

fn check(
    id: DoctorCheckId,
    category: DoctorCheckCategory,
    status: DoctorCheckStatus,
    required: bool,
    message: impl Into<String>,
    remediation: Option<DoctorRemediation>,
) -> DoctorCheck {
    DoctorCheck {
        id,
        category,
        status,
        required,
        message: message.into(),
        remediation,
    }
}

fn build_output(checks: Vec<DoctorCheck>) -> DoctorOutput {
    let summary = checks
        .iter()
        .fold(DoctorSummary::default(), |mut summary, check| {
            match check.status {
                DoctorCheckStatus::Pass => summary.pass += 1,
                DoctorCheckStatus::Warn => summary.warn += 1,
                DoctorCheckStatus::Fail => summary.fail += 1,
                DoctorCheckStatus::Skipped => summary.skipped += 1,
            }
            summary
        });
    let status = if checks
        .iter()
        .any(|check| check.required && check.status == DoctorCheckStatus::Fail)
    {
        DoctorStatus::Fail
    } else if summary.warn > 0 {
        DoctorStatus::Warn
    } else {
        DoctorStatus::Pass
    };
    DoctorOutput {
        schema_version: SchemaVersion(DOCTOR_SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        root: ".".to_string(),
        status,
        summary,
        checks,
    }
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root).ok().map(|path| {
        let relative = path.to_string_lossy().replace('\\', "/");
        if relative.is_empty() {
            ".".to_string()
        } else {
            relative
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_config_is_ready_with_stable_order() {
        let root = tempfile::tempdir().expect("temp root");
        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        assert_eq!(output.status, DoctorStatus::Warn);
        assert_eq!(output.root, ".");
        assert_eq!(
            output
                .checks
                .iter()
                .map(|check| check.id)
                .collect::<Vec<_>>(),
            vec![
                DoctorCheckId::Root,
                DoctorCheckId::Config,
                DoctorCheckId::Workspaces,
                DoctorCheckId::Plugins,
                DoctorCheckId::TypeAware,
                DoctorCheckId::Dependencies,
                DoctorCheckId::Cache,
                DoctorCheckId::GraphCache,
            ]
        );
        assert_eq!(
            output.checks[1].message,
            "Zero-config defaults resolved successfully."
        );
        assert_eq!(output.checks[4].status, DoctorCheckStatus::Skipped);
    }

    /// Issue: doctor reported `pass` on a tree that had never been installed,
    /// while the command exists to diagnose exactly that.
    #[test]
    fn missing_node_modules_warns_without_failing() {
        let root = tempfile::tempdir().expect("temp root");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let dependencies = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Dependencies)
            .expect("dependencies check is reported");
        assert_eq!(dependencies.status, DoctorCheckStatus::Warn);
        assert!(!dependencies.required);
        assert_eq!(output.status, DoctorStatus::Warn);
        assert!(
            dependencies
                .remediation
                .as_ref()
                .is_some_and(|remediation| remediation.mutating),
            "installing dependencies mutates the project"
        );
    }

    #[test]
    fn installed_dependencies_pass_the_dependency_check() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let dependencies = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Dependencies)
            .expect("dependencies check is reported");
        assert_eq!(dependencies.status, DoctorCheckStatus::Pass);
        assert_eq!(output.status, DoctorStatus::Pass);
    }

    /// A project with no cache yet is not a problem; only a cache that exists
    /// and would be thrown away is worth a warning.
    #[test]
    fn absent_cache_passes_the_cache_check() {
        let root = tempfile::tempdir().expect("temp root");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let cache = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Cache)
            .expect("cache check is reported");
        assert_eq!(cache.status, DoctorCheckStatus::Pass);
        assert!(!cache.required);
    }

    /// Framing this build never wrote is corruption, and must not be reported
    /// as a format bump.
    ///
    /// "cache format version changed" sends the reader to look for an upgrade;
    /// the fix for a blob with no fallow framing is to delete it. The upgrade
    /// message has its own test below, on a blob that keeps the framing and
    /// moves only the declared version.
    #[test]
    fn a_corrupt_cache_warns_as_undecodable_with_a_size_that_shows() {
        let root = tempfile::tempdir().expect("temp root");
        let cache_dir = root.path().join(".fallow");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        std::fs::write(
            cache_dir.join("cache.bin"),
            b"not-a-payload-this-build-wrote",
        )
        .expect("write foreign cache");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let cache = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Cache)
            .expect("cache check is reported");
        assert_eq!(cache.status, DoctorCheckStatus::Warn);
        assert!(!cache.required);
        assert!(
            cache.message.contains("could not be decoded"),
            "a blob without fallow's framing is corrupt, not stale: {}",
            cache.message
        );
        assert!(
            !cache.message.contains("cache format version changed"),
            "corruption must not send the reader hunting for an upgrade: {}",
            cache.message
        );
        assert!(
            cache.message.contains("30 bytes"),
            "a small blob must report a size that shows it exists, not 0.0 MB: {}",
            cache.message
        );
        assert_ne!(
            output.status,
            DoctorStatus::Fail,
            "a refused cache costs time, not correctness"
        );
    }

    /// The message a user reads after upgrading. A blob that keeps fallow's
    /// framing and declares a version this build does not write is stale, and
    /// costs exactly one rebuild.
    #[test]
    fn a_cache_from_an_older_format_version_warns_as_a_format_change() {
        let root = tempfile::tempdir().expect("temp root");
        let cache_dir = root.path().join(".fallow");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        // `FLWX` plus a little-endian version, the framing `fallow-extract`
        // writes. Version 1 is far below anything this build produces, so the
        // blob is stale rather than foreign.
        let mut framed = b"FLWX".to_vec();
        framed.extend_from_slice(&1_u32.to_le_bytes());
        framed.extend_from_slice(b"payload-from-an-older-release");
        std::fs::write(cache_dir.join("cache.bin"), framed).expect("write stale cache");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let cache = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::Cache)
            .expect("cache check is reported");
        assert_eq!(cache.status, DoctorCheckStatus::Warn);
        assert!(
            cache.message.contains("cache format version changed"),
            "{}",
            cache.message
        );
        assert!(
            !cache.message.contains("could not be decoded"),
            "an upgrade must not be reported as corruption: {}",
            cache.message
        );
    }

    /// The graph blob is the larger of the two persisted caches and is reused
    /// independently of the extraction blob, so a doctor that only looked at
    /// the extraction cache called a project healthy while the expensive half
    /// was discarded on every run.
    #[test]
    fn absent_graph_cache_passes_its_own_check() {
        let root = tempfile::tempdir().expect("temp root");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let graph_cache = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::GraphCache)
            .expect("graph cache check is reported");
        assert_eq!(graph_cache.status, DoctorCheckStatus::Pass);
        assert!(!graph_cache.required);
    }

    #[test]
    fn a_corrupt_graph_cache_warns_as_undecodable_with_a_size_that_shows() {
        let root = tempfile::tempdir().expect("temp root");
        let cache_dir = root.path().join(".fallow");
        std::fs::create_dir_all(&cache_dir).expect("create cache dir");
        std::fs::write(
            cache_dir.join("graph-cache.bin"),
            b"not-a-payload-this-build-wrote",
        )
        .expect("write foreign graph cache");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        let graph_cache = output
            .checks
            .iter()
            .find(|check| check.id == DoctorCheckId::GraphCache)
            .expect("graph cache check is reported");
        assert_eq!(graph_cache.status, DoctorCheckStatus::Warn);
        assert!(!graph_cache.required);
        assert!(
            graph_cache.message.contains("could not be decoded"),
            "a blob without fallow's framing is corrupt, not stale: {}",
            graph_cache.message
        );
        assert!(
            graph_cache.message.contains("30 bytes"),
            "a small blob must report a size that shows it exists, not 0.0 MB: {}",
            graph_cache.message
        );
        assert_ne!(
            output.status,
            DoctorStatus::Fail,
            "a refused cache costs time, not correctness"
        );
    }

    #[test]
    fn invalid_config_returns_complete_failed_report() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join(".fallowrc.json"), "{").expect("write invalid config");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        // Look checks up by id, not by position: the report's completeness is
        // the contract, the order in which the rows happen to be pushed is not.
        let check = |id: DoctorCheckId| {
            output
                .checks
                .iter()
                .find(|check| check.id == id)
                .unwrap_or_else(|| panic!("{id:?} is reported even when config fails"))
        };

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(
            output.checks.len(),
            8,
            "a failing config must not truncate the report"
        );
        assert_eq!(check(DoctorCheckId::Config).status, DoctorCheckStatus::Fail);
        assert_eq!(
            check(DoctorCheckId::Workspaces).status,
            DoctorCheckStatus::Skipped
        );
        assert!(
            !check(DoctorCheckId::Config)
                .message
                .contains(&root.path().display().to_string()),
            "the failure must not echo the host path"
        );
    }

    #[test]
    fn optional_missing_type_aware_companion_warns() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"typeAware":{"enabled":true}}"#,
        )
        .expect("write config");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        assert_eq!(output.status, DoctorStatus::Warn);
        assert_eq!(output.checks[4].status, DoctorCheckStatus::Warn);
        assert!(!output.checks[4].required);
    }

    #[test]
    fn required_missing_type_aware_companion_fails() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"typeAware":{"enabled":true,"require":"complete"}}"#,
        )
        .expect("write config");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.checks[4].status, DoctorCheckStatus::Fail);
        assert!(output.checks[4].required);
        assert_eq!(
            output.checks[4]
                .remediation
                .as_ref()
                .map(|remediation| remediation.mutating),
            Some(true)
        );
    }

    #[test]
    fn invalid_root_does_not_echo_the_host_path() {
        let missing = Path::new("/definitely/missing/fallow-doctor-private-root");
        let output = run_doctor(&DoctorOptions {
            root: missing,
            config_path: None,
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.root, ".");
        assert!(output.checks[0].message.contains("--root"));
        assert_eq!(
            output.checks[0].message,
            "Project root is not accessible. Set --root to an existing, readable directory."
        );
        assert!(
            output
                .checks
                .iter()
                .all(|check| !check.message.contains("fallow-doctor-private-root"))
        );
    }

    #[test]
    fn external_config_does_not_echo_its_host_path() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
        let config_dir = tempfile::tempdir().expect("temp config dir");
        let config_path = config_dir.path().join("external.fallowrc.json");
        std::fs::write(&config_path, "{}").expect("write config");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Pass);
        assert_eq!(
            output.checks[1].message,
            "The explicitly selected configuration resolved successfully."
        );
        assert!(
            !output.checks[1]
                .message
                .contains(&config_dir.path().display().to_string())
        );
    }

    #[test]
    fn parent_relative_external_config_stays_private() {
        let sandbox = tempfile::tempdir().expect("temp sandbox");
        let root = sandbox.path().join("project");
        std::fs::create_dir(&root).expect("create project root");
        std::fs::create_dir(root.join("node_modules")).expect("create node_modules");
        let config_path = root.join("../customer-secret.json");
        std::fs::write(&config_path, "{}").expect("write config");

        let output = run_doctor(&DoctorOptions {
            root: &root,
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Pass);
        assert_eq!(
            output.checks[1].message,
            "The explicitly selected configuration resolved successfully."
        );
        assert!(!output.checks[1].message.contains("customer-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn successful_config_below_external_symlink_stays_private() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
        let external = tempfile::tempdir().expect("external root");
        std::fs::write(external.path().join("config.json"), "{}").expect("write config");
        std::os::unix::fs::symlink(external.path(), root.path().join("external"))
            .expect("create external symlink");
        let config_path = root.path().join("external/config.json");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Pass);
        assert_eq!(
            output.checks[1].message,
            "The explicitly selected configuration resolved successfully."
        );
        assert!(!output.checks[1].message.contains("external/config.json"));
    }

    #[test]
    fn relative_explicit_config_is_preserved_in_remediation() {
        let root = tempfile::tempdir().expect("temp root");
        let config_path = root.path().join("custom.json");
        std::fs::write(&config_path, "{").expect("write invalid config");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(
            output.checks[1]
                .remediation
                .as_ref()
                .map(|remediation| remediation.command.as_str()),
            Some("fallow config --config=custom.json")
        );
    }

    #[test]
    fn nested_missing_project_relative_config_is_preserved_in_remediation() {
        let root = tempfile::tempdir().expect("temp root");
        let config_path = root.path().join("missing-dir/missing.json");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(
            output.checks[1]
                .remediation
                .as_ref()
                .map(|remediation| remediation.command.as_str()),
            Some("fallow config --config=missing-dir/missing.json")
        );
        assert!(
            !output.checks[1]
                .message
                .contains("same explicit --config value")
        );
    }

    #[test]
    fn leading_dash_config_name_is_bound_to_its_option() {
        let root = tempfile::tempdir().expect("temp root");
        let config_path = root.path().join("-missing.json");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(
            output.checks[1]
                .remediation
                .as_ref()
                .map(|remediation| remediation.command.as_str()),
            Some("fallow config --config=-missing.json")
        );
    }

    #[test]
    fn config_path_equal_to_root_never_renders_an_empty_argument() {
        let root = tempfile::tempdir().expect("temp root");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(root.path()),
        });

        let command = output.checks[1]
            .remediation
            .as_ref()
            .map(|remediation| remediation.command.as_str());
        assert_eq!(command, Some("fallow config --config=."));
        assert_ne!(command, Some("fallow config --config="));
    }

    #[test]
    fn missing_config_traversal_outside_root_stays_private() {
        let sandbox = tempfile::tempdir().expect("temp sandbox");
        let root = sandbox.path().join("project");
        std::fs::create_dir(&root).expect("create project root");
        let config_path = root.join("../missing.json");

        let output = run_doctor(&DoctorOptions {
            root: &root,
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert!(output.checks[1].remediation.is_none());
        assert!(
            output.checks[1]
                .message
                .contains("same explicit --config value")
        );
        assert!(!output.checks[1].message.contains("missing.json"));
    }

    #[cfg(unix)]
    #[test]
    fn missing_config_below_external_symlink_stays_private() {
        let root = tempfile::tempdir().expect("temp root");
        let external = tempfile::tempdir().expect("external root");
        std::os::unix::fs::symlink(external.path(), root.path().join("external"))
            .expect("create external symlink");
        let config_path = root.path().join("external/missing-dir/missing.json");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert!(output.checks[1].remediation.is_none());
        assert!(
            output.checks[1]
                .message
                .contains("same explicit --config value")
        );
        assert!(!output.checks[1].message.contains("missing-dir"));
    }

    #[test]
    fn failed_external_config_requires_reusing_the_private_value() {
        let root = tempfile::tempdir().expect("temp root");
        let config_dir = tempfile::tempdir().expect("temp config dir");
        let config_path = config_dir.path().join("external.json");
        std::fs::write(&config_path, "{").expect("write invalid config");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: Some(&config_path),
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert!(output.checks[1].remediation.is_none());
        assert!(
            output.checks[1]
                .message
                .contains("same explicit --config value")
        );
        assert!(
            !output.checks[1]
                .message
                .contains(&config_dir.path().display().to_string())
        );
    }

    #[test]
    fn missing_explicit_plugin_is_a_required_failure() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"plugins":["missing-plugin.json"]}"#,
        )
        .expect("write config");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: None,
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
        assert!(output.checks[3].required);
        assert!(
            output.checks[3]
                .message
                .contains("1 unresolved configured resource")
        );
        assert!(!output.checks[3].message.contains("missing-plugin.json"));
    }

    #[test]
    fn malformed_explicit_plugin_is_a_required_failure() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"plugins":["broken.json"]}"#,
        )
        .expect("write config");
        std::fs::write(root.path().join("broken.json"), "{").expect("write plugin");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: None,
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
        assert!(output.checks[3].required);
        assert!(
            output.checks[3]
                .message
                .contains("1 unresolved configured resource")
        );
        assert!(!output.checks[3].message.contains("broken.json"));
    }

    #[test]
    fn explicit_plugin_directory_without_definitions_is_a_required_failure() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("plugins")).expect("create plugin directory");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"plugins":["plugins"]}"#,
        )
        .expect("write config");

        let output = run_doctor(&DoctorOptions {
            root: root.path(),
            config_path: None,
        });

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.checks[3].status, DoctorCheckStatus::Fail);
        assert!(output.checks[3].required);
        assert!(
            output.checks[3]
                .message
                .contains("1 unresolved configured resource")
        );
    }

    #[test]
    fn inactive_external_plugin_warns_with_project_root_remediation() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join("package.json"),
            r#"{"name":"doctor-test"}"#,
        )
        .expect("write package manifest");
        std::fs::write(
            root.path().join("fallow-plugin-doctor.json"),
            r#"{"name":"doctor-plugin","enablers":["missing-framework"]}"#,
        )
        .expect("write plugin");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        assert_eq!(output.status, DoctorStatus::Warn);
        assert_eq!(output.checks[3].status, DoctorCheckStatus::Warn);
        assert!(
            output.checks[3]
                .message
                .contains("0 active of 1 configured")
        );
        assert_eq!(
            output.checks[3]
                .remediation
                .as_ref()
                .map(|remediation| (remediation.cwd.as_str(), remediation.mutating)),
            Some((".", false))
        );
    }

    #[test]
    fn active_external_plugin_passes() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::create_dir(root.path().join("node_modules")).expect("create node_modules");
        std::fs::write(
            root.path().join("package.json"),
            r#"{"name":"doctor-test","dependencies":{"doctor-framework":"1.0.0"}}"#,
        )
        .expect("write package manifest");
        std::fs::write(
            root.path().join("fallow-plugin-doctor.json"),
            r#"{"name":"doctor-plugin","enablers":["doctor-framework"]}"#,
        )
        .expect("write plugin");

        let output = run_doctor_with_discovery(
            &DoctorOptions {
                root: root.path(),
                config_path: None,
            },
            &|_| Err("missing companion".to_string()),
        );

        assert_eq!(output.status, DoctorStatus::Pass);
        assert_eq!(output.checks[3].status, DoctorCheckStatus::Pass);
        assert!(
            output.checks[3]
                .message
                .contains("1 active of 1 configured")
        );
    }
}
