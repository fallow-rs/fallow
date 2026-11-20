//! The environment variables that change a run decision, read once per run.
//!
//! [`EnvSnapshot::capture`] reads every [`EnvVar`] and the current directory
//! at the entry of a surface. The resolver of the engine reads the snapshot,
//! never the process environment, so a test gives fixed values and one run
//! reads one set of values.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use crate::{
    CACHE_DIR_ENV, CACHE_MAX_SIZE_ENV, PACKAGE_BASELINES_ENV, PRODUCTION_DEAD_CODE_ENV,
    PRODUCTION_DUPES_ENV, PRODUCTION_ENV, PRODUCTION_HEALTH_ENV, bool_env_value,
};

/// The number of variables in [`EnvVar::ALL`].
const ENV_VAR_COUNT: usize = 17;

/// An environment variable that changes a run decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvVar {
    /// `FALLOW_PRODUCTION`.
    Production,
    /// `FALLOW_PRODUCTION_DEAD_CODE`.
    ProductionDeadCode,
    /// `FALLOW_PRODUCTION_HEALTH`.
    ProductionHealth,
    /// `FALLOW_PRODUCTION_DUPES`.
    ProductionDupes,
    /// `FALLOW_MAX_FILE_SIZE`.
    MaxFileSize,
    /// `FALLOW_CACHE_DIR`.
    CacheDir,
    /// `FALLOW_CACHE_MAX_SIZE`.
    CacheMaxSize,
    /// `FALLOW_PACKAGE_BASELINES`.
    PackageBaselines,
    /// `FALLOW_EXTENDS_TIMEOUT_SECS`.
    ExtendsTimeoutSecs,
    /// `FALLOW_CHANGED_SINCE`.
    ChangedSince,
    /// `FALLOW_DIFF_FILE`.
    DiffFile,
    /// `FALLOW_COVERAGE`.
    Coverage,
    /// `FALLOW_COVERAGE_ROOT`.
    CoverageRoot,
    /// `FALLOW_AUDIT_BASE`.
    AuditBase,
    /// `FALLOW_TYPE_AWARE`.
    TypeAware,
    /// `FALLOW_TYPE_AWARE_PROJECTS`.
    TypeAwareProjects,
    /// `FALLOW_TYPE_AWARE_REQUIRE`.
    TypeAwareRequire,
}

impl EnvVar {
    /// Every variable, in the order of the snapshot.
    pub const ALL: [Self; ENV_VAR_COUNT] = [
        Self::Production,
        Self::ProductionDeadCode,
        Self::ProductionHealth,
        Self::ProductionDupes,
        Self::MaxFileSize,
        Self::CacheDir,
        Self::CacheMaxSize,
        Self::PackageBaselines,
        Self::ExtendsTimeoutSecs,
        Self::ChangedSince,
        Self::DiffFile,
        Self::Coverage,
        Self::CoverageRoot,
        Self::AuditBase,
        Self::TypeAware,
        Self::TypeAwareProjects,
        Self::TypeAwareRequire,
    ];

    /// The name of the variable in the environment.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Production => PRODUCTION_ENV,
            Self::ProductionDeadCode => PRODUCTION_DEAD_CODE_ENV,
            Self::ProductionHealth => PRODUCTION_HEALTH_ENV,
            Self::ProductionDupes => PRODUCTION_DUPES_ENV,
            Self::MaxFileSize => "FALLOW_MAX_FILE_SIZE",
            Self::CacheDir => CACHE_DIR_ENV,
            Self::CacheMaxSize => CACHE_MAX_SIZE_ENV,
            Self::PackageBaselines => PACKAGE_BASELINES_ENV,
            Self::ExtendsTimeoutSecs => "FALLOW_EXTENDS_TIMEOUT_SECS",
            Self::ChangedSince => "FALLOW_CHANGED_SINCE",
            Self::DiffFile => "FALLOW_DIFF_FILE",
            Self::Coverage => "FALLOW_COVERAGE",
            Self::CoverageRoot => "FALLOW_COVERAGE_ROOT",
            Self::AuditBase => "FALLOW_AUDIT_BASE",
            Self::TypeAware => "FALLOW_TYPE_AWARE",
            Self::TypeAwareProjects => "FALLOW_TYPE_AWARE_PROJECTS",
            Self::TypeAwareRequire => "FALLOW_TYPE_AWARE_REQUIRE",
        }
    }

    /// The variable with the name `name`, if it is one of [`Self::ALL`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|var| var.name() == name)
    }

    /// The position of the variable in [`Self::ALL`] and in the snapshot.
    const fn index(self) -> usize {
        self as usize
    }
}

