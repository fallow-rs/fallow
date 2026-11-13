//! The Impact record gate: which runs add a record to the user's Impact store.

use std::path::{Path, PathBuf};

/// Whether this run should be treated as CI for the Impact record gate. In
/// production it is `fallow_engine::ci_env::is_ci()`; under test it reads the per-test
/// `TEST_FORCE_CI` override (default `false`) so the suite records into its
/// isolated store regardless of the ambient CI env. The store path is ALWAYS
/// the per-test temp dir under `#[cfg(test)]` (see [`super::impact_config_dir`]), so
/// bypassing the CI gate in tests can never touch a real user store.
fn record_gate_is_ci() -> bool {
    #[cfg(test)]
    {
        super::TEST_FORCE_CI.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        fallow_engine::ci_env::is_ci()
    }
}

/// Whether the record gate skips a run on `root`: a CI run, or a project root
/// inside a system temp folder. A temp root is a test fixture or a throwaway
/// clone. Each one would add a store to the user config folder that nobody
/// reads again, and a test suite adds thousands of them (#4555).
pub(super) fn record_gate_skips(root: &Path) -> bool {
    record_gate_is_ci() || record_gate_is_temp_root(root)
}

/// Under test this is always `false`: the unit tests record on temp roots into
/// their isolated store. [`is_inside_any`] carries the tested logic.
fn record_gate_is_temp_root(root: &Path) -> bool {
    #[cfg(test)]
    {
        let _ = root;
        false
    }
    #[cfg(not(test))]
    {
        is_inside_any(root, &system_temp_dirs())
    }
}

/// The system temp folders: `std::env::temp_dir()`, and on Unix also `/tmp`,
/// because macOS sets `TMPDIR` to a per-user folder that is not `/tmp`.
/// `/var/tmp` is not in the list: Linux keeps it across reboots, and people
/// keep long-lived clones there.
#[cfg(not(test))]
fn system_temp_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    if cfg!(unix) {
        dirs.push(PathBuf::from("/tmp"));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    usable_temp_bases(dirs, home.as_deref())
}

/// Drop each temp folder candidate that is a filesystem root or the home
/// folder. A `TMPDIR` of `/` or `$HOME` would otherwise turn off Impact for
/// every project. A home folder inside a temp folder (a container with
/// `HOME=/tmp`, or a test) keeps the temp folder.
pub(super) fn usable_temp_bases(candidates: Vec<PathBuf>, home: Option<&Path>) -> Vec<PathBuf> {
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let home = home.map(canonical);
    candidates
        .into_iter()
        .filter(|base| base.parent().is_some())
        .filter(|base| home.as_ref() != Some(&canonical(base)))
        .collect()
}

/// Whether `root` is one of `bases` or inside one. The check compares the
/// given and the canonical form of each path, so a symlinked temp folder
/// (macOS `/tmp` and `/var` link into `/private`) and a root that does not
/// exist yet both match.
pub(super) fn is_inside_any(root: &Path, bases: &[PathBuf]) -> bool {
    let forms = |path: &Path| {
        let mut forms = vec![path.to_path_buf()];
        forms.extend(dunce::canonicalize(path).ok());
        forms
    };
    let roots = forms(root);
    bases
        .iter()
        .flat_map(|base| forms(base))
        .any(|base| roots.iter().any(|root| root.starts_with(&base)))
}
