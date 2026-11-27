//! Whether a source file belongs to a project that emits TypeScript
//! declarations.
//!
//! An un-exported type in the signature of an export is valid TypeScript. But
//! when declarations are emitted, the declaration of another module that holds
//! the value with an inferred type must name the type, and fails with TS4023
//! when the type has no export. So `unused-type` keeps such a type in a project
//! that emits declarations.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::discover::project_read::{read_project_dir, read_project_file};

/// Declaration-emit lookups of one run. Each folder is listed once and each
/// config is merged once, so the files of sibling folders share the work.
pub struct DeclarationEmit {
    root: PathBuf,
    by_dir: FxHashMap<PathBuf, Option<PathBuf>>,
    tsconfig_by_dir: FxHashMap<PathBuf, FolderVerdict>,
    package_by_dir: FxHashMap<PathBuf, FolderVerdict>,
    parsed: std::cell::RefCell<FxHashMap<PathBuf, Option<Arc<serde_json::Value>>>>,
}

/// What one folder says about declaration emit.
#[derive(Clone)]
enum FolderVerdict {
    /// The folder has no config of this kind, so a parent folder decides.
    Absent,
    /// The config emits or publishes declarations.
    Emits(PathBuf),
    /// The folder has a config of this kind and it emits none.
    NoEmit,
}

impl DeclarationEmit {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            by_dir: FxHashMap::default(),
            tsconfig_by_dir: FxHashMap::default(),
            package_by_dir: FxHashMap::default(),
            parsed: std::cell::RefCell::default(),
        }
    }

    /// The tsconfig that emits declarations for `file`, or the package.json
    /// that publishes them. `None` when the project of `file` emits none.
    ///
    /// The nearest folder with a `tsconfig*.json` decides for tsconfig: one
    /// config there that emits declarations is enough, because fallow does
    /// not know which config of the folder owns the file. The nearest
    /// `package.json` decides for a build tool that emits declarations
    /// without a tsconfig flag: its `types`, `typings`, `typesVersions` or
    /// `types` export condition names a `.d.ts` file.
    pub fn config_for(&mut self, file: &Path) -> Option<PathBuf> {
        let dir = file.parent()?.to_path_buf();
        if let Some(cached) = self.by_dir.get(&dir) {
            return cached.clone();
        }
        let found = self.lookup(&dir);
        self.by_dir.insert(dir, found.clone());
        found
    }

    fn lookup(&mut self, start: &Path) -> Option<PathBuf> {
        let mut tsconfig_decided = false;
        let mut package_decided = false;
        let dirs: Vec<PathBuf> = start
            .ancestors()
            .take_while(|dir| dir.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        for dir in dirs {
            if !tsconfig_decided {
                match self.tsconfig_verdict(&dir) {
                    FolderVerdict::Emits(config) => return Some(config),
                    FolderVerdict::NoEmit => tsconfig_decided = true,
                    FolderVerdict::Absent => {}
                }
            }
            if !package_decided {
                match self.package_verdict(&dir) {
                    FolderVerdict::Emits(manifest) => return Some(manifest),
                    FolderVerdict::NoEmit => package_decided = true,
                    FolderVerdict::Absent => {}
                }
            }
            if tsconfig_decided && package_decided {
                break;
            }
        }
        None
    }

    fn tsconfig_verdict(&mut self, dir: &Path) -> FolderVerdict {
        if let Some(verdict) = self.tsconfig_by_dir.get(dir) {
            return verdict.clone();
        }
        let configs = tsconfig_paths(&self.root, dir);
        let verdict = if configs.is_empty() {
            FolderVerdict::Absent
        } else {
            configs
                .into_iter()
                .find(|config| self.emits(config))
                .map_or(FolderVerdict::NoEmit, FolderVerdict::Emits)
        };
        self.tsconfig_by_dir
            .insert(dir.to_path_buf(), verdict.clone());
        verdict
    }

    fn package_verdict(&mut self, dir: &Path) -> FolderVerdict {
        if let Some(verdict) = self.package_by_dir.get(dir) {
            return verdict.clone();
        }
        let manifest = dir.join("package.json");
        let verdict = match read_project_file(&self.root, &manifest) {
            None => FolderVerdict::Absent,
            Some(source) if publishes_declarations(&source) => FolderVerdict::Emits(manifest),
            Some(_) => FolderVerdict::NoEmit,
        };
        self.package_by_dir
            .insert(dir.to_path_buf(), verdict.clone());
        verdict
    }

    /// Whether the merged config emits declarations. A config that does not
    /// read or parse, or whose `extends` chain does not fully resolve, counts
    /// as emitting unless a known option says otherwise, so an unclear
    /// project keeps the export.
    fn emits(&self, config: &Path) -> bool {
        let Some(merged) =
            fallow_config::MergedTsconfig::read_with_loader(config, &|path| self.load(path))
        else {
            return true;
        };
        let flag = |name: &str| {
            merged
                .compiler_option(name)
                .and_then(|value| value.value().as_bool())
        };
        if flag("noEmit") == Some(true) {
            return false;
        }
        if flag("declaration") == Some(true) || flag("composite") == Some(true) {
            return true;
        }
        if flag("declaration") == Some(false) {
            return false;
        }
        !merged.is_complete()
    }

    fn load(&self, path: &Path) -> Option<Arc<serde_json::Value>> {
        if let Some(source) = self.parsed.borrow().get(path) {
            return source.clone();
        }
        let source = read_project_file(&self.root, path)
            .and_then(|content| {
                fallow_config::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()
            })
            .map(Arc::new);
        self.parsed
            .borrow_mut()
            .insert(path.to_path_buf(), source.clone());
        source
    }
}

