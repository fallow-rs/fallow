//! Git commands for test fixtures.
//!
//! A fixture repository must not read the Git configuration of the machine,
//! and it must not follow the repository state of an enclosing Git operation.
//! The tests of each crate build their fixture commands here.

use std::path::Path;
use std::process::Command;

/// Null device for the Git config variables. Git for Windows also accepts it.
const NULL_CONFIG: &str = "/dev/null";
/// Fixed identity for fixture commits.
const FIXTURE_NAME: &str = "test";
const FIXTURE_EMAIL: &str = "test@test.com";

/// Build a `git` command for a test fixture in `dir`.
///
/// The command removes the ambient repository variables that a Git hook sets
/// ([`crate::changed_files::AMBIENT_GIT_ENV_VARS`]), ignores the global and
/// the system config, and uses a fixed author and committer. A hook, a
/// signing key or an identity of the developer thus cannot reach the fixture.
#[expect(
    clippy::disallowed_methods,
    reason = "test fixtures run git without the analysis wrapper of the engine"
)]
pub fn fixture_git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    crate::git_env::clear_ambient_git_env(&mut command);
    command
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", NULL_CONFIG)
        .env("GIT_CONFIG_SYSTEM", NULL_CONFIG)
        .env("GIT_AUTHOR_NAME", FIXTURE_NAME)
        .env("GIT_AUTHOR_EMAIL", FIXTURE_EMAIL)
        .env("GIT_COMMITTER_NAME", FIXTURE_NAME)
        .env("GIT_COMMITTER_EMAIL", FIXTURE_EMAIL);
    command
}

/// Run git in `dir` and return its trimmed stdout.
///
/// # Panics
///
/// Panics with stdout and stderr when git does not start or fails.
pub fn fixture_git_capture(dir: &Path, args: &[&str]) -> String {
    let output = fixture_git_command(dir)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("git {args:?} did not start: {error}"));
    assert!(
        output.status.success(),
        "git {args:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Run git in `dir`.
///
/// # Panics
///
/// Panics with stdout and stderr when git does not start or fails.
pub fn fixture_git(dir: &Path, args: &[&str]) {
    fixture_git_capture(dir, args);
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::{fixture_git, fixture_git_capture, fixture_git_command};

    /// How a command sets one environment variable.
    #[derive(Debug, PartialEq, Eq)]
    enum EnvState {
        /// The command does not name the variable, so the child inherits it.
        Inherited,
        /// The command removes the variable.
        Removed,
        /// The command sets the variable to this value.
        Set(String),
    }

    fn env_state(command: &Command, name: &str) -> EnvState {
        match command
            .get_envs()
            .find(|(key, _)| key.to_str() == Some(name))
        {
            None => EnvState::Inherited,
            Some((_, None)) => EnvState::Removed,
            Some((_, Some(value))) => EnvState::Set(value.to_string_lossy().into_owned()),
        }
    }

    #[test]
    fn the_command_removes_the_ambient_repository_variables() {
        let command = fixture_git_command(Path::new("."));
        for var in crate::git_env::AMBIENT_GIT_ENV_VARS {
            assert_eq!(env_state(&command, var), EnvState::Removed, "{var}");
        }
    }

    #[test]
    fn the_command_ignores_the_global_and_system_config() {
        let command = fixture_git_command(Path::new("."));
        for (name, value) in [
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
            ("GIT_AUTHOR_NAME", "test"),
            ("GIT_AUTHOR_EMAIL", "test@test.com"),
            ("GIT_COMMITTER_NAME", "test"),
            ("GIT_COMMITTER_EMAIL", "test@test.com"),
        ] {
            assert_eq!(
                env_state(&command, name),
                EnvState::Set(value.to_owned()),
                "{name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_global_hooks_path_does_not_reach_a_fixture_commit() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("temp dir");
        let (home, hooks, repo) = (
            temp.path().join("home"),
            temp.path().join("hooks"),
            temp.path().join("repo"),
        );
        for dir in [&home, &hooks, &repo] {
            std::fs::create_dir(dir).expect("create dir");
        }
        let marker = temp.path().join("hook-ran");
        let hook = hooks.join("pre-commit");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
        )
        .expect("write hook");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
            .expect("make the hook executable");
        std::fs::write(
            home.join(".gitconfig"),
            format!("[core]\n\thooksPath = {}\n", hooks.display()),
        )
        .expect("write global config");

        fixture_git(&repo, &["init", "-q"]);
        std::fs::write(repo.join("a.txt"), "a\n").expect("write file");
        for args in [&["add", "."][..], &["commit", "-q", "-m", "init"][..]] {
            let output = fixture_git_command(&repo)
                .env("HOME", &home)
                .env("XDG_CONFIG_HOME", home.join("xdg"))
                .args(args)
                .output()
                .expect("run git");
            assert!(
                output.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert!(!marker.exists(), "the hook of the global config ran");
    }

    #[test]
    fn capture_returns_trimmed_stdout() {
        let temp = tempfile::tempdir().expect("temp dir");
        fixture_git(temp.path(), &["init", "-q", "-b", "main"]);
        assert_eq!(
            fixture_git_capture(temp.path(), &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
    }
}
