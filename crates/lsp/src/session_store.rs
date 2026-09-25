//! Long-lived editor analysis sessions, one per project root.
//!
//! A run takes the session of each project root out of the store, walks the
//! project again, and parses only the files that changed. It puts the session
//! back when it finishes or is cancelled. A change to a config input marks
//! the store stale, and the next run then loads each session again.

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
        || matches!(
            name,
            "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "pnpm-workspace.yaml"
                | "yarn.lock"
                | "bun.lock"
                | "bun.lockb"
                | "fallow.json"
                | "fallow.jsonc"
                | "fallow.yaml"
                | "fallow.yml"
                | "fallow.toml"
        )
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

/// The sessions of the project roots, kept between runs.
#[derive(Debug, Default)]
pub struct EditorSessionStore {
    enabled: bool,
    /// A config input changed since the sessions loaded.
    stale: bool,
    sessions: FxHashMap<PathBuf, (SessionKey, EditorAnalysisSession)>,
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
            .filter_map(|root| self.sessions.remove(&root).map(|(_, session)| session))
            .collect()
    }

    /// Take the session of `project_root` for one run, when one is kept for
    /// the same settings.
    pub fn take(&mut self, project_root: &Path, key: &SessionKey) -> Option<EditorAnalysisSession> {
        let (kept_key, session) = self.sessions.remove(project_root)?;
        (kept_key == *key).then_some(session)
    }

    /// Keep the session of `project_root` for the next run. Returns the
    /// session when the store does not keep it, so the caller can write its
    /// parse cache.
    pub fn put(
        &mut self,
        project_root: &Path,
        key: SessionKey,
        session: EditorAnalysisSession,
    ) -> Option<EditorAnalysisSession> {
        if !self.enabled || self.stale {
            return Some(session);
        }
        self.sessions
            .insert(project_root.to_path_buf(), (key, session))
            .map(|(_, replaced)| replaced)
    }

    /// Forget every kept session and return them.
    pub fn drain(&mut self) -> Vec<EditorAnalysisSession> {
        self.sessions
            .drain()
            .map(|(_, (_, session))| session)
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
    fn a_kept_session_serves_the_same_root_and_settings_only() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let mut store = EditorSessionStore::new(true);

        assert!(store.put(root, key(), session(root)).is_none());
        let other = SessionKey {
            production_override: Some(true),
            ..key()
        };
        assert!(store.take(root, &other).is_none(), "the settings differ");
        assert!(store.put(root, key(), session(root)).is_none());
        assert!(store.take(root, &key()).is_some());
        assert!(store.take(root, &key()).is_none(), "a run takes it out");
    }

    #[test]
    fn a_disabled_store_keeps_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = EditorSessionStore::default();

        assert!(store.put(dir.path(), key(), session(dir.path())).is_some());
        assert!(store.take(dir.path(), &key()).is_none());
    }

    #[test]
    fn a_stale_store_retires_every_session_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().to_path_buf();
        let mut store = EditorSessionStore::new(true);
        assert!(store.put(&root, key(), session(&root)).is_none());

        store.mark_stale();
        assert!(
            store.put(&root, key(), session(&root)).is_some(),
            "a run that finishes after the change does not keep its session"
        );
        assert_eq!(store.retire(std::slice::from_ref(&root)).len(), 1);
        assert!(store.take(&root, &key()).is_none());
        assert!(store.put(&root, key(), session(&root)).is_none());
        assert!(store.retire(std::slice::from_ref(&root)).is_empty());
    }

    #[test]
    fn a_root_that_a_run_no_longer_analyzes_is_retired() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut store = EditorSessionStore::new(true);
        assert!(store.put(dir.path(), key(), session(dir.path())).is_none());

        assert_eq!(store.retire(&[]).len(), 1);
    }
}
