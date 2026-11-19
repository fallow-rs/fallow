//! The one loader of the fallow config file. Each engine entry point and the
//! programmatic API read the config file through [`load_config_file`], so the
//! trust policy and the error detail are the same on each surface.

use std::path::{Path, PathBuf};

use fallow_config::{ConfigLoadOptions, FallowConfig};

use crate::EngineError;

/// The options of [`load_config_file`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ConfigFileOptions {
    /// Permit `https://` entries in the `extends` chain of the config.
    pub allow_remote_extends: bool,
}

/// The config file of a project, before resolution.
#[derive(Debug)]
pub struct ConfigFile {
    /// The parsed config. This is the default config when no file exists.
    pub config: FallowConfig,
    /// The path of the loaded file. `None` when no file exists.
    pub path: Option<PathBuf>,
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
    let load_options = ConfigLoadOptions {
        allow_remote_extends: options.allow_remote_extends,
    };
    if let Some(path) = config_path {
        return FallowConfig::load_with_options(path, load_options)
            .map(|config| ConfigFile {
                config,
                path: Some(path.to_path_buf()),
            })
            .map_err(|err| explicit_load_error(path, &err));
    }
    match FallowConfig::find_and_load_with_options(root, load_options) {
        Ok(Some((config, path))) => Ok(ConfigFile {
            config,
            path: Some(path),
        }),
        Ok(None) => Ok(ConfigFile {
            config: FallowConfig::default(),
            path: None,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn remote_extends_is_fetched_with_the_opt_in() {
        let (_dir, root) = project(UNREACHABLE_REMOTE_EXTENDS);
        let options = ConfigFileOptions {
            allow_remote_extends: true,
        };
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
