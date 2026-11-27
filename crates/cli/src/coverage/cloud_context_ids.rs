//! Line-independent IDs for the hot-path, blast-radius and importance entries
//! that `coverage analyze --cloud` builds from the local AST, so an edit above
//! a function does not change the ID of that function. The hash recipe is the
//! one of the Fallow Cloud runtime-context rows. The inputs are the local
//! repo-relative path and the local name, so the ID equals the cloud ID only
//! when the cloud row has the same path and name (a proven path prefix and no
//! runtime rename, such as a `get ` prefix or a callee-named callback).

use std::fmt::Write as _;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};

use super::analyze::StaticIndex;
use super::cloud_join::CloudMergeEntries;

/// Hex characters of the hash part of an ID, the length of the cloud IDs.
const ID_HEX_LENGTH: usize = 8;

/// Counts the local functions per (repo-relative file, name), because the
/// name key is unique only when the file has one function of that name.
#[derive(Debug, Default)]
pub(super) struct ContextIdResolver {
    group_sizes: FxHashMap<(String, String), usize>,
}

impl ContextIdResolver {
    /// Counts the distinct functions of each (file, name) of the local AST.
    /// The local AST has every function of a file, also the functions that
    /// did not run. A count over the cloud rows only would see one `map`
    /// when only one of two `map` functions ran, and the other one could
    /// then take over its ID in a later period.
    pub(super) fn from_index(index: &StaticIndex) -> Self {
        let group_sizes = index
            .by_path_name
            .iter()
            .map(|((path, name), functions)| {
                let distinct: FxHashSet<&str> = functions
                    .iter()
                    .map(|function| function.stable_id.as_str())
                    .collect();
                ((normalized(Path::new(path)), name.clone()), distinct.len())
            })
            .collect();
        Self { group_sizes }
    }

    /// The ID of one function. The name key (file and name) when the read has
    /// no other function of that name in that file, so the ID survives a line
    /// move. Else the `stable_id` key: it holds the start line, so a function
    /// that moves past a same-name function gets a new ID and does not take
    /// over the ID of the other function.
    pub(super) fn id(&self, kind: &str, path: &Path, name: &str, stable_id: &str) -> String {
        let file = normalized(path);
        let unique = self
            .group_sizes
            .get(&(file.clone(), name.to_owned()))
            .is_none_or(|count| *count <= 1);
        let key = if unique {
            format!("name\0{file}\0{name}")
        } else {
            format!("stable\0{stable_id}")
        };
        let digest = Sha256::digest(format!("{kind}\0{key}").as_bytes());
        format!("fallow:{kind}:{}", hex_prefix(&digest))
    }

    /// Gives each built entry the line-independent ID and keeps its file+line
    /// ID in `legacy_id` for the transition window.
    pub(super) fn assign(&self, entries: &mut CloudMergeEntries) {
        for hot in &mut entries.hot_paths {
            let stable_id = hot.stable_id.clone().unwrap_or_default();
            let id = self.id("hot", &hot.path, &hot.function, &stable_id);
            hot.legacy_id = Some(std::mem::replace(&mut hot.id, id));
        }
        for blast in &mut entries.synthesized_blast_radius {
            let stable_id = blast.stable_id.clone().unwrap_or_default();
            let id = self.id("blast", &blast.file, &blast.function, &stable_id);
            blast.legacy_id = Some(std::mem::replace(&mut blast.id, id));
        }
        for (importance, _) in &mut entries.synthesized_importance {
            let stable_id = importance.stable_id.clone().unwrap_or_default();
            let id = self.id(
                "importance",
                &importance.file,
                &importance.function,
                &stable_id,
            );
            importance.legacy_id = Some(std::mem::replace(&mut importance.id, id));
        }
    }
}

/// The first `ID_HEX_LENGTH` lowercase hex characters of a digest.
fn hex_prefix(digest: &[u8]) -> String {
    digest.iter().take(ID_HEX_LENGTH / 2).fold(
        String::with_capacity(ID_HEX_LENGTH),
        |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        },
    )
}

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches('/')
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::analyze::tests::{static_index_with, static_info};

    fn resolver(functions: &[(&str, &str)]) -> ContextIdResolver {
        let infos = functions
            .iter()
            .enumerate()
            .map(|(line, (path, name))| static_info(path, name, line as u32 + 1, line as u32 + 2))
            .collect();
        ContextIdResolver::from_index(&static_index_with(infos))
    }

    #[test]
    fn matches_the_cloud_recipe_for_a_unique_name() {
        // `keyedId("blast", "name\0src/a.ts\0handler")` of fallow-cloud
        // `src/services/runtime-context-ids.ts`, computed with Node.
        let expected = "fallow:blast:d250d0be";
        let ids = resolver(&[("src/a.ts", "handler")]);
        assert_eq!(
            ids.id("blast", Path::new("src/a.ts"), "handler", "fallow:fn:1"),
            expected
        );
    }

    #[test]
    fn keeps_the_id_when_a_unique_function_moves_to_another_line() {
        let ids = resolver(&[("src/a.ts", "handler")]);
        assert_eq!(
            ids.id("hot", Path::new("src/a.ts"), "handler", "fallow:fn:line10"),
            ids.id("hot", Path::new("src/a.ts"), "handler", "fallow:fn:line14"),
        );
    }

    #[test]
    fn uses_the_stable_id_for_same_name_functions_of_one_file() {
        let ids = resolver(&[("src/a.ts", "map"), ("src/a.ts", "map")]);
        let first = ids.id("hot", Path::new("src/a.ts"), "map", "fallow:fn:3");
        let second = ids.id("hot", Path::new("src/a.ts"), "map", "fallow:fn:9");
        assert_ne!(first, second);
        assert_eq!(
            first,
            ids.id("hot", Path::new("src/a.ts"), "map", "fallow:fn:3")
        );
    }

    #[test]
    fn gives_each_kind_its_own_id() {
        let ids = resolver(&[("src/a.ts", "handler")]);
        let hot = ids.id("hot", Path::new("src/a.ts"), "handler", "s");
        let blast = ids.id("blast", Path::new("src/a.ts"), "handler", "s");
        assert!(hot.starts_with("fallow:hot:"));
        assert!(blast.starts_with("fallow:blast:"));
        assert_ne!(hot[11..], blast[13..]);
    }
}
