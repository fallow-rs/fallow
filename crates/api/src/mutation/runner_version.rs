//! The installed version of the test runner package, read before a run.
//!
//! A run gets the runner version from the `ready` response of the companion
//! and stops when the version is outside the manifest range. The plan and
//! `fallow doctor` start no companion, so they read the `package.json` of the
//! installed runner package. The lookup follows the Node resolution that the
//! companion uses for `vitest/package.json`: the `node_modules` directory of
//! the project root, then the `node_modules` directory of each ancestor. The
//! lookup runs no project code.

use std::fs;
use std::path::Path;

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

/// The version of `package` in the first `node_modules` directory from
/// `root` up that holds it. `None` when no directory holds a `package.json`
/// with a string `version`.
fn installed_package_version(root: &Path, package: &str) -> Option<String> {
    root.ancestors()
        .filter(|dir| dir.file_name().is_none_or(|name| name != "node_modules"))
        .find_map(|dir| {
            let text =
                fs::read_to_string(dir.join("node_modules").join(package).join("package.json"))
                    .ok()?;
            let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
            manifest
                .get("version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
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
