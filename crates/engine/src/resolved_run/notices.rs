//! The notices of a run: the decisions of the resolver that a surface shows
//! as one note each. Each notice is a [`WorkspaceDiagnostic`] at the root, so
//! it reaches every output that carries `workspace_diagnostics`.

use std::path::{Path, PathBuf};

use fallow_config::{
    CONFIG_FILE_NAMES, EnvSnapshot, EnvValue, EnvVar, FallowConfig, WorkspaceDiagnostic,
    WorkspaceDiagnosticKind,
};
use fallow_types::sourced::{Source, Sourced};
use rustc_hash::FxHashSet;

use super::ResolveError;

/// The variables of the production mode. A run that reads them and finds a
/// value that is not a bool ignores the variable.
const PRODUCTION_VARS: [EnvVar; 4] = [
    EnvVar::Production,
    EnvVar::ProductionDeadCode,
    EnvVar::ProductionHealth,
    EnvVar::ProductionDupes,
];

/// The inputs of [`run_notices`].
pub(super) struct NoticeInput<'a> {
    /// The canonical root of the run.
    pub(super) root: &'a Path,
    /// The snapshot with only the variables that the run reads.
    pub(super) env: &'a EnvSnapshot,
    /// The validated config.
    pub(super) file: &'a FallowConfig,
    /// The config file of the run.
    pub(super) config_file: &'a Sourced<Option<PathBuf>>,
    /// The config error that made the run use the default config.
    pub(super) fallback: Option<&'a ResolveError>,
}

