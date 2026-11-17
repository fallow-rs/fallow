//! `circularDependencies.allow`: the cycles that the config accepts
//! (issue #4623).

use std::path::Path;

use crate::discover::FileId;
use fallow_config::ResolvedConfig;
use rustc_hash::FxHashMap;

use crate::graph::{CycleOptions, MAX_CYCLE_LENGTH, ModuleGraph};

/// The cycles of `graph` that no `circularDependencies.allow` entry accepts.
/// Records which entries had an effect, for the unmatched-entry diagnostic.
///
/// The detector checks each entry while it lists cycles, so an accepted
/// cycle does not use a slot of the per-group cycle limit. An entry that
/// the detector cannot check counts as matched: an entry longer than the
/// longest listed cycle, and an entry whose files all lie in one group whose
/// listing stopped at the limit.
pub(super) fn find_unaccepted_cycles(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
) -> Vec<Vec<FileId>> {
    let options = CycleOptions {
        ignore_lazy_imports: config.circular_dependencies.ignore_lazy_imports,
    };
    let mut allow_list = CycleAllowList::new(config);
    let search = if allow_list.entries.is_empty() {
        graph.find_cycles_accepting(options, &mut |_| false)
    } else {
        graph.find_cycles_accepting(options, &mut |cycle| allow_list.accepts(graph, cycle))
    };
    allow_list.mark_unprovable(graph, &search.capped_sccs);
    allow_list.record();
    search.cycles
}

/// The `allow` entries of one cycle detection pass, with a flag per entry
/// that records whether the entry matched a cycle.
struct CycleAllowList<'a> {
    config: &'a ResolvedConfig,
    canonical_root: Option<std::path::PathBuf>,
    entries: Vec<Vec<String>>,
    matched: Vec<bool>,
}

impl<'a> CycleAllowList<'a> {
    fn new(config: &'a ResolvedConfig) -> Self {
        let entries: Vec<Vec<String>> = config
            .circular_dependencies
            .allow
            .iter()
            .map(|entry| {
                entry
                    .iter()
                    .map(|file| normalize_entry_path(file))
                    .collect()
            })
            .collect();
        let matched = vec![false; entries.len()];
        Self {
            config,
            canonical_root: dunce::canonicalize(&config.root).ok(),
            entries,
            matched,
        }
    }

    /// Whether an entry accepts `cycle`. Every entry that matches is marked
    /// as matched, so two equal entries do not report one as unmatched.
    fn accepts(&mut self, graph: &ModuleGraph, cycle: &[FileId]) -> bool {
        let files: Vec<String> = cycle
            .iter()
            .map(|id| self.relative_path(&graph.modules[id.0 as usize].path))
            .collect();
        let mut accepted = false;
        for (entry, matched) in self.entries.iter().zip(&mut self.matched) {
            if is_rotation(entry, &files) {
                *matched = true;
                accepted = true;
            }
        }
        accepted
    }

    /// Mark each unmatched entry that the detector cannot check as matched,
    /// so it gives no false unmatched-entry diagnostic.
    fn mark_unprovable(&mut self, graph: &ModuleGraph, capped_sccs: &[Vec<FileId>]) {
        if self.matched.iter().all(|matched| *matched) {
            return;
        }
        let mut group_of: FxHashMap<String, usize> = FxHashMap::default();
        for (group, scc) in capped_sccs.iter().enumerate() {
            for id in scc {
                let path = self.relative_path(&graph.modules[id.0 as usize].path);
                group_of.insert(path, group);
            }
        }
        for (entry, matched) in self.entries.iter().zip(&mut self.matched) {
            if *matched {
                continue;
            }
            let first_group = entry.first().and_then(|file| group_of.get(file));
            let in_one_capped_group =
                first_group.is_some() && entry.iter().all(|file| group_of.get(file) == first_group);
            *matched = entry.len() > MAX_CYCLE_LENGTH || in_one_capped_group;
        }
    }

    /// Store which entries matched a cycle in this pass.
    fn record(self) {
        self.config
            .circular_dependencies_allow_effect
            .record(self.matched);
    }

    fn relative_path(&self, path: &Path) -> String {
        let relative = path
            .strip_prefix(&self.config.root)
            .ok()
            .or_else(|| {
                self.canonical_root
                    .as_deref()
                    .and_then(|root| path.strip_prefix(root).ok())
            })
            .unwrap_or(path);
        relative.to_string_lossy().replace('\\', "/")
    }
}

/// Mark every `allow` entry as matched, for a pass that does not detect
/// cycles. The entries then give no diagnostic.
pub(super) fn record_all_matched(config: &ResolvedConfig) {
    config
        .circular_dependencies_allow_effect
        .record(vec![true; config.circular_dependencies.allow.len()]);
}

/// An entry path as fallow compares it: forward slashes, and no leading `./`.
fn normalize_entry_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut rest = path.as_str();
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    rest.to_owned()
}

/// Whether `cycle` is `entry` from some start file. A detected cycle has no
/// repeated file, so at most one start file can match.
fn is_rotation(entry: &[String], cycle: &[String]) -> bool {
    if entry.len() != cycle.len() || entry.is_empty() {
        return false;
    }
    let Some(start) = cycle.iter().position(|file| *file == entry[0]) else {
        return false;
    };
    entry
        .iter()
        .enumerate()
        .all(|(offset, file)| cycle[(start + offset) % cycle.len()] == *file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn rotation_matches_from_any_start_file() {
        let cycle = strings(&["a", "b", "c"]);
        assert!(is_rotation(&strings(&["a", "b", "c"]), &cycle));
        assert!(is_rotation(&strings(&["b", "c", "a"]), &cycle));
        assert!(is_rotation(&strings(&["c", "a", "b"]), &cycle));
    }

    #[test]
    fn rotation_rejects_another_order_or_length() {
        let cycle = strings(&["a", "b", "c"]);
        assert!(!is_rotation(&strings(&["a", "c", "b"]), &cycle));
        assert!(!is_rotation(&strings(&["a", "b"]), &cycle));
        assert!(!is_rotation(&strings(&["a", "b", "c", "d"]), &cycle));
        assert!(!is_rotation(&[], &[]));
    }

    #[test]
    fn entry_paths_drop_a_leading_dot_slash() {
        assert_eq!(normalize_entry_path("./src/a.ts"), "src/a.ts");
        assert_eq!(normalize_entry_path("src\\a.ts"), "src/a.ts");
        assert_eq!(normalize_entry_path("src/a.ts"), "src/a.ts");
    }
}