/// A parsed bool variable. `Invalid` keeps the fact, never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvValue<T> {
    /// The variable is not set.
    Unset,
    /// The variable has a valid value.
    Set(T),
    /// The variable is set, but its value is not valid.
    Invalid,
}

impl<T> EnvValue<T> {
    /// The valid value, or `None` for an unset or invalid variable.
    #[must_use]
    pub fn set(self) -> Option<T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Unset | Self::Invalid => None,
        }
    }
}

/// Why a run cannot use the value of a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvProblem {
    /// The value is not one of the values that the variable takes.
    InvalidValue,
    /// The value is not valid UTF-8.
    NotUnicode,
}

/// The values of every [`EnvVar`] and the current directory of one run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvSnapshot {
    values: [Option<OsString>; ENV_VAR_COUNT],
    cwd: Option<PathBuf>,
}

impl EnvSnapshot {
    /// Read every variable and the current directory from the process.
    #[must_use]
    pub fn capture() -> Self {
        Self {
            values: EnvVar::ALL.map(|var| std::env::var_os(var.name())),
            cwd: std::env::current_dir().ok(),
        }
    }

    /// A snapshot without variables and without a current directory.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The snapshot with `cwd` as the current directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// The snapshot with only the variables that `keep` accepts. The current
    /// directory stays.
    #[must_use]
    pub fn retain(mut self, keep: impl Fn(EnvVar) -> bool) -> Self {
        for var in EnvVar::ALL {
            if !keep(var) {
                self.values[var.index()] = None;
            }
        }
        self
    }

    /// The current directory, when the process has one.
    #[must_use]
    pub fn cwd(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// The raw value of `var`.
    #[must_use]
    pub fn get(&self, var: EnvVar) -> Option<&OsStr> {
        self.values[var.index()].as_deref()
    }

    /// Each variable that has a value, in the order of [`EnvVar::ALL`].
    pub fn set_vars(&self) -> impl Iterator<Item = EnvVar> + '_ {
        EnvVar::ALL
            .into_iter()
            .filter(|var| self.get(*var).is_some())
    }

    /// The cache directory of `FALLOW_CACHE_DIR`, parsed with
    /// [`crate::cache_dir_from_env_value`]. An empty value gives `None`.
    #[must_use]
    pub fn cache_dir(&self) -> Option<PathBuf> {
        self.get(EnvVar::CacheDir)
            .map(OsStr::to_os_string)
            .and_then(crate::cache_dir_from_env_value)
    }

    /// The time limit of the fetch of a remote `extends` entry, parsed from
    /// `FALLOW_EXTENDS_TIMEOUT_SECS` with
    /// [`crate::extends_timeout_from_env_value`].
    #[must_use]
    pub fn extends_timeout(&self) -> std::time::Duration {
        crate::extends_timeout_from_env_value(
            self.get(EnvVar::ExtendsTimeoutSecs).and_then(OsStr::to_str),
        )
    }

    /// The value of `var`, parsed with `parse`. `Ok(None)` is an unset
    /// variable.
    ///
    /// # Errors
    ///
    /// Returns [`EnvProblem::NotUnicode`] for a value that is not valid UTF-8
    /// and [`EnvProblem::InvalidValue`] for a value that `parse` rejects.
    pub fn parse<T>(
        &self,
        var: EnvVar,
        parse: impl FnOnce(&str) -> Option<T>,
    ) -> Result<Option<T>, EnvProblem> {
        let Some(raw) = self.get(var) else {
            return Ok(None);
        };
        let text = raw.to_str().ok_or(EnvProblem::NotUnicode)?;
        parse(text).map(Some).ok_or(EnvProblem::InvalidValue)
    }

    /// The paths of `var`, split with the path separator of the platform.
    #[must_use]
    pub fn paths(&self, var: EnvVar) -> Option<Vec<PathBuf>> {
        self.get(var)
            .map(|raw| std::env::split_paths(raw).collect())
    }

    /// The bool value of `var`, parsed with [`bool_env_value`].
    #[must_use]
    pub fn bool(&self, var: EnvVar) -> EnvValue<bool> {
        match self.get(var) {
            None => EnvValue::Unset,
            Some(raw) => raw
                .to_str()
                .and_then(bool_env_value)
                .map_or(EnvValue::Invalid, EnvValue::Set),
        }
    }

