//! The `FALLOW_PRODUCTION*` environment variables. The CLI and the typed MCP
//! routes read them at their own boundary through [`ProductionEnv`], so both
//! give one value the same meaning. The programmatic API and the Node bindings
//! do not read them: their callers pass explicit options.

use crate::ProductionAnalysis;

/// The global production variable.
pub const PRODUCTION_ENV: &str = "FALLOW_PRODUCTION";
/// The production variable of the dead-code analysis.
pub const PRODUCTION_DEAD_CODE_ENV: &str = "FALLOW_PRODUCTION_DEAD_CODE";
/// The production variable of the health analysis.
pub const PRODUCTION_HEALTH_ENV: &str = "FALLOW_PRODUCTION_HEALTH";
/// The production variable of the duplication analysis.
pub const PRODUCTION_DUPES_ENV: &str = "FALLOW_PRODUCTION_DUPES";

/// The boolean value of an environment variable: `1`, `true`, `yes` and `on`
/// are `true`; `0`, `false`, `no` and `off` are `false`. The case and the
/// surrounding white space do not matter. Each other value is `None`.
#[must_use]
pub fn bool_env_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// The values of the `FALLOW_PRODUCTION*` variables. `None` is a variable
/// that is not set or that has no boolean value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductionEnv {
    /// `FALLOW_PRODUCTION`.
    pub global: Option<bool>,
    /// `FALLOW_PRODUCTION_DEAD_CODE`.
    pub dead_code: Option<bool>,
    /// `FALLOW_PRODUCTION_HEALTH`.
    pub health: Option<bool>,
    /// `FALLOW_PRODUCTION_DUPES`.
    pub dupes: Option<bool>,
}

impl ProductionEnv {
    /// Read the variables from the environment of the process.
    #[must_use]
    pub fn from_process() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Read the variables through `lookup`, which returns the value of one
    /// variable.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let read = |name| lookup(name).as_deref().and_then(bool_env_value);
        Self {
            global: read(PRODUCTION_ENV),
            dead_code: read(PRODUCTION_DEAD_CODE_ENV),
            health: read(PRODUCTION_HEALTH_ENV),
            dupes: read(PRODUCTION_DUPES_ENV),
        }
    }

    /// The production override of one analysis from the environment: the
    /// variable of the analysis, else `FALLOW_PRODUCTION`. `None` means that
    /// the environment does not decide.
    #[must_use]
    pub const fn override_for(self, analysis: ProductionAnalysis) -> Option<bool> {
        let own = match analysis {
            ProductionAnalysis::DeadCode => self.dead_code,
            ProductionAnalysis::Health => self.health,
            ProductionAnalysis::Dupes => self.dupes,
        };
        match own {
            Some(value) => Some(value),
            None => self.global,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_values_ignore_case_and_white_space() {
        for value in ["1", "true", " YES ", "On"] {
            assert_eq!(bool_env_value(value), Some(true), "{value:?}");
        }
        for value in ["0", "false", "No", " off"] {
            assert_eq!(bool_env_value(value), Some(false), "{value:?}");
        }
        for value in ["", "2", "enabled"] {
            assert_eq!(bool_env_value(value), None, "{value:?}");
        }
    }

    #[test]
    fn the_variable_of_the_analysis_wins_over_the_global_one() {
        let env = ProductionEnv::from_lookup(|name| match name {
            PRODUCTION_ENV => Some("1".to_owned()),
            PRODUCTION_HEALTH_ENV => Some("false".to_owned()),
            PRODUCTION_DUPES_ENV => Some("maybe".to_owned()),
            _ => None,
        });
        assert_eq!(env.override_for(ProductionAnalysis::DeadCode), Some(true));
        assert_eq!(env.override_for(ProductionAnalysis::Health), Some(false));
        assert_eq!(env.override_for(ProductionAnalysis::Dupes), Some(true));
        assert_eq!(
            ProductionEnv::default().override_for(ProductionAnalysis::DeadCode),
            None
        );
    }
}
