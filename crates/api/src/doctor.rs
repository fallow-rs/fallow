//! Read-only project readiness inspection.

use std::path::{Path, PathBuf};

use fallow_config::{ResolvedConfig, WorkspaceDiagnostic, WorkspaceInfo};
use fallow_engine::resolved_run::{ResolvedRun, resolve_run};
use fallow_output::{
    DOCTOR_SCHEMA_VERSION, DoctorCheck, DoctorCheckCategory, DoctorCheckId, DoctorCheckStatus,
    DoctorOutput, DoctorRemediation, DoctorStatus, DoctorSummary,
};
use fallow_types::envelope::{SchemaVersion, ToolVersion};
use fallow_types::sourced::{Given, Origin};

const ROOT_LINK_REFUSED: &str = "Project root passes through a link that resolves outside the checkout or the project. Pass the resolved absolute path with --root, or cd -P to it.";

/// Inputs for a deterministic doctor run.
pub struct DoctorOptions<'a> {
    /// Project root, before canonical validation.
    pub root: &'a Path,
    /// Optional explicit fallow config path.
    pub config_path: Option<&'a Path>,
    /// True when the caller got no root and `root` is the current directory.
    /// The root check then also covers the `PWD` spelling of the current
    /// directory, as for an analysis run with no root.
    pub root_is_current_dir: bool,
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
    let mut checks = Vec::with_capacity(9);
    let validated = if options.root_is_current_dir {
        fallow_engine::validate::validate_current_dir_root(options.root)
    } else {
        fallow_engine::validate::validate_root(options.root)
    };
    let root = match validated {
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
            // The report names no path, so a refused link gets a fixed text.
            // An existing directory that fails the check is such a refusal.
            let message = if options.root.is_dir() {
                ROOT_LINK_REFUSED
            } else {
                "Project root is not accessible. Set --root to an existing, readable directory."
            };
            checks.push(check(
                DoctorCheckId::Root,
                DoctorCheckCategory::Project,
                DoctorCheckStatus::Fail,
                true,
                message,
                None,
            ));
            push_prerequisite_skips(&mut checks, "Project root readiness failed.");
            return build_output(checks);
        }
    };

    match resolve_doctor_run(&root, options.config_path, cache_dir) {
        Ok(run) => {
            let project = ReadyProject::of(&run);
            let cache_repair = || {
                if cache_dir.is_some_and(|path| !path.as_os_str().is_empty()) {
                    return Err(
                        "Set FALLOW_CACHE_DIR to the inspected cache directory and run fallow dead-code --quiet with the same --config value if one was selected.",
                    );
                }
                let explicit = options.config_path.and(project.path.as_deref());
                remediation_with_config("fallow dead-code --quiet", &root, explicit)
                    .ok_or("Run fallow dead-code --quiet with the same --config value.")
            };
            push_ready_project_checks(
                &mut checks,
                &root,
                &project,
                &run.plugin_readiness(),
                cache_repair,
                discover_companion,
            );
        }
        Err(error) => {
            push_project_failure_checks(&mut checks, &error.message(), &root, options.config_path);
        }
    }

    build_output(checks)
}

/// The run of the doctor. A config error is a failed check, not an error of
/// the doctor, so the caller reads the [`fallow_engine::resolved_run::ResolveError`].
fn resolve_doctor_run(
    root: &Path,
    config_path: Option<&Path>,
    cache_dir: Option<&Path>,
) -> Result<ResolvedRun, fallow_engine::resolved_run::ResolveError> {
    let analysis = crate::AnalysisOptions {
        root: Some(root.to_path_buf()),
        config_path: config_path.map(Path::to_path_buf),
        no_cache: true,
        threads: Some(1),
        ..crate::AnalysisOptions::default()
    };
    let mut inputs = crate::run::run_inputs(
        &analysis,
        &crate::run::ProgrammaticEntry::Doctor,
        crate::run::RouteSettings::default(),
    );
    inputs.cache_dir = cache_dir
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| Given {
            value: path.to_path_buf(),
            origin: Origin::ApiOption("cache_dir"),
        });
    resolve_run(&inputs)
}

