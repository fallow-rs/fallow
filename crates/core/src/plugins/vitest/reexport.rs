//! Follow a Vitest config that only re-exports its default export.
//!
//! A workspace can share one config through
//! `export { default } from "@scope/testing"`. Vitest then loads the
//! config of that module, so its includes apply to the workspace.

use std::path::{Path, PathBuf};

use fallow_extract::JsText;
use oxc_ast::ast::{ModuleExportName, Statement};
use rustc_hash::FxHashSet;

use super::super::config_parser;
use super::super::extends_walk::{ExtendsWalk, workspace_package_dir, workspace_package_file};

/// The maximum number of re-exports that one config lookup follows.
const MAX_REEXPORT_HOPS: usize = 4;

/// The config file and source that `config_path` re-exports as its default
/// export, after each hop of a re-export chain. `None` when the config has
/// its own default export, or when no project file gives the module.
pub(super) fn follow_default_reexport(
    config_path: &Path,
    source: &str,
    project_root: &Path,
) -> Option<(PathBuf, String)> {
    let mut walk = ExtendsWalk::new(project_root);
    let mut visited = FxHashSet::default();
    visited.insert(config_path.to_path_buf());
    let mut current: Option<(PathBuf, String)> = None;
    for _ in 0..MAX_REEXPORT_HOPS {
        let (path, text) = current
            .as_ref()
            .map_or((config_path, source), |(path, text)| {
                (path.as_path(), text.as_str())
            });
        let Some(specifier) = default_reexport_specifier(text, path) else {
            break;
        };
        let Some(next) = resolve_module(path, &specifier, &mut walk) else {
            break;
        };
        if !visited.insert(next.0.clone()) {
            break;
        }
        current = Some(next);
    }
    current
}

/// The specifier of `export { default } from "<specifier>"` when the module
/// has no default export of its own.
fn default_reexport_specifier(source: &str, path: &Path) -> Option<String> {
    config_parser::extract_from_source(source, path, |program| {
        let mut specifier = None;
        for stmt in &program.body {
            match stmt {
                Statement::ExportDefaultDeclaration(_) => return None,
                Statement::ExportFromDeclaration(decl) => {
                    let reexports_default = decl.specifiers.iter().any(|spec| {
                        is_default_name(&spec.local) && is_default_name(&spec.exported)
                    });
                    if reexports_default {
                        specifier = Some(decl.source.value.text().into_owned());
                    }
                }
                _ => {}
            }
        }
        specifier
    })
}

fn is_default_name(name: &ModuleExportName<'_>) -> bool {
    name.name() == "default"
}

/// The project file that `specifier` names from the directory of `from`. A
/// relative specifier names a sibling file. A package specifier names the
/// entry of a workspace package or of a linked package in the project.
fn resolve_module(
    from: &Path,
    specifier: &str,
    walk: &mut ExtendsWalk<'_>,
) -> Option<(PathBuf, String)> {
    if config_parser::is_relative_specifier(specifier) {
        return config_parser::resolve_sibling_module(from, specifier);
    }
    let package = crate::resolve::extract_package_name(specifier);
    let subpath = specifier.strip_prefix(package.as_str())?;
    let pkg_dir = workspace_package_dir(from.parent()?, &package, walk)?;
    let project_root = walk.project_root();
    if !config_parser::is_inside_root(&pkg_dir, project_root) {
        return None;
    }
    let file = workspace_package_file(&pkg_dir, subpath, project_root, project_root)
        .filter(|file| config_parser::is_inside_root(file, project_root))?;
    let source = crate::discover::project_read::read_project_file(project_root, &file)?;
    Some((file, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, contents: &str) -> PathBuf {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn a_config_with_its_own_default_export_is_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let source = "export { default as other } from './shared';\nexport default {};\n";
        let config = write(dir.path(), "vitest.config.ts", source);
        write(dir.path(), "shared.ts", "export default {};\n");
        assert_eq!(follow_default_reexport(&config, source, dir.path()), None);
    }

    #[test]
    fn a_reexport_chain_ends_at_the_last_module_with_a_default_export() {
        let dir = tempfile::tempdir().unwrap();
        let source = "export { default } from './a';\n";
        let config = write(dir.path(), "vitest.config.ts", source);
        write(dir.path(), "a.ts", "export { default } from './b';\n");
        let last = write(dir.path(), "b.ts", "export default { test: {} };\n");
        let followed = follow_default_reexport(&config, source, dir.path()).unwrap();
        assert_eq!(followed.0, last);
    }

    #[test]
    fn a_reexport_cycle_stops() {
        let dir = tempfile::tempdir().unwrap();
        let source = "export { default } from './a';\n";
        let config = write(dir.path(), "vitest.config.ts", source);
        let a = write(
            dir.path(),
            "a.ts",
            "export { default } from './vitest.config';\n",
        );
        let followed = follow_default_reexport(&config, source, dir.path()).unwrap();
        assert_eq!(followed.0, a);
    }
}
