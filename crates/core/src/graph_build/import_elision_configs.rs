//! The compiler options that decide which plain imports TypeScript removes
//! (issues #5262, #5309 and #5362).
//!
//! TypeScript removes a plain import that the file never reads as a value,
//! so the cycle detector does not count it. Some options keep each import:
//! `verbatimModuleSyntax`, `preserveValueImports`, and
//! `importsNotUsedAsValues` with `preserve` or `error`.
//! `emitDecoratorMetadata` keeps an import that decorator metadata reads,
//! and `strictNullChecks` decides whether a union with `null` emits a name.
//! Cycle detection and test coverage read these options for one importer.
//! A Deno project keeps each import, because Deno does not read
//! `tsconfig.json`.

use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::graph::{DecoratorMetadata, ImportElision, ModuleGraph};

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
/// The `compilerOptions` key for `strictNullChecks`, and the `strict` key
/// that sets its default.
const STRICT_NULL_CHECKS_FLAG: &str = "strictNullChecks";
const STRICT_FLAG: &str = "strict";

/// The elision of an unclear case: keep each import, and do not assume
/// decorator metadata.
const UNKNOWN: ImportElision = ImportElision::Keep(DecoratorMetadata::Off);
/// The elision of the TypeScript defaults.
const DEFAULTS: ImportElision = ImportElision::RemoveUnread(DecoratorMetadata::Off);

/// A per-folder cache of the [`ImportElision`] of the TypeScript files of a
/// folder.
pub struct ImportElisionConfigs {
    root: PathBuf,
    by_dir: FxHashMap<PathBuf, ImportElision>,
}

/// Read the compiler options that decide whether the decorator metadata of
/// each module of `graph` reads its imports. The persisted graph does not
/// track `tsconfig.json`, so each build and each cache load reads them.
pub fn read_decorator_metadata_options(graph: &mut ModuleGraph, root: &Path) {
    let mut configs = ImportElisionConfigs::new(root);
    graph.read_decorator_metadata_options(&mut |file| configs.elision_of(file));
}

