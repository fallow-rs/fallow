//! Discovery inputs of the runtime coverage sidecar.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::coverage::{discover_sidecar_in, path_binary_candidates, sidecar_missing_message};

/// The process inputs that sidecar discovery reads.
///
/// Discovery reads two environment overrides, the home folder (for
/// `~/.fallow/bin/fallow-cov`) and the `PATH` search list. Production code
/// reads them from the process with [`SidecarSearch::from_process`]. A test
/// builds its own value, so that the add-on of the host does not change the
/// result.
#[derive(Debug, Clone, Default)]
pub struct SidecarSearch {
    /// The trimmed, non-empty value of `FALLOW_COV_BIN`.
    pub cov_bin: Option<String>,
    /// The trimmed, non-empty value of `FALLOW_COV_BINARY_PATH`.
    pub cov_binary_path: Option<String>,
    /// The home folder that holds `.fallow/bin`.
    pub home: Option<PathBuf>,
    /// The `PATH` search list.
    pub path: Option<OsString>,
}

impl SidecarSearch {
    /// Read the discovery inputs from the environment of this process.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            cov_bin: env_non_empty("FALLOW_COV_BIN"),
            cov_binary_path: env_non_empty("FALLOW_COV_BINARY_PATH"),
            home: fallow_license::user_home_dir(),
            path: std::env::var_os("PATH"),
        }
    }

    /// Return true when an environment override selects the sidecar.
    #[must_use]
    pub const fn has_env_override(&self) -> bool {
        self.cov_bin.is_some() || self.cov_binary_path.is_some()
    }

    /// Return the canonical install location, `<home>/.fallow/bin/fallow-cov`.
    #[must_use]
    pub fn canonical_sidecar_path(&self) -> PathBuf {
        let home = self.home.clone().unwrap_or_else(|| PathBuf::from("."));
        let binary = if cfg!(windows) {
            "fallow-cov.exe"
        } else {
            "fallow-cov"
        };
        home.join(".fallow").join("bin").join(binary)
    }
}

/// Find the sidecar with explicit discovery inputs.
pub fn discover_sidecar_with(
    root: Option<&Path>,
    search: &SidecarSearch,
) -> Result<PathBuf, String> {
    if let Some(path) = &search.cov_bin {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BIN is set to {path} but no file exists there. Unset FALLOW_COV_BIN to let fallow find the runtime coverage add-on, or point it at the fallow-cov binary."
        ));
    }

    if let Some(path) = &search.cov_binary_path {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BINARY_PATH is set to {path} but no file exists there. Unset FALLOW_COV_BINARY_PATH to let fallow find the runtime coverage add-on, or point it at the fallow-cov binary."
        ));
    }

    let canonical = search.canonical_sidecar_path();
    discover_sidecar_in(root, &canonical, || {
        find_on_path("fallow-cov", search.path.as_deref())
    })
    .ok_or_else(|| sidecar_missing_message(root, &canonical))
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

fn find_on_path(binary: &str, path_var: Option<&OsStr>) -> Option<PathBuf> {
    std::env::split_paths(path_var?).find_map(|dir| {
        for candidate_name in path_binary_candidates(binary) {
            let candidate = dir.join(candidate_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{SidecarSearch, discover_sidecar_with, path_binary_candidates};

    /// Discovery inputs that point only into the temporary `root` folder.
    fn isolated_search(root: &Path) -> SidecarSearch {
        SidecarSearch {
            home: Some(root.join("home")),
            ..SidecarSearch::default()
        }
    }

    fn temp_root() -> PathBuf {
        tempfile::tempdir()
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"))
            .keep()
    }

    #[test]
    fn discovery_reads_only_the_given_home_and_path() {
        let root = temp_root();
        let search = isolated_search(&root);
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));

        let missing = discover_sidecar_with(Some(&root), &search)
            .expect_err("an empty home and no PATH must find no sidecar");
        assert!(
            missing.contains(&search.canonical_sidecar_path().display().to_string()),
            "the message must name the canonical path of the given home: {missing}"
        );

        let path_dir = root.join("path-bin");
        std::fs::create_dir_all(&path_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", path_dir.display()));
        let on_path = path_dir.join(path_binary_candidates("fallow-cov")[0].as_str());
        std::fs::write(&on_path, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", on_path.display()));
        let with_path = SidecarSearch {
            path: Some(path_dir.into_os_string()),
            ..search.clone()
        };
        assert_eq!(discover_sidecar_with(Some(&root), &with_path), Ok(on_path));

        let canonical = search.canonical_sidecar_path();
        std::fs::create_dir_all(canonical.parent().expect("canonical parent"))
            .unwrap_or_else(|err| panic!("failed to create canonical dir: {err}"));
        std::fs::write(&canonical, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", canonical.display()));
        assert_eq!(
            discover_sidecar_with(Some(&root), &with_path),
            Ok(canonical),
            "the sidecar in the given home wins over PATH"
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn discovery_env_override_comes_from_the_search_inputs() {
        let root = temp_root();
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        let explicit = root.join("explicit-fallow-cov");
        std::fs::write(&explicit, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", explicit.display()));
        let search = SidecarSearch {
            cov_bin: Some(explicit.display().to_string()),
            ..isolated_search(&root)
        };

        assert!(search.has_env_override());
        assert!(!isolated_search(&root).has_env_override());
        assert_eq!(discover_sidecar_with(Some(&root), &search), Ok(explicit));

        let missing = SidecarSearch {
            cov_binary_path: Some(root.join("absent").display().to_string()),
            ..isolated_search(&root)
        };
        let err = discover_sidecar_with(Some(&root), &missing)
            .expect_err("a missing override file is an error");
        assert!(err.contains("FALLOW_COV_BINARY_PATH"), "got: {err}");

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }
}
