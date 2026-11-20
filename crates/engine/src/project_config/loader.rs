//! The one loader of the fallow config file. Each engine entry point and the
//! programmatic API read the config file through [`load_config_file`], so the
//! trust policy and the error detail are the same on each surface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fallow_config::{ConfigLoadOptions, EnvSnapshot, FallowConfig};

use crate::EngineError;

/// The options of [`load_config_file`].
///
/// The default refuses remote `extends` entries and gives each fetch the
/// default time limit.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ConfigFileOptions {
    /// Permit `https://` entries in the `extends` chain of the config.
    pub allow_remote_extends: bool,
    /// The time limit of the fetch of one remote `extends` entry.
    pub extends_timeout: Duration,
}

impl ConfigFileOptions {
    /// The options with the trust policy `allow_remote_extends` and the
    /// time limit of `FALLOW_EXTENDS_TIMEOUT_SECS` in `env`.
    #[must_use]
    pub fn new(allow_remote_extends: bool, env: &EnvSnapshot) -> Self {
        Self {
            allow_remote_extends,
            extends_timeout: env.extends_timeout(),
        }
    }
}

impl Default for ConfigFileOptions {
    fn default() -> Self {
        let load = ConfigLoadOptions::default();
        Self {
            allow_remote_extends: load.allow_remote_extends,
            extends_timeout: load.extends_timeout,
        }
    }
}

/// The config file of a project, before resolution.
#[derive(Debug)]
pub struct ConfigFile {
    /// The parsed config. This is the default config when no file exists.
    pub config: FallowConfig,
    /// The path of the loaded file. `None` when no file exists.
    pub path: Option<PathBuf>,
    /// The bytes of the loaded file and of its local `extends` chain.
    pub chain: ConfigChainSnapshot,
}

/// The bytes of a config file and of each local file of its `extends`
/// chain. The loader reads them before and after it parses the file, so a
/// long-lived caller can see that the parsed config is out of date.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigChainSnapshot {
    /// Each file with its bytes, or `None` when the file did not read.
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
    /// A file of the chain changed while the loader parsed it.
    changed_during_load: bool,
}

impl ConfigChainSnapshot {
    /// Read the file at `path` and each local file that it extends. No path
    /// gives an empty snapshot.
    fn read(path: Option<&Path>) -> Self {
        let files = path
            .into_iter()
            .flat_map(FallowConfig::local_source_files)
            .map(|file| {
                let bytes = std::fs::read(&file).ok();
                (file, bytes)
            })
            .collect();
        Self {
            files,
            changed_during_load: false,
        }
    }

    /// The snapshot `after` the parse, marked as changed when it differs
    /// from the snapshot `before` the parse.
    fn around_load(before: &Self, after: Self) -> Self {
        Self {
            changed_during_load: *before != after,
            ..after
        }
    }

    /// Whether a file of the chain changed while the loader parsed it, or
    /// has other bytes now.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed_during_load
            || self
                .files
                .iter()
                .any(|(path, bytes)| std::fs::read(path).ok() != *bytes)
    }
}

/// The error of [`load_config_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigLoadError {
    /// The config file cannot be read or parsed, or its `extends` chain is
    /// refused.
    Parse {
        /// The explicit config path. `None` for a discovered file, whose path
        /// is in `message`.
        path: Option<PathBuf>,
        /// The top error.
        message: String,
        /// The top error and its causes. For a discovered file, this is the
        /// same text as `message`.
        chain: String,
    },
}

impl ConfigLoadError {
    /// The top error, without its causes.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Parse { message, .. } => message,
        }
    }

    /// The full error text: the error and its causes.
    #[must_use]
    pub fn chain(&self) -> &str {
        match self {
            Self::Parse { chain, .. } => chain,
        }
    }
}

impl From<ConfigLoadError> for EngineError {
    fn from(err: ConfigLoadError) -> Self {
        Self::new(format!("invalid config: {}", err.chain()))
    }
}

/// The config file that discovery finds from `root` up to the repository
/// root, without a load. A caller that only checks whether a project has a
/// config file, or that writes to the config file, calls this function.
#[must_use]
pub fn find_config_file(root: &Path) -> Option<PathBuf> {
    FallowConfig::find_config_path(root)
}

