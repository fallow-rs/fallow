//! Confine the files that `--save-baseline`, `--save-regression-baseline`,
//! `--save-snapshot`, `--output-file` and `--sarif-file` write to the project,
//! and keep the default cache directory inside it.
//!
//! A relative path resolves against the working directory, as it always did,
//! so the matching read path (`--baseline`, `--regression-baseline`) still
//! finds the file. The resolved path must then lie inside the project root, or
//! inside the Git work tree that contains the root when the working directory
//! is inside that tree too, or inside a shared directory (`GITHUB_WORKSPACE`,
//! `CI_PROJECT_DIR` and `RUNNER_TEMP` when they are set, and the system temp
//! directory). An existing character device or named pipe is allowed too.
//! The default destinations of a bare `--save-snapshot` and a bare
//! `--save-regression-baseline` are checked the same way.
//! The work tree keeps the monorepo form working, where a job runs from the
//! repository root with `--root packages/app` and saves to a
//! repository-relative path. The CI workspace keeps the Action form working
//! where the repository is checked out into a subdirectory. The temp
//! directories keep the CI form working, where a job saves a baseline outside
//! the checkout between steps.
//!
//! The check runs before the analysis. It then records the scope in
//! [`fallow_engine::write_guard`], and each writer checks the path again right
//! before the write, without following a symlink at the final component.
//! That second check narrows the window for a swapped path component to the
//! moment of the write. It does not close it for an intermediate directory.

use std::path::{Path, PathBuf};

use fallow_engine::write_guard::{self, WriteScope, WriteTarget};

use crate::{Cli, Command};

/// Return an error message when a file that a save or report flag writes
/// resolves outside the allowed directories. When the scope can be built, it
/// is also recorded for the check that each writer runs before the write.
///
/// The save flags are checked only for the commands that write them. A
/// command that rejects the flags, such as `audit`, keeps its own error about
/// the flag. `--output-file` is checked for every command that reaches this
/// point, and `--sarif-file` for the commands that write it.
/// The check fails closed: when the working directory or the root cannot be
/// resolved, a write is rejected.
pub fn write_path_error(cli: &Cli, root: &Path) -> Option<String> {
    if let Some(message) = report_input_alias_error(cli) {
        return Some(message);
    }
    let targets = write_targets(cli, root);
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            let (first_flag, _, _) = targets.first()?;
            return Some(format!(
                "{first_flag} cannot be checked, because the working directory cannot be read ({err}). Run fallow from an existing directory."
            ));
        }
    };
    let shared = write_guard::shared_dirs();
    let scopes = WriteScope::new(root, &cwd, shared.clone()).and_then(|scope| {
        // The discovered config file is the file fallow reads for this
        // project, so the Git work tree of the root counts for it wherever
        // the run starts. Its resolved path is still checked, so a config
        // symlink that points outside stays rejected.
        WriteScope::new(root, root, shared).map(|config_scope| (scope, config_scope))
    });
    let (scope, config_scope) = match scopes {
        Ok(scopes) => scopes,
        Err(message) => {
            let (first_flag, _, _) = targets.first()?;
            return Some(format!("{first_flag} cannot be checked: {message}"));
        }
    };
    let error = targets
        .iter()
        .find_map(|(flag, path, target)| match target {
            WriteTarget::DiscoveredConfig => config_scope.check(flag, path, &cwd),
            WriteTarget::Path => scope.check(flag, path, &cwd),
        });
    if error.is_none() {
        write_guard::confine(cwd, scope, config_scope);
    }
    error
}

/// `--output-file` is opened, and so truncated, before `fallow report` reads
/// its saved inputs. An output file that is one of those inputs would erase
/// the saved report before it is parsed, so refuse it up front.
fn report_input_alias_error(cli: &Cli) -> Option<String> {
    let output_file = cli.output_file.as_deref()?;
    let Some(Command::Report {
        from,
        mutation_report,
        ..
    }) = cli.command.as_ref()
    else {
        return None;
    };
    [
        ("--from", Some(from.as_path())),
        ("--mutation-report", mutation_report.as_deref()),
    ]
    .into_iter()
    .find_map(|(flag, input)| {
        let input = input?;
        same_file(input, output_file).then(|| {
            format!(
                "--output-file {} is the same file as {flag} {}. Write the report to another file, so that the saved report stays readable.",
                output_file.display(),
                input.display()
            )
        })
    })
}

/// Whether `output` names the existing file `input`, through a symlink,
/// another spelling or a hard link. An output path through a folder that does
/// not exist yet, such as `new/../report.json`, is resolved the way the writer
/// will open it: it creates the missing folder, so a `..` after it returns to
/// the resolved folder before it.
fn same_file(input: &Path, output: &Path) -> bool {
    if same_existing_file(input, output) {
        return true;
    }
    std::fs::symlink_metadata(output).is_err()
        && std::path::absolute(output)
            .is_ok_and(|absolute| same_existing_file(input, &resolve_step_by_step(&absolute)))
}

