//! The precedence of run decisions.
//!
//! Each decision has one candidate per input. [`pick`] keeps the candidate
//! with the highest rank `(Layer, Scope)`, so one order decides every
//! decision.

use std::path::Path;

use fallow_config::{
    PRODUCTION_DEAD_CODE_ENV, PRODUCTION_DUPES_ENV, PRODUCTION_ENV, PRODUCTION_HEALTH_ENV,
    ProductionAnalysis, ProductionConfig,
};
use fallow_types::sourced::{Given, Scope, Source, Sourced};

/// The rule name of the production default: production mode is off.
const PRODUCTION_DEFAULT: &str = "production-off";
/// The source name of a production config that the caller built without a
/// config file.
const IN_MEMORY_CONFIG: &str = "config";

/// The candidate with the highest rank wins. A tie keeps the first candidate.
/// The fallback applies only when no candidate is present.
fn pick<T>(
    fallback: Sourced<T>,
    candidates: impl IntoIterator<Item = Option<Sourced<T>>>,
) -> Sourced<T> {
    candidates
        .into_iter()
        .flatten()
        .reduce(|best, candidate| {
            if candidate.rank() > best.rank() {
                candidate
            } else {
                best
            }
        })
        .unwrap_or(fallback)
}

/// One value for each analysis that has a production mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerAnalysis<T> {
    /// The value of the dead-code analysis.
    pub dead_code: T,
    /// The value of the health analysis.
    pub health: T,
    /// The value of the duplication analysis.
    pub dupes: T,
}

impl<T> PerAnalysis<T> {
    /// The value of `analysis`.
    pub const fn get(&self, analysis: ProductionAnalysis) -> &T {
        match analysis {
            ProductionAnalysis::DeadCode => &self.dead_code,
            ProductionAnalysis::Health => &self.health,
            ProductionAnalysis::Dupes => &self.dupes,
        }
    }

    fn from_fn(mut f: impl FnMut(ProductionAnalysis) -> T) -> Self {
        Self {
            dead_code: f(ProductionAnalysis::DeadCode),
            health: f(ProductionAnalysis::Health),
            dupes: f(ProductionAnalysis::Dupes),
        }
    }
}

/// The production inputs of one run, before the config. The config is a
/// separate argument of [`decide_production`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ProductionInputs {
    /// The global value that the caller gives, for example `--production`.
    pub global_flag: Option<Given<bool>>,
    /// The value of each analysis that the caller gives, for example
    /// `--production-health`.
    pub own_flag: PerAnalysis<Option<Given<bool>>>,
    /// `FALLOW_PRODUCTION`, when the host reads it.
    pub global_env: Option<bool>,
    /// `FALLOW_PRODUCTION_<ANALYSIS>`, when the host reads it.
    pub own_env: PerAnalysis<Option<bool>>,
}

/// The production mode of each analysis, with the input that decides it.
///
/// The order is: the value of the analysis that the caller gives, the global
/// value that the caller gives, the variable of the analysis, the global
/// variable, the config, then the default `false`. `file` is the config file
/// that gave `config`. Without a file, `config` is a value that the caller
/// built: its value is the fallback, with [`Source::Default`] when it is the
/// default config and [`Source::Derived`] when it is not.
pub fn decide_production(
    inputs: &ProductionInputs,
    config: &ProductionConfig,
    file: Option<&Path>,
) -> PerAnalysis<Sourced<bool>> {
    let given = |value: Option<Given<bool>>, scope| {
        value.map(|given| {
            let source = Source::Given {
                origin: given.origin,
            };
            Sourced::new(given.value, source, scope)
        })
    };
    let environment = |value: Option<bool>, name, scope| {
        value.map(|value| Sourced::new(value, Source::Environment { name }, scope))
    };
    PerAnalysis::from_fn(|analysis| {
        pick(
            production_fallback(config, analysis),
            [
                given(*inputs.own_flag.get(analysis), Scope::Analysis),
                given(inputs.global_flag, Scope::Global),
                environment(
                    *inputs.own_env.get(analysis),
                    production_env_name(analysis),
                    Scope::Analysis,
                ),
                environment(inputs.global_env, PRODUCTION_ENV, Scope::Global),
                file.map(|file| production_config_value(config, file, analysis)),
            ],
        )
    })
}

const fn production_env_name(analysis: ProductionAnalysis) -> &'static str {
    match analysis {
        ProductionAnalysis::DeadCode => PRODUCTION_DEAD_CODE_ENV,
        ProductionAnalysis::Health => PRODUCTION_HEALTH_ENV,
        ProductionAnalysis::Dupes => PRODUCTION_DUPES_ENV,
    }
}

const fn production_config_key(analysis: ProductionAnalysis) -> &'static str {
    match analysis {
        ProductionAnalysis::DeadCode => "production.deadCode",
        ProductionAnalysis::Health => "production.health",
        ProductionAnalysis::Dupes => "production.dupes",
    }
}

/// The config candidate. The bool form applies to every analysis; each key
/// of the object form applies to one analysis.
fn production_config_value(
    config: &ProductionConfig,
    file: &Path,
    analysis: ProductionAnalysis,
) -> Sourced<bool> {
    let (key, scope) = match config {
        ProductionConfig::Global(_) => ("production", Scope::Global),
        ProductionConfig::PerAnalysis(_) => (production_config_key(analysis), Scope::Analysis),
    };
    Sourced::new(
        config.for_analysis(analysis),
        Source::ConfigFile {
            file: file.to_path_buf(),
            key,
        },
        scope,
    )
}

fn production_fallback(config: &ProductionConfig, analysis: ProductionAnalysis) -> Sourced<bool> {
    let source = if *config == ProductionConfig::default() {
        Source::Default {
            rule: PRODUCTION_DEFAULT,
        }
    } else {
        Source::Derived {
            from: IN_MEMORY_CONFIG,
        }
    };
    Sourced::new(config.for_analysis(analysis), source, Scope::Global)
}

#[cfg(test)]
#[path = "pick_tests.rs"]
mod tests;
