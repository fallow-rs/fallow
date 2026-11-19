//! A builder of resolved runs for tests, and a count of duplication runs.
//!
//! The builder fills [`RunInputs`] with test defaults and resolves them with
//! the steps of [`super::resolve::resolve_run`], so a test run has the same
//! decisions as a run of a surface.

use std::path::PathBuf;

use fallow_config::{
    CommandKind, EnvSnapshot, FallowConfig, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin};

use super::ResolvedRun;
use super::resolve::{ResolveError, resolve_run, resolve_run_inline};

/// The origin of each value that the builder gives.
const ORIGIN: Origin = Origin::ApiOption("ResolvedRunBuilder");

const fn given<T>(value: T) -> Given<T> {
    Given {
        value,
        origin: ORIGIN,
    }
}

/// A builder of one [`ResolvedRun`] for a test.
///
/// The defaults are the Rust API surface, an empty environment, the combined
/// command with every analysis, and one thread.
#[derive(Debug, Clone)]
pub struct ResolvedRunBuilder {
    root: PathBuf,
    surface: Surface,
    env: Vec<(String, String)>,
    command: CommandKind,
    selection: SelectionInput,
    inline: Option<FallowConfig>,
    config_file: Option<PathBuf>,
    production: Option<bool>,
    production_per_analysis: Vec<(PlannedAnalysis, bool)>,
    threads: usize,
}

impl ResolvedRunBuilder {
    /// A builder for a run in `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            surface: Surface::Api,
            env: Vec::new(),
            command: CommandKind::Combined,
            selection: SelectionInput::Booleans {
                dead_code: true,
                duplication: true,
                health: true,
                origin: ORIGIN,
            },
            inline: None,
            config_file: None,
            production: None,
            production_per_analysis: Vec::new(),
            threads: 1,
        }
    }

    /// The surface of the run.
    #[must_use]
    pub const fn surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    /// The environment variables of the run, as `(name, value)` pairs.
    #[must_use]
    pub fn env(mut self, pairs: &[(&str, &str)]) -> Self {
        self.env = pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect();
        self
    }

    /// The command and the selection of the run.
    #[must_use]
    pub fn command(mut self, command: CommandKind, selection: SelectionInput) -> Self {
        self.command = command;
        self.selection = selection;
        self
    }

    /// A config in place of a config file.
    #[must_use]
    pub fn config(mut self, config: FallowConfig) -> Self {
        self.inline = Some(config);
        self
    }

    /// An explicit config file.
    #[must_use]
    pub fn config_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_file = Some(path.into());
        self
    }

    /// The given production mode of one analysis, or of every analysis for
    /// `None`.
    #[must_use]
    pub fn production(mut self, analysis: Option<PlannedAnalysis>, on: bool) -> Self {
        match analysis {
            Some(analysis) => self.production_per_analysis.push((analysis, on)),
            None => self.production = Some(on),
        }
        self
    }

    /// The thread count of the run.
    #[must_use]
    pub const fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    fn inputs(&self) -> RunInputs {
        let pairs: Vec<(&str, &str)> = self
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let env = EnvSnapshot::from_pairs(&pairs, Some(&self.root));
        let mut inputs = RunInputs::new(self.surface, env, self.command, self.selection.clone());
        inputs.root = Some(given(self.root.clone()));
        inputs.config_path = self.config_file.clone().map(given);
        inputs.threads = Some(given(self.threads));
        inputs.production = self.production.map(given);
        for (analysis, on) in &self.production_per_analysis {
            let slot = match analysis {
                PlannedAnalysis::DeadCode => &mut inputs.production_dead_code,
                PlannedAnalysis::Duplication => &mut inputs.production_dupes,
                PlannedAnalysis::Health => &mut inputs.production_health,
            };
            *slot = Some(given(*on));
        }
        inputs
    }

    /// Resolve the run.
    ///
    /// # Panics
    ///
    /// Panics when the run does not resolve.
    #[must_use]
    pub fn build(self) -> ResolvedRun {
        match self.try_build() {
            Ok(run) => run,
            Err(err) => panic!("the test run does not resolve: {}", err.message()),
        }
    }

    /// Resolve the run.
    ///
    /// # Errors
    ///
    /// Returns the [`ResolveError`] of the resolver.
    fn try_build(self) -> Result<ResolvedRun, ResolveError> {
        let inputs = self.inputs();
        match self.inline {
            Some(config) => resolve_run_inline(&inputs, config),
            None => resolve_run(&inputs),
        }
    }

    /// Resolve the run, then apply `adjust` to the resolved config of each
    /// production mode.
    ///
    /// # Panics
    ///
    /// Panics when the run does not resolve.
    #[cfg(test)]
    #[must_use]
    fn build_adjusted(
        self,
        mut adjust: impl FnMut(&mut fallow_config::ResolvedConfig),
    ) -> ResolvedRun {
        let mut run = self.build();
        for entry in [run.off.as_mut(), run.on.as_mut()].into_iter().flatten() {
            adjust(&mut entry.config);
        }
        run
    }
}

