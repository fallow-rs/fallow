//! Resolution of flag keys that name a member of an imported registry.
//!
//! Extraction sees one file only. A read such as `useFlag(FLAGS.X)`, where
//! `FLAGS` is imported, therefore reaches the engine with the registry name
//! and the member name but without the key. This module finds the exported
//! registry and returns the key.

use std::path::{Component, Path, PathBuf};

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{FlagKeyRegistry, FlagRegistryRead, ImportedName, ModuleInfo};
use rustc_hash::FxHashMap;

/// Extensions tried, in order, for an import specifier without one.
const SOURCE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// JavaScript extensions that TypeScript sources import under.
const SCRIPT_EXTENSIONS: &[&str] = &["js", "jsx", "mjs", "cjs"];

/// The exported flag-key registries of a project.
pub struct RegistryIndex<'m> {
    by_file: FxHashMap<FileId, &'m [FlagKeyRegistry]>,
    by_name: FxHashMap<&'m str, Vec<&'m FlagKeyRegistry>>,
    file_ids: FxHashMap<&'m Path, FileId>,
}

impl<'m> RegistryIndex<'m> {
    /// Index the registries of `modules`. Returns `None` when no module reads
    /// through an imported registry, so the common project pays nothing.
    pub fn build(files: &'m [DiscoveredFile], modules: &'m [ModuleInfo]) -> Option<Self> {
        let has_reads = modules.iter().any(|module| {
            module
                .flag_registry_facts
                .as_ref()
                .is_some_and(|facts| !facts.reads.is_empty())
        });
        if !has_reads {
            return None;
        }

        let mut by_file = FxHashMap::default();
        let mut by_name: FxHashMap<&str, Vec<&FlagKeyRegistry>> = FxHashMap::default();
        for module in modules {
            let Some(facts) = module.flag_registry_facts.as_ref() else {
                continue;
            };
            if facts.registries.is_empty() {
                continue;
            }
            by_file.insert(module.file_id, facts.registries.as_slice());
            for registry in &facts.registries {
                by_name
                    .entry(registry.export_name.as_str())
                    .or_default()
                    .push(registry);
            }
        }
        let file_ids = files
            .iter()
            .map(|file| (file.path.as_path(), file.id))
            .collect();
        Some(Self {
            by_file,
            by_name,
            file_ids,
        })
    }

    /// Resolve the flag key of `read` in `module`, which lives at `path`.
    ///
    /// The registry comes from the file that a relative import names. When
    /// the import is not relative (a path alias), or when the file does not
    /// declare the registry itself (a barrel file), the one registry in the
    /// project with the imported name is used. Two or more registries with
    /// that name leave the read unresolved.
    pub fn resolve(
        &self,
        module: &ModuleInfo,
        path: &Path,
        read: &FlagRegistryRead,
    ) -> Option<&'m str> {
        let import = module
            .imports
            .iter()
            .find(|import| import.local_name == read.registry && !import.is_type_only)?;
        let ImportedName::Named(imported) = &import.imported_name else {
            return None;
        };
        let registry = self
            .registry_in_imported_file(path, &import.source, imported)
            .or_else(|| self.unique_registry(imported))?;
        registry
            .members
            .iter()
            .find(|(member, _)| *member == read.member)
            .map(|(_, key)| key.as_str())
    }

    fn registry_in_imported_file(
        &self,
        importer: &Path,
        specifier: &str,
        export_name: &str,
    ) -> Option<&'m FlagKeyRegistry> {
        let file_id = self.resolve_relative(importer, specifier)?;
        self.by_file
            .get(&file_id)?
            .iter()
            .find(|registry| registry.export_name == export_name)
    }

    fn unique_registry(&self, export_name: &str) -> Option<&'m FlagKeyRegistry> {
        match self.by_name.get(export_name)?.as_slice() {
            [registry] => Some(registry),
            _ => None,
        }
    }

    fn resolve_relative(&self, importer: &Path, specifier: &str) -> Option<FileId> {
        if !(specifier.starts_with("./") || specifier.starts_with("../")) {
            return None;
        }
        let base = normalize(&importer.parent()?.join(specifier));
        candidates(&base).find_map(|candidate| self.file_ids.get(candidate.as_path()).copied())
    }
}

/// The files an import of `base` can name, in resolution order.
fn candidates(base: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let script_stem = base
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| SCRIPT_EXTENSIONS.contains(ext))
        .map(|_| base.with_extension(""));
    let with_extension = SOURCE_EXTENSIONS
        .iter()
        .map(move |ext| append_extension(base, ext));
    let script_sources = script_stem.into_iter().flat_map(|stem| {
        SOURCE_EXTENSIONS
            .iter()
            .map(move |ext| stem.with_extension(ext))
    });
    let index_files = SOURCE_EXTENSIONS
        .iter()
        .map(move |ext| base.join(format!("index.{ext}")));
    std::iter::once(base.to_path_buf())
        .chain(with_extension)
        .chain(script_sources)
        .chain(index_files)
}

fn append_extension(base: &Path, ext: &str) -> PathBuf {
    let mut path = base.as_os_str().to_owned();
    path.push(".");
    path.push(ext);
    PathBuf::from(path)
}

/// Remove `.` and `..` components without touching the file system.
fn normalize(path: &Path) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_cover_extensions_script_rewrites_and_index_files() {
        let base = Path::new("/p/src/flags.js");
        let all: Vec<PathBuf> = candidates(base).collect();
        assert_eq!(all[0], PathBuf::from("/p/src/flags.js"));
        assert!(all.contains(&PathBuf::from("/p/src/flags.ts")));
        assert!(all.contains(&PathBuf::from("/p/src/flags.js/index.ts")));

        let dir: Vec<PathBuf> = candidates(Path::new("/p/src/config")).collect();
        assert!(dir.contains(&PathBuf::from("/p/src/config.tsx")));
        assert!(dir.contains(&PathBuf::from("/p/src/config/index.ts")));
    }

    #[test]
    fn normalize_removes_dot_components() {
        assert_eq!(
            normalize(Path::new("/p/src/features/../config/./flags")),
            PathBuf::from("/p/src/config/flags")
        );
    }
}
