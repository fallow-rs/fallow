//! The resolver: raw run inputs to one [`ResolvedRun`].

use std::path::{Path, PathBuf};

use fallow_config::{
    AnalysisRequest, AnalysisSelection, CommandKind, ConfigFallback, EnvSnapshot, EnvVar,
    FallowConfig, PlannedAnalysis, ResolvedConfig, RunCommand, RunInputs, RunPlan, RunRequest,
    SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Scope, Source, Sourced};

use super::env_policy::applies;
use super::pick::{PerAnalysis, ProductionInputs, decide_production};
use super::steps::{self, ModeEntry};
use super::{AnalysisDecision, ResolvedRun, RunSettings, config_key_name, join_cwd, skip_key};
use crate::project_config::{ConfigFile, ConfigFileOptions, ConfigLoadError, load_config_file};

/// The field of the run inputs that an input error names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    /// The project root.
    Root,
    /// The explicit config path.
    ConfigPath,
    /// The thread count.
    Threads,
}

/// What is wrong with an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputProblem {
    /// The input is relative or missing, and the process has no current
    /// directory.
    CwdUnavailable,
    /// The input names no usable directory or file.
    Invalid,
    /// The input is zero.
    Zero,
}

/// What a run plan leaves out when it selects no analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyCause {
    /// The request excludes every analysis.
    Request,
    /// The config turns off each analysis that the request leaves in.
    Config,
}

/// The error of [`resolve_run`].
#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveError {
    /// An input of the run is not valid.
    Input {
        /// The input.
        field: InputField,
        /// What is wrong with it.
        problem: InputProblem,
        /// The message for the user.
        message: String,
    },
    /// The config file cannot be read or parsed.
    ConfigLoad(ConfigLoadError),
    /// The config file has an invalid plugin, boundary or rule pack.
    ConfigInvalid {
        /// The config file.
        file: Option<PathBuf>,
        /// The message for the user.
        message: String,
    },
    /// The workspace discovery failed.
    Workspace {
        /// The message for the user.
        message: String,
    },
    /// The run selects no analysis.
    NothingSelected {
        /// The command of the run.
        command: CommandKind,
        /// What left each analysis out.
        cause: EmptyCause,
    },
}

impl ResolveError {
    /// Whether the config failed to load, to validate or to give workspace
    /// metadata. Only such an error can fall back to the defaults.
    const fn is_config_error(&self) -> bool {
        matches!(
            self,
            Self::ConfigLoad(_) | Self::ConfigInvalid { .. } | Self::Workspace { .. }
        )
    }

    /// The message for the user. A config error has the text of the
    /// `config_for_project*` entry points.
    pub fn message(&self) -> String {
        match self {
            Self::Input { message, .. }
            | Self::ConfigInvalid { message, .. }
            | Self::Workspace { message } => message.clone(),
            Self::ConfigLoad(err) => format!("invalid config: {}", err.chain()),
            Self::NothingSelected { command, cause } => {
                let by = match cause {
                    EmptyCause::Request => "the request",
                    EmptyCause::Config => "the config",
                };
                format!("{by} leaves no analysis for the `{command:?}` run")
            }
        }
    }

    const fn input(field: InputField, problem: InputProblem, message: String) -> Self {
        Self::Input {
            field,
            problem,
            message,
        }
    }
}

const fn given_source<T>(given: &Given<T>) -> Source {
    Source::Given {
        origin: given.origin,
    }
}

fn given_value<T: Clone>(given: &Given<T>, scope: Scope) -> Sourced<T> {
    Sourced::new(given.value.clone(), given_source(given), scope)
}

/// Resolve the raw inputs of one run.
///
/// The steps are: the root, the config path, the load, the validation, the
/// plan, the production mode of each analysis, the run settings, and one
/// resolved config for each production mode that a planned analysis uses.
/// With [`ConfigFallback::DefaultsOnDiscoveredError`], a discovered config
/// that fails to load, to validate or to give workspace metadata gives the
/// default config.
///
/// # Errors
///
/// Returns a [`ResolveError`] for an invalid input, a config error, a
/// workspace error, or a run that selects no analysis.
pub fn resolve_run(inputs: &RunInputs) -> Result<ResolvedRun, ResolveError> {
    let surface = inputs.surface();
    let (env, env_read, env_ignored) = split_env(inputs.env(), surface, inputs.command);
    if inputs
        .threads
        .as_ref()
        .is_some_and(|given| given.value == 0)
    {
        return Err(ResolveError::input(
            InputField::Threads,
            InputProblem::Zero,
            "the thread count must be at least 1".to_owned(),
        ));
    }
    let root = resolve_root(inputs)?;
    let config_path = resolve_config_path(inputs)?;
    let ctx = Context {
        inputs,
        env: &env,
        root: root.value(),
    };
    let loaded = match ctx.load(config_path.as_ref()) {
        Ok(loaded) => loaded,
        Err(err) if err.is_config_error() && ctx.falls_back(config_path.as_ref()) => {
            tracing::debug!("config fell back to the defaults: {}", err.message());
            ctx.defaults()?
        }
        Err(err) => return Err(err),
    };
    let Loaded {
        file,
        config_file,
        decisions,
        settings,
        off,
        on,
        fell_back,
    } = loaded;
    Ok(ResolvedRun {
        surface,
        command: inputs.command,
        root,
        config_file,
        file,
        dead_code: decisions.dead_code,
        duplication: decisions.dupes,
        health: decisions.health,
        off,
        on,
        settings,
        env,
        env_read,
        env_ignored,
        fell_back,
    })
}

