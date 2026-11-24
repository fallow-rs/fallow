//! Which entries of `ignoreExports`, `duplicates.ignore` and `health.ignore`
//! match a file or an export of the module graph (issue #5167). The dead-code
//! envelope reports each entry that matches nothing.

use fallow_config::{IgnoreExportName, ResolvedConfig};
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
/// a module or any other project file, see
/// [`ResolvedConfig::path_glob_effect`].
fn record_ignore_list_effect(graph: &ModuleGraph, config: &ResolvedConfig) {
    let names = config.ignore_export_names();
    let mut file_hits = vec![false; config.compiled_ignore_exports.len()];
    let mut name_hits = vec![false; names.len()];

    for module in &graph.modules {
        let relative = module
            .path
            .strip_prefix(&config.root)
            .unwrap_or(&module.path)
            .to_string_lossy()
            .replace('\\', "/");
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

    let mut path_hits = config
        .path_glob_effect(
            &[&config.duplicates.ignore, &config.health.ignore],
            graph.modules.iter().map(|module| module.path.as_path()),
        )
        .into_iter();
    let effect = &config.ignore_list_effect;
    effect.ignore_export_files.record(file_hits);
    effect.ignore_export_names.record(name_hits);
    effect
        .duplicates_ignore
        .record(path_hits.next().unwrap_or_default());
    effect
        .health_ignore
        .record(path_hits.next().unwrap_or_default());
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
