//! The compiler options that decide which plain imports TypeScript removes
//! (issues #5262 and #5309).
//!
//! TypeScript removes a plain import that the file never reads as a value,
//! so the cycle detector does not count it. Some options keep each import:
//! `verbatimModuleSyntax`, `preserveValueImports`, and
//! `importsNotUsedAsValues` with `preserve` or `error`.
//! `emitDecoratorMetadata` keeps an import that a decorated file uses as a
//! type. This reader finds those options for one importer. It treats a Deno
//! project as one that keeps imports, because Deno does not read
//! `tsconfig.json`.

use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::graph::ImportElision;

const TSCONFIG: &str = "tsconfig.json";
const DENO_CONFIGS: [&str; 2] = ["deno.json", "deno.jsonc"];

/// The `compilerOptions` keys that keep each import when they are `true`.
const KEEP_IMPORT_FLAGS: [&str; 2] = ["verbatimModuleSyntax", "preserveValueImports"];
/// `importsNotUsedAsValues` keeps an import of types as a side-effect import
/// with these values.
const KEEP_IMPORT_MODES: [&str; 2] = ["preserve", "error"];

/// The `compilerOptions` key that turns a decorated type annotation into a
/// runtime reference.
const DECORATOR_METADATA_FLAG: &str = "emitDecoratorMetadata";

/// A per-folder cache of the [`ImportElision`] of the TypeScript files of a
/// folder.
pub(super) struct ImportElisionConfigs {
    root: PathBuf,
    by_dir: FxHashMap<PathBuf, ImportElision>,
}

impl ImportElisionConfigs {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            by_dir: FxHashMap::default(),
        }
    }

    /// The [`ImportElision`] of the compiler options of `file`.
    ///
    /// The nearest `tsconfig.json` between the file and the project root
    /// decides, together with the `tsconfig.*.json` files next to it and the
    /// configs inside the project root that its `references` name. A
    /// solution-style config (`files: []`) names its real configs there.
    /// The strictest of these configs wins.
    ///
    /// Each unclear case keeps imports, so it never hides a cycle: a config
    /// that does not parse or whose `extends` chain does not resolve, a Deno
    /// config, and a file outside the project root. With no config, TypeScript uses its defaults and removes each
    /// import that the file never reads as a value.
    pub(super) fn elision_of(&mut self, file: &Path) -> ImportElision {
        file.parent()
            .map_or(ImportElision::Keep, |dir| self.dir_elision(dir))
    }

    fn dir_elision(&mut self, dir: &Path) -> ImportElision {
        if let Some(&elision) = self.by_dir.get(dir) {
            return elision;
        }
        let elision = if !dir.starts_with(&self.root)
            || DENO_CONFIGS.iter().any(|name| dir.join(name).is_file())
        {
            ImportElision::Keep
        } else if dir.join(TSCONFIG).is_file() {
            self.dir_configs_elision(dir)
        } else if dir == self.root {
            ImportElision::RemoveUnread
        } else {
            dir.parent()
                .map_or(ImportElision::Keep, |parent| self.dir_elision(parent))
        };
        self.by_dir.insert(dir.to_path_buf(), elision);
        elision
    }
}

