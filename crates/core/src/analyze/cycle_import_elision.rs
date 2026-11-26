//! The compiler options that keep a TypeScript type import in the output
//! (issue #5262).
//!
//! TypeScript removes a plain import of names that are only types, so the
//! cycle detector does not count it. Some options keep each import:
//! `verbatimModuleSyntax`, `preserveValueImports`, and
//! `importsNotUsedAsValues` with `preserve` or `error`. This reader finds
//! those options for one importer. It treats a Deno project as one that
//! keeps imports, because Deno does not read `tsconfig.json`.

use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashMap;

const TSCONFIG: &str = "tsconfig.json";
const DENO_CONFIGS: [&str; 2] = ["deno.json", "deno.jsonc"];

/// The `compilerOptions` keys that keep each import when they are `true`.
const KEEP_IMPORT_FLAGS: [&str; 2] = ["verbatimModuleSyntax", "preserveValueImports"];
/// `importsNotUsedAsValues` keeps an import of types as a side-effect import
/// with these values.
const KEEP_IMPORT_MODES: [&str; 2] = ["preserve", "error"];

/// A per-folder cache of whether the TypeScript files of a folder keep each
/// import.
pub(super) struct ImportKeepingConfigs {
    root: PathBuf,
    by_dir: FxHashMap<PathBuf, bool>,
}

impl ImportKeepingConfigs {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            by_dir: FxHashMap::default(),
        }
    }

    /// Whether the compiler options of `file` keep each import.
    ///
    /// The nearest `tsconfig.json` between the file and the project root
    /// decides, together with the `tsconfig.*.json` files next to it and the
    /// configs inside the project root that its `references` name. A
    /// solution-style config (`files: []`) names its real configs there.
    /// When one of these configs keeps imports, the file keeps imports.
    ///
    /// Each unclear case keeps imports, so it never hides a cycle: a config
    /// that does not parse, a Deno config, and a file outside the project
    /// root. With no config, TypeScript uses its defaults and removes type
    /// imports.
    pub(super) fn keeps_imports(&mut self, file: &Path) -> bool {
        file.parent().is_none_or(|dir| self.dir_keeps_imports(dir))
    }

    fn dir_keeps_imports(&mut self, dir: &Path) -> bool {
        if let Some(&keeps) = self.by_dir.get(dir) {
            return keeps;
        }
        let keeps = if !dir.starts_with(&self.root)
            || DENO_CONFIGS.iter().any(|name| dir.join(name).is_file())
        {
            true
        } else if dir.join(TSCONFIG).is_file() {
            self.dir_configs_keep_imports(dir)
        } else if dir == self.root {
            false
        } else {
            dir.parent()
                .is_some_and(|parent| self.dir_keeps_imports(parent))
        };
        self.by_dir.insert(dir.to_path_buf(), keeps);
        keeps
    }
}

impl ImportKeepingConfigs {
    /// Whether `tsconfig.json` in `dir`, a `tsconfig.*.json` next to it, or a
    /// config that its `references` name keeps each import.
    fn dir_configs_keep_imports(&self, dir: &Path) -> bool {
        let config = dir.join(TSCONFIG);
        let Some(references) = self.references(&config) else {
            return true;
        };
        let siblings = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(is_sibling_config)
            });
        std::iter::once(config)
            .chain(siblings)
            .chain(references)
            .any(|config| config_keeps_imports(&config))
    }

    /// The config files inside the project root that the `references` of
    /// `config` name, or `None` when `config` does not parse. A reference to
    /// a folder names the `tsconfig.json` in it. A reference outside the
    /// root is another project, so it does not decide for this file.
    fn references(&self, config: &Path) -> Option<Vec<PathBuf>> {
        let source = std::fs::read_to_string(config).ok()?;
        let value: serde_json::Value =
            fallow_config::jsonc::parse_to_value(source.trim_start_matches('\u{FEFF}')).ok()?;
        let dir = config.parent()?;
        let paths = value
            .get("references")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|reference| reference.get("path")?.as_str())
            .map(|path| {
                let target = dir.join(path);
                if target
                    .extension()
                    .is_some_and(|extension| extension == "json")
                {
                    target
                } else {
                    target.join(TSCONFIG)
                }
            })
            .filter(|target| normalize_lexically(target).starts_with(&self.root))
            .collect();
        Some(paths)
    }
}

/// `path` with each `.` and `..` folded away, without a read of the disk.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

fn is_sibling_config(name: &str) -> bool {
    name.strip_prefix("tsconfig.")
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|middle| !middle.is_empty())
}

