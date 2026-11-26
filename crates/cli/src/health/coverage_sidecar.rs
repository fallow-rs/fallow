//! Discovery inputs of the runtime coverage sidecar.

use std::path::{Path, PathBuf};

use fallow_process::HostPaths;

use super::coverage::{
    discover_sidecar_in, find_scoped_platform_sidecar, path_binary_candidates, sidecar_binary_name,
    sidecar_missing_message,
};

/// The process inputs that sidecar discovery reads.
///
/// Discovery reads two environment overrides, and the host paths: the home
/// folder (for `~/.fallow/bin/fallow-cov`) and the `PATH` search list.
/// Production code reads them from the process with
/// [`SidecarSearch::from_process`]. A test builds its own value, so that the
/// add-on of the host does not change the result.
#[derive(Debug, Clone, Default)]
pub struct SidecarSearch {
    /// The trimmed, non-empty value of `FALLOW_COV_BIN`.
    pub cov_bin: Option<String>,
    /// The trimmed, non-empty value of `FALLOW_COV_BINARY_PATH`.
    pub cov_binary_path: Option<String>,
    /// The home folder that holds `.fallow/bin`, and the `PATH` search list.
    pub host: HostPaths,
}

impl SidecarSearch {
    /// Read the discovery inputs from the environment of this process.
    #[must_use]
    pub fn from_process() -> Self {
        Self {
            cov_bin: env_non_empty("FALLOW_COV_BIN"),
            cov_binary_path: env_non_empty("FALLOW_COV_BINARY_PATH"),
            host: HostPaths::from_env(),
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
        let home = self.host.home.clone().unwrap_or_else(|| PathBuf::from("."));
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
        find_on_path("fallow-cov", &search.host)
    })
    .ok_or_else(|| sidecar_missing_message(root, &canonical))
}