/// The snapshot without the variables that the run does not read, and the
/// set variables that the run reads and ignores.
fn split_env(
    env: &EnvSnapshot,
    surface: Surface,
    command: CommandKind,
) -> (EnvSnapshot, Vec<EnvVar>, Vec<EnvVar>) {
    let (read, ignored) = env
        .set_vars()
        .partition(|var| applies(*var, surface, command));
    let kept = env.clone().retain(|var| applies(var, surface, command));
    (kept, read, ignored)
}

fn resolve_root(inputs: &RunInputs) -> Result<Sourced<PathBuf>, ResolveError> {
    let candidate = match &inputs.root {
        Some(given) => Some(given_value(given, Scope::Global)),
        None => inputs.env().cwd().map(|cwd| {
            Sourced::new(
                cwd.to_path_buf(),
                Source::Default {
                    rule: "current-dir",
                },
                Scope::Global,
            )
        }),
    };
    let Some(candidate) = candidate else {
        return Err(ResolveError::input(
            InputField::Root,
            InputProblem::CwdUnavailable,
            "no root is given and the current directory is not available".to_owned(),
        ));
    };
    let path = join_cwd(candidate.value(), inputs.env().cwd()).ok_or_else(|| {
        ResolveError::input(
            InputField::Root,
            InputProblem::CwdUnavailable,
            format!(
                "the relative root '{}' needs a current directory",
                candidate.value().display()
            ),
        )
    })?;
    let canonical = crate::validate::validate_root(&path)
        .map_err(|message| ResolveError::input(InputField::Root, InputProblem::Invalid, message))?;
    Ok(candidate.map(|_| canonical))
}

fn resolve_config_path(inputs: &RunInputs) -> Result<Option<Given<PathBuf>>, ResolveError> {
    let Some(given) = &inputs.config_path else {
        return Ok(None);
    };
    let path = join_cwd(&given.value, inputs.env().cwd()).ok_or_else(|| {
        ResolveError::input(
            InputField::ConfigPath,
            InputProblem::CwdUnavailable,
            format!(
                "the relative config path '{}' needs a current directory",
                given.value.display()
            ),
        )
    })?;
    Ok(Some(Given {
        value: path,
        origin: given.origin,
    }))
}

/// The inputs that every step of one resolve reads.
struct Context<'a> {
    inputs: &'a RunInputs,
    /// The snapshot without the variables that the run does not read.
    env: &'a EnvSnapshot,
    root: &'a Path,
}

/// The result of the steps after the root and the config path.
struct Loaded {
    file: FallowConfig,
    config_file: Sourced<Option<PathBuf>>,
    decisions: PerAnalysis<AnalysisDecision>,
    settings: RunSettings,
    off: Option<ModeEntry>,
    on: Option<ModeEntry>,
    fell_back: bool,
}

