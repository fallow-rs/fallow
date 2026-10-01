use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;
use serde_json::Value;

pub(super) fn has_matching_manifest_json(
    root: &Path,
    discovered_files: &[PathBuf],
    candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    matches_manifest: impl Fn(&Value) -> bool,
) -> bool {
    manifest_json_candidates(root, discovered_files)
        .into_iter()
        // Outside production mode the discovery walk already recorded which
        // directories actually contain a `manifest.json`, so skip filesystem
        // reads for candidate directories that have none. In production
        // (`None`) fall back to probing every candidate.
        .filter(|path| match candidate_index {
            Some(index) => path
                .parent()
                .is_some_and(|dir| index.dir_contains(dir, std::ffi::OsStr::new("manifest.json"))),
            None => true,
        })
        .any(|path| {
            let Ok(source) = std::fs::read_to_string(path) else {
                return false;
            };
            parse_manifest_json(&source).is_some_and(|manifest| matches_manifest(&manifest))
        })
}

pub(super) fn parse_manifest_json(source: &str) -> Option<Value> {
    serde_json::from_str(source).ok()
}

fn manifest_json_candidates(root: &Path, discovered_files: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = FxHashSet::default();
    let mut candidates = Vec::new();
    push_manifest_candidate(root, &mut seen, &mut candidates);

    for file in discovered_files {
        let mut current = file.parent();
        while let Some(dir) = current {
            if !dir.starts_with(root) {
                break;
            }
            push_manifest_candidate(dir, &mut seen, &mut candidates);
            if dir == root {
                break;
            }
            current = dir.parent();
        }
    }

    candidates
}

fn push_manifest_candidate(
    dir: &Path,
    seen: &mut FxHashSet<PathBuf>,
    candidates: &mut Vec<PathBuf>,
) {
    let candidate = dir.join("manifest.json");
    if seen.insert(candidate.clone()) {
        candidates.push(candidate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_probe_skips_directories_outside_source_ancestors() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let nested = root.join("packages/plugin");
        let unused = root.join("packages/unused");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::create_dir_all(&unused).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("manifest.json"), "{ malformed").unwrap();
        std::fs::write(nested.join("manifest.json"), r#"{"kind":"target"}"#).unwrap();
        std::fs::write(unused.join("manifest.json"), r#"{"kind":"unused"}"#).unwrap();
        std::fs::write(outside.join("manifest.json"), r#"{"kind":"target"}"#).unwrap();

        let discovered = vec![nested.join("src/main.ts")];
        let indexed_paths = [
            root.join("manifest.json"),
            nested.join("manifest.json"),
            unused.join("manifest.json"),
            outside.join("manifest.json"),
        ];
        let index = super::super::registry::ConfigCandidateIndex::build(
            indexed_paths.iter().map(PathBuf::as_path),
        );
        let kind_is = |kind: &'static str| {
            move |value: &Value| value.get("kind").and_then(Value::as_str) == Some(kind)
        };

        for candidate_index in [None, Some(&index)] {
            assert!(has_matching_manifest_json(
                &root,
                &discovered,
                candidate_index,
                kind_is("target")
            ));
            // `packages/unused` has a manifest but no discovered source, so
            // the probe must not read it.
            assert!(!has_matching_manifest_json(
                &root,
                &discovered,
                candidate_index,
                kind_is("unused")
            ));
            // A manifest outside the analysis root is never read.
            assert!(!has_matching_manifest_json(
                &outside.join("nested"),
                &[],
                candidate_index,
                kind_is("target")
            ));
        }
    }
}