/// The resolved project of a doctor run: the config file, the resolved
/// config and the workspace metadata of the dead-code analysis.
struct ReadyProject {
    path: Option<PathBuf>,
    config: ResolvedConfig,
    workspaces: Vec<WorkspaceInfo>,
    workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// The type-aware setting and policy of the run, or the readiness text of
    /// a variable whose value the run cannot use.
    type_aware: Result<(bool, fallow_config::TypeAwareRequire), &'static str>,
}

impl ReadyProject {
    fn of(run: &ResolvedRun) -> Self {
        let fallow_engine::resolved_run::AnalysisSlot::Runs(dead_code) =
            run.analysis(fallow_config::PlannedAnalysis::DeadCode)
        else {
            unreachable!("the doctor run selects the dead-code analysis");
        };
        Self {
            path: run.config_file().value().clone(),
            config: dead_code.config().clone(),
            workspaces: dead_code.workspaces().to_vec(),
            workspace_diagnostics: dead_code.workspace_diagnostics().to_vec(),
            type_aware: run
                .type_aware()
                .map(|settings| (*settings.enabled.value(), *settings.require.value()))
                .map_err(|err| type_aware_readiness_text(&err)),
        }
    }
}

/// The readiness text of a type-aware variable that the run cannot use.
fn type_aware_readiness_text(err: &fallow_engine::resolved_run::ResolveError) -> &'static str {
    use fallow_engine::resolved_run::{InputField, InputProblem, ResolveError};
    match err {
        ResolveError::Input {
            field: InputField::Env(fallow_config::EnvVar::TypeAwareRequire),
            problem: InputProblem::NotUnicode,
            ..
        } => "FALLOW_TYPE_AWARE_REQUIRE must contain valid UTF-8.",
        ResolveError::Input {
            field: InputField::Env(fallow_config::EnvVar::TypeAwareRequire),
            ..
        } => "FALLOW_TYPE_AWARE_REQUIRE must be best-effort or complete.",
        ResolveError::Input {
            problem: InputProblem::NotUnicode,
            ..
        } => "FALLOW_TYPE_AWARE must contain valid UTF-8.",
        _ => "FALLOW_TYPE_AWARE must contain a supported boolean value.",
    }
}

fn push_ready_project_checks<F>(
    checks: &mut Vec<DoctorCheck>,
    root: &Path,
    project: &ReadyProject,
    configured_plugin_diagnostics: &[fallow_config::ConfiguredPluginDiagnostic],
    cache_repair: impl Fn() -> Result<DoctorRemediation, &'static str>,
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
        project.type_aware,
        discover_companion,
    ));

    checks.push(dependencies_check(root));
    for mut check in [
        cache_check(&project.config),
        graph_cache_check(&project.config),
    ] {
        if check.status == DoctorCheckStatus::Warn {
            match cache_repair() {
                Ok(repair) => check.remediation = Some(repair),
                Err(hint) => {
                    check.message.push(' ');
                    check.message.push_str(hint);
                }
            }
        }
        checks.push(check);
    }
    checks.push(mutation_runner_check(root, project.config.mutation.runner));
}

/// Report whether the project has an installed dependency tree.
///
/// Advisory, never required: analysis runs without `node_modules`, it just
/// runs blind to package `exports`, to plugins that activate on an installed
/// package, and to a dependency's installed shape.
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
        Some(remediation(
            &project_package_manager(root).install_command(),
            true,
        )),
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
            None,
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
            None,
        ),
    }
}

