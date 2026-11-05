//! Refuse project-scope writes through a symlink.
//!
//! A cloned repository can commit a symlink where fallow writes a project
//! file. A write, backup or delete through it could then reach a file outside
//! the project. Under `--user` the files live in the home directory, where
//! dotfile managers link them on purpose, so the check does not apply there.

use std::path::{Component, Path, PathBuf};

use super::{Mode, Plan, Report, SetupHooksOptions, display_rel};

/// The first entry below `root`, up to and including `path`, that is a
/// symlink. A path outside `root`, or one with a `..` or `.` component,
/// returns `path` itself. A missing entry ends the walk, because nothing
/// below it exists yet.
pub fn symlinked_component(root: &Path, path: &Path) -> Option<PathBuf> {
    let Ok(relative) = path.strip_prefix(root) else {
        return Some(path.to_path_buf());
    };
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Some(path.to_path_buf());
        };
        current.push(name);
        match current.symlink_metadata() {
            Ok(meta) if meta.file_type().is_symlink() => return Some(current),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
    None
}

/// In project scope, the first symlink on the way to a file that the
/// agent-hook plan would write, back up or delete. Always `None` under
/// `--user`.
pub fn symlinked_hook_target(
    opts: &SetupHooksOptions<'_>,
    mode: Mode,
) -> Result<Option<PathBuf>, String> {
    Ok(Plan::resolve(opts, mode)?.symlinked_target(opts, mode))
}

/// Resolve and execute the agent-hook plan without printing a summary.
/// `fallow agent install` renders its own combined report from the returned
/// [`Report`]. Returns `Ok(None)` when no surface is selected.
pub fn execute_agent_hooks(
    opts: &SetupHooksOptions<'_>,
    mode: Mode,
) -> Result<Option<Report>, String> {
    let plan = Plan::resolve(opts, mode)?;
    if plan.is_empty() {
        return Ok(None);
    }
    plan.execute(opts, mode).map(Some)
}

impl Plan {
    fn symlinked_target(&self, opts: &SetupHooksOptions<'_>, mode: Mode) -> Option<PathBuf> {
        if opts.user {
            return None;
        }
        let mut paths: Vec<&Path> = Vec::new();
        if let Some(claude) = &self.claude {
            paths.extend([claude.settings_path.as_path(), claude.script_path.as_path()]);
        }
        if let Some(codex) = &self.codex {
            paths.extend([codex.hooks_path.as_path(), codex.script_path.as_path()]);
            paths.extend(codex.agents_path.as_deref());
        }
        let gitignore = opts.root.join(".gitignore");
        if mode == Mode::Install && self.claude.is_some() && opts.gitignore_claude {
            paths.push(&gitignore);
        }
        paths
            .into_iter()
            .find_map(|path| symlinked_component(opts.root, path))
    }

    pub(super) fn refuse_symlinks(
        &self,
        opts: &SetupHooksOptions<'_>,
        mode: Mode,
    ) -> Result<(), String> {
        match self.symlinked_target(opts, mode) {
            Some(link) => Err(format!(
                "Refusing to change {}: it is a symlink, and fallow does not write, back up or delete through a symlink in the project. Replace it with a regular file or directory.",
                display_rel(opts.root, &link)
            )),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_hooks::HookAgentArg;

    fn opts(root: &Path) -> SetupHooksOptions<'_> {
        SetupHooksOptions {
            root,
            agent: Some(HookAgentArg::Claude),
            dry_run: false,
            force: false,
            user: false,
            home: None,
            gitignore_claude: false,
            uninstall: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn hooks_install_refuses_a_symlinked_gitignore_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("ignore");
        std::fs::write(&target, "keep\n").unwrap();
        std::os::unix::fs::symlink(&target, tmp.path().join(".gitignore")).unwrap();
        let o = SetupHooksOptions {
            gitignore_claude: true,
            ..opts(tmp.path())
        };

        let err = execute_agent_hooks(&o, Mode::Install).unwrap_err();

        assert!(err.contains(".gitignore"), "{err}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
        assert!(!tmp.path().join(".claude").exists());
    }

    #[test]
    fn symlinked_component_rejects_paths_that_leave_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let escape = tmp.path().join("..").join("x");
        assert_eq!(symlinked_component(tmp.path(), &escape), Some(escape));
        let elsewhere = PathBuf::from("/elsewhere/.mcp.json");
        assert_eq!(symlinked_component(tmp.path(), &elsewhere), Some(elsewhere));
        assert_eq!(
            symlinked_component(tmp.path(), &tmp.path().join(".claude/settings.json")),
            None
        );
    }
}
