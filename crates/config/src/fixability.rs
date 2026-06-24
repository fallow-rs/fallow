use std::path::{Path, PathBuf};

use crate::FallowConfig;

/// Classification of whether fallow can apply config edits at `root`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFixPlan {
    /// A fallow config file exists; append entries in place.
    Edit { config_path: PathBuf },
    /// No fallow config exists, but a workspace marker sits above `root`,
    /// so creating one inside this subpackage would fragment the monorepo.
    BlockedMonorepo { workspace_root: PathBuf },
    /// No fallow config exists and config creation was disabled.
    BlockedNoCreate { target: PathBuf },
    /// No fallow config exists; the writer can create one at `target`.
    Create { target: PathBuf },
}

/// Classify how config-editing fixes should behave for `root`.
#[must_use]
pub fn classify_config_fix_plan(
    root: &Path,
    explicit: Option<&PathBuf>,
    no_create_config: bool,
) -> ConfigFixPlan {
    if let Some(existing) = resolve_existing_config_path(root, explicit) {
        return ConfigFixPlan::Edit {
            config_path: existing,
        };
    }
    let target = root.join(".fallowrc.json");
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
    let pkg_path = dir.join("package.json");
    if !pkg_path.exists() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(&pkg_path) else {
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
}
