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

use oxc_span::Span;

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

    // Speculative imports are synthesised by fallow (e.g. the Vitest
    // `__mocks__/<file>` auto-mock sibling) to credit a side-effect file when
    // it exists. The user never wrote the synthesised path, so when it fails
    // to resolve drop the entry silently rather than surfacing it as an
    // `unresolved-import` finding. The credit path is unaffected: a
    // speculative import whose target resolves still produces a regular
    // `ResolvedImport`. See issue #378.
    if imp.is_speculative && matches!(target, ResolveResult::Unresolvable(_)) {
        return Vec::new();
    }

    if !imp.destructured_names.is_empty() {
        // `const { a, b } = await import('./x')` -> Named imports
        return imp
            .destructured_names
            .iter()
            .map(|name| {
                let imported_name = if name == "default" {
                    ImportedName::Default
                } else {
                    ImportedName::Named(name.clone())
                };
                ResolvedImport {
                    info: ImportInfo {
                        source: imp.source.clone(),
                        imported_name,
                        local_name: name.clone(),
                        is_type_only: false,
                        from_style: false,
                        span: imp.span,
                        source_span: Span::default(),
                    },
                    target: target.clone(),
                }
            })
            .collect();
    }

    if imp.local_name.is_some() {
        // `const mod = await import('./x')` -> Namespace with local_name
        return vec![ResolvedImport {
            info: ImportInfo {
                source: imp.source.clone(),
                imported_name: ImportedName::Namespace,
                local_name: imp.local_name.clone().unwrap_or_default(),
                is_type_only: false,
                from_style: false,
                span: imp.span,
                source_span: Span::default(),
            },
            target,
        }];
    }

    // Side-effect only: `await import('./x')` with no assignment
    vec![ResolvedImport {
        info: ImportInfo {
            source: imp.source.clone(),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: imp.span,
            source_span: Span::default(),
        },
        target,
    }]
}

/// Resolve dynamic import patterns via glob matching against discovered files.
/// When canonical paths are available, uses those for matching. Otherwise falls
/// back to raw file paths from `files` (avoids allocating a separate PathBuf vec).
pub(super) fn resolve_dynamic_patterns(
    from_dir: &Path,
    patterns: &[DynamicImportPattern],
    canonical_paths: &[PathBuf],
    files: &[DiscoveredFile],
) -> Vec<(DynamicImportPattern, Vec<FileId>)> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let glob_str = make_glob_from_pattern(pattern);
            let matcher = globset::Glob::new(&glob_str)
                .ok()
                .map(|g| g.compile_matcher())?;
            let matched: Vec<FileId> = if canonical_paths.is_empty() {
                // Root is canonical — use raw file paths directly (no extra allocation)
                files
                    .iter()
                    .filter(|f| {
                        f.path.strip_prefix(from_dir).is_ok_and(|relative| {
                            let rel_str = format!("./{}", relative.to_string_lossy());
                            matcher.is_match(&rel_str)
                        })
                    })
                    .map(|f| f.id)
                    .collect()
            } else {
                canonical_paths
                    .iter()
                    .enumerate()
                    .filter(|(_idx, canonical)| {
                        canonical.strip_prefix(from_dir).is_ok_and(|relative| {
                            let rel_str = format!("./{}", relative.to_string_lossy());
                            matcher.is_match(&rel_str)
                        })
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
