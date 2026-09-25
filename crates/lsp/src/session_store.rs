//! Long-lived editor analysis sessions, one per project root.
//!
//! A run takes the session of each project root out of the store, walks the
//! project again, and parses only the files that changed. It puts the session
//! back when it finishes or is cancelled. A change to a config input marks
//! the store stale, and the next run then loads each session again. A kept
//! session also loads again when one of its config files changed, because
//! a config file that a user names or extends can have any name.

use std::path::{Path, PathBuf};

use fallow_api::EditorAnalysisSession;
use rustc_hash::FxHashMap;

/// Set to `0`, `false`, `off` or `no` to load a new session on each run, as
/// the server did before sessions lived across runs.
pub const SESSION_REUSE_ENV: &str = "FALLOW_LSP_REUSE_SESSION";

/// Whether the value of [`SESSION_REUSE_ENV`] keeps session reuse on.
#[must_use]
pub fn session_reuse_allowed(value: Option<&str>) -> bool {
    !value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
}

/// The fixed file names, other than [`fallow_config::CONFIG_FILE_NAMES`],
/// that can change the resolved config or the workspace set of a session.
/// The server asks the client to watch each of them. The `fallow.*` names
/// are legacy spellings that `initializationOptions.configPath` can name.
pub const SESSION_INPUT_FILE_NAMES: &[&str] = &[
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "deno.json",
    "deno.jsonc",
    "fallow.json",
    "fallow.jsonc",
    "fallow.yaml",
    "fallow.yml",
];

/// Whether a change to `path` can change the resolved config or the
/// workspace set of a session. Source files do not: a run parses them again.
#[must_use]
pub fn session_input_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name.ends_with(".d.ts") {
        return false;
    }
    name.starts_with("tsconfig")
        || name.starts_with("jsconfig")
        || SESSION_INPUT_FILE_NAMES.contains(&name)
        || fallow_config::CONFIG_FILE_NAMES.contains(&name)
}

/// The editor settings that shape the config of a session. A run reuses a
/// session only for the same settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionKey {
    pub config_path: Option<PathBuf>,
    pub allow_remote_extends: bool,
    pub production_override: Option<bool>,
}

/// The config files of a session and their content when the session
/// loaded: the config file and each local `extends` target.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigSources {
    /// Each file with its content, or `None` when the file did not read.
    files: Vec<(PathBuf, Option<Vec<u8>>)>,
    /// A config file changed while the session loaded.
    changed_during_load: bool,
}

impl ConfigSources {
    /// Read the config file at `config_path` and each local file that it
    /// extends. No config file gives an empty set.
    #[must_use]
    pub fn read(config_path: Option<&Path>) -> Self {
        let files = config_path
            .map(fallow_config::FallowConfig::local_source_files)
            .unwrap_or_default()
            .into_iter()
            .map(|path| {
                let content = std::fs::read(&path).ok();
                (path, content)
            })
            .collect();
        Self {
            files,
            changed_during_load: false,
        }
    }

    /// The config files of a session. `before` was read before the session
    /// loaded its config, and `after` was read after. When the two differ, a
    /// config file changed during the load, and the session can hold either
    /// version. The next run then loads the session again.
    #[must_use]
    pub fn around_load(before: &Self, after: Self) -> Self {
        Self {
            changed_during_load: *before != after,
            ..after
        }
    }

    /// Whether a config file changed since the session loaded.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed_during_load
            || self
                .files
                .iter()
                .any(|(path, content)| std::fs::read(path).ok() != *content)
    }
}

/// A session kept between runs, with the settings and config files that it
/// loaded with.
#[derive(Debug)]
struct KeptSession {
    key: SessionKey,
    sources: ConfigSources,
    session: EditorAnalysisSession,
}

/// The sessions of the project roots, kept between runs.
#[derive(Debug, Default)]
pub struct EditorSessionStore {
    enabled: bool,
    /// A config input changed since the sessions loaded.
    stale: bool,
    sessions: FxHashMap<PathBuf, KeptSession>,
}