impl ImportElisionConfigs {
    /// The strictest [`ImportElision`] of `tsconfig.json` in `dir`, the
    /// `tsconfig.*.json` files next to it, and the configs that its
    /// `references` name.
    fn dir_configs_elision(&self, dir: &Path) -> ImportElision {
        let config = dir.join(TSCONFIG);
        let Some(references) = self.references(&config) else {
            return ImportElision::Keep;
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
            .map(|config| config_elision(&config))
            .fold(ImportElision::RemoveUnread, stricter)
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

/// The elision that keeps more imports of `a` and `b`.
const fn stricter(a: ImportElision, b: ImportElision) -> ImportElision {
    match (a, b) {
        (ImportElision::Keep, _) | (_, ImportElision::Keep) => ImportElision::Keep,
        (ImportElision::KeepDecoratedTypes, _) | (_, ImportElision::KeepDecoratedTypes) => {
            ImportElision::KeepDecoratedTypes
        }
        _ => ImportElision::RemoveUnread,
    }
}

/// The elision of one config and its `extends` chain. A config whose chain
/// does not resolve keeps imports: a framework can generate the parent at
/// build time (SvelteKit `$app/tsconfig`), and that parent can set
/// `verbatimModuleSyntax`.
fn config_elision(config: &Path) -> ImportElision {
    let Some(merged) = fallow_config::MergedTsconfig::read(config) else {
        return ImportElision::Keep;
    };
    if !merged.is_complete() {
        return ImportElision::Keep;
    }
    let is_on = |flag: &str| {
        merged
            .compiler_option(flag)
            .is_some_and(|value| value.value().as_bool() == Some(true))
    };
    if KEEP_IMPORT_FLAGS.iter().any(|flag| is_on(flag))
        || merged
            .compiler_option_str("importsNotUsedAsValues")
            .is_some_and(|mode| {
                KEEP_IMPORT_MODES
                    .iter()
                    .any(|keep| keep.eq_ignore_ascii_case(mode))
            })
    {
        ImportElision::Keep
    } else if is_on(DECORATOR_METADATA_FLAG) {
        ImportElision::KeepDecoratedTypes
    } else {
        ImportElision::RemoveUnread
    }
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
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("src/a.ts")),
            ImportElision::RemoveUnread
        );
    }

    #[test]
    fn no_config_removes_type_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("a.ts")),
            ImportElision::RemoveUnread
        );
    }

    #[test]
    fn options_that_keep_imports() {
        for options in [
            r#"{ "verbatimModuleSyntax": true }"#,
            r#"{ "preserveValueImports": true }"#,
            r#"{ "importsNotUsedAsValues": "preserve" }"#,
            r#"{ "importsNotUsedAsValues": "error" }"#,
            r#"{ "importsNotUsedAsValues": "Preserve" }"#,
        ] {
            let tmp = tempfile::tempdir().expect("tempdir");
            write(
                tmp.path(),
                TSCONFIG,
                &format!(r#"{{ "compilerOptions": {options} }}"#),
            );
            let mut configs = ImportElisionConfigs::new(tmp.path());
            assert_eq!(
                configs.elision_of(&tmp.path().join("a.ts")),
                ImportElision::Keep,
                "{options}"
            );
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
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&app.join("src/main.ts")),
            ImportElision::Keep
        );
        assert_eq!(
            configs.elision_of(&lib.join("index.ts")),
            ImportElision::RemoveUnread
        );
        assert_eq!(
            configs.elision_of(&tmp.path().join("other/x.ts")),
            ImportElision::Keep
        );
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
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("src/a.ts")),
            ImportElision::Keep
        );
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
        let mut configs = ImportElisionConfigs::new(&root);
        assert_eq!(
            configs.elision_of(&root.join("a.ts")),
            ImportElision::RemoveUnread
        );
    }

    #[test]
    fn file_outside_the_root_keeps_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).expect("create dir");
        let mut configs = ImportElisionConfigs::new(&root);
        assert_eq!(
            configs.elision_of(&tmp.path().join("other/a.ts")),
            ImportElision::Keep
        );
    }

    #[test]
    fn unresolved_extends_keeps_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), TSCONFIG, r#"{ "extends": "$app/tsconfig" }"#);
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("a.ts")),
            ImportElision::Keep
        );
    }

    #[test]
    fn decorator_metadata_keeps_decorated_types() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(
            tmp.path(),
            TSCONFIG,
            r#"{ "compilerOptions": { "emitDecoratorMetadata": true } }"#,
        );
        let both = tmp.path().join("both");
        write(
            &both,
            TSCONFIG,
            r#"{ "compilerOptions": { "emitDecoratorMetadata": true, "verbatimModuleSyntax": true } }"#,
        );
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("a.ts")),
            ImportElision::KeepDecoratedTypes
        );
        assert_eq!(configs.elision_of(&both.join("a.ts")), ImportElision::Keep);
    }

    #[test]
    fn unreadable_config_and_deno_keep_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let broken = tmp.path().join("broken");
        write(&broken, TSCONFIG, "{ not json");
        let deno = tmp.path().join("deno");
        write(&deno, "deno.json", "{}");
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&broken.join("a.ts")),
            ImportElision::Keep
        );
        assert_eq!(configs.elision_of(&deno.join("a.ts")), ImportElision::Keep);
    }
}