impl ImportElisionConfigs {
    pub fn new(root: &Path) -> Self {
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
    /// config, and a file outside the project root. Such a case does not
    /// assume decorator metadata. With no config, TypeScript uses its
    /// defaults and removes each import that the file never reads as a
    /// value.
    pub fn elision_of(&mut self, file: &Path) -> ImportElision {
        file.parent().map_or(UNKNOWN, |dir| self.dir_elision(dir))
    }

    fn dir_elision(&mut self, dir: &Path) -> ImportElision {
        if let Some(&elision) = self.by_dir.get(dir) {
            return elision;
        }
        let elision = if !dir.starts_with(&self.root)
            || DENO_CONFIGS.iter().any(|name| dir.join(name).is_file())
        {
            UNKNOWN
        } else if dir.join(TSCONFIG).is_file() {
            self.dir_configs_elision(dir)
        } else if dir == self.root {
            DEFAULTS
        } else {
            dir.parent()
                .map_or(UNKNOWN, |parent| self.dir_elision(parent))
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
            return UNKNOWN;
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
            .fold(DEFAULTS, stricter)
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

/// The elision that keeps more imports of `a` and `b`. Decorator metadata
/// that one config turns on stays on, and the config without
/// `strictNullChecks` decides, as it reads more imports.
const fn stricter(a: ImportElision, b: ImportElision) -> ImportElision {
    let metadata = match (a.decorator_metadata(), b.decorator_metadata()) {
        (
            DecoratorMetadata::On {
                strict_null_checks: a,
            },
            DecoratorMetadata::On {
                strict_null_checks: b,
            },
        ) => DecoratorMetadata::On {
            strict_null_checks: a && b,
        },
        (on @ DecoratorMetadata::On { .. }, _) | (_, on @ DecoratorMetadata::On { .. }) => on,
        _ => DecoratorMetadata::Off,
    };
    match (a, b) {
        (ImportElision::Keep(_), _) | (_, ImportElision::Keep(_)) => ImportElision::Keep(metadata),
        _ => ImportElision::RemoveUnread(metadata),
    }
}

/// The elision of one config and its `extends` chain. A config whose chain
/// does not resolve keeps imports: a framework can generate the parent at
/// build time (SvelteKit `$app/tsconfig`), and that parent can set
/// `verbatimModuleSyntax`.
fn config_elision(config: &Path) -> ImportElision {
    let Some(merged) = fallow_config::MergedTsconfig::read(config) else {
        return UNKNOWN;
    };
    if !merged.is_complete() {
        return UNKNOWN;
    }
    let flag = |flag: &str| {
        merged
            .compiler_option(flag)
            .and_then(|value| value.value().as_bool())
    };
    let is_on = |name: &str| flag(name) == Some(true);
    let metadata = if is_on(DECORATOR_METADATA_FLAG) {
        DecoratorMetadata::On {
            strict_null_checks: flag(STRICT_NULL_CHECKS_FLAG).unwrap_or_else(|| is_on(STRICT_FLAG)),
        }
    } else {
        DecoratorMetadata::Off
    };
    let keeps_imports = KEEP_IMPORT_FLAGS.iter().any(|flag| is_on(flag))
        || merged
            .compiler_option_str("importsNotUsedAsValues")
            .is_some_and(|mode| {
                KEEP_IMPORT_MODES
                    .iter()
                    .any(|keep| keep.eq_ignore_ascii_case(mode))
            });
    if keeps_imports {
        ImportElision::Keep(metadata)
    } else {
        ImportElision::RemoveUnread(metadata)
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
        assert_eq!(configs.elision_of(&tmp.path().join("src/a.ts")), DEFAULTS);
    }

    #[test]
    fn no_config_removes_type_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(configs.elision_of(&tmp.path().join("a.ts")), DEFAULTS);
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
                UNKNOWN,
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
        assert_eq!(configs.elision_of(&app.join("src/main.ts")), UNKNOWN);
        assert_eq!(configs.elision_of(&lib.join("index.ts")), DEFAULTS);
        assert_eq!(configs.elision_of(&tmp.path().join("other/x.ts")), UNKNOWN);
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
        assert_eq!(configs.elision_of(&tmp.path().join("src/a.ts")), UNKNOWN);
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
        assert_eq!(configs.elision_of(&root.join("a.ts")), DEFAULTS);
    }

    #[test]
    fn file_outside_the_root_keeps_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).expect("create dir");
        let mut configs = ImportElisionConfigs::new(&root);
        assert_eq!(configs.elision_of(&tmp.path().join("other/a.ts")), UNKNOWN);
    }

    #[test]
    fn unresolved_extends_keeps_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write(tmp.path(), TSCONFIG, r#"{ "extends": "$app/tsconfig" }"#);
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(configs.elision_of(&tmp.path().join("a.ts")), UNKNOWN);
    }

    const LOOSE: DecoratorMetadata = DecoratorMetadata::On {
        strict_null_checks: false,
    };

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
        let strict = tmp.path().join("strict");
        write(
            &strict,
            TSCONFIG,
            r#"{ "compilerOptions": { "emitDecoratorMetadata": true, "strict": true } }"#,
        );
        let loose = tmp.path().join("loose");
        write(
            &loose,
            TSCONFIG,
            r#"{ "compilerOptions": { "emitDecoratorMetadata": true, "strict": true, "strictNullChecks": false } }"#,
        );
        write(
            &loose,
            "tsconfig.build.json",
            r#"{ "compilerOptions": { "emitDecoratorMetadata": true, "strict": true } }"#,
        );
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(
            configs.elision_of(&tmp.path().join("a.ts")),
            ImportElision::RemoveUnread(LOOSE)
        );
        assert_eq!(
            configs.elision_of(&both.join("a.ts")),
            ImportElision::Keep(LOOSE),
            "a config that keeps each import still emits metadata"
        );
        assert_eq!(
            configs.elision_of(&strict.join("a.ts")),
            ImportElision::RemoveUnread(DecoratorMetadata::On {
                strict_null_checks: true
            })
        );
        assert_eq!(
            configs.elision_of(&loose.join("a.ts")),
            ImportElision::RemoveUnread(LOOSE),
            "the config that keeps more imports wins"
        );
    }

    #[test]
    fn unreadable_config_and_deno_keep_imports() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let broken = tmp.path().join("broken");
        write(&broken, TSCONFIG, "{ not json");
        let deno = tmp.path().join("deno");
        write(&deno, "deno.json", "{}");
        let mut configs = ImportElisionConfigs::new(tmp.path());
        assert_eq!(configs.elision_of(&broken.join("a.ts")), UNKNOWN);
        assert_eq!(configs.elision_of(&deno.join("a.ts")), UNKNOWN);
    }
}
