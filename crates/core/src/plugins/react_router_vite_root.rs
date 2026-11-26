//! A React Router app below the package root (issue #5295).
//!
//! A package script can start React Router with a Vite config in a
//! sub-folder: `react-router dev --config preview/vite.config.ts`. The Vite
//! plugin of React Router reads `react-router.config.*` and the app directory
//! from the Vite `root` of that config, and from the process directory when
//! the config has no `root`. The package root is the process directory, and
//! the normal plugin run covers it already. This module covers a `root` below
//! the package root.

use std::path::{Path, PathBuf};

use oxc_ast::ast::Expression;

use super::super::{PluginResult, config_parser};
use super::{
    ROUTE_CONFIG_EXPORTS, add_app_dir_patterns, add_default_entry_server_dependencies,
    add_referenced_dependencies, collect_route_config_from_disk, extract_app_directory,
};

/// The file names of a React Router project config, in lookup order.
const PROJECT_CONFIG_FILES: &[&str] = &[
    "react-router.config.ts",
    "react-router.config.js",
    "react-router.config.mjs",
    "react-router.config.cjs",
];

/// Add the app of the Vite `root` that the Vite config `config_path` sets,
/// when that root is a folder below the package root `root`.
pub(super) fn add_vite_root_app(
    result: &mut PluginResult,
    source: &str,
    config_path: &Path,
    root: &Path,
) {
    let Some(project_dir) = vite_root_dir(source, config_path, root) else {
        return;
    };
    if project_dir.is_empty() {
        return;
    }
    let app_dir = match project_config(root, &project_dir) {
        Some((path, config_source)) => {
            add_referenced_dependencies(result, &config_source, &path);
            let relative = format!(
                "{project_dir}/{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            result.always_used_files.push(relative.clone());
            result.push_used_export_rule(relative, ROUTE_CONFIG_EXPORTS.iter().copied());
            extract_app_directory(&config_source, &path, root)
        }
        None => format!("{project_dir}/app"),
    };
    add_app_dir_patterns(result, &app_dir);
    add_default_entry_server_dependencies(result, root, &app_dir);
    collect_route_config_from_disk(result, root, &app_dir);
}

/// The package-relative folder that the `root` option of the Vite config
/// names. `__dirname` and `import.meta.dirname` name the folder of the
/// config. A config without a static `root` returns `None`.
fn vite_root_dir(source: &str, config_path: &Path, root: &Path) -> Option<String> {
    let raw = config_parser::extract_from_source(source, config_path, |program| {
        let config = config_parser::find_config_object(program)?;
        let value = &config_parser::find_property(config, "root")?.value;
        root_value_path(value)
    })?;
    let dir = config_parser::normalize_filesystem_config_path(&raw, config_path, root)?;
    Some(
        dir.trim_end_matches('/')
            .trim_start_matches("./")
            .to_string(),
    )
}

fn root_value_path(value: &Expression<'_>) -> Option<PathBuf> {
    if config_parser::is_dirname_anchor(value.get_inner_expression()) {
        return Some(PathBuf::from("."));
    }
    config_parser::expression_to_path(value)
}

/// The path and source of the React Router project config in `project_dir`.
fn project_config(root: &Path, project_dir: &str) -> Option<(PathBuf, String)> {
    PROJECT_CONFIG_FILES.iter().find_map(|name| {
        let path = root.join(project_dir).join(name);
        if !path.is_file() {
            return None;
        }
        let source = crate::discover::project_read::read_project_file(root, &path)?;
        Some((path, source))
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::super::super::{Plugin, PluginResult};
    use super::super::ReactRouterPlugin;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry| entry.pattern == pattern)
    }

    fn resolve_vite_config(files: &[(&str, &str)], vite_config: &str) -> PluginResult {
        let dir = tempdir().expect("temp dir");
        for (path, source) in files {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
            fs::write(path, source).expect("write file");
        }
        let config_path = dir.path().join(vite_config);
        let source = fs::read_to_string(&config_path).expect("read config");
        ReactRouterPlugin.resolve_config(&config_path, &source, dir.path())
    }

    #[test]
    fn a_dirname_root_reads_the_project_config_of_that_folder() {
        let result = resolve_vite_config(
            &[
                (
                    "preview/vite.config.ts",
                    "export default defineConfig({ root: __dirname });",
                ),
                (
                    "preview/react-router.config.ts",
                    "export default { appDirectory: 'src' };",
                ),
            ],
            "preview/vite.config.ts",
        );
        assert!(has_entry_pattern(
            &result,
            "preview/src/root.{ts,tsx,js,jsx}"
        ));
        assert!(
            result
                .always_used_files
                .iter()
                .any(|file| file == "preview/react-router.config.ts")
        );
    }

    #[test]
    fn a_project_config_without_app_directory_uses_the_app_folder_beside_it() {
        let result = resolve_vite_config(
            &[
                (
                    "preview/vite.config.ts",
                    "export default defineConfig({ root: __dirname });",
                ),
                ("preview/react-router.config.ts", "export default {};"),
            ],
            "preview/vite.config.ts",
        );
        assert!(has_entry_pattern(
            &result,
            "preview/app/root.{ts,tsx,js,jsx}"
        ));
    }

    #[test]
    fn a_root_outside_the_package_adds_nothing() {
        let result = resolve_vite_config(
            &[("vite.config.ts", "export default { root: '../other' };")],
            "vite.config.ts",
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn a_literal_root_without_project_config_uses_its_app_folder() {
        let result = resolve_vite_config(
            &[("vite.preview.ts", "export default { root: 'preview' };")],
            "vite.preview.ts",
        );
        assert!(has_entry_pattern(
            &result,
            "preview/app/routes/**/*.{ts,tsx,js,jsx}"
        ));
    }

    #[test]
    fn a_config_without_root_adds_nothing() {
        let result = resolve_vite_config(
            &[("preview/vite.config.ts", "export default { plugins: [] };")],
            "preview/vite.config.ts",
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn a_root_at_the_package_root_adds_nothing() {
        let result = resolve_vite_config(
            &[(
                "vite.config.ts",
                "export default { root: import.meta.dirname };",
            )],
            "vite.config.ts",
        );
        assert!(result.entry_patterns.is_empty());
    }
}
