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

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use oxc_span::Span;
use rustc_hash::FxHashMap;

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{DynamicImportInfo, DynamicImportPattern, ImportInfo, ImportedName};

use super::ResolveResult;
use super::ResolvedImport;
use super::fallbacks::make_glob_from_pattern;
use super::specifier::resolve_import_specifier;
use super::types::ResolveContext;
use fallow_config::extglob::ExtglobMatcher;

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
    let target = resolve_import_specifier(ctx, file_path, &imp.source, false);

    // Speculative candidates (the `__mocks__` sibling and root-level
    // candidates synthesized for factory-less `vi.mock`/`jest.mock` calls)
    // exist solely to discover an internal manual-mock file. A package-space
    // result means the specifier stayed bare after alias substitution;
    // `pkg/__mocks__/...` is not a runner convention, so treating it as a hit
    // would fabricate a phantom package name (`@scope/__mocks__`, issue #2213)
    // for the unlisted-dependency analysis. Before dropping, a root-relative
    // `__mocks__/<specifier>` candidate gets one last chance against the
    // runner's root-level manual-mock convention (issue #2225).
    if imp.is_speculative
        && (target.is_bare_package() || matches!(target, ResolveResult::Unresolvable(_)))
    {
        if let Some(file_id) = resolve_root_manual_mock(ctx, file_path, &imp.source) {
            return vec![dynamic_import_with(
                imp,
                ImportedName::Namespace,
                imp.local_name.clone().unwrap_or_default(),
                ResolveResult::SyntheticAutoImport(file_id),
            )];
        }
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

    // An escaping namespace without a binding is a whole-module edge: a
    // `Namespace` import with an empty local name credits every export.
    if imp.value_escapes {
        return vec![dynamic_import_with(
            imp,
            ImportedName::Namespace,
            String::new(),
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

/// Resolve a root-level manual-mock candidate (`__mocks__/<specifier>`) to a
/// project file.
///
/// A factory-less `vi.mock` / `jest.mock` of a bare package specifier loads
/// its manual mock from the `__mocks__` directory at the runner's project
/// root: Jest applies node-module manual mocks automatically, Vitest only for
/// registered `vi.mock` calls (issue #2225). The extractor synthesizes the
/// candidate with a root-relative `__mocks__/` source that always classifies
/// as package space, so this probe is its only resolution path. Ancestors of
/// the importing file are probed up to the analysis root so workspace members
/// with their own runner root are covered.
fn resolve_root_manual_mock(
    ctx: &ResolveContext<'_>,
    file_path: &Path,
    source: &str,
) -> Option<FileId> {
    let candidate = source.strip_prefix("__mocks__/")?;
    if candidate.is_empty() {
        return None;
    }
    let mut dir = file_path.parent();
    while let Some(current) = dir {
        let base = current.join("__mocks__").join(candidate);
        for ext in ctx.extensions {
            let mut with_ext = base.clone().into_os_string();
            with_ext.push(ext);
            if let Some(file_id) = project_file_id(ctx, Path::new(&with_ext)) {
                return Some(file_id);
            }
            let index = base.join(format!("index{ext}"));
            if let Some(file_id) = project_file_id(ctx, &index) {
                return Some(file_id);
            }
        }
        if current == ctx.root {
            break;
        }
        dir = current.parent();
    }
    None
}

/// Look up a constructed candidate path in the discovered-file indexes.
fn project_file_id(ctx: &ResolveContext<'_>, path: &Path) -> Option<FileId> {
    ctx.raw_path_to_id
        .get(path)
        .or_else(|| ctx.path_to_id.get(path))
        .copied()
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
            is_type_only_star: false,
            from_style: false,
            span: imp.span,
            source_span: Span::default(),
        },
        target,
    }
}

/// Session-local cache of compiled glob matchers keyed by glob string.
///
/// The include glob derives only from the pattern text, so one compiled
/// matcher serves every module that contains the same dynamic import pattern.
/// A rebased exclude can hold directory names of the importing file, so such
/// a pattern compiles one matcher per importing directory. Failed compilations are cached as `None` so a
/// malformed pattern is not recompiled or reported again per module.
#[derive(Default)]
pub(super) struct GlobMatcherCache {
    map: Mutex<FxHashMap<String, Option<Arc<ExtglobMatcher>>>>,
}

impl GlobMatcherCache {
    /// Return the cached matcher for `glob_str` with the exclude globs
    /// `excludes`, compiling it on first miss.
    fn get(&self, glob_str: &str, excludes: &[&str]) -> Option<Arc<ExtglobMatcher>> {
        // A NUL byte cannot occur in a path glob, so it separates the globs
        // of the key without ambiguity.
        let mut key = glob_str.to_string();
        for exclude in excludes {
            key.push('\0');
            key.push_str(exclude);
        }
        if let Ok(cache) = self.map.lock()
            && let Some(value) = cache.get(&key)
        {
            return value.clone();
        }
        let value = ExtglobMatcher::new(glob_str, excludes).map(Arc::new);
        // Two threads can compile the same pattern at the same time. Only the
        // thread that inserts the entry first gives the warning.
        let first_insert = self
            .map
            .lock()
            .map_or(true, |mut cache| cache.insert(key, value.clone()).is_none());
        if value.is_none() && first_insert {
            tracing::warn!(
                "Dynamic import pattern `{glob_str}` is not a valid glob and matches no files"
            );
        }
        value
    }
}

/// Resolve dynamic import patterns via glob matching against discovered files.
/// When canonical paths are available, uses those for matching. Otherwise falls
/// back to raw file paths from `files` (avoids allocating a separate PathBuf vec).
/// Every input pattern retains one output row, including empty matches, so the
/// rows stay positional for resolver-cache restoration.
///
/// A pattern is relative to the importing directory `from_dir`, and its leading
/// `../` steps move the match base up, as Vite and webpack do. A `../` step
/// that leaves `root` makes the pattern match no files. `root` must have the same form as `from_dir`:
/// canonical when the candidate paths are canonical.
pub(super) fn resolve_dynamic_patterns(
    glob_cache: &GlobMatcherCache,
    root: &Path,
    from_dir: &Path,
    patterns: &[DynamicImportPattern],
    canonical_paths: &[PathBuf],
    files: &[DiscoveredFile],
) -> Vec<(DynamicImportPattern, Vec<FileId>)> {
    patterns
        .iter()
        .map(|pattern| {
            let glob_str = make_glob_from_pattern(pattern);
            let Some(based) = BasedPattern::new(&glob_str, &pattern.excludes, from_dir, root)
            else {
                return (pattern.clone(), Vec::new());
            };
            let excludes: Vec<&str> = based.excludes.iter().map(String::as_str).collect();
            let Some(matcher) = glob_cache.get(based.include, &excludes) else {
                return (pattern.clone(), Vec::new());
            };
            let matches = |path: &Path| {
                path.strip_prefix(based.base)
                    .is_ok_and(|relative| matcher.is_match(relative))
            };
            let matched: Vec<FileId> = if canonical_paths.is_empty() {
                files
                    .iter()
                    .filter(|f| matches(&f.path))
                    .map(|f| f.id)
                    .collect()
            } else {
                canonical_paths
                    .iter()
                    .enumerate()
                    .filter(|(_idx, canonical)| matches(canonical))
                    .map(|(idx, _)| files[idx].id)
                    .collect()
            };
            (pattern.clone(), matched)
        })
        .collect()
}

/// A dynamic import glob rewritten against the directory that its leading
/// `./` and `../` steps select. The include and the exclude globs carry no
/// leading `./` or `../`, so each candidate path relative to `base` is
/// matched as-is.
struct BasedPattern<'a> {
    base: &'a Path,
    include: &'a str,
    excludes: Vec<String>,
}

impl<'a> BasedPattern<'a> {
    /// Returns `None` when a `../` step leaves the filesystem root or the
    /// project `root`. A pattern without `../` steps keeps `from_dir` as its
    /// base and gets no root check.
    fn new(glob: &'a str, excludes: &[String], from_dir: &'a Path, root: &Path) -> Option<Self> {
        let (ups, include) = split_parent_steps(glob);
        let base = from_dir.ancestors().nth(ups)?;
        if ups > 0 && !base.starts_with(root) {
            return None;
        }
        let excludes = excludes
            .iter()
            .filter_map(|exclude| rebase_exclude(exclude, ups, from_dir))
            .collect();
        Some(Self {
            base,
            include,
            excludes,
        })
    }
}

/// Rewrite an exclude glob, relative to `from_dir`, to the base that is `ups`
/// steps above `from_dir`. An exclude with fewer `../` steps gets the glob-escaped
/// names of the directories between the base and its own start. An exclude
/// with more `../` steps starts above the base and is dropped, so it removes
/// no file.
fn rebase_exclude(exclude: &str, ups: usize, from_dir: &Path) -> Option<String> {
    let (exclude_ups, rest) = split_parent_steps(exclude);
    if exclude_ups > ups {
        return None;
    }
    let skipped: Vec<_> = from_dir
        .components()
        .rev()
        .skip(exclude_ups)
        .take(ups - exclude_ups)
        .collect();
    if skipped.len() != ups - exclude_ups {
        return None;
    }
    let mut rebased = String::new();
    for component in skipped.iter().rev() {
        let Component::Normal(name) = component else {
            return None;
        };
        escape_glob_literal(&name.to_string_lossy(), &mut rebased);
        rebased.push('/');
    }
    rebased.push_str(rest);
    Some(rebased)
}

/// Split the leading `./` and `../` steps from a relative glob. Returns the
/// number of `../` steps and the rest of the glob.
fn split_parent_steps(glob: &str) -> (usize, &str) {
    let mut ups = 0;
    let mut rest = glob;
    loop {
        if let Some(after) = rest.strip_prefix("./") {
            rest = after;
        } else if let Some(after) = rest.strip_prefix("../") {
            rest = after;
            ups += 1;
        } else {
            return (ups, rest);
        }
    }
}

/// Append `text` to `out` with each glob metacharacter as a one-character
/// class, the same escape that the extractor applies to literal pattern text.
fn escape_glob_literal(text: &str, out: &mut String) {
    for character in text.chars() {
        if matches!(character, '*' | '?' | '[' | ']' | '{' | '}' | '(') {
            out.push('[');
            out.push(character);
            out.push(']');
        } else {
            out.push(character);
        }
    }
}