/// Return the path of the `.sig` file next to `binary`.
pub(super) fn signature_path(binary: &Path) -> PathBuf {
    let mut path = binary.as_os_str().to_os_string();
    path.push(".sig");
    PathBuf::from(path)
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

/// Find the sidecar in the first `PATH` folder that has a candidate.
///
/// In that folder, a signed candidate wins over an unsigned one. On Windows a
/// folder can hold an extensionless shell shim next to `fallow-cov.exe`.
fn find_on_path(binary: &str, host: &HostPaths) -> Option<PathBuf> {
    host.search_dirs().find_map(|dir| {
        let entries: Vec<PathBuf> = path_binary_candidates(binary)
            .into_iter()
            .map(|name| dir.join(name))
            .filter(|candidate| candidate.is_file())
            .collect();
        let first = entries.first().cloned()?;
        Some(
            entries
                .into_iter()
                .find_map(|entry| signed_path_entry(&entry))
                .unwrap_or(first),
        )
    })
}

/// The `name` of the npm package that holds the Node launcher.
const LAUNCHER_PACKAGE_NAME: &str = "@fallow-cli/fallow-cov";

/// The largest shim or `package.json` file that discovery reads.
const MAX_METADATA_BYTES: u64 = 64 * 1024;

/// Return the signed binary for a `PATH` entry, or `None`.
///
/// A global install puts the `@fallow-cli/fallow-cov` launcher on `PATH`. The
/// launcher has no `.sig` file. The signed binary is in the platform package
/// of the same install. Discovery reads the file system only and never starts
/// the launcher.
///
/// The shim and `package.json` files only select a candidate. They are not a
/// trust boundary: the signature check runs on the returned binary before
/// fallow starts it.
fn signed_path_entry(entry: &Path) -> Option<PathBuf> {
    if signature_path(entry).is_file() {
        return Some(entry.to_path_buf());
    }
    launcher_package_dirs(entry)
        .iter()
        .find_map(|package| package_platform_sidecar(package))
}

/// The possible launcher package folders of a `PATH` entry.
///
/// - npm, Yarn and Bun on Unix link the entry to `<package>/bin/fallow-cov`.
/// - pnpm writes a shell shim that names its target in a `cmd-shim-target=`
///   comment.
/// - npm on Windows writes a shim in `<prefix>` and installs the package in
///   `<prefix>/node_modules`.
fn launcher_package_dirs(entry: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(target) = dunce::canonicalize(entry) {
        dirs.extend(launcher_package_of(&target));
    }
    if let Some(target) = shim_target(entry) {
        dirs.extend(launcher_package_of(&target));
    }
    if let Some(dir) = entry.parent() {
        dirs.push(
            dir.join("node_modules")
                .join("@fallow-cli")
                .join("fallow-cov"),
        );
    }
    dirs
}

/// Return `<package>` for a launcher at `<package>/bin/<name>`.
fn launcher_package_of(launcher: &Path) -> Option<PathBuf> {
    let bin_dir = launcher.parent()?;
    if bin_dir.file_name()? != "bin" {
        return None;
    }
    bin_dir.parent().map(Path::to_path_buf)
}

fn read_metadata(path: &Path) -> Option<String> {
    use std::io::Read;

    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn shim_target(shim: &Path) -> Option<PathBuf> {
    read_metadata(shim)?.lines().find_map(|line| {
        let (_, target) = line.split_once("cmd-shim-target=")?;
        let target = Path::new(target.trim());
        target.is_absolute().then(|| target.to_path_buf())
    })
}

/// Find the signed platform binary of a launcher package.
///
/// npm nests the platform package below the launcher package. Yarn, Bun and
/// pnpm put it next to the launcher package in the `@fallow-cli` folder.
fn package_platform_sidecar(package: &Path) -> Option<PathBuf> {
    let package = dunce::canonicalize(package).ok()?;
    if !is_launcher_package(&package) {
        return None;
    }
    let binary_name = sidecar_binary_name();
    find_scoped_platform_sidecar(
        &package.join("node_modules").join("@fallow-cli"),
        binary_name,
    )
    .or_else(|| find_scoped_platform_sidecar(package.parent()?, binary_name))
}

fn is_launcher_package(package: &Path) -> bool {
    read_metadata(&package.join("package.json"))
        .and_then(|contents| serde_json::from_str::<serde_json::Value>(&contents).ok())
        .is_some_and(|manifest| {
            manifest.get("name").and_then(serde_json::Value::as_str) == Some(LAUNCHER_PACKAGE_NAME)
        })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_process::HostPaths;

    use super::{SidecarSearch, discover_sidecar_with, path_binary_candidates};

    /// Discovery inputs that point only into the temporary `root` folder.
    fn isolated_search(root: &Path) -> SidecarSearch {
        SidecarSearch {
            host: HostPaths::with_home(root.join("home")),
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
            host: HostPaths {
                search_path: Some(path_dir.into_os_string()),
                ..search.host.clone()
            },
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

    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("parent"))
            .unwrap_or_else(|err| panic!("failed to create parent of {}: {err}", path.display()));
        std::fs::write(path, contents)
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    }

    /// Write the `@fallow-cli/fallow-cov` launcher package at `package` and
    /// return the path of its `bin/fallow-cov` launcher.
    fn write_launcher_package(package: &Path) -> PathBuf {
        write_file(
            &package.join("package.json"),
            r#"{"name":"@fallow-cli/fallow-cov","version":"0.5.1"}"#,
        );
        let launcher = package.join("bin").join("fallow-cov");
        write_file(&launcher, "#!/usr/bin/env node\n");
        launcher
    }

    /// Write a signed platform package in `scope_dir` and return its binary.
    fn write_platform_package(scope_dir: &Path) -> PathBuf {
        let binary = scope_dir
            .join("fallow-cov-test-platform")
            .join(super::sidecar_binary_name());
        write_file(&binary, "binary");
        write_file(&super::signature_path(&binary), "sig");
        binary
    }

    fn search_with_path(root: &Path, path_dir: &Path) -> SidecarSearch {
        let search = isolated_search(root);
        SidecarSearch {
            host: HostPaths {
                search_path: Some(path_dir.as_os_str().to_owned()),
                ..search.host.clone()
            },
            ..search
        }
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path)
            .unwrap_or_else(|err| panic!("failed to canonicalize {}: {err}", path.display()))
    }

    /// `npm install -g` links `<prefix>/bin/fallow-cov` to the Node launcher
    /// and nests the signed platform package below the launcher package.
    #[cfg(unix)]
    #[test]
    fn path_launcher_of_a_global_npm_install_resolves_to_the_platform_binary() {
        let root = temp_root();
        let prefix = root.join("prefix");
        let package = prefix.join("lib/node_modules/@fallow-cli/fallow-cov");
        let launcher = write_launcher_package(&package);
        let binary = write_platform_package(&package.join("node_modules/@fallow-cli"));
        let bin_dir = prefix.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        std::os::unix::fs::symlink(&launcher, bin_dir.join("fallow-cov")).expect("symlink");
        let project = root.join("project");
        std::fs::create_dir_all(&project).expect("create project");

        let found = discover_sidecar_with(Some(&project), &search_with_path(&root, &bin_dir))
            .expect("the global install must be found");
        assert_eq!(canonical(&found), canonical(&binary));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// Yarn and Bun hoist the platform package next to the launcher package.
    #[cfg(unix)]
    #[test]
    fn path_launcher_with_a_hoisted_platform_package_resolves_to_the_platform_binary() {
        let root = temp_root();
        let scope = root.join("global/node_modules/@fallow-cli");
        let launcher = write_launcher_package(&scope.join("fallow-cov"));
        let binary = write_platform_package(&scope);
        let bin_dir = root.join("global-bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        std::os::unix::fs::symlink(&launcher, bin_dir.join("fallow-cov")).expect("symlink");

        let found = discover_sidecar_with(None, &search_with_path(&root, &bin_dir))
            .expect("the hoisted install must be found");
        assert_eq!(canonical(&found), canonical(&binary));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// pnpm writes a shell shim, not a link. The shim names its target in a
    /// `cmd-shim-target=` comment.
    #[test]
    fn pnpm_shim_on_path_resolves_to_the_platform_binary() {
        let root = temp_root();
        let scope = root.join("store/links/node_modules/@fallow-cli");
        let launcher = write_launcher_package(&scope.join("fallow-cov"));
        let binary = write_platform_package(&scope);
        let bin_dir = root.join("pnpm-home").join("bin");
        write_file(
            &bin_dir.join(path_binary_candidates("fallow-cov")[0].as_str()),
            &format!(
                "#!/bin/sh\nexec node \"$basedir/x\" \"$@\"\n# cmd-shim-target={}\n",
                launcher.display()
            ),
        );

        let found = discover_sidecar_with(None, &search_with_path(&root, &bin_dir))
            .expect("the pnpm install must be found");
        assert_eq!(canonical(&found), canonical(&binary));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// The real pnpm layout links the global package into the store. The
    /// platform package is a link next to it in the store, not in the global
    /// folder.
    #[cfg(unix)]
    #[test]
    fn pnpm_shim_with_a_store_link_resolves_to_the_platform_binary() {
        use std::os::unix::fs::symlink;

        let root = temp_root();
        let store_scope = root.join("store/links/fallow-cov/node_modules/@fallow-cli");
        write_launcher_package(&store_scope.join("fallow-cov"));
        let platform_scope = root.join("store/links/platform/node_modules/@fallow-cli");
        let binary = write_platform_package(&platform_scope);
        symlink(
            platform_scope.join("fallow-cov-test-platform"),
            store_scope.join("fallow-cov-test-platform"),
        )
        .expect("link platform package");
        let global_scope = root.join("pnpm-home/global/v11/hash/node_modules/@fallow-cli");
        std::fs::create_dir_all(&global_scope).expect("create global scope");
        symlink(
            store_scope.join("fallow-cov"),
            global_scope.join("fallow-cov"),
        )
        .expect("link launcher package");
        let bin_dir = root.join("pnpm-home/bin");
        write_file(
            &bin_dir.join("fallow-cov"),
            &format!(
                "#!/bin/sh\n# cmd-shim-target={}\n",
                global_scope.join("fallow-cov/bin/fallow-cov").display()
            ),
        );

        let found = discover_sidecar_with(None, &search_with_path(&root, &bin_dir))
            .expect("the pnpm store install must be found");
        assert_eq!(canonical(&found), canonical(&binary));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// A relative `cmd-shim-target` does not select a package, so the shim
    /// stays the result.
    #[test]
    fn relative_shim_target_is_ignored() {
        let root = temp_root();
        let scope = root.join("relative/node_modules/@fallow-cli");
        write_launcher_package(&scope.join("fallow-cov"));
        write_platform_package(&scope);
        let bin_dir = root.join("relative/bin");
        let shim = bin_dir.join(path_binary_candidates("fallow-cov")[0].as_str());
        write_file(
            &shim,
            "#!/bin/sh\n# cmd-shim-target=../node_modules/@fallow-cli/fallow-cov/bin/fallow-cov\n",
        );

        let found = discover_sidecar_with(None, &search_with_path(&root, &bin_dir))
            .expect("the PATH entry is still a candidate");
        assert_eq!(found, shim);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// A signed binary on `PATH` is used as it is, also when a launcher
    /// package sits next to it.
    #[test]
    fn signed_path_entry_is_kept() {
        let root = temp_root();
        let prefix = root.join("signed-prefix");
        let package = prefix.join("node_modules/@fallow-cli/fallow-cov");
        write_launcher_package(&package);
        write_platform_package(&package.join("node_modules/@fallow-cli"));
        let entry = prefix.join(path_binary_candidates("fallow-cov")[0].as_str());
        write_file(&entry, "binary");
        write_file(&super::signature_path(&entry), "sig");

        let found = discover_sidecar_with(None, &search_with_path(&root, &prefix))
            .expect("the signed entry must be found");
        assert_eq!(found, entry);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// npm on Windows writes `<prefix>/fallow-cov.cmd` and installs the
    /// package below `<prefix>/node_modules`, with no `lib` folder.
    #[test]
    fn path_shim_with_a_sibling_node_modules_resolves_to_the_platform_binary() {
        let root = temp_root();
        let prefix = root.join("npm-prefix");
        let package = prefix.join("node_modules/@fallow-cli/fallow-cov");
        write_launcher_package(&package);
        let binary = write_platform_package(&package.join("node_modules/@fallow-cli"));
        write_file(
            &prefix.join(path_binary_candidates("fallow-cov")[0].as_str()),
            "#!/bin/sh\n",
        );

        let found = discover_sidecar_with(None, &search_with_path(&root, &prefix))
            .expect("the Windows npm layout must be found");
        assert_eq!(canonical(&found), canonical(&binary));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    /// A launcher that does not belong to `@fallow-cli/fallow-cov` stays as
    /// it is, so that the signature check reports the file on `PATH`.
    #[test]
    fn unknown_path_launcher_is_kept() {
        let root = temp_root();
        let prefix = root.join("other-prefix");
        let package = prefix.join("node_modules/@fallow-cli/fallow-cov");
        write_file(
            &package.join("package.json"),
            r#"{"name":"not-fallow-cov"}"#,
        );
        write_platform_package(&package.join("node_modules/@fallow-cli"));
        let shim = prefix.join(path_binary_candidates("fallow-cov")[0].as_str());
        write_file(&shim, "#!/bin/sh\n");

        let found = discover_sidecar_with(None, &search_with_path(&root, &prefix))
            .expect("the PATH entry is still a candidate");
        assert_eq!(found, shim);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }
}
