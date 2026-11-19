//! Trusted discovery and canonicalization of the version-matched companion.

use super::*;

pub(super) fn canonicalize_root(root: &Path) -> Result<PathBuf, TypeAwareError> {
    dunce::canonicalize(root).map_err(|err| {
        TypeAwareError::from(format!(
            "failed to resolve project root {}: {err}",
            root.display()
        ))
    })
}

/// A failed sidecar discovery.
#[derive(Debug)]
pub(super) struct DiscoveryError {
    pub(super) message: String,
    /// True when no sidecar file exists where discovery looked. False when a
    /// file exists but its path does not resolve.
    pub(super) not_found: bool,
}

impl DiscoveryError {
    fn not_found(message: String) -> Self {
        Self {
            message,
            not_found: true,
        }
    }
}

impl From<String> for DiscoveryError {
    fn from(message: String) -> Self {
        Self {
            message,
            not_found: false,
        }
    }
}

/// A companion that discovery found, and where.
#[derive(Debug)]
pub(super) struct Discovered {
    pub(super) path: PathBuf,
    /// True when the companion comes from the `fallow type-aware setup`
    /// directory.
    pub(super) from_setup_directory: bool,
}

pub(super) fn discover_type_aware_sidecar(root: &Path) -> Result<PathBuf, DiscoveryError> {
    discover_type_aware_sidecar_with_source(root).map(|found| found.path)
}

pub(super) fn discover_type_aware_sidecar_with_source(
    root: &Path,
) -> Result<Discovered, DiscoveryError> {
    let current_exe = std::env::current_exe().ok();
    let store = crate::type_aware::setup_store::store_root();
    discover_type_aware_sidecar_from(
        non_empty_env("FALLOW_TYPE_AWARE_BIN").as_deref(),
        current_exe.as_deref(),
        store.as_deref(),
        Some(root),
    )
}

/// Discovery order: `FALLOW_TYPE_AWARE_BIN`, then the sibling of the active
/// executable, then the `fallow type-aware setup` directory.
pub(super) fn discover_type_aware_sidecar_from(
    override_value: Option<&str>,
    current_exe: Option<&Path>,
    setup_store: Option<&Path>,
    project_root: Option<&Path>,
) -> Result<Discovered, DiscoveryError> {
    if let Some(value) = override_value {
        let path = PathBuf::from(value);
        if path.is_file() {
            return canonical_sidecar_path(&path)
                .map(|path| Discovered {
                    path,
                    from_setup_directory: false,
                })
                .map_err(DiscoveryError::from);
        }
        return Err(DiscoveryError::not_found(format!(
            "FALLOW_TYPE_AWARE_BIN is set to {value}, but no file exists there. Point it at a trusted {SIDECAR_BINARY} executable."
        )));
    }

    if let Some(path) = current_exe.and_then(find_installed_sidecar) {
        return Ok(Discovered {
            path,
            from_setup_directory: false,
        });
    }

    if let Some(path) = setup_store.and_then(crate::type_aware::setup_store::find_setup_companion) {
        return Ok(Discovered {
            path,
            from_setup_directory: true,
        });
    }

    Err(DiscoveryError::not_found(format!(
        "Type-aware sidecar `{SIDECAR_BINARY}` was not found {}. {} The Fallow binary itself does not search project node_modules or PATH. The normal command still works without --type-aware.",
        searched_locations(cfg!(windows)),
        missing_sidecar_install_hint(project_root, current_exe)
    )))
}

/// The locations that discovery names in its error. No setup bundle exists
/// for Windows, so the Windows text does not point to the setup directory.
pub(super) fn searched_locations(windows: bool) -> &'static str {
    if windows {
        "next to the active Fallow executable"
    } else {
        "next to the active Fallow executable or in the `fallow type-aware setup` directory"
    }
}

/// How to get a companion that speaks the protocol of this Fallow build.
pub fn protocol_mismatch_hint() -> String {
    format!(
        "Run `fallow type-aware setup` to install the companion of this Fallow version, or install `{SIDECAR_BINARY}@{}` from npm.",
        env!("CARGO_PKG_VERSION")
    )
}

/// How to install the exact-version companion for each way to run Fallow.
/// The project command uses the package manager of `project_root` (npm when
/// no lockfile or `packageManager` field names one); the command is only shown.
/// A binary outside `node_modules` gets the `FALLOW_TYPE_AWARE_BIN` route,
/// because no npm launcher passes the companion to it.
pub(super) fn missing_sidecar_install_hint(
    project_root: Option<&Path>,
    current_exe: Option<&Path>,
) -> String {
    if crate::companion_install::is_standalone_fallow_binary(current_exe) {
        return standalone_install_hint(cfg!(windows));
    }
    let project_command = fallow_config::PackageManager::suggested_add_dev_command(
        project_root,
        &format!("{SIDECAR_BINARY}@{}", env!("CARGO_PKG_VERSION")),
    );
    format!(
        "For a project that has `fallow` as a devDependency, add the companion at the same version: `{project_command}`. For a global `fallow`, run `npm install --global {SIDECAR_BINARY}@{version}`. For npx, start both packages together: `npx -p fallow@{version} -p {SIDECAR_BINARY}@{version} fallow`, or set FALLOW_TYPE_AWARE_BIN to a trusted executable path.",
        version = env!("CARGO_PKG_VERSION")
    )
}

/// On Windows the npm launcher starts the companion through Node with a
/// script path, and `fallow type-aware setup` has no Windows bundle, so the
/// hint names only the npm install of Fallow there.
pub(super) fn standalone_install_hint(windows: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");
    let intro = "This Fallow binary is not the npm install (for example Homebrew or `cargo install`). No npm launcher passes the companion to it.";
    let npm_install = format!("`npm install --global fallow@{version} {SIDECAR_BINARY}@{version}`");
    if windows {
        return format!(
            "{intro} Install Fallow and the companion from npm: {npm_install}. Then run the `fallow` command of that install."
        );
    }
    format!(
        "{intro} Run `fallow type-aware setup` to download the signed companion of this version into your user directory. The companion needs Node.js 20 or later on PATH. Or set FALLOW_TYPE_AWARE_BIN to a trusted `{SIDECAR_BINARY}` executable."
    )
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

pub(super) fn find_installed_sidecar(current_exe: &Path) -> Option<PathBuf> {
    let current_exe = dunce::canonicalize(current_exe).ok()?;
    find_sidecar_in(current_exe.parent()?)
}

/// The companion executable directly inside `dir`: a regular file whose real
/// path stays in `dir`, so a link cannot point discovery somewhere else.
#[expect(
    clippy::filetype_is_file,
    reason = "security-sensitive sidecar discovery accepts regular files only"
)]
pub fn find_sidecar_in(dir: &Path) -> Option<PathBuf> {
    let install_dir = dunce::canonicalize(dir).ok()?;
    binary_names(SIDECAR_BINARY).into_iter().find_map(|name| {
        let candidate = install_dir.join(name);
        let metadata = candidate.symlink_metadata().ok()?;
        if !metadata.file_type().is_file() {
            return None;
        }
        let canonical = dunce::canonicalize(candidate).ok()?;
        (canonical.parent() == Some(install_dir.as_path())).then_some(canonical)
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