impl Context<'_> {
    fn falls_back(&self, config_path: Option<&Given<PathBuf>>) -> bool {
        config_path.is_none()
            && self.inputs.config_fallback == ConfigFallback::DefaultsOnDiscoveredError
    }

    fn allow_remote_extends(&self) -> Sourced<bool> {
        self.inputs.allow_remote_extends.as_ref().map_or_else(
            || {
                Sourced::new(
                    false,
                    Source::Default {
                        rule: "remote-extends-off",
                    },
                    Scope::Global,
                )
            },
            |given| given_value(given, Scope::Global),
        )
    }

    /// Load, validate, plan, decide and resolve the config of the run.
    fn load(&self, config_path: Option<&Given<PathBuf>>) -> Result<Loaded, ResolveError> {
        let options = ConfigFileOptions {
            allow_remote_extends: *self.allow_remote_extends().value(),
        };
        let ConfigFile { config, path } = load_config_file(
            self.root,
            config_path.map(|given| given.value.as_path()),
            options,
        )
        .map_err(ResolveError::ConfigLoad)?;
        steps::validate_config(self.root, &config).map_err(|err| ResolveError::ConfigInvalid {
            file: path.clone(),
            message: err.to_string(),
        })?;
        let config_file = match (config_path, path) {
            (Some(given), Some(path)) => {
                Sourced::new(Some(path), given_source(given), Scope::Global)
            }
            (None, Some(path)) => Sourced::new(
                Some(path),
                Source::Derived { from: "discovery" },
                Scope::Global,
            ),
            (_, None) => Sourced::new(None, Source::Default { rule: "no-config" }, Scope::Global),
        };
        self.decide(config, config_file, false)
    }

    /// The steps of [`Self::load`] with the default config.
    fn defaults(&self) -> Result<Loaded, ResolveError> {
        let config_file = Sourced::new(
            None,
            Source::Derived {
                from: "config-fallback",
            },
            Scope::Global,
        );
        self.decide(FallowConfig::default(), config_file, true)
    }

    fn decide(
        &self,
        file: FallowConfig,
        config_file: Sourced<Option<PathBuf>>,
        fell_back: bool,
    ) -> Result<Loaded, ResolveError> {
        let path = config_file.value().clone();
        let plan = self.plan(&file)?;
        let production =
            decide_production(&self.production_inputs(), &file.production, path.as_deref());
        let selection = plan_selections(plan, self.inputs, path.as_deref());
        let decisions = PerAnalysis {
            dead_code: AnalysisDecision {
                selection: selection.dead_code,
                production: production.dead_code,
            },
            health: AnalysisDecision {
                selection: selection.health,
                production: production.health,
            },
            dupes: AnalysisDecision {
                selection: selection.dupes,
                production: production.dupes,
            },
        };
        let mut settings = self.settings(&file, path.as_deref());
        let (off, on) =
            self.mode_entries(&file, path.is_some(), &decisions, &settings, fell_back)?;
        let cache_dir = off.as_ref().or(on.as_ref()).map_or_else(
            || self.cache_dir_without_entry(file.cache.dir.clone()),
            |entry| entry.config.cache_dir.clone(),
        );
        settings.cache_dir = settings.cache_dir.map(|_| cache_dir);
        Ok(Loaded {
            file,
            config_file,
            decisions,
            settings,
            off,
            on,
            fell_back,
        })
    }

    fn plan(&self, file: &FallowConfig) -> Result<RunPlan, ResolveError> {
        let request = run_request(self.inputs.command, &self.inputs.selection);
        let plan = fallow_config::plan_run(file.plan_sections(), request);
        if plan.runs_any() {
            return Ok(plan);
        }
        let cause = if [request.dead_code, request.duplication, request.health]
            .iter()
            .all(|request| *request == AnalysisRequest::Excluded)
        {
            EmptyCause::Request
        } else if self.inputs.surface() == Surface::Cli {
            EmptyCause::Config
        } else {
            return Ok(plan);
        };
        Err(ResolveError::NothingSelected {
            command: self.inputs.command,
            cause,
        })
    }

    /// The production inputs of the run: the given values, and the variables
    /// that the env policy lets the run read.
    fn production_inputs(&self) -> ProductionInputs {
        let env = |var| self.env.bool(var).set();
        ProductionInputs {
            global_flag: self.inputs.production,
            own_flag: PerAnalysis {
                dead_code: self.inputs.production_dead_code,
                health: self.inputs.production_health,
                dupes: self.inputs.production_dupes,
            },
            global_env: env(EnvVar::Production),
            own_env: PerAnalysis {
                dead_code: env(EnvVar::ProductionDeadCode),
                health: env(EnvVar::ProductionHealth),
                dupes: env(EnvVar::ProductionDupes),
            },
        }
    }

    /// One resolved config for each production mode that a planned analysis
    /// uses.
    fn mode_entries(
        &self,
        file: &FallowConfig,
        from_file: bool,
        decisions: &PerAnalysis<AnalysisDecision>,
        settings: &RunSettings,
        lossy: bool,
    ) -> Result<(Option<ModeEntry>, Option<ModeEntry>), ResolveError> {
        let needs = |on: bool| {
            [&decisions.dead_code, &decisions.dupes, &decisions.health]
                .iter()
                .any(|decision| {
                    decision.selection.value().runs() && *decision.production.value() == on
                })
        };
        let entry = |on: bool| -> Result<Option<ModeEntry>, ResolveError> {
            if !needs(on) {
                return Ok(None);
            }
            let mut config = file.clone();
            steps::flatten_production(&mut config, from_file, on);
            let options = settings.resolve_options();
            let mut entry = if lossy {
                steps::mode_entry_lossy(self.root, config, options, self.env)
            } else {
                steps::mode_entry(self.root, config, options, self.env).map_err(|err| {
                    ResolveError::Workspace {
                        message: err.to_string(),
                    }
                })?
            };
            self.apply_given_settings(&mut entry.config);
            Ok(Some(entry))
        };
        Ok((entry(false)?, entry(true)?))
    }

    /// Apply the given settings that win over the environment.
    fn apply_given_settings(&self, config: &mut ResolvedConfig) {
        if let Some(given) = &self.inputs.max_file_size_mb {
            config.max_file_size_bytes =
                fallow_config::resolve_max_file_size_bytes(Some(given.value));
        }
        if let Some(given) = &self.inputs.cache_dir {
            config.override_cache_dir(given.value.clone());
        }
        if let Some(given) = &self.inputs.include_entry_exports {
            config.include_entry_exports = given.value;
        }
        if self
            .inputs
            .package_baselines
            .as_ref()
            .is_some_and(|given| !given.value)
        {
            config.workspace_changed_since.clear();
        }
    }
}

