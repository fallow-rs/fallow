//! The host folders and the `PATH` search list of this process.
//!
//! [`HostPaths::from_env`] is the one reader of `HOME`, `USERPROFILE`,
//! `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `APPDATA`, `LOCALAPPDATA` and `PATH`.
//! Production code calls it at a boundary and passes the value down. A test
//! builds its own value, so that the home folder or the tools of the host do
//! not change the result. `scripts/check-host-reads.mjs` fails on a direct
//! read of these variables outside this module and outside tests.

use std::ffi::OsString;
use std::path::PathBuf;

/// The host folders and the search list that Fallow reads.
///
/// Each field is `None` when the variable is unset or empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostPaths {
    /// The home folder: `HOME`, else `USERPROFILE`.
    pub home: Option<PathBuf>,
    /// `XDG_CONFIG_HOME`.
    pub config_dir: Option<PathBuf>,
    /// `XDG_DATA_HOME`.
    pub data_dir: Option<PathBuf>,
    /// `APPDATA` (Windows).
    pub app_data: Option<PathBuf>,
    /// `LOCALAPPDATA` (Windows).
    pub local_app_data: Option<PathBuf>,
    /// The `PATH` search list.
    pub search_path: Option<OsString>,
}

impl HostPaths {
    /// Read the host paths from the environment of this process.
    #[must_use]
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    /// Read the host paths with a lookup function in place of the process
    /// environment.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<OsString>) -> Self {
        let value = |key: &str| lookup(key).filter(|value| !value.is_empty());
        let folder = |key: &str| value(key).map(PathBuf::from);
        Self {
            home: folder("HOME").or_else(|| folder("USERPROFILE")),
            config_dir: folder("XDG_CONFIG_HOME"),
            data_dir: folder("XDG_DATA_HOME"),
            app_data: folder("APPDATA"),
            local_app_data: folder("LOCALAPPDATA"),
            search_path: value("PATH"),
        }
    }

    /// Host paths with only a home folder. Tests use it to point the
    /// per-user folders at a temporary folder.
    #[must_use]
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
            ..Self::default()
        }
    }

    /// The platform base of per-user config: `APPDATA` on Windows,
    /// `~/Library/Application Support` on macOS, else `XDG_CONFIG_HOME` or
    /// `~/.config`.
    #[must_use]
    pub fn platform_config_base(&self) -> Option<PathBuf> {
        if cfg!(windows) {
            self.app_data.clone()
        } else if cfg!(target_os = "macos") {
            self.home
                .as_ref()
                .map(|home| home.join("Library").join("Application Support"))
        } else {
            self.xdg_config_base()
        }
    }

    /// The XDG config base on every platform: `XDG_CONFIG_HOME`, else
    /// `~/.config`.
    #[must_use]
    pub fn xdg_config_base(&self) -> Option<PathBuf> {
        self.config_dir
            .clone()
            .or_else(|| self.home.as_ref().map(|home| home.join(".config")))
    }

    /// The folders of the `PATH` search list, in order.
    pub fn search_dirs(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.search_path
            .iter()
            .flat_map(|value| std::env::split_paths(value))
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "tests fail on a broken setup")]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn home_prefers_home_over_userprofile() {
        let host = HostPaths::from_lookup(lookup(&[
            ("HOME", "/home/alice"),
            ("USERPROFILE", r"C:\Users\alice"),
        ]));
        assert_eq!(host.home, Some(PathBuf::from("/home/alice")));
    }

    #[test]
    fn home_falls_back_to_userprofile() {
        let host = HostPaths::from_lookup(lookup(&[("USERPROFILE", r"C:\Users\alice")]));
        assert_eq!(host.home, Some(PathBuf::from(r"C:\Users\alice")));
    }

    #[test]
    fn empty_values_count_as_unset() {
        let host = HostPaths::from_lookup(lookup(&[
            ("HOME", ""),
            ("USERPROFILE", r"C:\Users\alice"),
            ("XDG_CONFIG_HOME", ""),
            ("PATH", ""),
        ]));
        assert_eq!(host.home, Some(PathBuf::from(r"C:\Users\alice")));
        assert_eq!(host.config_dir, None);
        assert_eq!(host.search_path, None);
    }

    #[test]
    fn xdg_config_base_prefers_the_variable() {
        let host = HostPaths::from_lookup(lookup(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/x")]));
        assert_eq!(host.xdg_config_base(), Some(PathBuf::from("/x")));
        assert_eq!(
            HostPaths::with_home("/h").xdg_config_base(),
            Some(PathBuf::from("/h").join(".config"))
        );
    }

    #[test]
    fn platform_config_base_follows_the_platform() {
        let host = HostPaths::from_lookup(lookup(&[
            ("HOME", "/h"),
            ("XDG_CONFIG_HOME", "/x"),
            ("APPDATA", "/a"),
        ]));
        let expected = if cfg!(windows) {
            PathBuf::from("/a")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/h")
                .join("Library")
                .join("Application Support")
        } else {
            PathBuf::from("/x")
        };
        assert_eq!(host.platform_config_base(), Some(expected));
    }

    #[test]
    fn search_dirs_split_the_search_path() {
        let joined = std::env::join_paths(["/one", "/two"]).expect("join");
        let host = HostPaths {
            search_path: Some(joined),
            ..HostPaths::default()
        };
        assert_eq!(
            host.search_dirs().collect::<Vec<_>>(),
            vec![PathBuf::from("/one"), PathBuf::from("/two")]
        );
        assert_eq!(HostPaths::default().search_dirs().count(), 0);
    }
}
