//! The installed version of the test runner package, read before a run.
//!
//! A run gets the runner version from the `ready` response of the companion
//! and stops when the version is outside the manifest range. The plan and
//! `fallow doctor` start no companion, so they read the `package.json` of the
//! installed runner package. The lookup follows the Node resolution that the
//! companion uses for `vitest/package.json`: the `node_modules` directory of
//! the project root, then the `node_modules` directory of each ancestor, then
//! the same lookup from the real directory of each package that the root
//! `package.json` lists. The lookup runs no project code.

use std::fs;
use std::path::{Path, PathBuf};

use fallow_output::MutationPlanRunner;

use super::manifest::{RunnerRange, manifest};
use super::runner_select::Runner;

/// The version facts of the selected runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerVersionCheck {
    /// The installed version of the runner package. `None` when the package
    /// is not installed, or when the check does not read a version.
    pub installed_version: Option<String>,
    /// The verdict. `None` when the version is the version of a runtime
    /// (`node` or `bun`), because the run selects the runtime when it starts.
    pub supported: Option<bool>,
}

/// Reads the installed version of the package of `runner` from `root`.
///
/// Only Vitest is a package. The `node-test` and `bun-test` ranges apply to
/// the runtime, so the check gives no verdict for them.
pub fn check_runner_version(
    runner: Runner,
    range: &RunnerRange,
    root: &Path,
) -> RunnerVersionCheck {
    if runner != Runner::Vitest {
        return RunnerVersionCheck {
            installed_version: None,
            supported: None,
        };
    }
    let installed_version = installed_package_version(root, &range.package);
    let supported = installed_version
        .as_deref()
        .is_some_and(|version| range.contains(version));
    RunnerVersionCheck {
        installed_version,
        supported: Some(supported),
    }
}

/// The `package.json` fields of the root whose packages can bring the
/// project its runner. The companion reads the same fields in the same order.
const ROOT_DEPENDENCY_FIELDS: [&str; 3] =
    ["dependencies", "devDependencies", "optionalDependencies"];

/// The `package.json` fields of a listed package that make the package bring
/// its own runner. A `devDependencies` entry does not count, because a
/// workspace package can link its own runner that the project does not run.
/// A `peerDependencies` entry does not count, because an unmet peer lets the
/// lookup reach a runner that pnpm hoists into `node_modules/.pnpm/node_modules`.
const BRINGS_RUNNER_FIELDS: [&str; 2] = ["dependencies", "optionalDependencies"];

/// True when the companion finds an installed runner `package` from `root`.
pub fn installs_package(root: &Path, package: &str) -> bool {
    runner_manifest(root, package).is_some()
}