/// Load the config file of a project: the file at `config_path`, else the
/// first config file that discovery finds from `root` up to the repository
/// root. A project without a config file gets the default config.
///
/// # Errors
///
/// Returns [`ConfigLoadError::Parse`] when the file cannot be read or parsed,
/// or when `options` refuses an entry of its `extends` chain.
pub fn load_config_file(
    root: &Path,
    config_path: Option<&Path>,
    options: ConfigFileOptions,
) -> Result<ConfigFile, ConfigLoadError> {
    #[cfg(any(test, feature = "testing"))]
    load_count::record(root);
    let load_options = ConfigLoadOptions::new(options.allow_remote_extends)
        .with_extends_timeout(options.extends_timeout);
    if let Some(path) = config_path {
        let before = ConfigChainSnapshot::read(Some(path));
        return FallowConfig::load_with_options(path, load_options)
            .map(|config| ConfigFile {
                config,
                path: Some(path.to_path_buf()),
                chain: ConfigChainSnapshot::around_load(
                    &before,
                    ConfigChainSnapshot::read(Some(path)),
                ),
            })
            .map_err(|err| explicit_load_error(path, &err));
    }
    let before = ConfigChainSnapshot::read(find_config_file(root).as_deref());
    match FallowConfig::find_and_load_with_options(root, load_options) {
        Ok(Some((config, path))) => Ok(ConfigFile {
            config,
            chain: ConfigChainSnapshot::around_load(
                &before,
                ConfigChainSnapshot::read(Some(&path)),
            ),
            path: Some(path),
        }),
        Ok(None) => Ok(ConfigFile {
            config: FallowConfig::default(),
            path: None,
            chain: ConfigChainSnapshot::around_load(&before, ConfigChainSnapshot::default()),
        }),
        Err(message) => Err(ConfigLoadError::Parse {
            path: None,
            chain: message.clone(),
            message,
        }),
    }
}

/// The error of an explicit config path. The alternate format of a config
/// error report adds each cause to the top error.
fn explicit_load_error(path: &Path, err: &impl std::fmt::Display) -> ConfigLoadError {
    ConfigLoadError::Parse {
        path: Some(path.to_path_buf()),
        message: err.to_string(),
        chain: format!("{err:#}"),
    }
}

