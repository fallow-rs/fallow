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

fn run_doctor_with_discovery<F>(options: &DoctorOptions<'_>, discover_companion: &F) -> DoctorOutput
where
    F: Fn(&Path) -> Result<(), String>,
{
    let mut checks = Vec::with_capacity(5);
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
        Ok(readiness) => push_ready_project_checks(
            &mut checks,
            &root,
            &readiness.project,
            &readiness.configured_plugin_diagnostics,
            discover_companion,
        ),
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

        assert_eq!(output.status, DoctorStatus::Pass);
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
            ]
        );
        assert_eq!(
            output.checks[1].message,
            "Zero-config defaults resolved successfully."
        );
        assert_eq!(output.checks[4].status, DoctorCheckStatus::Skipped);
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

        assert_eq!(output.status, DoctorStatus::Fail);
        assert_eq!(output.checks.len(), 5);
        assert_eq!(output.checks[1].status, DoctorCheckStatus::Fail);
        assert_eq!(output.checks[2].status, DoctorCheckStatus::Skipped);
        assert!(
            !output.checks[1]
                .message
                .contains(&root.path().display().to_string())
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
