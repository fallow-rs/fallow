//! Files that one package.json `imports` entry names under different
//! conditions.
//!
//! The resolver gives an import of `#x` an edge to each condition target of
//! the entry, such as `x.browser.ts` and `x.ts`. Each environment loads one of
//! them, so the files export the same names by design. They are one module,
//! not two origins of a duplicate export.

use rustc_hash::FxHashMap;

use crate::discover::FileId;

use super::ExportEntry;

/// The condition-target families of a project, as a union of file sets.
#[derive(Default)]
pub(super) struct ConditionTargetFamilies {
    parent: FxHashMap<FileId, FileId>,
}

impl ConditionTargetFamilies {
    /// Join the targets that one `#` import or re-export statement reaches.
    pub(super) fn build(resolved_modules: &[crate::resolve::ResolvedModule]) -> Self {
        let mut families = Self::default();
        for module in resolved_modules {
            for targets in hash_import_targets(module).values() {
                if let Some((first, rest)) = targets.split_first() {
                    for other in rest {
                        families.join(*first, *other);
                    }
                }
            }
        }
        families
    }

    fn root(&self, mut file: FileId) -> FileId {
        while let Some(&parent) = self.parent.get(&file) {
            if parent == file {
                break;
            }
            file = parent;
        }
        file
    }

    fn join(&mut self, left: FileId, right: FileId) {
        let left_root = self.root(left);
        let right_root = self.root(right);
        if left_root != right_root {
            self.parent.insert(right_root, left_root);
        }
    }

    /// Keep one file per family in `entries`: the file with the lowest path.
    /// Entry order is preserved.
    pub(super) fn collapse(&self, entries: Vec<ExportEntry>) -> Vec<ExportEntry> {
        if self.parent.is_empty() {
            return entries;
        }
        let mut representative: FxHashMap<FileId, (&std::path::Path, FileId)> =
            FxHashMap::default();
        for entry in &entries {
            let root = self.root(entry.file_id);
            representative
                .entry(root)
                .and_modify(|(path, file)| {
                    if entry.path.as_path() < *path {
                        *path = entry.path.as_path();
                        *file = entry.file_id;
                    }
                })
                .or_insert((entry.path.as_path(), entry.file_id));
        }
        let keep: Vec<bool> = entries
            .iter()
            .map(|entry| representative[&self.root(entry.file_id)].1 == entry.file_id)
            .collect();
        entries
            .into_iter()
            .zip(keep)
            .filter_map(|(entry, keep)| keep.then_some(entry))
            .collect()
    }
}

/// The project files that each `#` import or re-export statement of `module`
/// reaches, keyed by the specifier and the start of its source span.
fn hash_import_targets(
    module: &crate::resolve::ResolvedModule,
) -> FxHashMap<(&str, u32), Vec<FileId>> {
    let imports = module
        .resolved_imports
        .iter()
        .chain(&module.resolved_dynamic_imports)
        .map(|import| (&import.info.source, import.info.source_span, &import.target));
    let re_exports = module.re_exports.iter().map(|re_export| {
        (
            &re_export.info.source,
            re_export.info.source_span,
            &re_export.target,
        )
    });
    let mut targets: FxHashMap<(&str, u32), Vec<FileId>> = FxHashMap::default();
    for (source, span, target) in imports.chain(re_exports) {
        if !source.starts_with('#') {
            continue;
        }
        if let Some(file) = target.internal_file_id() {
            targets
                .entry((source.as_str(), span.start))
                .or_default()
                .push(file);
        }
    }
    targets
}