    /// A snapshot from `(name, value)` pairs. A name that is not an
    /// [`EnvVar`] is ignored.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn from_pairs(pairs: &[(&str, &str)], cwd: Option<&Path>) -> Self {
        let mut snapshot = Self {
            values: Default::default(),
            cwd: cwd.map(Path::to_path_buf),
        };
        for (name, value) in pairs {
            if let Some(var) = EnvVar::from_name(name) {
                snapshot.values[var.index()] = Some(OsString::from(value));
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use rustc_hash::FxHashSet;

    use super::*;

    #[test]
    fn every_variable_has_its_own_name_and_position() {
        let names: FxHashSet<&str> = EnvVar::ALL.iter().map(|var| var.name()).collect();
        assert_eq!(names.len(), EnvVar::ALL.len());
        for (position, var) in EnvVar::ALL.into_iter().enumerate() {
            assert_eq!(var.index(), position, "{var:?}");
            assert_eq!(EnvVar::from_name(var.name()), Some(var));
        }
    }

    #[test]
    fn bool_keeps_an_invalid_value_apart_from_an_unset_one() {
        let env = EnvSnapshot::from_pairs(
            &[
                ("FALLOW_PRODUCTION", " On "),
                ("FALLOW_PRODUCTION_HEALTH", "maybe"),
            ],
            None,
        );
        assert_eq!(env.bool(EnvVar::Production), EnvValue::Set(true));
        assert_eq!(env.bool(EnvVar::ProductionHealth), EnvValue::Invalid);
        assert_eq!(env.bool(EnvVar::ProductionDupes), EnvValue::Unset);
    }

    #[test]
    fn parse_tells_an_invalid_value_from_an_unset_variable() {
        let env = EnvSnapshot::from_pairs(&[("FALLOW_TYPE_AWARE", " On ")], None);
        assert_eq!(env.parse(EnvVar::TypeAware, bool_env_value), Ok(Some(true)));
        assert_eq!(
            env.parse(EnvVar::TypeAwareRequire, bool_env_value),
            Ok(None)
        );
        let env = EnvSnapshot::from_pairs(&[("FALLOW_TYPE_AWARE", "maybe")], None);
        assert_eq!(
            env.parse(EnvVar::TypeAware, bool_env_value),
            Err(EnvProblem::InvalidValue)
        );
    }

    #[cfg(unix)]
    #[test]
    fn parse_rejects_a_value_that_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let mut env = EnvSnapshot::none();
        env.values[EnvVar::TypeAware.index()] = Some(OsString::from_vec(vec![0x66, 0x80]));
        assert_eq!(
            env.parse(EnvVar::TypeAware, bool_env_value),
            Err(EnvProblem::NotUnicode)
        );
    }

    #[test]
    fn extends_timeout_reads_the_snapshot_and_keeps_the_default_for_a_bad_value() {
        let secs = |pairs: &[(&str, &str)]| {
            EnvSnapshot::from_pairs(pairs, None)
                .extends_timeout()
                .as_secs()
        };
        assert_eq!(secs(&[("FALLOW_EXTENDS_TIMEOUT_SECS", "12")]), 12);
        assert_eq!(secs(&[("FALLOW_EXTENDS_TIMEOUT_SECS", "0")]), 5);
        assert_eq!(secs(&[("FALLOW_EXTENDS_TIMEOUT_SECS", "soon")]), 5);
        assert_eq!(secs(&[]), 5);
        assert_eq!(
            crate::ConfigLoadOptions::new(true)
                .extends_timeout
                .as_secs(),
            5
        );
    }

    #[test]
    fn retain_drops_the_rejected_variables_and_keeps_the_cwd() {
        let cwd = Path::new("/work");
        let env = EnvSnapshot::from_pairs(
            &[
                ("FALLOW_PRODUCTION", "1"),
                ("FALLOW_CACHE_DIR", "c"),
                ("HOME", "/h"),
            ],
            Some(cwd),
        )
        .retain(|var| var != EnvVar::Production);
        assert_eq!(env.get(EnvVar::Production), None);
        assert_eq!(env.get(EnvVar::CacheDir), Some(OsStr::new("c")));
        assert_eq!(env.set_vars().collect::<Vec<_>>(), vec![EnvVar::CacheDir]);
        assert_eq!(env.cwd(), Some(cwd));
    }
}
