//! The resolver: raw run inputs to one [`ResolvedRun`].

use std::path::{Path, PathBuf};

use fallow_config::{
    AnalysisRequest, AnalysisSelection, AnalysisSnapshot, CommandKind, ConfigFallback, EnvSnapshot,
    EnvVar, FallowConfig, PlannedAnalysis, RunCommand, RunInputs, RunPlan, RunRequest,
    SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin, Scope, Source, Sourced};

use super::env_policy::applies;
use super::pick::{PerAnalysis, ProductionInputs, decide_production};
use super::steps::{self, GivenSettings, ModeEntry};
use super::type_aware::{TypeAwareConfigInput, TypeAwareDecision};
use super::{AnalysisDecision, ResolvedRun, RunSettings, config_key_name, join_cwd, skip_key};
use crate::project_config::{
    ConfigChainSnapshot, ConfigFile, ConfigFileOptions, ConfigLoadError, load_config_file,
};

/// The field of the run inputs that an input error names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputField {
    /// The project root.
    Root,
    /// The explicit config path.
    ConfigPath,
    /// The thread count.
    Threads,
    /// An environment variable.
    Env(EnvVar),
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
    /// The value is not one of the values that the input takes.
    InvalidValue,
    /// The value is not valid UTF-8.
    NotUnicode,
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
    /// The config file that the error names, when it names one.
    #[must_use]
    pub fn config_file(&self) -> Option<&Path> {
        match self {
            Self::ConfigLoad(ConfigLoadError::Parse { path, .. })
            | Self::ConfigInvalid { file: path, .. } => path.as_deref(),
            Self::Input { .. } | Self::Workspace { .. } | Self::NothingSelected { .. } => None,
        }
    }

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
/// default config, and [`ResolvedRun::fallback_error`] keeps the error. A run
/// of [`Surface::Lsp`] scopes a shared cache directory to the root, so two
/// editor roots do not write the same cache files.
///
/// # Errors
///
/// Returns a [`ResolveError`] for an invalid input, a config error, a
/// workspace error, or a run that selects no analysis.
pub fn resolve_run(inputs: &RunInputs) -> Result<ResolvedRun, ResolveError> {
    resolve_with(inputs, |ctx, config_path| {
        ctx.load(config_path.map(|given| (given.value.as_path(), given_source(given))))
    })
}

/// Which config file the base side of a comparison with a base ref reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseConfig {
    /// The config file of the head run. A head run without a config file
    /// gives discovery from the base root.
    Head,
    /// The base copy of the config file. An explicit config file under the
    /// head root gives the file at the same path under the base root. A file
    /// that the base does not have, and a discovered head config, give
    /// discovery from the base root. An explicit file outside the head root
    /// has no base copy, so the base side reads that file.
    BaseCopy,
}

/// The base side of a comparison with a base ref.
#[derive(Debug, Clone)]
pub struct BaseSide {
    /// The analysis root in the base checkout.
    pub root: Given<PathBuf>,
    /// The config file rule of the base side.
    pub config: BaseConfig,
    /// The cache directory of the base side. `None` keeps the cache input of
    /// the head run.
    pub cache_dir: Option<Given<PathBuf>>,
    /// The revision that the side holds. A base revision skips the config
    /// validation that needs a folder of the change. The staged side of
    /// `audit --staged` holds the change, so it is the current revision.
    pub snapshot: AnalysisSnapshot,
}

impl BaseSide {
    /// The side of a base revision.
    #[must_use]
    pub const fn base(
        root: Given<PathBuf>,
        config: BaseConfig,
        cache_dir: Option<Given<PathBuf>>,
    ) -> Self {
        Self {
            root,
            config,
            cache_dir,
            snapshot: AnalysisSnapshot::Base,
        }
    }
}

/// Resolve the base side of a comparison with a base ref: the inputs of
/// `head` on the base root, with the config file of the [`BaseConfig`] rule.
/// The base run prints no progress.
///
/// # Errors
///
/// Returns a [`ResolveError`] as [`resolve_run`] does.
pub fn resolve_base_run(head: &ResolvedRun, base: BaseSide) -> Result<ResolvedRun, ResolveError> {
    resolve_base_side(head, base, false)
}

/// Resolve the base side of a comparison with a base ref as
/// [`resolve_base_run`] does, with the decisions of `head` for the analyses:
/// the base side runs each analysis that the head run runs, and no other,
/// with the production mode of the head run. The config of the base side
/// then cannot turn an analysis off or change a production mode, so the
/// findings of the two sides compare.
///
/// # Errors
///
/// Returns a [`ResolveError`] as [`resolve_run`] does.
pub fn resolve_base_run_with_head_decisions(
    head: &ResolvedRun,
    base: BaseSide,
) -> Result<ResolvedRun, ResolveError> {
    resolve_base_side(head, base, true)
}