/// Report whether a mutation run can start the Vitest of the project.
///
/// Advisory, never required, and never a `fail`, because mutation testing is
/// opt-in. The check reads the `package.json` of the installed package, the
/// same file that the companion reads before it loads Vitest, so it runs no
/// project code. The check is skipped when `mutation.runner` does not select
/// Vitest, the root `package.json` declares neither `vitest` nor `vite-plus`,
/// and the companion finds no installed Vitest. A workspace package can
/// declare a Vitest that the package manager installs at the root, and a
/// toolchain such as Vite+ can bring its own Vitest, so each install that the
/// companion finds counts.
/// The `node-test` and `bun-test` runners skip the check too: their range
/// applies to the runtime that a run selects.
fn mutation_runner_check(root: &Path, configured: fallow_config::MutationRunner) -> DoctorCheck {
    use crate::mutation::runner_select::{Runner, declares_vitest, select_runner};
    use crate::mutation::runner_version::{
        check_runner_version, installs_package, runner_range, runner_version_help, supported_range,
        unsupported_message,
    };

    let runner = select_runner(configured, root, &[]);
    if runner != Runner::Vitest {
        return skipped(
            DoctorCheckId::MutationRunner,
            DoctorCheckCategory::Companion,
            "The mutation runner checks the version of its runtime when a run starts.",
        );
    }
    if configured == fallow_config::MutationRunner::Auto
        && !declares_vitest(root)
        && !installs_package(root, runner.name())
    {
        return skipped(
            DoctorCheckId::MutationRunner,
            DoctorCheckCategory::Companion,
            "The project declares no mutation test runner.",
        );
    }
    let range = match runner_range(runner) {
        Ok(range) => range,
        Err(message) => {
            return check(
                DoctorCheckId::MutationRunner,
                DoctorCheckCategory::Companion,
                DoctorCheckStatus::Warn,
                false,
                message,
                None,
            );
        }
    };
    let version = check_runner_version(runner, range, root);
    if let (Some(installed), Some(true)) = (&version.installed_version, version.supported) {
        return check(
            DoctorCheckId::MutationRunner,
            DoctorCheckCategory::Companion,
            DoctorCheckStatus::Pass,
            false,
            format!(
                "{} {installed} is in the supported mutation runner range {}.",
                range.package,
                supported_range(range)
            ),
            None,
        );
    }
    check(
        DoctorCheckId::MutationRunner,
        DoctorCheckCategory::Companion,
        DoctorCheckStatus::Warn,
        false,
        format!(
            "{}. A mutation run stops before the first mutant. Fix: {}.",
            unsupported_message(range, version.installed_version.as_deref()),
            runner_version_help(range)
        ),
        Some(remediation(
            &fallow_config::PackageManager::suggested_add_dev_command(
                Some(root),
                &format!("{}@^{}", range.package, range.min_version),
            ),
            true,
        )),
    )
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
    project: &ReadyProject,
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
    checks.push(skipped(
        DoctorCheckId::MutationRunner,
        DoctorCheckCategory::Companion,
        "Configuration readiness did not establish which mutation runner this project uses.",
    ));
}

fn type_aware_check<F>(
    root: &Path,
    type_aware: Result<(bool, fallow_config::TypeAwareRequire), &'static str>,
    discover_companion: &F,
) -> DoctorCheck
where
    F: Fn(&Path) -> Result<(), String>,
{
    let (enabled, require) = match type_aware {
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
                    &fallow_config::PackageManager::suggested_add_dev_command(
                        Some(root),
                        &format!("fallow-type-aware@{}", env!("CARGO_PKG_VERSION")),
                    ),
                    true,
                )),
            )
        }
    }
}

/// The package manager that a suggested install command names. npm is the
/// default when the project declares none and has no lockfile.
fn project_package_manager(root: &Path) -> fallow_config::PackageManager {
    fallow_config::PackageManager::detect_nearest(root)
        .unwrap_or(fallow_config::PackageManager::Npm)
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
        (
            DoctorCheckId::MutationRunner,
            DoctorCheckCategory::Companion,
        ),
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
        let relative = path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if relative.is_empty() {
            ".".to_string()
        } else {
            relative
        }
    })
}

#[cfg(test)]
#[path = "doctor_tests.rs"]
mod tests;
