//! Action builders for dependency and duplicate-export findings.

use super::*;

/// Build the `IssueAction` vec for the three `unused_dependencies`,
/// `unused_dev_dependencies`, `unused_optional_dependencies` views over the
/// same bare [`UnusedDependency`] struct. Each wrapper differs only in the
/// `package_json_location` string (`"dependencies"` / `"devDependencies"` /
/// `"optionalDependencies"`) in the fix-action description. All three
/// share the cross-workspace swap (when `dep.used_in_workspaces` is
/// non-empty the primary fix flips from `remove-dependency` to
/// `move-dependency` because the dep is imported by ANOTHER workspace and
/// `fallow fix` cannot safely remove it).
pub(super) fn build_unused_dependency_actions(
    dep: &UnusedDependency,
    package_json_location: &str,
) -> Vec<IssueAction> {
    let mut actions = Vec::with_capacity(2);
    let cross_workspace = !dep.used_in_workspaces.is_empty();
    actions.push(if cross_workspace {
        IssueAction::Fix(FixAction {
            kind: FixActionType::MoveDependency,
            auto_fixable: false,
            description: "Move this dependency to the workspace package.json that imports it"
                .to_string(),
            note: Some(
                "fallow fix will not remove dependencies that are imported by another workspace"
                    .to_string(),
            ),
            available_in_catalogs: None,
            suggested_target: None,
        })
    } else {
        IssueAction::Fix(FixAction {
            kind: FixActionType::RemoveDependency,
            auto_fixable: true,
            description: format!("Remove from {package_json_location} in package.json"),
            note: None,
            available_in_catalogs: None,
            suggested_target: None,
        })
    });
    actions.push(build_ignore_dependencies_suppress_action(&dep.package_name));
    actions
}

/// Build a paste-ready `ignoreExports` config value from a duplicate-export
/// finding's locations. Returns one `{ file, exports: ["*"] }` entry per
/// distinct file in insertion order. `None` when no locations carry a path.
pub(super) fn build_duplicate_exports_ignore_rules(
    export: &DuplicateExport,
) -> Option<Vec<IgnoreExportsRule>> {
    let mut entries: Vec<IgnoreExportsRule> = Vec::with_capacity(export.locations.len());
    for loc in &export.locations {
        // Normalize separators to forward slashes so pasting the action value
        // into `.fallowrc.json` produces a portable rule. On Windows
        // `to_string_lossy` preserves backslashes, which the old
        // `inject_actions` post-pass implicitly normalized because it read
        // the path AFTER `strip_root_prefix` had already run through
        // `normalize_uri`; the typed wrapper builds the value before
        // serialization, so the normalization has to be explicit here.
        let path = loc.path.to_string_lossy().replace('\\', "/");
        if path.is_empty() {
            continue;
        }
        if entries.iter().any(|existing| existing.file == path) {
            continue;
        }
        entries.push(IgnoreExportsRule {
            file: path,
            exports: vec!["*".to_string()],
        });
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries)
    }
}
