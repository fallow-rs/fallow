//! Resolution of dynamic `import()` calls and glob-based dynamic import patterns.
//!
//! Handles two distinct forms of dynamic imports:
//!
//! 1. **Concrete dynamic imports** (`import('./foo')`) — resolved via the standard
//!    specifier resolver. Destructured awaits (`const { a } = await import(...)`)
//!    expand into individual named imports; assigned awaits become namespace imports;
//!    bare calls become side-effect imports.
//!
//! 2. **Dynamic import patterns** (`import(\`./routes/${name}\`)`) — resolved via
//!    glob matching against the discovered file set. The template literal is converted
//!    to a glob pattern and matched against file paths relative to the importing
//!    directory, producing a list of candidate `FileId`s.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oxc_span::Span;
use rustc_hash::FxHashMap;

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{DynamicImportInfo, DynamicImportPattern, ImportInfo, ImportedName};

use super::ResolveResult;
use super::ResolvedImport;
use super::fallbacks::make_glob_from_pattern;
use super::specifier::resolve_specifier;
use super::types::ResolveContext;

/// Resolve dynamic `import()` calls, expanding destructured names into individual imports.
pub(super) fn resolve_dynamic_imports(
    ctx: &ResolveContext,
    file_path: &Path,
    dynamic_imports: &[DynamicImportInfo],
) -> Vec<ResolvedImport> {
    dynamic_imports
        .iter()
        .flat_map(|imp| resolve_single_dynamic_import(ctx, file_path, imp))
        .collect()
}

/// Convert a single dynamic import into one or more `ResolvedImport` entries.
pub(super) fn resolve_single_dynamic_import(
    ctx: &ResolveContext,
    file_path: &Path,
    imp: &DynamicImportInfo,
) -> Vec<ResolvedImport> {
    let target = resolve_specifier(ctx, file_path, &imp.source, false);

    // Speculative candidates (the `__mocks__` sibling synthesized for
    // factory-less `vi.mock`/`jest.mock` calls) exist solely to discover an
    // internal manual-mock file. A package-space result means the specifier
    // stayed bare after alias substitution; `pkg/__mocks__/...` is not a
    // runner convention, so treating it as a hit would fabricate a phantom
    // package name (`@scope/__mocks__`, issue #2213) for the
    // unlisted-dependency analysis.
    if imp.is_speculative
        && (target.is_bare_package() || matches!(target, ResolveResult::Unresolvable(_)))
    {
        return Vec::new();
    }

    if !imp.destructured_names.is_empty() {
        return resolve_destructured_dynamic_import(imp, &target);
    }

    if imp.local_name.is_some() {
        return vec![dynamic_import_with(
            imp,
            ImportedName::Namespace,
            imp.local_name.clone().unwrap_or_default(),
            target,
        )];
    }

    vec![dynamic_import_with(
        imp,
        ImportedName::SideEffect,
        String::new(),
        target,
    )]
}

/// Expand a destructured-await dynamic import into one named/default import per
/// destructured binding.
fn resolve_destructured_dynamic_import(
    imp: &DynamicImportInfo,
    target: &ResolveResult,
) -> Vec<ResolvedImport> {
    imp.destructured_names
        .iter()
        .map(|name| {
            let imported_name = if name == "default" {
                ImportedName::Default
            } else {
                ImportedName::Named(name.clone())
            };
            dynamic_import_with(imp, imported_name, name.clone(), target.clone())
        })
        .collect()
}

/// Build a single `ResolvedImport` from a dynamic import, its imported-name
/// shape, local binding, and resolved target.
fn dynamic_import_with(
    imp: &DynamicImportInfo,
    imported_name: ImportedName,
    local_name: String,
    target: ResolveResult,
) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: imp.source.clone(),
            imported_name,
            local_name,
            is_type_only: false,
            from_style: false,
            span: imp.span,
            source_span: Span::default(),
        },
        target,
    }
}

/// Session-local cache of compiled glob matchers keyed by glob string.
///
/// The glob string derives only from the pattern text, not the importing
/// directory, so one compiled matcher serves every module that contains the
/// same dynamic import pattern. Failed compilations are cached as `None` so a
/// malformed pattern is not recompiled per module.
#[derive(Default)]
pub(super) struct GlobMatcherCache {
    map: Mutex<FxHashMap<String, Option<globset::GlobMatcher>>>,
}

impl GlobMatcherCache {
    /// Return the cached matcher for `glob_str`, compiling it on first miss.
    fn get(&self, glob_str: &str) -> Option<globset::GlobMatcher> {
        if let Ok(cache) = self.map.lock()
            && let Some(value) = cache.get(glob_str)
        {
            return value.clone();
        }
        let value = globset::Glob::new(glob_str)
            .ok()
            .map(|g| g.compile_matcher());
        if let Ok(mut cache) = self.map.lock() {
            cache.insert(glob_str.to_string(), value.clone());
        }
        value
    }
}

/// Resolve dynamic import patterns via glob matching against discovered files.
/// When canonical paths are available, uses those for matching. Otherwise falls
/// back to raw file paths from `files` (avoids allocating a separate PathBuf vec).
pub(super) fn resolve_dynamic_patterns(
    glob_cache: &GlobMatcherCache,
    from_dir: &Path,
    patterns: &[DynamicImportPattern],
    canonical_paths: &[PathBuf],
    files: &[DiscoveredFile],
) -> Vec<(DynamicImportPattern, Vec<FileId>)> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let glob_str = make_glob_from_pattern(pattern);
            // Candidates are the paths relative to `from_dir`, which carry no
            // leading "./". Stripping the prefix from the glob once lets each
            // candidate be matched as-is instead of allocating a "./"-prefixed
            // String per file per pattern.
            let matcher = glob_cache.get(glob_str.strip_prefix("./").unwrap_or(&glob_str))?;
            let matched: Vec<FileId> = if canonical_paths.is_empty() {
                files
                    .iter()
                    .filter(|f| {
                        f.path
                            .strip_prefix(from_dir)
                            .is_ok_and(|relative| matcher.is_match(relative))
                    })
                    .map(|f| f.id)
                    .collect()
            } else {
                canonical_paths
                    .iter()
                    .enumerate()
                    .filter(|(_idx, canonical)| {
                        canonical
                            .strip_prefix(from_dir)
                            .is_ok_and(|relative| matcher.is_match(relative))
                    })
                    .map(|(idx, _)| files[idx].id)
                    .collect()
            };
            if matched.is_empty() {
                None
            } else {
                Some((pattern.clone(), matched))
            }
        })
        .collect()
}