/// The run request of a command and its selection input.
fn run_request(command: CommandKind, selection: &SelectionInput) -> RunRequest {
    let run_command = if command == CommandKind::Audit {
        RunCommand::Audit
    } else {
        RunCommand::Combined
    };
    match selection {
        SelectionInput::Lists { only, skip, .. } => RunRequest::from_lists(run_command, only, skip),
        SelectionInput::Booleans {
            dead_code,
            duplication,
            health,
            ..
        } => RunRequest::from_booleans(run_command, *dead_code, *duplication, *health),
        SelectionInput::Options {
            dead_code,
            duplication,
            health,
            ..
        } => RunRequest::from_options(run_command, *dead_code, *duplication, *health),
        SelectionInput::Single(analysis) => {
            let request = |candidate| {
                if candidate == *analysis {
                    AnalysisRequest::Named
                } else {
                    AnalysisRequest::Excluded
                }
            };
            RunRequest {
                command: run_command,
                dead_code: request(PlannedAnalysis::DeadCode),
                duplication: request(PlannedAnalysis::Duplication),
                health: request(PlannedAnalysis::Health),
            }
        }
    }
}

/// The selection of each analysis with its source.
fn plan_selections(
    plan: RunPlan,
    inputs: &RunInputs,
    file: Option<&Path>,
) -> PerAnalysis<Sourced<AnalysisSelection>> {
    let request_source = match &inputs.selection {
        SelectionInput::Lists { origin, .. }
        | SelectionInput::Booleans { origin, .. }
        | SelectionInput::Options { origin, .. } => Source::Given { origin: *origin },
        SelectionInput::Single(_) => Source::Derived { from: "command" },
    };
    let request = run_request(inputs.command, &inputs.selection);
    let sourced = |selection: AnalysisSelection, analysis: PlannedAnalysis| {
        let source = if let Some(key) = skip_key(selection) {
            file.map_or(Source::Derived { from: "config" }, |file| {
                Source::ConfigFile {
                    file: file.to_path_buf(),
                    key: config_key_name(key),
                }
            })
        } else if request_is_default(request, analysis) {
            Source::Default {
                rule: "runs-by-default",
            }
        } else {
            request_source.clone()
        };
        Sourced::new(selection, source, Scope::Analysis)
    };
    PerAnalysis {
        dead_code: sourced(plan.dead_code, PlannedAnalysis::DeadCode),
        health: sourced(plan.health, PlannedAnalysis::Health),
        dupes: sourced(plan.duplication, PlannedAnalysis::Duplication),
    }
}

fn request_is_default(request: RunRequest, analysis: PlannedAnalysis) -> bool {
    let value = match analysis {
        PlannedAnalysis::DeadCode => request.dead_code,
        PlannedAnalysis::Duplication => request.duplication,
        PlannedAnalysis::Health => request.health,
    };
    value == AnalysisRequest::Default
}