fn resolve_base_side(
    head: &ResolvedRun,
    base: BaseSide,
    head_decisions: bool,
) -> Result<ResolvedRun, ResolveError> {
    let BaseSide {
        root,
        config,
        cache_dir,
        snapshot,
    } = base;
    let file = base_config_file(head, &root.value, config);
    let mut inputs = (*head.inputs).clone();
    if head_decisions {
        keep_head_decisions(&mut inputs, head, root.origin);
    }
    inputs.root = Some(root);
    inputs.config_path = None;
    inputs.config_fallback = ConfigFallback::Fail;
    if cache_dir.is_some() {
        inputs.cache_dir = cache_dir;
    }
    inputs.quiet = true;
    resolve_with(&inputs, |ctx, _| {
        ctx.load_snapshot(
            file.as_ref()
                .map(|(path, source)| (path.as_path(), source.clone())),
            snapshot,
        )
    })
}

/// Give `inputs` the plan and the production modes of `head` as inputs of
/// `origin`.
fn keep_head_decisions(inputs: &mut RunInputs, head: &ResolvedRun, origin: Origin) {
    let runs = |analysis| Some(head.decision(analysis).selection.value().runs());
    inputs.selection = SelectionInput::Options {
        dead_code: runs(PlannedAnalysis::DeadCode),
        duplication: runs(PlannedAnalysis::Duplication),
        health: runs(PlannedAnalysis::Health),
        origin,
    };
    let production = |analysis| {
        Some(Given {
            value: *head.production(analysis).value(),
            origin,
        })
    };
    inputs.production = None;
    inputs.production_dead_code = production(PlannedAnalysis::DeadCode);
    inputs.production_health = production(PlannedAnalysis::Health);
    inputs.production_dupes = production(PlannedAnalysis::Duplication);
}

/// The config file of a base side with its source, or `None` for discovery
/// from the base root.
fn base_config_file(
    head: &ResolvedRun,
    base_root: &Path,
    rule: BaseConfig,
) -> Option<(PathBuf, Source)> {
    let path = head.config_file.value().as_ref()?;
    let source = head.config_file.source();
    let explicit = matches!(source, Source::Given { .. });
    match rule {
        BaseConfig::Head if explicit => Some((path.clone(), source.clone())),
        BaseConfig::Head => Some((
            path.clone(),
            Source::Derived {
                from: "head-config",
            },
        )),
        BaseConfig::BaseCopy if !explicit => None,
        BaseConfig::BaseCopy => {
            let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.clone());
            match canonical.strip_prefix(head.root.value()) {
                Ok(relative) => {
                    let copy = base_root.join(relative);
                    copy.is_file()
                        .then_some((copy, Source::Derived { from: "base-copy" }))
                }
                Err(_) => Some((path.clone(), source.clone())),
            }
        }
    }
}

/// Resolve the raw inputs of one run with `config` in place of a config file.
/// The config is validated, and the run has no config file.
///
/// # Errors
///
/// Returns a [`ResolveError`] as [`resolve_run`] does.
#[cfg(any(test, feature = "testing"))]
pub fn resolve_run_inline(
    inputs: &RunInputs,
    config: FallowConfig,
) -> Result<ResolvedRun, ResolveError> {
    resolve_with(inputs, |ctx, _| ctx.inline(config))
}

