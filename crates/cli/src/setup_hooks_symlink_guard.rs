//! Refuse project-scope writes through a symlink that leaves the project.
//!
//! A cloned repository can commit a symlink where fallow writes a project
//! file. A write, backup or delete through it could then reach a file outside
//! the project. A link that resolves inside the project, such as
//! `CLAUDE.md -> AGENTS.md`, is safe and stays allowed. Under `--user` the
//! harness files live in the home directory, where dotfile managers link them
//! on purpose, so the check does not apply to them. The project `.gitignore`
//! that `--gitignore-claude` edits gets the check in both scopes.

use std::path::{Path, PathBuf};

use super::{Mode, Plan, Report, SetupHooksOptions, display_rel};

/// The shared middle of every refusal message, after the link path.
pub const OUTSIDE_LINK: &str = "is a symlink that dangles or resolves outside the project root or into a .git or node_modules directory, and fallow does not write, back up or delete through it";

/// The shared remedy of every refusal message.
pub const OUTSIDE_LINK_REMEDY: &str = "replace it with a regular file or a link inside the project";

/// The error text for a command that refuses to change `path` because of
/// `link`. Both are display paths.
pub fn refusal_message(path: &str, link: &str) -> String {
    format!("Refusing to change {path}: {link} {OUTSIDE_LINK}; {OUTSIDE_LINK_REMEDY}.")
}

/// The error text for a command that refuses to check `path`, a path that
/// it reads or tests but does not write, because of `link`. Both are
/// display paths.
pub fn check_refusal_message(path: &str, link: &str) -> String {
    format!("Refusing to check {path}: {link} {OUTSIDE_LINK}; {OUTSIDE_LINK_REMEDY}.")
}

/// A planned file and the first symlink on the way to it that resolves
/// outside the project root.
#[derive(Debug, PartialEq, Eq)]
pub struct SymlinkedTarget {
    /// The file that the plan would write, back up or delete.
    pub path: PathBuf,
    /// The symlink: the file itself or a directory between the root and it.
    pub link: PathBuf,
}

/// The first project file that the agent-hook plan would write, back up or
/// delete through a symlink that resolves outside the project root. Under
/// `--user` only the project `.gitignore` counts.
pub fn symlinked_hook_target(
    opts: &SetupHooksOptions<'_>,
    mode: Mode,
) -> Result<Option<SymlinkedTarget>, String> {
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
    fn symlinked_target(
        &self,
        opts: &SetupHooksOptions<'_>,
        mode: Mode,
    ) -> Option<SymlinkedTarget> {
        let mut paths: Vec<&Path> = Vec::new();
        if !opts.user {
            if let Some(claude) = &self.claude {
                paths.extend([claude.settings_path.as_path(), claude.script_path.as_path()]);
            }
            if let Some(codex) = &self.codex {
                paths.extend([codex.hooks_path.as_path(), codex.script_path.as_path()]);
                paths.extend(codex.agents_path.as_deref());
            }
        }
        let gitignore = opts.root.join(".gitignore");
        if mode == Mode::Install && self.claude.is_some() && opts.gitignore_claude {
            paths.push(&gitignore);
        }
        paths.into_iter().find_map(|path| {
            fallow_types::path_guard::outside_link(opts.root, path).map(|link| SymlinkedTarget {
                path: path.to_path_buf(),
                link,
            })
        })
    }

    pub(super) fn refuse_symlinks(
        &self,
        opts: &SetupHooksOptions<'_>,
        mode: Mode,
    ) -> Result<(), String> {
        match self.symlinked_target(opts, mode) {
            Some(target) => Err(refusal_message(
                &display_rel(opts.root, &target.path),
                &display_rel(opts.root, &target.link),
            )),
            None => Ok(()),
        }
    }
}

#[cfg(all(test, unix))]
#[path = "setup_hooks_status_link_tests.rs"]
mod status_link_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::setup_hooks::HookAgentArg;

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn user_hooks_install_refuses_a_symlinked_gitignore_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("ignore");
        std::fs::write(&target, "keep\n").unwrap();
        std::os::unix::fs::symlink(&target, tmp.path().join(".gitignore")).unwrap();
        let o = SetupHooksOptions {
            gitignore_claude: true,
            user: true,
            home: Some(home.path()),
            ..opts(tmp.path())
        };

        let err = execute_agent_hooks(&o, Mode::Install).unwrap_err();

        assert!(err.contains(".gitignore"), "{err}");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep\n");
        assert!(!home.path().join(".claude").exists());
    }

    #[cfg(unix)]
    #[test]
    fn outside_link_allows_links_that_resolve_inside_the_root() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("AGENTS.md"), "# Guide\n").unwrap();
        symlink("AGENTS.md", root.join("CLAUDE.md")).unwrap();
        std::fs::create_dir_all(root.join(".agents/skills")).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        symlink("../.agents/skills", root.join(".claude/skills")).unwrap();

        assert_eq!(
            fallow_types::path_guard::outside_link(root, &root.join("CLAUDE.md")),
            None
        );
        assert_eq!(
            fallow_types::path_guard::outside_link(
                root,
                &root.join(".claude/skills/fallow/SKILL.md")
            ),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn outside_link_refuses_links_that_resolve_outside_or_dangle() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(outside.path().join("mcp.json"), "{}").unwrap();
        symlink(outside.path().join("mcp.json"), root.join(".mcp.json")).unwrap();
        symlink(outside.path().join("later"), root.join("AGENTS.md")).unwrap();
        symlink(".mcp.json", root.join("chain.json")).unwrap();
        symlink(outside.path(), root.join(".codex")).unwrap();

        for name in [".mcp.json", "AGENTS.md", "chain.json"] {
            assert_eq!(
                fallow_types::path_guard::outside_link(root, &root.join(name)),
                Some(root.join(name))
            );
        }
        assert_eq!(
            fallow_types::path_guard::outside_link(root, &root.join(".codex/hooks.json")),
            Some(root.join(".codex"))
        );
    }

    #[test]
    fn outside_link_rejects_paths_that_leave_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let escape = tmp.path().join("..").join("x");
        assert_eq!(
            fallow_types::path_guard::outside_link(tmp.path(), &escape),
            Some(escape)
        );
        let elsewhere = PathBuf::from("/elsewhere/.mcp.json");
        assert_eq!(
            fallow_types::path_guard::outside_link(tmp.path(), &elsewhere),
            Some(elsewhere)
        );
        assert_eq!(
            fallow_types::path_guard::outside_link(
                tmp.path(),
                &tmp.path().join(".claude/settings.json")
            ),
            None
        );
    }
}