impl Context<'_> {
    /// The settings of the run with their sources. The values equal the
    /// values that the steps write on each resolved config.
    fn settings(&self, file: &FallowConfig, path: Option<&Path>) -> RunSettings {
        let inputs = self.inputs;
        let default = |rule| Source::Default { rule };
        let environment = |var: EnvVar| Source::Environment { name: var.name() };
        let config_key = |key| {
            path.map_or(Source::Derived { from: "config" }, |file| {
                Source::ConfigFile {
                    file: file.to_path_buf(),
                    key,
                }
            })
        };
        let pick_given = |given: Option<&Given<bool>>, fallback: Sourced<bool>| {
            given.map_or(fallback, |given| given_value(given, Scope::Global))
        };
        let threads = inputs.threads.as_ref().map_or_else(
            || {
                let threads =
                    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
                Sourced::new(threads, default("available-parallelism"), Scope::Global)
            },
            |given| given_value(given, Scope::Global),
        );
        let no_cache = pick_given(
            inputs.no_cache.as_ref(),
            Sourced::new(false, default("cache-on"), Scope::Global),
        );
        let max_file_size_bytes =
            match (&inputs.max_file_size_mb, steps::max_file_size_env(self.env)) {
                (Some(given), _) => Sourced::new(
                    fallow_config::resolve_max_file_size_bytes(Some(given.value)),
                    given_source(given),
                    Scope::Global,
                ),
                (None, Some(mb)) => Sourced::new(
                    fallow_config::resolve_max_file_size_bytes(Some(mb)),
                    environment(EnvVar::MaxFileSize),
                    Scope::Global,
                ),
                (None, None) => Sourced::new(
                    fallow_config::resolve_max_file_size_bytes(None),
                    default("max-file-size"),
                    Scope::Global,
                ),
            };
        let cache_max_size_mb = match self
            .env
            .get(EnvVar::CacheMaxSize)
            .and_then(fallow_config::cache_max_size_from_env_value)
        {
            Some(mb) => Sourced::new(Some(mb), environment(EnvVar::CacheMaxSize), Scope::Global),
            None if file.cache.max_size_mb.is_some() => Sourced::new(
                file.cache.max_size_mb,
                config_key("cache.maxSizeMb"),
                Scope::Global,
            ),
            None => Sourced::new(None, default("cache-size"), Scope::Global),
        };
        let package_baselines = pick_given(
            inputs.package_baselines.as_ref(),
            match self.env.get(EnvVar::PackageBaselines) {
                Some(raw) if fallow_config::package_baselines_disabled_by_env_value(raw) => {
                    Sourced::new(false, environment(EnvVar::PackageBaselines), Scope::Global)
                }
                _ => Sourced::new(true, default("package-baselines-on"), Scope::Global),
            },
        );
        let include_entry_exports = pick_given(
            inputs.include_entry_exports.as_ref(),
            if file.include_entry_exports {
                Sourced::new(true, config_key("includeEntryExports"), Scope::Global)
            } else {
                Sourced::new(false, default("entry-exports-off"), Scope::Global)
            },
        );
        RunSettings {
            threads,
            no_cache,
            allow_remote_extends: self.allow_remote_extends(),
            max_file_size_bytes,
            cache_dir: self.cache_dir_source(file, path),
            cache_max_size_mb,
            package_baselines,
            include_entry_exports,
            output: inputs.output,
            quiet: inputs.quiet,
        }
    }

    /// The source of the cache directory. [`Context::decide`] sets the value.
    fn cache_dir_source(&self, file: &FallowConfig, path: Option<&Path>) -> Sourced<PathBuf> {
        let source = if let Some(given) = &self.inputs.cache_dir {
            given_source(given)
        } else if self.cache_dir_env().is_some() {
            Source::Environment {
                name: EnvVar::CacheDir.name(),
            }
        } else {
            match (path, &file.cache.dir) {
                (Some(path), Some(_)) => Source::ConfigFile {
                    file: path.to_path_buf(),
                    key: "cache.dir",
                },
                _ => Source::Default { rule: "cache-dir" },
            }
        };
        Sourced::new(PathBuf::new(), source, Scope::Global)
    }

    fn cache_dir_env(&self) -> Option<PathBuf> {
        self.env
            .get(EnvVar::CacheDir)
            .map(std::ffi::OsStr::to_os_string)
            .and_then(fallow_config::cache_dir_from_env_value)
    }

    /// The cache directory of a run that resolves no config, by the rules of
    /// a resolved config.
    fn cache_dir_without_entry(&self, configured: Option<PathBuf>) -> PathBuf {
        let host = self
            .inputs
            .cache_dir
            .as_ref()
            .map(|given| given.value.clone())
            .or_else(|| self.cache_dir_env());
        host.map_or_else(
            || fallow_config::config_cache_dir(self.root, configured),
            |dir| join_cwd(&dir, Some(self.root)).unwrap_or(dir),
        )
    }
}
