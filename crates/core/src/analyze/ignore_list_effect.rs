//! Which entries of `ignoreExports`, `duplicates.ignore` and `health.ignore`
//! match a file or an export of the module graph (issue #5167). The dead-code
//! envelope reports each entry that matches nothing.

use fallow_config::{IgnoreExportName, ResolvedConfig};
use globset::GlobMatcher;
use rustc_hash::FxHashSet;

use crate::graph::{ModuleGraph, ModuleNode};

/// The export name of a re-export that forwards every export of its source.
const STAR: &str = "*";

/// Start the config pattern records of a dead-code pass over `graph`: reset
/// the records that the detectors fill, and record the ignore list effect.
pub(super) fn begin_pass(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    config.begin_dead_code_pass(workspaces);
    record_ignore_list_effect(graph, config);
}

/// Write the answer of this pass to `config.ignore_list_effect`.
///
/// An `ignoreExports` file glob has an effect when it matches the
/// root-relative path of a module. An export name has an effect when a module that its `file` glob
/// matches declares or re-exports the name. A module with an `export *`
/// can forward any name, so it gives every name of its rules an effect: the
/// check never reports a name that a star re-export can supply.
///
/// A `duplicates.ignore` or `health.ignore` glob has an effect when it matches
/// a module or any other project file. The health pipeline also applies
/// `health.ignore` to Markdown and template files outside the graph, so the
/// graph alone does not prove that a glob matches nothing.
fn record_ignore_list_effect(graph: &ModuleGraph, config: &ResolvedConfig) {
    let names = config.ignore_export_names();
    let duplicates = compile(&config.duplicates.ignore);
    let health = compile(&config.health.ignore);
    let mut file_hits = vec![false; config.compiled_ignore_exports.len()];
    let mut name_hits = vec![false; names.len()];
    let mut duplicates_hits = initial_hits(&duplicates);
    let mut health_hits = initial_hits(&health);

    for module in &graph.modules {
        let relative = module
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&module.path)
            .to_string_lossy()
            .replace('\\', "/");
        mark_matches(&duplicates, &relative, &mut duplicates_hits);
        mark_matches(&health, &relative, &mut health_hits);
        let matched_rules: Vec<usize> = config
            .compiled_ignore_exports
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.matcher.is_match(relative.as_str()))
            .map(|(index, _)| index)
            .collect();
        if matched_rules.is_empty() {
            continue;
        }
        for &rule in &matched_rules {
            file_hits[rule] = true;
        }
        mark_declared_names(module, &names, &matched_rules, &mut name_hits);
    }

    if duplicates_hits.contains(&false) || health_hits.contains(&false) {
        mark_project_files(
            config,
            &duplicates,
            &health,
            &mut duplicates_hits,
            &mut health_hits,
        );
    }

    let effect = &config.ignore_list_effect;
    effect.ignore_export_files.record(file_hits);
    effect.ignore_export_names.record(name_hits);
    effect.duplicates_ignore.record(duplicates_hits);
    effect.health_ignore.record(health_hits);
}

/// Directory names that the project walk does not enter. No analysis reads
/// files there, so a glob that matches only such files has no effect.
const SKIPPED_DIRECTORIES: [&str; 2] = ["node_modules", ".git"];

/// Credit the `duplicates.ignore` and `health.ignore` globs that match a
/// project file outside the graph. The walk follows the ignore files of
/// source discovery, and it ends once every glob has an effect. It runs only
/// when a glob matched no module, so a config without stale entries pays
/// nothing.
fn mark_project_files(
    config: &ResolvedConfig,
    duplicates: &[Option<GlobMatcher>],
    health: &[Option<GlobMatcher>],
    duplicates_hits: &mut [bool],
    health_hits: &mut [bool],
) {
    let mut builder = fallow_config::source_walk_builder(&config.root);
    builder.filter_entry(|entry| {
        entry
            .file_name()
            .to_str()
            .is_none_or(|name| !SKIPPED_DIRECTORIES.contains(&name))
    });
    for entry in builder.build().flatten() {
        if entry.file_type().is_none_or(|kind| kind.is_dir()) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(&config.root)
            .unwrap_or_else(|_| entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        mark_matches(duplicates, &relative, duplicates_hits);
        mark_matches(health, &relative, health_hits);
        if !duplicates_hits.contains(&false) && !health_hits.contains(&false) {
            return;
        }
    }
}

fn mark_declared_names(
    module: &ModuleNode,
    names: &[IgnoreExportName<'_>],
    matched_rules: &[usize],
    hits: &mut [bool],
) {
    let forwards_any = module
        .re_exports
        .iter()
        .any(|edge| edge.exported_name == STAR);
    let declared: FxHashSet<String> = module
        .exports
        .iter()
        .map(|export| export.name.to_string())
        .chain(
            module
                .re_exports
                .iter()
                .map(|edge| edge.exported_name.clone()),
        )
        .collect();
    for (hit, name) in hits.iter_mut().zip(names) {
        if matched_rules.contains(&name.rule) && (forwards_any || declared.contains(name.export)) {
            *hit = true;
        }
    }
}

fn mark_matches(globs: &[Option<GlobMatcher>], relative: &str, hits: &mut [bool]) {
    for (hit, glob) in hits.iter_mut().zip(globs) {
        if glob.as_ref().is_some_and(|glob| glob.is_match(relative)) {
            *hit = true;
        }
    }
}

/// Compile the globs of one config list. Config validation rejects a glob
/// that does not compile, so `None` does not occur in a loaded config.
fn compile(patterns: &[String]) -> Vec<Option<GlobMatcher>> {
    patterns
        .iter()
        .map(|pattern| {
            fallow_config::compile_path_glob(pattern)
                .ok()
                .map(|glob| glob.compile_matcher())
        })
        .collect()
}

/// One flag per glob. A glob without a matcher starts as matched, so the
/// check reports nothing for it.
fn initial_hits(globs: &[Option<GlobMatcher>]) -> Vec<bool> {
    globs.iter().map(Option::is_none).collect()
}
