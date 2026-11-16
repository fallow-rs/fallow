//! Runtime-coverage sidecar discovery must not start a package manager.
//!
//! A package manager reads config from the analyzed root (`.yarnrc`
//! `yarn-path`, `.yarnrc.yml` `yarnPath` and plugins, `.pnpmfile.cjs`) and
//! can run a script that the repository ships. Sidecar discovery runs before
//! the sidecar signature check, so a spawn there runs project code on every
//! `--runtime-coverage` run. These tests put fake `npm`, `pnpm`, `yarn`,
//! `bun` and `corepack` executables first on `PATH`. Each fake writes a
//! marker file when something starts it.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::common::{commit_all, fallow_bin, git, scrub_analysis_env};

const FAKE_TOOLS: &[&str] = &["npm", "pnpm", "yarn", "bun", "corepack"];

struct Sandbox {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    fake_bin: PathBuf,
    marker: PathBuf,
    coverage: PathBuf,
    projects: Vec<PathBuf>,
}

impl Sandbox {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let base = tmp.path().canonicalize().expect("canonical temp dir");

        let home = base.join("home");
        fs::create_dir_all(&home).expect("create home");

        let marker = base.join("spawned.log");
        let fake_bin = base.join("fake-bin");
        fs::create_dir_all(&fake_bin).expect("create fake bin");
        for tool in FAKE_TOOLS {
            let script = fake_bin.join(tool);
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\nprintf '%s %s\\n' '{tool}' \"$*\" >> '{}'\nexit 1\n",
                    marker.display()
                ),
            )
            .expect("write fake tool");
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755))
                .expect("chmod fake tool");
        }

        let coverage = base.join("coverage-final.json");
        fs::write(&coverage, b"{}").expect("write coverage input");

        let projects = [
            ("npm", "npm@10.9.0", "package-lock.json"),
            ("pnpm", "pnpm@9.15.0", "pnpm-lock.yaml"),
            ("yarn", "yarn@1.22.22", "yarn.lock"),
            ("bun", "bun@1.2.0", "bun.lock"),
        ]
        .iter()
        .map(|(name, package_manager, lockfile)| {
            let root = base.join(format!("project-{name}"));
            write_project(&root, Some(package_manager), lockfile);
            root
        })
        .chain(std::iter::once({
            // Lockfile only: detection falls back to `yarn.lock`.
            let root = base.join("project-lockfile-only");
            write_project(&root, None, "yarn.lock");
            root
        }))
        .collect();

        Self {
            _tmp: tmp,
            home,
            fake_bin,
            marker,
            coverage,
            projects,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(fallow_bin());
        cmd.env("RUST_LOG", "").env("NO_COLOR", "1");
        scrub_analysis_env(&mut cmd);
        for key in [
            "FALLOW_COV_BIN",
            "FALLOW_COV_BINARY_PATH",
            "FALLOW_LICENSE",
            "FALLOW_LICENSE_PATH",
            "FALLOW_BIN",
            "FALLOW_FORMAT",
            "FALLOW_QUIET",
        ] {
            cmd.env_remove(key);
        }
        cmd.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.fake_bin.display()))
            .args(args);
        cmd
    }

    fn spawned(&self) -> String {
        fs::read_to_string(&self.marker).unwrap_or_default()
    }
}

fn write_project(root: &Path, package_manager: Option<&str>, lockfile: &str) {
    fs::create_dir_all(root.join("src")).expect("create project");
    let package_json = package_manager.map_or_else(
        || r#"{"name":"victim","version":"1.0.0","main":"src/index.js"}"#.to_owned(),
        |pm| {
            format!(
                r#"{{"name":"victim","version":"1.0.0","main":"src/index.js","packageManager":"{pm}"}}"#
            )
        },
    );
    fs::write(root.join("package.json"), package_json).expect("write package.json");
    fs::write(root.join("src/index.js"), "export const value = 1;\n").expect("write source");
    fs::write(root.join(lockfile), "").expect("write lockfile");
    fs::write(root.join(".yarnrc"), "yarn-path \"./tools/yarn.js\"\n").expect("write .yarnrc");
    // `audit` needs a repository with a base branch before it reaches
    // sidecar discovery.
    git(root, &["init", "-q", "-b", "main"]);
    commit_all(root, "initial");
    // `audit` also needs a changed file against that base.
    fs::write(root.join("src/index.js"), "export const value = 2;\n").expect("change source");
}

/// Run fallow and assert that no fake package manager started. Returns the
/// combined stdout and stderr.
fn assert_no_spawn(sandbox: &Sandbox, label: &str, args: &[&str]) -> String {
    let output = sandbox.command(args).output().expect("run fallow");
    let combined = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let spawned = sandbox.spawned();
    assert!(
        spawned.is_empty(),
        "`{label}` started a package manager during sidecar discovery:\n{spawned}\n{combined}",
    );
    combined
}

#[test]
fn runtime_coverage_commands_start_no_package_manager() {
    let sandbox = Sandbox::new();
    let coverage = sandbox.coverage.to_string_lossy().into_owned();
    for root in &sandbox.projects {
        let root = root.to_string_lossy().into_owned();
        for command in ["health", "audit", "security"] {
            let combined = assert_no_spawn(
                &sandbox,
                &format!("{command} --runtime-coverage in {root}"),
                &[
                    command,
                    "--root",
                    &root,
                    "--runtime-coverage",
                    &coverage,
                    "--format",
                    "json",
                    "--quiet",
                ],
            );
            // Without this, a command that stops before discovery would pass.
            assert!(
                combined.contains("Sidecar binary fallow-cov not found"),
                "`{command}` in {root} did not reach sidecar discovery:\n{combined}"
            );
        }
    }
}

/// The JSON mode of `coverage setup` promises no prompts, writes, installs or
/// network. It must not start a package manager either.
#[test]
fn coverage_setup_json_starts_no_package_manager() {
    let sandbox = Sandbox::new();
    for root in &sandbox.projects {
        let root = root.to_string_lossy().into_owned();
        assert_no_spawn(
            &sandbox,
            &format!("coverage setup --yes --json in {root}"),
            &["coverage", "setup", "--root", &root, "--yes", "--json"],
        );
    }
}

#[test]
fn missing_sidecar_message_names_no_package_manager_command() {
    let sandbox = Sandbox::new();
    let coverage = sandbox.coverage.to_string_lossy().into_owned();
    let root = sandbox.projects[2].to_string_lossy().into_owned();
    let output = sandbox
        .command(&[
            "health",
            "--root",
            &root,
            "--runtime-coverage",
            &coverage,
            "--format",
            "json",
            "--quiet",
        ])
        .output()
        .expect("run fallow");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Sidecar binary fallow-cov not found"),
        "expected the missing-sidecar message, got:\n{combined}"
    );
    assert!(
        !combined.contains("yarn bin"),
        "the message must not name a package-manager lookup, got:\n{combined}"
    );
    assert!(
        combined.contains("FALLOW_COV_BIN"),
        "the message must name FALLOW_COV_BIN, got:\n{combined}"
    );
}
