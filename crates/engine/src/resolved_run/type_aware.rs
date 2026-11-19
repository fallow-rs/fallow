//! The type-aware settings of a run: on or off, the TypeScript project
//! configs and the completeness policy, each with its source.
//!
//! The order is the given input (a flag or an option), then the environment
//! (`FALLOW_TYPE_AWARE`, `FALLOW_TYPE_AWARE_PROJECTS`,
//! `FALLOW_TYPE_AWARE_REQUIRE`), then the config. For an audit,
//! `audit.typeAware` comes before `typeAware.enabled`.

use std::path::{Path, PathBuf};

use fallow_config::{
    CommandKind, EnvProblem, EnvSnapshot, EnvVar, TypeAwareConfig, TypeAwareInputs,
    TypeAwareRequire, bool_env_value,
};
use fallow_types::sourced::{Given, Scope, Source, Sourced};

use super::resolve::{InputField, InputProblem, ResolveError};

/// The type-aware settings of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAwareSettings {
    /// Whether the run refines its findings with the type-aware companion.
    pub enabled: Sourced<bool>,
    /// The TypeScript project configs. Config entries are relative to the
    /// root.
    pub projects: Sourced<Vec<PathBuf>>,
    /// The completeness policy.
    pub require: Sourced<TypeAwareRequire>,
}

/// The type-aware settings of one run, with the first variable whose value
/// the run cannot use.
#[derive(Debug, Clone)]
pub(super) struct TypeAwareDecision {
    settings: TypeAwareSettings,
    problem: Option<(EnvVar, EnvProblem)>,
}

impl TypeAwareDecision {
    /// The settings, or the error of the first variable whose value the run
    /// cannot use.
    pub(super) fn result(&self) -> Result<&TypeAwareSettings, ResolveError> {
        match self.problem {
            Some((var, problem)) => Err(env_error(var, problem)),
            None => Ok(&self.settings),
        }
    }
}

/// The config input of [`decide_type_aware`].
#[derive(Debug, Clone, Copy)]
pub struct TypeAwareConfigInput<'a> {
    /// The `typeAware` section of the config.
    pub section: &'a TypeAwareConfig,
    /// The `audit.typeAware` key of the config.
    pub audit: Option<bool>,
    /// The config file, for the source of a config value.
    pub file: Option<&'a Path>,
}

/// Decide the type-aware settings of a run of `command` from the given
/// inputs, the environment snapshot and the config.
///
/// `env` holds only the variables that the run reads. A given completeness
/// policy hides the value of `FALLOW_TYPE_AWARE_REQUIRE`; a given on or off
/// value does not hide the value of `FALLOW_TYPE_AWARE`.
///
/// # Errors
///
/// Returns [`ResolveError::Input`] with [`InputField::Env`] for a
/// `FALLOW_TYPE_AWARE` or `FALLOW_TYPE_AWARE_REQUIRE` value that the run
/// cannot use.
pub fn decide_type_aware(
    command: CommandKind,
    given: &TypeAwareInputs,
    env: &EnvSnapshot,
    config: TypeAwareConfigInput<'_>,
) -> Result<TypeAwareSettings, ResolveError> {
    let decision = decision(command, given, env, config);
    decision.result().cloned()
}

pub(super) fn decision(
    command: CommandKind,
    given: &TypeAwareInputs,
    env: &EnvSnapshot,
    config: TypeAwareConfigInput<'_>,
) -> TypeAwareDecision {
    let mut problem = None;
    let env_enabled = keep_first_problem(
        &mut problem,
        EnvVar::TypeAware,
        env.parse(EnvVar::TypeAware, bool_env_value),
    );
    let env_require = if given.require.is_some() {
        None
    } else {
        keep_first_problem(
            &mut problem,
            EnvVar::TypeAwareRequire,
            env.parse(EnvVar::TypeAwareRequire, TypeAwareRequire::from_env_value),
        )
    };
    let config_key = |key| {
        config
            .file
            .map_or(Source::Derived { from: "config" }, |file| {
                Source::ConfigFile {
                    file: file.to_path_buf(),
                    key,
                }
            })
    };
    let environment = |var: EnvVar| Source::Environment { name: var.name() };
    let audit = if command == CommandKind::Audit {
        config.audit
    } else {
        None
    };
    let enabled = match (&given.enabled, env_enabled, audit) {
        (Some(given), _, _) => given_value(given),
        (None, Some(on), _) => sourced(on, environment(EnvVar::TypeAware)),
        (None, None, Some(on)) => sourced(on, config_key("audit.typeAware")),
        (None, None, None) if config.section.enabled => {
            sourced(true, config_key("typeAware.enabled"))
        }
        (None, None, None) => sourced(
            false,
            Source::Default {
                rule: "type-aware-off",
            },
        ),
    };
    let projects = match (&given.projects, env.paths(EnvVar::TypeAwareProjects)) {
        (Some(given), _) => given_value(given),
        (None, Some(paths)) => sourced(paths, environment(EnvVar::TypeAwareProjects)),
        (None, None) if !config.section.projects.is_empty() => sourced(
            config.section.projects.iter().map(PathBuf::from).collect(),
            config_key("typeAware.projects"),
        ),
        (None, None) => sourced(
            Vec::new(),
            Source::Default {
                rule: "no-projects",
            },
        ),
    };
    let require = match (&given.require, env_require) {
        (Some(given), _) => given_value(given),
        (None, Some(require)) => sourced(require, environment(EnvVar::TypeAwareRequire)),
        (None, None) if config.section.require != TypeAwareRequire::BestEffort => {
            sourced(config.section.require, config_key("typeAware.require"))
        }
        (None, None) => sourced(
            config.section.require,
            Source::Default {
                rule: "best-effort",
            },
        ),
    };
    TypeAwareDecision {
        settings: TypeAwareSettings {
            enabled,
            projects,
            require,
        },
        problem,
    }
}

/// The parsed value, or `None` with the problem kept when no earlier
/// variable has one.
fn keep_first_problem<T>(
    problem: &mut Option<(EnvVar, EnvProblem)>,
    var: EnvVar,
    result: Result<Option<T>, EnvProblem>,
) -> Option<T> {
    result.unwrap_or_else(|err| {
        problem.get_or_insert((var, err));
        None
    })
}

const fn sourced<T>(value: T, source: Source) -> Sourced<T> {
    Sourced::new(value, source, Scope::Global)
}

fn given_value<T: Clone>(given: &Given<T>) -> Sourced<T> {
    Sourced::new(
        given.value.clone(),
        Source::Given {
            origin: given.origin,
        },
        Scope::Global,
    )
}

/// The input error of a variable, with the text of the CLI.
fn env_error(var: EnvVar, problem: EnvProblem) -> ResolveError {
    let name = var.name();
    let (problem, message) = match (var, problem) {
        (_, EnvProblem::NotUnicode) => (
            InputProblem::NotUnicode,
            format!("{name} must be valid UTF-8"),
        ),
        (EnvVar::TypeAwareRequire, EnvProblem::InvalidValue) => (
            InputProblem::InvalidValue,
            format!("{name} must be best-effort or complete"),
        ),
        (_, EnvProblem::InvalidValue) => (
            InputProblem::InvalidValue,
            format!("{name} must be one of true, false, 1, 0, yes, no, on, or off"),
        ),
    };
    ResolveError::Input {
        field: InputField::Env(var),
        problem,
        message,
    }
}

#[cfg(test)]
#[path = "type_aware_tests.rs"]
mod tests;