/// The steps of [`resolve_run`], with `load` in place of the load step.
fn resolve_with(
    inputs: &RunInputs,
    load: impl FnOnce(&Context<'_>, Option<&Given<PathBuf>>) -> Result<Loaded, ResolveError>,
) -> Result<ResolvedRun, ResolveError> {
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
    let given = ctx.given_settings();
    let (loaded, fallback) = match load(&ctx, config_path.as_ref()) {
        Ok(loaded) => (loaded, None),
        Err(err) if err.is_config_error() && ctx.falls_back(config_path.as_ref()) => {
            tracing::debug!("config fell back to the defaults: {}", err.message());
            (ctx.defaults()?, Some(err))
        }
        Err(err) => return Err(err),
    };
    let Loaded {
        file,
        config_file,
        config_snapshot,
        decisions,
        settings,
        type_aware,
        mut off,
        mut on,
    } = loaded;
    let notices = super::notices::run_notices(&super::notices::NoticeInput {
        root: root.value(),
        env: &env,
        file: &file,
        config_file: &config_file,
        fallback: fallback.as_ref(),
    });
    // Each session of the run carries the notices with the diagnostics of
    // workspace discovery, so every output of the run holds them.
    for entry in [off.as_mut(), on.as_mut()].into_iter().flatten() {
        entry.workspace_diagnostics = fallow_types::workspace::merge_workspace_diagnostics(
            std::mem::take(&mut entry.workspace_diagnostics),
            notices.clone(),
        );
    }
    Ok(ResolvedRun {
        surface,
        command: inputs.command,
        root,
        config_file,
        config_snapshot,
        file,
        dead_code: decisions.dead_code,
        duplication: decisions.dupes,
        health: decisions.health,
        off,
        on,
        settings,
        type_aware,
        given,
        env,
        env_read,
        env_ignored,
        fallback,
        notices,
        inputs: Box::new(inputs.clone()),
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
    // A relative root is validated as the user gave it, so the check also
    // covers its spelling from `PWD`.
    let validated = match (inputs.root.is_some(), inputs.env().cwd()) {
        (true, Some(cwd)) => crate::validate::validate_root_in(candidate.value(), cwd),
        (true, None) => crate::validate::validate_root(&path),
        (false, _) => crate::validate::validate_current_dir_root(&path),
    };
    let canonical = validated
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
    config_snapshot: ConfigChainSnapshot,
    decisions: PerAnalysis<AnalysisDecision>,
    settings: RunSettings,
    type_aware: TypeAwareDecision,
    off: Option<ModeEntry>,
    on: Option<ModeEntry>,
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
    /// `explicit` is a config file with its source; `None` discovers the
    /// config from the root.
    fn load(&self, explicit: Option<(&Path, Source)>) -> Result<Loaded, ResolveError> {
        self.load_snapshot(explicit, AnalysisSnapshot::Current)
    }

    /// [`Self::load`] for the given revision. The base revision of
    /// `audit --base` skips the validation that needs a folder of the
    /// change, such as the module folder of the `deep-modules` preset.
    fn load_snapshot(
        &self,
        explicit: Option<(&Path, Source)>,
        snapshot: AnalysisSnapshot,
    ) -> Result<Loaded, ResolveError> {
        let options = ConfigFileOptions::new(*self.allow_remote_extends().value(), self.env);
        let ConfigFile {
            config,
            path,
            chain,
        } = load_config_file(self.root, explicit.as_ref().map(|(path, _)| *path), options)
            .map_err(ResolveError::ConfigLoad)?;
        steps::validate_command_config(self.inputs.command, self.root, &config, snapshot).map_err(
            |err| ResolveError::ConfigInvalid {
                file: path.clone(),
                message: err.to_string(),
            },
        )?;
        let config_file = match (explicit, path) {
            (Some((_, source)), Some(path)) => Sourced::new(Some(path), source, Scope::Global),
            (None, Some(path)) => Sourced::new(
                Some(path),
                Source::Derived { from: "discovery" },
                Scope::Global,
            ),
            (_, None) => Sourced::new(None, Source::Default { rule: "no-config" }, Scope::Global),
        };
        Ok(Loaded {
            config_snapshot: chain,
            ..self.decide(config, config_file, false)?
        })
    }

    /// The steps of [`Self::load`] with a config that the caller built.
    #[cfg(any(test, feature = "testing"))]
    fn inline(&self, config: FallowConfig) -> Result<Loaded, ResolveError> {
        steps::validate_config(self.root, &config).map_err(|err| ResolveError::ConfigInvalid {
            file: None,
            message: err.to_string(),
        })?;
        let config_file = Sourced::new(
            None,
            Source::Derived {
                from: "inline-config",
            },
            Scope::Global,
        );
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
        let (mut off, mut on) =
            self.mode_entries(&file, path.is_some(), &decisions, &settings, fell_back)?;
        if self.inputs.surface() == Surface::Lsp {
            let mut scoped = false;
            for entry in [off.as_mut(), on.as_mut()].into_iter().flatten() {
                scoped |= steps::scope_cache_dir_to_root(&mut entry.config);
            }
            if scoped {
                settings.cache_dir = Sourced::new(
                    PathBuf::new(),
                    Source::Derived {
                        from: "editor-cache-per-root",
                    },
                    Scope::Global,
                );
            }
        }
        let cache_dir = off.as_ref().or(on.as_ref()).map_or_else(
            || self.cache_dir_without_entry(file.cache.dir.clone()),
            |entry| entry.config.cache_dir.clone(),
        );
        settings.cache_dir = settings.cache_dir.map(|_| cache_dir);
        let type_aware = super::type_aware::decision(
            self.inputs.command,
            &self.inputs.type_aware,
            self.env,
            TypeAwareConfigInput {
                section: &file.type_aware,
                audit: file.audit.type_aware,
                file: path.as_deref(),
            },
        );
        Ok(Loaded {
            file,
            config_file,
            config_snapshot: ConfigChainSnapshot::default(),
            decisions,
            settings,
            type_aware,
            off,
            on,
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
            let given = self.given_settings();
            let env = given.env_for_steps(self.env);
            let mut entry = if lossy {
                steps::mode_entry_lossy(self.root, config, options, &env)
            } else {
                steps::mode_entry(self.root, config, options, &env).map_err(|err| {
                    ResolveError::Workspace {
                        message: err.to_string(),
                    }
                })?
            };
            given.apply(&mut entry.config);
            Ok(Some(entry))
        };
        Ok((entry(false)?, entry(true)?))
    }

    /// The given settings that win over the environment.
    fn given_settings(&self) -> GivenSettings {
        let inputs = self.inputs;
        GivenSettings {
            max_file_size_mb: inputs.max_file_size_mb.as_ref().map(|given| given.value),
            cache_dir: inputs.cache_dir.as_ref().map(|given| given.value.clone()),
            include_entry_exports: inputs
                .include_entry_exports
                .as_ref()
                .map(|given| given.value),
            package_baselines: inputs.package_baselines.as_ref().map(|given| given.value),
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
                Sourced::new(
                    super::available_threads(),
                    default("available-parallelism"),
                    Scope::Global,
                )
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