/// A count of the calls of [`load_config_file`] for each project root, for
/// tests that check how often one call reads the config file.
#[cfg(any(test, feature = "testing"))]
pub mod load_count {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, PoisonError};

    use rustc_hash::FxHashMap;

    static LOADS: Mutex<Option<FxHashMap<PathBuf, usize>>> = Mutex::new(None);

    pub(super) fn record(root: &Path) {
        let mut loads = LOADS.lock().unwrap_or_else(PoisonError::into_inner);
        *loads
            .get_or_insert_with(FxHashMap::default)
            .entry(root.to_path_buf())
            .or_default() += 1;
    }

    /// The number of config loads for `root` since the process started. Each
    /// test uses its own temporary root, so tests that run in parallel do not
    /// change the count of another test.
    #[must_use]
    pub fn config_loads(root: &Path) -> usize {
        LOADS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|loads| loads.get(root).copied())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_load_counts_for_its_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join(".git")).expect("git marker");
        assert_eq!(load_count::config_loads(dir.path()), 0);
        for _ in 0..2 {
            load_config_file(dir.path(), None, ConfigFileOptions::default()).expect("loads");
        }
        assert_eq!(load_count::config_loads(dir.path()), 2);
    }

    /// No server listens on port 1, so the fetch fails at once and offline.
    const UNREACHABLE_REMOTE_EXTENDS: &str = r#"{ "extends": "https://127.0.0.1:1/fallow.json" }"#;

    fn project(config: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().to_path_buf();
        std::fs::create_dir(root.join(".git")).expect("git marker");
        std::fs::write(root.join(".fallowrc.json"), config).expect("config");
        (dir, root)
    }

    #[test]
    fn explicit_error_keeps_the_path_and_the_cause_chain() {
        let (_dir, root) = project(r#"{ "production": "yes" }"#);
        let path = root.join(".fallowrc.json");
        let cause = FallowConfig::load(&path).expect_err("wrong type");
        let err = load_config_file(&root, Some(&path), ConfigFileOptions::default())
            .expect_err("wrong type");
        assert_eq!(
            err,
            ConfigLoadError::Parse {
                path: Some(path),
                message: cause.to_string(),
                chain: format!("{cause:#}"),
            }
        );
    }

    /// An error with one cause. Its alternate format adds the cause, as the
    /// default handler of a `miette::Report` does.
    struct WrappedError;

    impl std::fmt::Display for WrappedError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Failed to read config file")?;
            if f.alternate() {
                f.write_str(": no such file")?;
            }
            Ok(())
        }
    }

    #[test]
    fn explicit_error_chain_adds_each_cause() {
        let err = explicit_load_error(Path::new("fallow.json"), &WrappedError);
        assert_eq!(
            err,
            ConfigLoadError::Parse {
                path: Some(PathBuf::from("fallow.json")),
                message: "Failed to read config file".to_owned(),
                chain: "Failed to read config file: no such file".to_owned(),
            }
        );
    }

    #[test]
    fn file_options_take_the_extends_timeout_of_the_snapshot() {
        let env = EnvSnapshot::from_pairs(&[("FALLOW_EXTENDS_TIMEOUT_SECS", "9")], None);
        assert_eq!(
            ConfigFileOptions::new(true, &env).extends_timeout,
            Duration::from_secs(9)
        );
        assert_eq!(
            ConfigFileOptions::default().extends_timeout,
            Duration::from_secs(5)
        );
    }

    #[test]
    fn remote_extends_is_fetched_with_the_opt_in() {
        let (_dir, root) = project(UNREACHABLE_REMOTE_EXTENDS);
        let options = ConfigFileOptions::new(true, &EnvSnapshot::none());
        for config_path in [None, Some(root.join(".fallowrc.json"))] {
            let err =
                load_config_file(&root, config_path.as_deref(), options).expect_err("fetch fails");
            assert!(
                err.chain().contains("Failed to fetch remote config"),
                "{err:?}"
            );
        }
    }

    #[test]
    fn discovered_error_has_no_path_and_one_text() {
        let (_dir, root) = project(r#"{ "rules": "#);
        let cause = FallowConfig::find_and_load(&root).expect_err("syntax error");
        let err =
            load_config_file(&root, None, ConfigFileOptions::default()).expect_err("syntax error");
        assert_eq!(
            err,
            ConfigLoadError::Parse {
                path: None,
                message: cause.clone(),
                chain: cause,
            }
        );
    }

    #[test]
    fn the_chain_snapshot_sees_an_edit_of_each_local_file() {
        let (_dir, root) = project(r#"{ "extends": "./base.json" }"#);
        let base = root.join("base.json");
        std::fs::write(&base, r#"{ "entry": ["a.ts"] }"#).expect("base");
        let config = root.join(".fallowrc.json");
        for config_path in [None, Some(config.as_path())] {
            let file =
                load_config_file(&root, config_path, ConfigFileOptions::default()).expect("loads");
            assert!(!file.chain.changed());
            std::fs::write(&base, r#"{ "entry": ["b.ts"] }"#).expect("same-size edit");
            assert!(file.chain.changed());
            std::fs::write(&base, r#"{ "entry": ["a.ts"] }"#).expect("restore");
        }
    }

    #[test]
    fn a_chain_that_differs_around_the_parse_counts_as_changed() {
        let (_dir, root) = project("{}");
        let config = root.join(".fallowrc.json");
        let before = ConfigChainSnapshot::read(Some(&config));
        std::fs::write(&config, r#"{ "entry": [] }"#).expect("edit during the parse");
        let after = ConfigChainSnapshot::read(Some(&config));
        assert!(ConfigChainSnapshot::around_load(&before, after.clone()).changed());
        assert!(!ConfigChainSnapshot::around_load(&after.clone(), after).changed());
        assert!(
            ConfigChainSnapshot::around_load(&before, ConfigChainSnapshot::default()).changed(),
            "a discovered file that is gone at the parse counts as changed"
        );
    }

    #[test]
    fn remote_extends_is_refused_by_default() {
        let (_dir, root) = project(r#"{ "extends": "https://example.com/fallow.json" }"#);
        let err = load_config_file(&root, None, ConfigFileOptions::default()).expect_err("refused");
        assert!(err.chain().contains("Remote config extends"), "{err:?}");
    }

    #[test]
    fn a_project_without_a_file_gets_the_default_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join(".git")).expect("git marker");
        let file = load_config_file(dir.path(), None, ConfigFileOptions::default())
            .expect("defaults load");
        assert_eq!(file.path, None);
        assert_eq!(
            file.config.production,
            fallow_config::ProductionConfig::default()
        );
    }
}
