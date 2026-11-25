use std::path::{Path, PathBuf};

use crate::FallowConfig;

/// Classification of whether fallow can apply config edits at `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFixPlan {
    /// A fallow config file exists; append entries in place.
    Edit {
        /// Path to the existing config file to edit.
        config_path: PathBuf,
    },
    /// No fallow config exists, but a workspace marker sits above `root`,
    /// so creating one inside this subpackage would fragment the monorepo.
    BlockedMonorepo {
        /// Directory containing the workspace marker that blocked creation.
        workspace_root: PathBuf,
    },
    /// No fallow config exists and config creation was disabled.
    BlockedNoCreate {
        /// Path where the config would have been created.
        target: PathBuf,
    },
    /// No fallow config exists; the writer can create one at `target`.
    Create {
        /// Path for the new config file (`<root>/.fallowrc.json`).
        target: PathBuf,
    },
}

/// Classify how config-editing fixes should behave for `root`.
#[must_use]
pub fn classify_config_fix_plan(
    root: &Path,
    explicit: Option<&PathBuf>,
    no_create_config: bool,
) -> ConfigFixPlan {
    if let Some(existing) = resolve_existing_config_path(root, explicit) {
        // Discovery can load a config link that git does not track, for
        // example to a dotfiles config. A fix never writes through a link
        // that leaves the root.
        let is_link = existing
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink());
        if is_link && !fallow_types::read_guard::is_inside_root(root, &existing) {
            return ConfigFixPlan::BlockedNoCreate { target: existing };
        }
        return ConfigFixPlan::Edit {
            config_path: existing,
        };
    }
    let target = root.join(".fallowrc.json");
    // An entry at the target that discovery refused, such as a link out of
    // the root, must not be replaced by a new config file.
    if target.symlink_metadata().is_ok() {
        return ConfigFixPlan::BlockedNoCreate { target };
    }
    if let Some(workspace_root) = find_workspace_root_above(root) {
        return ConfigFixPlan::BlockedMonorepo { workspace_root };
    }
    if no_create_config {
        return ConfigFixPlan::BlockedNoCreate { target };
    }
    ConfigFixPlan::Create { target }
}

/// Whether `fallow fix --yes` can apply config edits at `root` with default
/// config-creation behavior. Drives JSON `auto_fixable` for config actions.
#[must_use]
pub fn is_config_fixable(root: &Path, explicit: Option<&PathBuf>) -> bool {
    matches!(
        classify_config_fix_plan(root, explicit, false),
        ConfigFixPlan::Edit { .. } | ConfigFixPlan::Create { .. }
    )
}

fn resolve_existing_config_path(root: &Path, explicit: Option<&PathBuf>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        let absolute = if path.is_absolute() {
            path.clone()
        } else {
            std::env::current_dir().map_or_else(|_| path.clone(), |cwd| cwd.join(path))
        };
        if absolute.exists() {
            return Some(absolute);
        }
        return None;
    }
    FallowConfig::find_config_path(root)
}

fn find_workspace_root_above(start: &Path) -> Option<PathBuf> {
    let mut current = start.parent()?;
    loop {
        if has_workspace_marker(current) {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn has_workspace_marker(dir: &Path) -> bool {
    const SENTINELS: &[&str] = &[
        "pnpm-workspace.yaml",
        "turbo.json",
        "lerna.json",
        "rush.json",
    ];
    for name in SENTINELS {
        if dir.join(name).exists() {
            return true;
        }
    }
    if crate::workspace::load_root_deno_workspace_patterns(dir)
        .ok()
        .flatten()
        .is_some_and(|(_path, patterns)| !patterns.is_empty())
    {
        return true;
    }
    let pkg_path = dir.join("package.json");
    if !pkg_path.exists() {
        return false;
    }
    let Ok(content) = fallow_types::read_guard::read_to_string_bounded(
        &pkg_path,
        fallow_types::read_guard::DEFAULT_MAX_READ_BYTES,
    ) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    value
        .get("workspaces")
        .is_some_and(|v| v.is_array() || v.is_object())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_config_link_out_of_the_root_is_neither_edited_nor_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(dir.path().join("outside.json"), "{}").unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside.json"), root.join(".fallowrc.json"))
            .unwrap();
        let plan = classify_config_fix_plan(&root, None, false);
        assert!(
            matches!(plan, ConfigFixPlan::BlockedNoCreate { .. }),
            "{plan:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn an_untracked_config_link_out_of_the_root_is_not_edited() {
        crate::set_tracked_link_probe(crate::config::test_tracked_link_probe);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .status()
            .expect("run git");
        assert!(status.success(), "git init");
        std::fs::write(dir.path().join("dotfiles.json"), "{}").unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("dotfiles.json"),
            root.join(".fallowrc.json"),
        )
        .unwrap();
        let plan = classify_config_fix_plan(&root, None, false);
        assert!(
            matches!(plan, ConfigFixPlan::BlockedNoCreate { .. }),
            "{plan:?}"
        );
    }

    #[test]
    fn config_fixable_true_when_config_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".fallowrc.json"), "{}").unwrap();
        assert!(is_config_fixable(dir.path(), None));
    }

    #[test]
    fn config_fixable_true_when_can_create_at_root() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_config_fixable(dir.path(), None));
    }

    #[test]
    fn config_fixable_false_when_monorepo_subpackage() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .unwrap();
        let sub = dir.path().join("packages/app");
        std::fs::create_dir_all(&sub).unwrap();
        assert!(!is_config_fixable(&sub, None));
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_pipe_as_a_parent_manifest_does_not_block_the_check() {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(dir.path().join("package.json"))
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo must create the test pipe");
        let sub = dir.path().join("packages/app");
        std::fs::create_dir_all(&sub).unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(is_config_fixable(&sub, None));
        });
        let fixable = receive
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the check must not wait on a pipe");
        assert!(fixable);
    }
}
