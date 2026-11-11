//! Trusted discovery and canonicalization of the version-matched companion.

use super::*;

pub(super) fn canonicalize_root(root: &Path) -> Result<PathBuf, TypeAwareError> {
    dunce::canonicalize(root).map_err(|err| {
        TypeAwareError(format!(
            "failed to resolve project root {}: {err}",
            root.display()
        ))
    })
}

pub(super) fn discover_type_aware_sidecar(_root: &Path) -> Result<PathBuf, String> {
    let current_exe = std::env::current_exe().ok();
    discover_type_aware_sidecar_from(
        non_empty_env("FALLOW_TYPE_AWARE_BIN").as_deref(),
        current_exe.as_deref(),
    )
}

pub(super) fn discover_type_aware_sidecar_from(
    override_value: Option<&str>,
    current_exe: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(value) = override_value {
        let path = PathBuf::from(value);
        if path.is_file() {
            return canonical_sidecar_path(&path);
        }
        return Err(format!(
            "FALLOW_TYPE_AWARE_BIN is set to {value}, but no file exists there. Point it at a trusted {SIDECAR_BINARY} executable."
        ));
    }

    if let Some(path) = current_exe.and_then(find_installed_sidecar) {
        return Ok(path);
    }

    Err(format!(
        "Type-aware sidecar `{SIDECAR_BINARY}` was not found next to the active Fallow executable. For an npm install, add the companion at the same version: `npm install --save-dev {SIDECAR_BINARY}@{}`. The `fallow` npm launcher then finds it. Otherwise put it in the same directory as Fallow or set FALLOW_TYPE_AWARE_BIN to a trusted executable path. The Fallow binary itself does not search project node_modules or PATH. The normal command still works without --type-aware.",
        env!("CARGO_PKG_VERSION")
    ))
}

pub(super) fn canonical_sidecar_path(path: &Path) -> Result<PathBuf, String> {
    dunce::canonicalize(path).map_err(|err| {
        format!(
            "failed to resolve type-aware sidecar {}: {err}",
            path.display()
        )
    })
}

pub(super) fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    })
}

#[expect(
    clippy::filetype_is_file,
    reason = "security-sensitive sidecar discovery accepts regular files only"
)]
pub(super) fn find_installed_sidecar(current_exe: &Path) -> Option<PathBuf> {
    let current_exe = dunce::canonicalize(current_exe).ok()?;
    let install_dir = current_exe.parent()?;
    binary_names(SIDECAR_BINARY).into_iter().find_map(|name| {
        let candidate = install_dir.join(name);
        let metadata = candidate.symlink_metadata().ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        let canonical = dunce::canonicalize(candidate).ok()?;
        (canonical.parent() == Some(install_dir)).then_some(canonical)
    })
}

pub(super) fn binary_names(binary: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            binary.to_owned(),
            format!("{binary}.exe"),
            format!("{binary}.cmd"),
            format!("{binary}.bat"),
        ]
    } else {
        vec![binary.to_owned()]
    }
}
