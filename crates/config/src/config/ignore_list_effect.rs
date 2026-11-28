//! Which entries of `ignoreExports`, `duplicates.ignore`, `health.ignore`
//! and `overrides[].files` matched a file or an export in the latest
//! dead-code pass (issues #5167 and #5227).

use std::path::Path;

use crate::PathMatcher;

use super::{EntryEffectRecord, ResolvedConfig};

/// The export name of an `ignoreExports` rule that covers every export of
/// its files. It names no single export, so it can only miss as a file glob.
const ALL_EXPORTS: &str = "*";

/// One answer per entry of each list. The dead-code pass writes them from
/// the module graph, and the dead-code envelope reads them.
#[derive(Debug, Clone, Default)]
pub struct IgnoreListEffect {
    /// One flag per `ignoreExports` rule: its `file` glob matched a file.
    pub ignore_export_files: EntryEffectRecord,
    /// One flag per name of [`ResolvedConfig::ignore_export_names`]: a file
    /// of its rule declares the name.
    pub ignore_export_names: EntryEffectRecord,
    /// One flag per `duplicates.ignore` glob: it matched a file.
    pub duplicates_ignore: EntryEffectRecord,
    /// One flag per `health.ignore` glob: it matched a file.
    pub health_ignore: EntryEffectRecord,
    /// One flag per glob of [`ResolvedConfig::override_globs`]: it matched a
    /// file.
    pub override_files: EntryEffectRecord,
}

/// One glob of an `overrides[].files` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverrideGlob<'a> {
    /// The index of the override in `overrides`.
    pub entry: usize,
    /// The glob, as written in the config.
    pub pattern: &'a str,
}

/// An export name of an `ignoreExports` rule, with the `file` glob of the
/// rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoreExportName<'a> {
    /// The index of the rule in `ignoreExports`.
    pub rule: usize,
    /// The `file` glob of the rule, as written in the config.
    pub file: &'a str,
    /// The export name, as written in the config.
    pub export: &'a str,
}

impl ResolvedConfig {
    /// The export names of every `ignoreExports` rule in config order,
    /// without `*`. [`IgnoreListEffect::ignore_export_names`] has one flag
    /// per entry of this list.
    #[must_use]
    pub fn ignore_export_names(&self) -> Vec<IgnoreExportName<'_>> {
        self.ignore_export_rules
            .iter()
            .enumerate()
            .flat_map(|(rule, entry)| {
                entry
                    .exports
                    .iter()
                    .filter(|export| export.as_str() != ALL_EXPORTS)
                    .map(move |export| IgnoreExportName {
                        rule,
                        file: entry.file.as_str(),
                        export: export.as_str(),
                    })
            })
            .collect()
    }

    /// The `files` globs of every `overrides` entry, in config order.
    /// [`IgnoreListEffect::override_files`] has one flag per entry of this
    /// list.
    #[must_use]
    pub fn override_globs(&self) -> Vec<OverrideGlob<'_>> {
        self.overrides
            .iter()
            .enumerate()
            .flat_map(|(entry, resolved)| {
                resolved.files.iter().map(move |pattern| OverrideGlob {
                    entry,
                    pattern: pattern.as_str(),
                })
            })
            .collect()
    }
}

/// Directory names that the project walk does not enter. No analysis reads
/// files there, so a glob that matches only such files has no effect.
const SKIPPED_DIRECTORIES: [&str; 2] = ["node_modules", ".git"];

impl ResolvedConfig {
    /// One flag list per glob list of `lists`, with one flag per glob: `true`
    /// when the glob matches the root-relative path of one of `paths`, or of
    /// another project file.
    ///
    /// The duplication and health pipelines apply their `ignore` globs to more
    /// files than the analysis input, for example Markdown and template
    /// files, so `paths` alone does not prove that a glob matches nothing. A
    /// project walk with the ignore files of source discovery checks the globs
    /// that matched none of `paths`, and it ends once every glob has an
    /// effect. A config without stale globs does not start the walk.
    #[must_use]
    pub fn path_glob_effect<'p>(
        &self,
        lists: &[&[String]],
        paths: impl IntoIterator<Item = &'p Path>,
    ) -> Vec<Vec<bool>> {
        let globs: Vec<Vec<Option<PathMatcher>>> = lists.iter().map(|list| compile(list)).collect();
        let mut hits: Vec<Vec<bool>> = globs.iter().map(|list| initial_hits(list)).collect();
        for path in paths {
            mark_matches(&globs, &self.relative(path), &mut hits);
        }
        if any_unmatched(&hits) {
            self.mark_project_files(&globs, &mut hits);
        }
        hits
    }

    fn mark_project_files(&self, globs: &[Vec<Option<PathMatcher>>], hits: &mut [Vec<bool>]) {
        let builder = crate::source_walk_builder(&self.root, |entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !SKIPPED_DIRECTORIES.contains(&name))
        });
        for entry in builder.build().flatten() {
            if entry.file_type().is_none_or(|kind| kind.is_dir()) {
                continue;
            }
            mark_matches(globs, &self.relative(entry.path()), hits);
            if !any_unmatched(hits) {
                return;
            }
        }
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn any_unmatched(hits: &[Vec<bool>]) -> bool {
    hits.iter().any(|list| list.contains(&false))
}

fn mark_matches(globs: &[Vec<Option<PathMatcher>>], relative: &str, hits: &mut [Vec<bool>]) {
    for (list_hits, list_globs) in hits.iter_mut().zip(globs) {
        for (hit, glob) in list_hits.iter_mut().zip(list_globs) {
            if glob.as_ref().is_some_and(|glob| glob.is_match(relative)) {
                *hit = true;
            }
        }
    }
}

/// Compile the globs of one config list. Config validation rejects a glob
/// that does not compile, so `None` does not occur in a loaded config.
fn compile(patterns: &[String]) -> Vec<Option<PathMatcher>> {
    patterns
        .iter()
        .map(|pattern| {
            crate::compile_path_glob(pattern)
                .ok()
                .and_then(|glob| crate::try_compile_matcher(&glob).ok())
        })
        .collect()
}

/// One flag per glob. A glob without a matcher starts as matched, so the
/// check reports nothing for it.
fn initial_hits(globs: &[Option<PathMatcher>]) -> Vec<bool> {
    globs.iter().map(Option::is_none).collect()
}