/// The `version` of the installed `package` as the companion resolves it.
/// `None` when the package is not installed or its `package.json` has no
/// string `version`.
fn installed_package_version(root: &Path, package: &str) -> Option<String> {
    let text = fs::read_to_string(runner_manifest(root, package)?).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

/// The `package.json` of the runner `package` that the companion loads. The
/// project root comes first. Then each package that the root `package.json`
/// lists and that declares `package` in [`BRINGS_RUNNER_FIELDS`] can bring its
/// own runner, as a toolchain such as Vite+ does. A strict package manager
/// such as pnpm keeps that runner out of the root `node_modules`, next to the
/// real directory of the package, so the lookup starts from the real path. A
/// package that does not declare the runner is skipped: a lookup from it can
/// reach an unrelated runner that pnpm hoists into `node_modules/.pnpm/node_modules`.
fn runner_manifest(root: &Path, package: &str) -> Option<PathBuf> {
    if let Some(manifest) = node_modules_manifest(root, package) {
        return Some(manifest);
    }
    let root_manifest = read_manifest(&root.join("package.json"))?;
    dependency_names(&root_manifest, &ROOT_DEPENDENCY_FIELDS)
        .into_iter()
        .find_map(|name| {
            let dependency = fs::canonicalize(node_modules_manifest(root, &name)?).ok()?;
            let declares_runner =
                dependency_names(&read_manifest(&dependency)?, &BRINGS_RUNNER_FIELDS)
                    .iter()
                    .any(|dependency_name| dependency_name == package);
            if !declares_runner {
                return None;
            }
            node_modules_manifest(dependency.parent()?, package)
        })
}

/// The `package.json` of `package` in the first `node_modules` directory from
/// `from` up that holds it, as Node looks it up. A directory named
/// `node_modules` gets no nested `node_modules` lookup.
fn node_modules_manifest(from: &Path, package: &str) -> Option<PathBuf> {
    from.ancestors()
        .filter(|dir| dir.file_name().is_none_or(|name| name != "node_modules"))
        .map(|dir| dir.join("node_modules").join(package).join("package.json"))
        .find(|manifest| manifest.is_file())
}

fn read_manifest(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// The names that `manifest` lists in `fields`, in field order and without
/// duplicates.
fn dependency_names(manifest: &serde_json::Value, fields: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for field in fields {
        let Some(deps) = manifest.get(field).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for name in deps.keys() {
            if !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// The manifest range of `runner`.
///
/// # Errors
///
/// Returns the validation error of the embedded manifest, or a message when
/// the manifest has no range for `runner`.
pub fn runner_range(runner: Runner) -> Result<&'static RunnerRange, String> {
    let name = runner.name();
    manifest()?
        .runner(name)
        .ok_or_else(|| format!("the protocol manifest has no `{name}` runner"))
}

/// The `runner` object of the plan for `runner` at `root`.
///
/// # Errors
///
/// Returns the error of [`runner_range`].
pub fn plan_runner(runner: Runner, root: &Path) -> Result<MutationPlanRunner, String> {
    let range = runner_range(runner)?;
    let check = check_runner_version(runner, range, root);
    let unsupported = check.supported == Some(false);
    Ok(MutationPlanRunner {
        name: runner.name().to_owned(),
        package: range.package.clone(),
        message: unsupported
            .then(|| unsupported_message(range, check.installed_version.as_deref())),
        help: unsupported.then(|| runner_version_help(range)),
        installed_version: check.installed_version,
        supported_range: supported_range(range),
        supported: check.supported,
    })
}

/// The supported range as `>=<min> <<max>`, the form of the run error.
pub fn supported_range(range: &RunnerRange) -> String {
    format!(">={} <{}", range.min_version, range.max_version_exclusive)
}

/// The problem of a check without support: the same sentence as the run
/// error for an installed version, and a not-installed sentence otherwise.
pub fn unsupported_message(range: &RunnerRange, installed_version: Option<&str>) -> String {
    match installed_version {
        Some(version) => format!(
            "{} {version} is not supported; the supported range is {}",
            range.package,
            supported_range(range)
        ),
        None => format!(
            "{} is not installed; the supported range is {}",
            range.package,
            supported_range(range)
        ),
    }
}

/// The help of a runner version outside `range`. It names the supported
/// range as an install step. When the minimum is not a `.0.0` release, it
/// also says that a minor update inside the major version is enough.
pub fn runner_version_help(range: &RunnerRange) -> String {
    let RunnerRange {
        package,
        min_version,
        max_version_exclusive,
        ..
    } = range;
    let install =
        format!("install {package} {min_version} or a later version below {max_version_exclusive}");
    match min_version.split_once('.') {
        Some((major, rest)) if rest != "0.0" => {
            format!("{install}; a {package} {major}.x release from {min_version} is in the range")
        }
        _ => install,
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
mod tests {
    use super::*;

    fn vitest_range() -> &'static RunnerRange {
        manifest().unwrap().runner("vitest").unwrap()
    }

    fn install_vitest(dir: &Path, version: &str) {
        let package = dir.join("node_modules/vitest");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"vitest","version":"{version}"}}"#),
        )
        .unwrap();
    }

    #[test]
    fn an_installed_vitest_outside_the_range_is_not_supported() {
        let root = tempfile::tempdir().unwrap();
        install_vitest(root.path(), "3.2.4");
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version.as_deref(), Some("3.2.4"));
        assert_eq!(check.supported, Some(false));
        assert_eq!(
            unsupported_message(vitest_range(), Some("3.2.4")),
            "vitest 3.2.4 is not supported; the supported range is >=4.1.0 <6.0.0"
        );
    }

    #[test]
    fn a_vitest_in_an_ancestor_node_modules_is_found_as_node_finds_it() {
        let root = tempfile::tempdir().unwrap();
        install_vitest(root.path(), "4.1.5");
        let package = root.path().join("packages/app");
        fs::create_dir_all(&package).unwrap();
        let check = check_runner_version(Runner::Vitest, vitest_range(), &package);
        assert_eq!(check.installed_version.as_deref(), Some("4.1.5"));
        assert_eq!(check.supported, Some(true));
    }

    fn list_dev_dependency(root: &Path, name: &str) {
        fs::write(
            root.join("package.json"),
            format!(r#"{{"devDependencies":{{"{name}":"1.0.0"}}}}"#),
        )
        .unwrap();
    }

    /// Writes a toolchain package that lists Vitest as its own dependency.
    fn write_package(dir: &Path, name: &str) {
        let package = dir.join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("package.json"),
            format!(r#"{{"name":"{name}","dependencies":{{"vitest":"5.1.1"}}}}"#),
        )
        .unwrap();
    }

    #[test]
    fn a_vitest_of_a_listed_dependency_is_found_when_the_root_has_none() {
        let root = tempfile::tempdir().unwrap();
        list_dev_dependency(root.path(), "toolchain");
        write_package(&root.path().join("node_modules"), "toolchain");
        install_vitest(&root.path().join("node_modules/toolchain"), "5.1.1");
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version.as_deref(), Some("5.1.1"));
        assert_eq!(check.supported, Some(true));
    }

    #[cfg(unix)]
    #[test]
    fn a_pnpm_symlink_leads_to_the_sibling_vitest_of_a_listed_dependency() {
        let root = tempfile::tempdir().unwrap();
        let store = root.path().join("node_modules/.pnpm/toolchain@1.0.0");
        list_dev_dependency(root.path(), "toolchain");
        write_package(&store.join("node_modules"), "toolchain");
        install_vitest(&store, "5.1.1");
        std::os::unix::fs::symlink(
            store.join("node_modules/toolchain"),
            root.path().join("node_modules/toolchain"),
        )
        .unwrap();
        assert!(installs_package(root.path(), "vitest"));
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version.as_deref(), Some("5.1.1"));
    }

    #[test]
    fn the_root_vitest_wins_over_the_vitest_of_a_listed_dependency() {
        let root = tempfile::tempdir().unwrap();
        list_dev_dependency(root.path(), "toolchain");
        install_vitest(root.path(), "4.1.5");
        write_package(&root.path().join("node_modules"), "toolchain");
        install_vitest(&root.path().join("node_modules/toolchain"), "5.1.1");
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version.as_deref(), Some("4.1.5"));
    }

    #[test]
    fn a_vitest_of_an_unlisted_package_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("package.json"), "{}").unwrap();
        write_package(&root.path().join("node_modules"), "toolchain");
        install_vitest(&root.path().join("node_modules/toolchain"), "5.1.1");
        assert!(!installs_package(root.path(), "vitest"));
    }

    #[cfg(unix)]
    #[test]
    fn a_listed_package_that_reaches_only_a_hoisted_vitest_is_skipped() {
        let root = tempfile::tempdir().unwrap();
        let pnpm = root.path().join("node_modules/.pnpm");
        let library = pnpm.join("library@1.0.0/node_modules/library");
        let toolchain = pnpm.join("toolchain@1.0.0");
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"library":"1.0.0"},"devDependencies":{"toolchain":"1.0.0"}}"#,
        )
        .unwrap();
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("package.json"), r#"{"name":"library"}"#).unwrap();
        write_package(&toolchain.join("node_modules"), "toolchain");
        install_vitest(&toolchain, "5.1.1");
        install_vitest(&pnpm, "4.1.5");
        std::os::unix::fs::symlink(&library, root.path().join("node_modules/library")).unwrap();
        std::os::unix::fs::symlink(
            toolchain.join("node_modules/toolchain"),
            root.path().join("node_modules/toolchain"),
        )
        .unwrap();
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version.as_deref(), Some("5.1.1"));
        fs::remove_file(root.path().join("node_modules/toolchain")).unwrap();
        assert!(!installs_package(root.path(), "vitest"));
    }

    #[test]
    fn a_vitest_that_a_listed_package_has_only_as_a_dev_dependency_is_not_found() {
        let root = tempfile::tempdir().unwrap();
        let ui = root.path().join("node_modules/ui");
        fs::create_dir_all(&ui).unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"dependencies":{"ui":"workspace:*"}}"#,
        )
        .unwrap();
        fs::write(
            ui.join("package.json"),
            r#"{"name":"ui","devDependencies":{"vitest":"5.1.1"}}"#,
        )
        .unwrap();
        install_vitest(&ui, "5.1.1");
        assert!(!installs_package(root.path(), "vitest"));
    }

    #[test]
    fn an_optional_dependency_counts_and_a_peer_dependency_does_not() {
        for (field, found) in [("optionalDependencies", true), ("peerDependencies", false)] {
            let root = tempfile::tempdir().unwrap();
            let plugin = root.path().join("node_modules/plugin");
            fs::create_dir_all(&plugin).unwrap();
            list_dev_dependency(root.path(), "plugin");
            fs::write(
                plugin.join("package.json"),
                format!(r#"{{"name":"plugin","{field}":{{"vitest":"5.1.1"}}}}"#),
            )
            .unwrap();
            install_vitest(&plugin, "5.1.1");
            assert_eq!(installs_package(root.path(), "vitest"), found, "{field}");
        }
    }

    #[test]
    fn a_missing_vitest_is_not_supported_and_has_no_version() {
        let root = tempfile::tempdir().unwrap();
        let check = check_runner_version(Runner::Vitest, vitest_range(), root.path());
        assert_eq!(check.installed_version, None);
        assert_eq!(check.supported, Some(false));
    }

    #[test]
    fn a_runtime_runner_gets_no_verdict() {
        let root = tempfile::tempdir().unwrap();
        let range = manifest().unwrap().runner("node-test").unwrap();
        let check = check_runner_version(Runner::NodeTest, range, root.path());
        assert_eq!(check.installed_version, None);
        assert_eq!(check.supported, None);
    }
}