/// A count of the duplication runs of the sessions of each project root, for
/// tests that check whether a run skips duplication.
pub mod duplication_runs {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    use rustc_hash::FxHashMap;

    static RUNS: Mutex<Option<FxHashMap<PathBuf, usize>>> = Mutex::new(None);

    pub(crate) fn record(root: &Path) {
        let mut runs = RUNS.lock().unwrap_or_else(PoisonError::into_inner);
        *runs
            .get_or_insert_with(FxHashMap::default)
            .entry(root.to_path_buf())
            .or_default() += 1;
    }

    /// The number of duplication runs of a session with the root `root` since
    /// the process started. Each test uses its own temporary root, so tests
    /// that run in parallel do not change the count of another test.
    #[must_use]
    pub fn count(root: &Path) -> usize {
        RUNS.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|runs| runs.get(root).copied())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use fallow_config::ProductionConfig;
    use fallow_types::sourced::Source;

    use super::*;
    use crate::resolved_run::AnalysisSlot;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        std::fs::create_dir(root.join(".git")).expect("git marker");
        std::fs::write(root.join("package.json"), "{}").expect("package.json");
        (dir, root)
    }

    #[test]
    fn an_inline_config_decides_without_a_file() {
        let (_dir, root) = project();
        let run = ResolvedRunBuilder::new(&root)
            .config(FallowConfig {
                production: ProductionConfig::Global(true),
                ..FallowConfig::default()
            })
            .build();
        let production = run.production(PlannedAnalysis::DeadCode);
        assert!(*production.value());
        assert_eq!(production.source(), &Source::Derived { from: "config" });
        assert_eq!(
            run.config_file().source(),
            &Source::Derived {
                from: "inline-config"
            }
        );
    }

    #[test]
    fn the_surface_decides_which_variables_apply() {
        let (_dir, root) = project();
        let builder = ResolvedRunBuilder::new(&root)
            .env(&[("FALLOW_PRODUCTION", "1")])
            .command(
                CommandKind::DeadCode,
                SelectionInput::Single(PlannedAnalysis::DeadCode),
            );
        let api = builder.clone().build();
        let cli = builder.surface(Surface::Cli).build();
        assert!(!*api.production(PlannedAnalysis::DeadCode).value());
        assert!(*cli.production(PlannedAnalysis::DeadCode).value());
    }

    #[test]
    fn a_given_value_wins_and_the_adjustment_borrows_a_local() {
        let (_dir, root) = project();
        std::fs::write(root.join("custom.json"), r#"{ "production": true }"#).expect("config");
        let mut adjusted = 0;
        let run = ResolvedRunBuilder::new(&root)
            .config_file(root.join("custom.json"))
            .production(Some(PlannedAnalysis::Health), false)
            .production(None, true)
            .threads(3)
            .build_adjusted(|config| {
                adjusted += 1;
                config.show_cascade = true;
            });
        assert_eq!(adjusted, 2, "one config per production mode");
        assert!(!*run.production(PlannedAnalysis::Health).value());
        assert!(*run.production(PlannedAnalysis::DeadCode).value());
        assert_eq!(*run.settings().threads.value(), 3);
        let AnalysisSlot::Runs(dead_code) = run.analysis(PlannedAnalysis::DeadCode) else {
            panic!("dead code runs");
        };
        assert!(dead_code.config().show_cascade);
    }

    #[test]
    fn try_build_returns_the_resolver_error() {
        let (_dir, root) = project();
        let err = ResolvedRunBuilder::new(&root)
            .threads(0)
            .try_build()
            .expect_err("zero threads");
        assert!(err.message().contains("thread"), "{}", err.message());
    }
}