fn config_keeps_imports(config: &Path) -> bool {
    let Some(merged) = fallow_config::MergedTsconfig::read(config) else {
        return true;
    };
    KEEP_IMPORT_FLAGS.iter().any(|flag| {
        merged
            .compiler_option(flag)
            .is_some_and(|value| value.value().as_bool() == Some(true))
    }) || merged
        .compiler_option_str("importsNotUsedAsValues")
        .is_some_and(|mode| KEEP_IMPORT_MODES.contains(&mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        std::fs::create_dir_all(dir).expect("create dir");
        std::fs::write(dir.join(name), content).expect("write file");
    }

    #[test]
    fn default_options_remove_type_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            TSCONFIG,
            r#"{ "compilerOptions": { "strict": true } }"#,
        );
        let mut configs = ImportKeepingConfigs::new(tmp.path());
        assert!(!configs.keeps_imports(&tmp.path().join("src/a.ts")));
    }

    #[test]
    fn no_config_removes_type_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut configs = ImportKeepingConfigs::new(tmp.path());
        assert!(!configs.keeps_imports(&tmp.path().join("a.ts")));
    }

    #[test]
    fn options_that_keep_imports() {
        for options in [
            r#"{ "verbatimModuleSyntax": true }"#,
            r#"{ "preserveValueImports": true }"#,
            r#"{ "importsNotUsedAsValues": "preserve" }"#,
            r#"{ "importsNotUsedAsValues": "error" }"#,
        ] {
            let tmp = tempfile::tempdir().expect("tempdir");
            write(
                tmp.path(),
                TSCONFIG,
                &format!(r#"{{ "compilerOptions": {options} }}"#),
            );
            let mut configs = ImportKeepingConfigs::new(tmp.path());
            assert!(configs.keeps_imports(&tmp.path().join("a.ts")), "{options}");
        }
    }

    #[test]
    fn nearest_config_and_its_siblings_decide() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            TSCONFIG,
            r#"{ "compilerOptions": { "verbatimModuleSyntax": true } }"#,
        );
        let app = tmp.path().join("app");
        write(
            &app,
            TSCONFIG,
            r#"{ "files": [], "references": [{ "path": "./tsconfig.app.json" }] }"#,
        );
        let lib = tmp.path().join("lib");
        write(&lib, TSCONFIG, "{}");
        write(
            &app,
            "tsconfig.app.json",
            r#"{ "compilerOptions": { "preserveValueImports": true } }"#,
        );
        let mut configs = ImportKeepingConfigs::new(tmp.path());
        assert!(configs.keeps_imports(&app.join("src/main.ts")));
        assert!(!configs.keeps_imports(&lib.join("index.ts")));
        assert!(configs.keeps_imports(&tmp.path().join("other/x.ts")));
    }

    #[test]
    fn referenced_config_in_a_subfolder_decides() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            TSCONFIG,
            r#"{ "files": [], "references": [{ "path": "./config/app.json" }] }"#,
        );
        write(
            &tmp.path().join("config"),
            "app.json",
            r#"{ "compilerOptions": { "verbatimModuleSyntax": true } }"#,
        );
        let mut configs = ImportKeepingConfigs::new(tmp.path());
        assert!(configs.keeps_imports(&tmp.path().join("src/a.ts")));
    }

    #[test]
    fn reference_outside_the_root_does_not_decide() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("app");
        write(
            &root,
            TSCONFIG,
            r#"{ "references": [{ "path": "../shared" }] }"#,
        );
        write(
            &tmp.path().join("shared"),
            TSCONFIG,
            r#"{ "compilerOptions": { "verbatimModuleSyntax": true } }"#,
        );
        let mut configs = ImportKeepingConfigs::new(&root);
        assert!(!configs.keeps_imports(&root.join("a.ts")));
    }

    #[test]
    fn file_outside_the_root_keeps_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).expect("create dir");
        let mut configs = ImportKeepingConfigs::new(&root);
        assert!(configs.keeps_imports(&tmp.path().join("other/a.ts")));
    }

    #[test]
    fn unreadable_config_and_deno_keep_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let broken = tmp.path().join("broken");
        write(&broken, TSCONFIG, "{ not json");
        let deno = tmp.path().join("deno");
        write(&deno, "deno.json", "{}");
        let mut configs = ImportKeepingConfigs::new(tmp.path());
        assert!(configs.keeps_imports(&broken.join("a.ts")));
        assert!(configs.keeps_imports(&deno.join("a.ts")));
    }
}
