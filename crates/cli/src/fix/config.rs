use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use fallow_config::{FallowConfig, IgnoreExportRule, OutputFormat};
use fallow_core::results::{AnalysisResults, DuplicateExport};
use rustc_hash::FxHashSet;

pub(super) fn apply_config_fixes(
    root: &Path,
    config_path: Option<&PathBuf>,
    results: &AnalysisResults,
    output: OutputFormat,
    dry_run: bool,
    fixes: &mut Vec<serde_json::Value>,
) -> bool {
    if results.duplicate_exports.is_empty() {
        return false;
    }

    let Some(config_path) = resolve_config_path(root, config_path) else {
        if !matches!(output, OutputFormat::Json) {
            eprintln!(
                "Skipped duplicate-export config fix: no fallow config file at {}. \
                 Run `fallow init` to create one, then re-run `fallow fix --yes`. \
                 (Note: `package.json#fallow` is not a supported config location.)",
                root.display()
            );
        }
        fixes.push(serde_json::json!({
            "type": "add_ignore_exports",
            "config_key": "ignoreExports",
            "skipped": true,
            "skip_reason": "missing_config",
            "description": "Skipped: no fallow config file was found. Run `fallow init` to create one.",
        }));
        return false;
    };

    let entries = ignore_export_entries(root, &config_path, &results.duplicate_exports);
    if entries.is_empty() {
        return false;
    }

    let config_file = display_path(root, &config_path);
    if dry_run {
        if !matches!(output, OutputFormat::Json) {
            eprintln!(
                "Would add {} ignoreExports rule(s) to {}",
                entries.len(),
                config_file
            );
        }
        fixes.push(serde_json::json!({
            "type": "add_ignore_exports",
            "config_key": "ignoreExports",
            "file": config_file,
            "entries": entries,
        }));
        return false;
    }

    match fallow_config::add_ignore_exports_rule(&config_path, &entries) {
        Ok(()) => {
            fixes.push(serde_json::json!({
                "type": "add_ignore_exports",
                "config_key": "ignoreExports",
                "file": config_file,
                "entries": entries,
                "applied": true,
            }));
            false
        }
        Err(e) => {
            eprintln!(
                "Error: failed to write ignoreExports rules to {}: {e}",
                config_path.display()
            );
            true
        }
    }
}

fn resolve_config_path(root: &Path, explicit: Option<&PathBuf>) -> Option<PathBuf> {
    explicit.map_or_else(
        || FallowConfig::find_config_path(root),
        |path| {
            if path.is_absolute() {
                Some(path.clone())
            } else {
                std::env::current_dir()
                    .ok()
                    .map_or_else(|| Some(path.clone()), |cwd| Some(cwd.join(path)))
            }
        },
    )
}

fn ignore_export_entries(
    root: &Path,
    config_path: &Path,
    duplicate_exports: &[DuplicateExport],
) -> Vec<IgnoreExportRule> {
    let config_dir = config_path.parent().unwrap_or(root);
    let mut seen = FxHashSet::default();
    let mut entries = Vec::new();
    for item in duplicate_exports {
        for location in &item.locations {
            let file = relative_from_config_dir(root, config_dir, &location.path);
            if seen.insert(file.clone()) {
                entries.push(IgnoreExportRule {
                    file,
                    exports: vec!["*".to_owned()],
                });
            }
        }
    }
    entries
}

fn relative_from_config_dir(root: &Path, config_dir: &Path, file_path: &Path) -> String {
    let root_relative = file_path.strip_prefix(root).unwrap_or(file_path);
    let config_relative = config_dir
        .strip_prefix(root)
        .unwrap_or_else(|_| Path::new(""));
    lexical_relative(config_relative, root_relative)
        .unwrap_or_else(|| root_relative.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
}

fn lexical_relative(from_dir: &Path, to_file: &Path) -> Option<PathBuf> {
    let from = normal_components(from_dir)?;
    let to = normal_components(to_file)?;
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut relative = PathBuf::new();
    for _ in common..from.len() {
        relative.push("..");
    }
    for component in &to[common..] {
        relative.push(component);
    }
    Some(relative)
}

fn normal_components(path: &Path) -> Option<Vec<OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => components.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => components.push(OsString::from("..")),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components)
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::results::DuplicateLocation;

    fn duplicate(paths: &[PathBuf]) -> DuplicateExport {
        DuplicateExport {
            export_name: "Button".to_owned(),
            locations: paths
                .iter()
                .map(|path| DuplicateLocation {
                    path: path.clone(),
                    line: 1,
                    col: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn config_fix_reanchors_paths_to_workspace_config_dir() {
        let root = Path::new("/repo");
        let config_path = root.join("packages/ui/.fallowrc.json");
        let entries = ignore_export_entries(
            root,
            &config_path,
            &[duplicate(&[
                root.join("packages/ui/src/index.ts"),
                root.join("packages/shared/src/index.ts"),
            ])],
        );

        assert_eq!(entries[0].file, "src/index.ts");
        assert_eq!(entries[1].file, "../shared/src/index.ts");
    }

    #[test]
    fn config_fix_dedupes_exact_files_preserving_first_order() {
        let root = Path::new("/repo");
        let config_path = root.join(".fallowrc.json");
        let entries = ignore_export_entries(
            root,
            &config_path,
            &[duplicate(&[
                root.join("src/a.ts"),
                root.join("src/b.ts"),
                root.join("src/a.ts"),
            ])],
        );

        let files: Vec<&str> = entries.iter().map(|entry| entry.file.as_str()).collect();
        assert_eq!(files, vec!["src/a.ts", "src/b.ts"]);
    }
}