/// The notices of one run, in a fixed order: the ignored variables, the
/// fallback, the shadowed config files and the unknown security categories.
pub(super) fn run_notices(input: &NoticeInput<'_>) -> Vec<WorkspaceDiagnostic> {
    // A notice names its path relative to the root, as every root-anchored
    // diagnostic does, so no output holds the absolute path of the host.
    let notice = |path: PathBuf, kind| {
        WorkspaceDiagnostic::new(input.root, path, kind).into_root_relative(input.root)
    };
    let mut notices: Vec<WorkspaceDiagnostic> = PRODUCTION_VARS
        .into_iter()
        .filter(|var| input.env.bool(*var) == EnvValue::Invalid)
        .map(|var| {
            notice(
                input.root.to_path_buf(),
                WorkspaceDiagnosticKind::EnvValueIgnored {
                    name: var.name().to_owned(),
                },
            )
        })
        .collect();
    if let Some(err) = input.fallback {
        notices.push(notice(
            input.root.to_path_buf(),
            WorkspaceDiagnosticKind::ConfigLoadFellBack {
                error: err.message(),
            },
        ));
    }
    notices.extend(
        shadowed_config_files(input.config_file)
            .into_iter()
            .map(|(used, ignored)| {
                let kind = WorkspaceDiagnosticKind::ConfigFileShadowed {
                    used,
                    ignored: file_name(&ignored),
                };
                notice(ignored, kind)
            }),
    );
    notices.extend(
        unknown_security_categories(input.file)
            .into_iter()
            .map(|id| {
                notice(
                    input.root.to_path_buf(),
                    WorkspaceDiagnosticKind::UnknownSecurityCategory { id },
                )
            }),
    );
    notices
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The config files that discovery ignored in the directory of the
/// discovered config: the names after it in precedence order. An explicit
/// config file shadows nothing, because the user chose it.
fn shadowed_config_files(config_file: &Sourced<Option<PathBuf>>) -> Vec<(String, PathBuf)> {
    let Some(path) = config_file.value() else {
        return Vec::new();
    };
    if !matches!(config_file.source(), Source::Derived { from: "discovery" }) {
        return Vec::new();
    }
    let used = file_name(path);
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let Some(index) = CONFIG_FILE_NAMES.iter().position(|name| *name == used) else {
        return Vec::new();
    };
    CONFIG_FILE_NAMES
        .iter()
        .skip(index + 1)
        .map(|name| dir.join(name))
        .filter(|candidate| candidate.exists())
        .map(|ignored| (used.clone(), ignored))
        .collect()
}

/// The ids of `security.categories` that fallow does not know, without
/// repeats, in the order of the config.
fn unknown_security_categories(file: &FallowConfig) -> Vec<String> {
    let Some(categories) = &file.security.categories else {
        return Vec::new();
    };
    let known: FxHashSet<String> = fallow_security::security_categories()
        .into_iter()
        .map(|category| category.id)
        .collect();
    let mut seen = FxHashSet::default();
    [categories.include.as_ref(), categories.exclude.as_ref()]
        .into_iter()
        .flatten()
        .flatten()
        .filter(|id| !known.contains(id.as_str()) && seen.insert(id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use fallow_config::{
        CommandKind, ConfigFallback, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput,
        Surface,
    };
    use fallow_types::sourced::{Given, Origin};

    use super::super::{ResolvedRun, resolve_run};

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        std::fs::create_dir(root.join(".git")).expect("git marker");
        std::fs::write(root.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
        for (name, content) in files {
            std::fs::write(root.join(name), content).expect("project file");
        }
        (dir, root)
    }

    fn resolve(root: &std::path::Path, surface: Surface, env: &[(&str, &str)]) -> ResolvedRun {
        let mut inputs = RunInputs::new(
            surface,
            EnvSnapshot::from_pairs(env, Some(root)),
            CommandKind::DeadCode,
            SelectionInput::Single(PlannedAnalysis::DeadCode),
        );
        inputs.root = Some(Given {
            value: root.to_path_buf(),
            origin: Origin::ApiOption("test"),
        });
        if surface == Surface::Lsp {
            inputs.config_fallback = ConfigFallback::DefaultsOnDiscoveredError;
        }
        resolve_run(&inputs).expect("run resolves")
    }

    fn kinds(run: &ResolvedRun) -> Vec<&'static str> {
        run.notices()
            .iter()
            .map(|notice| notice.kind.id())
            .collect()
    }

    #[test]
    fn an_invalid_production_value_is_a_notice_by_name() {
        let (_dir, root) = project(&[]);
        let run = resolve(
            &root,
            Surface::Cli,
            &[("FALLOW_PRODUCTION_HEALTH", "perhaps")],
        );
        let value = serde_json::to_string(run.notices()).expect("notices serialize");
        assert!(value.contains("FALLOW_PRODUCTION_HEALTH"), "{value}");
        assert!(!value.contains("perhaps"), "{value}");
        assert_eq!(kinds(&run), ["env-value-ignored"]);
        assert_eq!(run.notices()[0].path, std::path::Path::new("."));
    }

    #[test]
    fn a_valid_value_and_an_unread_variable_give_no_notice() {
        let (_dir, root) = project(&[]);
        assert!(
            kinds(&resolve(
                &root,
                Surface::Cli,
                &[("FALLOW_PRODUCTION", "true")]
            ))
            .is_empty()
        );
        // The language server does not read the production variables.
        assert!(
            kinds(&resolve(
                &root,
                Surface::Lsp,
                &[("FALLOW_PRODUCTION", "perhaps")]
            ))
            .is_empty()
        );
    }

    #[test]
    fn a_fallback_and_a_shadowed_file_are_notices() {
        let (_dir, root) = project(&[(".fallowrc.json", "{ not json"), ("fallow.toml", "")]);
        let run = resolve(&root, Surface::Lsp, &[]);
        assert_eq!(kinds(&run), ["config-load-fell-back"]);

        let (_dir, root) = project(&[(".fallowrc.json", "{}"), ("fallow.toml", "")]);
        let run = resolve(&root, Surface::Cli, &[]);
        assert_eq!(kinds(&run), ["config-file-shadowed"]);
        assert_eq!(run.notices()[0].path, std::path::Path::new("fallow.toml"));
    }

    #[test]
    fn a_fallback_notice_holds_no_absolute_path() {
        let (_dir, root) = project(&[(".fallowrc.json", "{ not json")]);
        let run = resolve(&root, Surface::Lsp, &[]);
        let host = root.display().to_string();
        let notice = serde_json::to_string(run.notices()).expect("notices serialize");
        assert!(notice.contains(".fallowrc.json"), "{notice}");
        assert!(!notice.contains(&host), "{notice}");
        let super::super::AnalysisSlot::Runs(runnable) = run.analysis(PlannedAnalysis::DeadCode)
        else {
            panic!("dead code runs");
        };
        let session =
            serde_json::to_string(runnable.workspace_diagnostics()).expect("diagnostics serialize");
        assert!(!session.contains(&host), "{session}");
    }

    #[test]
    fn an_unknown_security_category_is_a_notice_in_each_session() {
        let (_dir, root) = project(&[(
            ".fallowrc.json",
            r#"{"security":{"categories":{"include":["sql-injektion","sql-injektion"]}}}"#,
        )]);
        let run = resolve(&root, Surface::Cli, &[]);
        assert_eq!(kinds(&run), ["unknown-security-category"]);
        let super::super::AnalysisSlot::Runs(runnable) = run.analysis(PlannedAnalysis::DeadCode)
        else {
            panic!("dead code runs");
        };
        assert!(
            runnable
                .workspace_diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.kind.id() == "unknown-security-category")
        );
    }
}