impl EditorSessionStore {
    /// A store that keeps sessions between runs when `enabled`, and else
    /// drops each session after its run.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            ..Self::default()
        }
    }

    /// Turn reuse on or off. Turning it off returns the kept sessions, so the
    /// caller can write their parse cache.
    pub fn set_enabled(&mut self, enabled: bool) -> Vec<EditorAnalysisSession> {
        self.enabled = enabled;
        if enabled {
            return Vec::new();
        }
        self.drain()
    }

    /// A config input changed. The next run loads each session again.
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// Return the kept sessions when a config input changed since they
    /// loaded, and forget them. Also forget the roots that a run no longer
    /// analyzes. The caller writes the parse cache of the returned sessions
    /// outside the store lock.
    pub fn retire(&mut self, project_roots: &[PathBuf]) -> Vec<EditorAnalysisSession> {
        if std::mem::take(&mut self.stale) {
            return self.drain();
        }
        let gone: Vec<PathBuf> = self
            .sessions
            .keys()
            .filter(|root| !project_roots.contains(root))
            .cloned()
            .collect();
        gone.into_iter()
            .filter_map(|root| self.sessions.remove(&root).map(|kept| kept.session))
            .collect()
    }

    /// Take the session of `project_root` for one run, with its config
    /// files. A session kept for other settings is dropped.
    pub fn take(
        &mut self,
        project_root: &Path,
        key: &SessionKey,
    ) -> Option<(EditorAnalysisSession, ConfigSources)> {
        let kept = self.sessions.remove(project_root)?;
        (kept.key == *key).then_some((kept.session, kept.sources))
    }

    /// Whether a session of `project_root` is kept for the same settings.
    #[must_use]
    pub fn keeps(&self, project_root: &Path, key: &SessionKey) -> bool {
        self.sessions
            .get(project_root)
            .is_some_and(|kept| kept.key == *key)
    }

    /// Keep the session of `project_root` for the next run. Returns the
    /// session when the store does not keep it, so the caller can write its
    /// parse cache.
    pub fn put(
        &mut self,
        project_root: &Path,
        key: SessionKey,
        sources: ConfigSources,
        session: EditorAnalysisSession,
    ) -> Option<EditorAnalysisSession> {
        if !self.enabled || self.stale {
            return Some(session);
        }
        self.sessions
            .insert(
                project_root.to_path_buf(),
                KeptSession {
                    key,
                    sources,
                    session,
                },
            )
            .map(|replaced| replaced.session)
    }

    /// Whether the store keeps sessions between runs.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(test)]
    pub fn kept_session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Forget every kept session and return them.
    pub fn drain(&mut self) -> Vec<EditorAnalysisSession> {
        self.sessions
            .drain()
            .map(|(_, kept)| kept.session)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SessionKey {
        SessionKey {
            config_path: None,
            allow_remote_extends: false,
            production_override: None,
        }
    }

    fn session(root: &Path) -> EditorAnalysisSession {
        EditorAnalysisSession::load_default(root)
    }

    #[test]
    fn reuse_is_on_unless_the_env_value_turns_it_off() {
        assert!(session_reuse_allowed(None));
        assert!(session_reuse_allowed(Some("1")));
        assert!(session_reuse_allowed(Some("on")));
        for off in ["0", "false", "OFF", " no "] {
            assert!(!session_reuse_allowed(Some(off)), "{off}");
        }
    }

    #[test]
    fn config_inputs_mark_a_session_stale_and_sources_do_not() {
        for input in [
            "package.json",
            "tsconfig.app.json",
            "pnpm-workspace.yaml",
            "deno.jsonc",
            ".fallowrc.json",
            "fallow.toml",
        ] {
            assert!(session_input_file(Path::new(input)), "{input}");
        }
        for source in ["index.ts", "types.d.ts", "App.vue", "styles.css"] {
            assert!(!session_input_file(Path::new(source)), "{source}");
        }
    }

    #[test]
    fn config_sources_see_an_edit_to_an_extended_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join("fallow.json");
        let base = dir.path().join("base.json");
        std::fs::write(&config, r#"{"extends":"./base.json"}"#).expect("config");
        std::fs::write(&base, r#"{"entry":["a.ts"]}"#).expect("base");

        let sources = ConfigSources::read(Some(&config));
        assert!(!sources.changed());
        std::fs::write(&base, r#"{"entry":["b.ts"]}"#).expect("same-size edit");
        assert!(sources.changed());
        assert!(!ConfigSources::read(None).changed());
    }

    #[test]
    fn config_sources_that_moved_during_the_load_count_as_changed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let config = dir.path().join("fallow.json");
        std::fs::write(&config, "{}").expect("config");
        let before = ConfigSources::read(Some(&config));
        std::fs::write(&config, r#"{"entry":[]}"#).expect("edit during the load");
        let after = ConfigSources::read(Some(&config));

        assert!(ConfigSources::around_load(&before, after.clone()).changed());
        assert!(!ConfigSources::around_load(&after.clone(), after).changed());
    }

    #[test]
    fn a_kept_session_serves_the_same_root_and_settings_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let mut store = EditorSessionStore::new(true);

        assert!(
            store
                .put(root, key(), ConfigSources::default(), session(root))
                .is_none()
        );
        let other = SessionKey {
            production_override: Some(true),
            ..key()
        };
        assert!(store.take(root, &other).is_none(), "the settings differ");
        assert!(
            store
                .put(root, key(), ConfigSources::default(), session(root))
                .is_none()
        );
        assert!(store.take(root, &key()).is_some());
        assert!(store.take(root, &key()).is_none(), "a run takes it out");
    }

    #[test]
    fn a_disabled_store_keeps_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = EditorSessionStore::default();

        assert!(
            store
                .put(
                    dir.path(),
                    key(),
                    ConfigSources::default(),
                    session(dir.path())
                )
                .is_some()
        );
        assert!(store.take(dir.path(), &key()).is_none());
    }

    #[test]
    fn a_store_turned_off_returns_the_session_of_a_later_run() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let mut store = EditorSessionStore::new(true);
        assert!(
            store
                .put(root, key(), ConfigSources::default(), session(root))
                .is_none()
        );

        assert_eq!(store.set_enabled(false).len(), 1);
        assert!(
            store
                .put(root, key(), ConfigSources::default(), session(root))
                .is_some(),
            "a run that finishes after shutdown writes its own parse cache"
        );
    }

    #[test]
    fn a_stale_store_retires_every_session_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let mut store = EditorSessionStore::new(true);
        assert!(
            store
                .put(&root, key(), ConfigSources::default(), session(&root))
                .is_none()
        );

        store.mark_stale();
        assert!(
            store
                .put(&root, key(), ConfigSources::default(), session(&root))
                .is_some(),
            "a run that finishes after the change does not keep its session"
        );
        assert_eq!(store.retire(std::slice::from_ref(&root)).len(), 1);
        assert!(store.take(&root, &key()).is_none());
        assert!(
            store
                .put(&root, key(), ConfigSources::default(), session(&root))
                .is_none()
        );
        assert!(store.retire(std::slice::from_ref(&root)).is_empty());
    }

    #[test]
    fn a_root_that_a_run_no_longer_analyzes_is_retired() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = EditorSessionStore::new(true);
        assert!(
            store
                .put(
                    dir.path(),
                    key(),
                    ConfigSources::default(),
                    session(dir.path())
                )
                .is_none()
        );

        assert_eq!(store.retire(&[]).len(), 1);
    }
}