/// The `tsconfig*.json` files of `dir`, sorted.
fn tsconfig_paths(root: &Path, dir: &Path) -> Vec<PathBuf> {
    let Some(entries) = read_project_dir(root, dir) else {
        return Vec::new();
    };
    let mut configs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("tsconfig"))
                && path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("json")
                        || extension.eq_ignore_ascii_case("jsonc")
                })
                && path.is_file()
        })
        .collect();
    configs.sort();
    configs
}

/// Whether a package.json names a declaration file as its types.
fn publishes_declarations(source: &str) -> bool {
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(source) else {
        return false;
    };
    let mut targets = Vec::new();
    for field in ["types", "typings"] {
        if let Some(target) = manifest.get(field).and_then(serde_json::Value::as_str) {
            targets.push(target);
        }
    }
    if let Some(versions) = manifest.get("typesVersions") {
        collect_strings(versions, &mut targets);
    }
    if let Some(exports) = manifest.get("exports") {
        collect_types_conditions(exports, &mut targets);
    }
    targets.into_iter().any(is_declaration_path)
}

fn is_declaration_path(target: &str) -> bool {
    [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|suffix| target.ends_with(suffix))
}

fn collect_strings<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(text) => out.push(text),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        serde_json::Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// The strings under each `types` condition of an `exports` value.
fn collect_types_conditions<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .for_each(|item| collect_types_conditions(item, out)),
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                if key == "types" || key.starts_with("types@") {
                    collect_strings(item, out);
                } else {
                    collect_types_conditions(item, out);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for (path, content) in files {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, content).expect("write");
        }
        dir
    }

    fn config_for(files: &[(&str, &str)], file: &str) -> Option<String> {
        let dir = project(files);
        let root = dir.path().canonicalize().expect("canonical root");
        DeclarationEmit::new(&root)
            .config_for(&root.join(file))
            .map(|config| {
                config
                    .strip_prefix(&root)
                    .expect("inside root")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
    }

    #[test]
    fn nearest_tsconfig_with_declaration_emits() {
        let files = [
            (
                "tsconfig.json",
                r#"{ "compilerOptions": { "noEmit": true } }"#,
            ),
            (
                "server/tsconfig.json",
                r#"{ "compilerOptions": { "declaration": true } }"#,
            ),
            ("server/src/a.ts", ""),
            ("web/src/b.ts", ""),
        ];
        assert_eq!(
            config_for(&files, "server/src/a.ts").as_deref(),
            Some("server/tsconfig.json")
        );
        assert_eq!(config_for(&files, "web/src/b.ts"), None);
    }

    #[test]
    fn declaration_from_extends_and_composite_emit() {
        let files = [
            (
                "tsconfig.base.json",
                r#"{ "compilerOptions": { "composite": true } }"#,
            ),
            ("tsconfig.json", r#"{ "extends": "./tsconfig.base.json" }"#),
            ("src/a.ts", ""),
        ];
        assert_eq!(
            config_for(&files, "src/a.ts").as_deref(),
            Some("tsconfig.base.json")
        );
    }

    #[test]
    fn no_emit_wins_over_declaration() {
        let files = [
            (
                "tsconfig.json",
                r#"{ "compilerOptions": { "declaration": true, "noEmit": true } }"#,
            ),
            ("src/a.ts", ""),
        ];
        assert_eq!(config_for(&files, "src/a.ts"), None);
    }

    #[test]
    fn unresolved_extends_counts_as_emitting() {
        let files = [
            ("tsconfig.json", r#"{ "extends": "@missing/tsconfig" }"#),
            ("src/a.ts", ""),
        ];
        assert_eq!(
            config_for(&files, "src/a.ts").as_deref(),
            Some("tsconfig.json")
        );
    }

    #[test]
    fn unparsable_tsconfig_counts_as_emitting() {
        let files = [("tsconfig.json", "{ not json"), ("src/a.ts", "")];
        assert_eq!(
            config_for(&files, "src/a.ts").as_deref(),
            Some("tsconfig.json")
        );
    }

    #[test]
    fn sibling_folders_share_the_parent_verdict() {
        let dir = project(&[
            (
                "tsconfig.json",
                r#"{ "compilerOptions": { "declaration": true } }"#,
            ),
            ("src/a/x.ts", ""),
            ("src/b/y.ts", ""),
        ]);
        let root = dir.path().canonicalize().expect("canonical root");
        let mut emit = DeclarationEmit::new(&root);
        let expected = Some(root.join("tsconfig.json"));
        assert_eq!(emit.config_for(&root.join("src/a/x.ts")), expected);
        assert_eq!(emit.config_for(&root.join("src/b/y.ts")), expected);
        // `src/a`, `src/b`, `src` and the root, each listed once.
        assert_eq!(emit.tsconfig_by_dir.len(), 4);
    }

    #[test]
    fn package_json_with_declaration_types_emits() {
        let published = [
            (
                "package.json",
                r#"{ "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
            ),
            ("src/a.ts", ""),
        ];
        assert_eq!(
            config_for(&published, "src/a.ts").as_deref(),
            Some("package.json")
        );
        let source_types = [
            ("package.json", r#"{ "types": "./src/index.ts" }"#),
            ("src/a.ts", ""),
        ];
        assert_eq!(config_for(&source_types, "src/a.ts"), None);
    }
}