/// Resolve `path` one component at a time. Each existing prefix is
/// canonicalized, so a `..` after a symlinked folder leaves its target, as
/// the operating system does. A missing component is kept as written.
fn resolve_step_by_step(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            other => {
                resolved.push(other);
                if let Ok(canonical) = std::fs::canonicalize(&resolved) {
                    resolved = canonical;
                }
            }
        }
    }
    resolved
}

fn same_existing_file(left: &Path, right: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(left), Ok(right)) = (std::fs::metadata(left), std::fs::metadata(right)) {
            return left.dev() == right.dev() && left.ino() == right.ino();
        }
        false
    }
    #[cfg(not(unix))]
    {
        matches!(
            (std::fs::canonicalize(left), std::fs::canonicalize(right)),
            (Ok(left), Ok(right)) if left == right
        )
    }
}

/// When the default cache directory `<root>/.fallow` resolves outside the
/// project, for example through a committed symlink, return a note and do not
/// use it for the cache. `FALLOW_CACHE_DIR` and `--no-cache` skip the check,
/// because the default directory is then not used for the cache.
pub fn default_cache_dir_note(
    cli: &Cli,
    root: &Path,
    env: &fallow_config::EnvSnapshot,
) -> Option<String> {
    if cli.no_cache || env.cache_dir().is_some() {
        return None;
    }
    let cache_dir = root.join(".fallow");
    cache_dir.symlink_metadata().ok()?;
    let cwd = std::env::current_dir().ok()?;
    let resolved = write_guard::resolve(&cache_dir);
    let inside = WriteScope::new(root, &cwd, write_guard::shared_dirs())
        .is_ok_and(|scope| scope.contains(&resolved));
    (!inside).then(|| {
        format!(
            "note: {} resolves to {}, which is outside the project, so it is not used for the cache in this run. Remove the link, or set FALLOW_CACHE_DIR or cache.dir to relocate the cache.",
            cache_dir.display(),
            resolved.display()
        )
    })
}

/// Every file on the command line that a save or report flag writes, with
/// the flag that asked for it.
fn write_targets(cli: &Cli, root: &Path) -> Vec<(&'static str, PathBuf, WriteTarget)> {
    let mut targets = if matches!(
        cli.command,
        None | Some(
            Command::Check { .. }
                | Command::Architecture { .. }
                | Command::Dupes { .. }
                | Command::Health { .. }
        )
    ) {
        save_targets(cli, root)
    } else {
        Vec::new()
    };
    // `--output-file` is opened before any command runs. `--sarif-file` is
    // written only by the commands below; another command either ignores it
    // or rejects it with its own error, which stays the error the user sees.
    let writes_sarif = matches!(
        cli.command,
        None | Some(
            Command::Check { .. }
                | Command::Architecture { .. }
                | Command::Security {
                    subcommand: None,
                    ..
                }
        )
    );
    for (flag, path) in [
        ("--output-file", cli.output_file.as_deref()),
        (
            "--sarif-file",
            cli.sarif_file.as_deref().filter(|_| writes_sarif),
        ),
    ] {
        if let Some(path) = path.filter(|path| !path.as_os_str().is_empty()) {
            targets.push((flag, path.to_path_buf(), WriteTarget::Path));
        }
    }
    targets
}

/// Every file that a save flag writes.
///
/// An explicit path is checked as given. A bare `--save-snapshot` writes into
/// `<root>/.fallow/snapshots`, and a bare `--save-regression-baseline`
/// rewrites the config file, so their default destinations are checked too:
/// a committed `.fallow` or config symlink must not carry the write out of
/// the project.
fn save_targets(cli: &Cli, root: &Path) -> Vec<(&'static str, PathBuf, WriteTarget)> {
    let mut targets = Vec::new();
    if let Some(path) = cli.save_baseline.as_deref() {
        targets.push(("--save-baseline", path.to_path_buf(), WriteTarget::Path));
    }
    if let Some(value) = cli.save_regression_baseline.as_ref() {
        match value.as_deref().filter(|path| !path.is_empty()) {
            Some(path) => targets.push((
                "--save-regression-baseline",
                PathBuf::from(path),
                WriteTarget::Path,
            )),
            None => targets.push((
                "--save-regression-baseline",
                crate::regression::regression_config_target(cli.config.as_deref(), root),
                if cli.config.is_some() {
                    WriteTarget::Path
                } else {
                    WriteTarget::DiscoveredConfig
                },
            )),
        }
    }
    let health_snapshot = match cli.command.as_ref() {
        Some(Command::Health { save_snapshot, .. }) => save_snapshot.as_ref(),
        _ => None,
    };
    for snapshot in [cli.save_snapshot.as_ref(), health_snapshot]
        .into_iter()
        .flatten()
    {
        match snapshot.as_deref().filter(|path| !path.is_empty()) {
            Some(path) => targets.push(("--save-snapshot", PathBuf::from(path), WriteTarget::Path)),
            None => targets.push((
                "--save-snapshot",
                root.join(".fallow").join("snapshots").join("snapshot.json"),
                WriteTarget::Path,
            )),
        }
    }
    targets.retain(|(_, path, _)| !path.as_os_str().is_empty());
    targets
}
