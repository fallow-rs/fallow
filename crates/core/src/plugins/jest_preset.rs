//! Jest `preset` values that name a config file by a relative path.
//!
//! Jest reads a preset that starts with `.` as a file path relative to the
//! root directory of the config, and merges that file into the config. A
//! preset that is a package name loads `<package>/jest-preset` and only
//! credits the package.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::super::PluginResult;
use super::super::config_parser;
use super::{ProjectWalk, extract_jest_config, jest_root_dir};

/// The extensions that Jest tries for a preset path without a file extension.
const PRESET_EXTENSIONS: &[&str] = &["js", "cjs", "mjs", "json", "ts"];

/// True when Jest reads `preset` as a file path, not as a package name.
pub(super) fn is_relative_preset(preset: &str) -> bool {
    preset.starts_with('.')
}

/// Read the relative preset file of a config as Jest config and mark it used.
///
/// Jest replaces `<rootDir>` in the preset with the root directory of the
/// config that names the preset, so the preset reuses that directory. Jest
/// does not follow the `preset` of a preset.
pub(super) fn follow_relative_preset(
    parse_source: &str,
    parse_path: &Path,
    root: &Path,
    result: &mut PluginResult,
    walk: &mut ProjectWalk<'_>,
    depth: usize,
) {
    if walk.preset_root_dir.is_some() {
        return;
    }
    let Some(preset) = config_parser::extract_config_string(parse_source, parse_path, &["preset"])
    else {
        return;
    };
    let root_dir = jest_root_dir(parse_source, parse_path, root);
    let Some(file) = relative_preset_file(&preset, &root_dir, walk.project_root) else {
        return;
    };
    let Ok(source) = std::fs::read_to_string(&file) else {
        return;
    };
    result.setup_files.push(file.clone());
    walk.preset_root_dir = Some(root_dir);
    extract_jest_config(&file, &source, root, result, walk, depth + 1);
    walk.preset_root_dir = None;
    // Each config that names the preset reads it again with its own root
    // directory. A preset does not follow a preset, so no cycle can start here.
    let key = std::fs::canonicalize(&file).unwrap_or(file);
    walk.visited.remove(&key);
}

/// The file that a relative `preset` names, resolved against `root_dir`.
/// Returns `None` when no file exists or the file is outside `project_root`.
fn relative_preset_file(preset: &str, root_dir: &Path, project_root: &Path) -> Option<PathBuf> {
    if !is_relative_preset(preset) {
        return None;
    }
    let target = config_parser::lexical_normalize(&root_dir.join(preset));
    std::iter::once(target.clone())
        .chain(
            PRESET_EXTENSIONS
                .iter()
                .map(|ext| with_appended_extension(&target, ext)),
        )
        .find(|candidate| candidate.is_file())
        .filter(|candidate| config_parser::is_inside_root(candidate, project_root))
}

/// Append `.ext` to the full file name. `Path::with_extension` would replace
/// the last segment of a name such as `jest-preset.front`.
fn with_appended_extension(path: &Path, ext: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".");
    name.push(ext);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_that_share_a_preset_each_get_its_root_dir_setup_files() {
        use crate::plugins::Plugin;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("jest-preset.js"),
            "module.exports = { setupFilesAfterEnv: ['<rootDir>/setup.js'] };",
        )
        .unwrap();
        for name in ["a", "b"] {
            let package = root.join("packages").join(name);
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(
                package.join("jest.config.js"),
                "module.exports = { preset: '../../jest-preset.js' };",
            )
            .unwrap();
            std::fs::write(package.join("setup.js"), "").unwrap();
        }
        let source = "module.exports = { projects: ['<rootDir>/packages/*'] };";

        let result = super::super::JestPlugin.resolve_config_in_project(
            &root.join("jest.config.js"),
            source,
            root,
            root,
        );

        for name in ["a", "b"] {
            let setup = root.join("packages").join(name).join("setup.js");
            assert!(
                result.setup_files.contains(&setup),
                "project {name} must get the preset setup file: {:?}",
                result.setup_files
            );
        }
    }

    #[test]
    fn relative_preset_file_probes_extensions_and_stays_inside_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join("packages/web")).unwrap();
        std::fs::write(project.join("jest-preset.front.js"), "").unwrap();
        std::fs::write(dir.path().join("outside.js"), "").unwrap();
        let root_dir = project.join("packages/web");

        let found = relative_preset_file("../../jest-preset.front", &root_dir, &project);
        assert_eq!(found, Some(project.join("jest-preset.front.js")));
        assert_eq!(
            relative_preset_file("../../../outside.js", &root_dir, &project),
            None
        );
        assert_eq!(relative_preset_file("ts-jest", &root_dir, &project), None);
        assert_eq!(relative_preset_file("./missing", &root_dir, &project), None);
    }
}
