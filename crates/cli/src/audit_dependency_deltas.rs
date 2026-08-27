//! Dependency deltas for the review brief: what a changed `package.json`
//! adds or moves across a major version, read from the base and head manifest
//! text. Pure over its inputs; the git plumbing lives with the other brief
//! passes in `audit.rs`.
//!
//! Firing precision mirrors the public-API rule R1: a manifest yields at most
//! one "added" candidate set and one "major bump" candidate set, never one
//! decision per package. Minor and patch bumps are not candidates; a range
//! that does not start with a numeric version (a workspace, file, git, or tag
//! specifier) is skipped rather than guessed at.

use rustc_hash::FxHashMap;
use serde_json::Value;

/// The `package.json` sections a dependency entry can live in.
const DEPENDENCY_SECTIONS: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "optionalDependencies",
    "peerDependencies",
];

/// One dependency entry that changed between base and head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyChange {
    /// The package name.
    pub name: String,
    /// The base range, `None` when the entry is new.
    pub from: Option<String>,
    /// The head range.
    pub to: String,
    /// 1-based line of the entry in the head manifest, `0` when not found.
    pub line: u32,
}

/// The dependency changes in one manifest, split by candidate kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ManifestDependencyDeltas {
    /// Entries absent at base.
    pub added: Vec<DependencyChange>,
    /// Entries whose range moved across a major version (or a `0.x` minor).
    pub major_bumped: Vec<DependencyChange>,
}

/// Whether `rel_path` is a `package.json` manifest fallow should diff.
#[must_use]
pub fn is_manifest_path(rel_path: &str) -> bool {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    name == "package.json" && !rel_path.contains("node_modules/")
}

/// Diff the dependency sections of a manifest between `base` and `head` text.
/// Malformed JSON on either side yields no deltas: a manifest fallow cannot
/// read is not evidence of anything.
#[must_use]
pub fn manifest_dependency_deltas(base: &str, head: &str) -> ManifestDependencyDeltas {
    let base_entries = dependency_entries(base);
    let head_entries = dependency_entries(head);
    if head_entries.is_empty() {
        return ManifestDependencyDeltas::default();
    }

    let mut deltas = ManifestDependencyDeltas::default();
    let mut names: Vec<&String> = head_entries.keys().collect();
    names.sort();
    for name in names {
        let to = &head_entries[name];
        let line = entry_line(head, name);
        match base_entries.get(name) {
            None => deltas.added.push(DependencyChange {
                name: name.clone(),
                from: None,
                to: to.clone(),
                line,
            }),
            Some(from) if is_major_bump(from, to) => deltas.major_bumped.push(DependencyChange {
                name: name.clone(),
                from: Some(from.clone()),
                to: to.clone(),
                line,
            }),
            Some(_) => {}
        }
    }
    deltas
}

/// Whether moving a range from `from` to `to` crosses a major version. Both
/// sides must start with a numeric version once the range operator is
/// stripped; anything else (workspace, file, git, tag, wildcard) is not a
/// bump fallow can classify. A `0.x` line treats a minor move as major, the
/// semver convention for pre-1.0 packages.
#[must_use]
pub fn is_major_bump(from: &str, to: &str) -> bool {
    let (Some((from_major, from_minor)), Some((to_major, to_minor))) =
        (leading_version(from), leading_version(to))
    else {
        return false;
    };
    if from_major != to_major {
        return true;
    }
    from_major == 0 && from_minor != to_minor
}

/// The `(major, minor)` pair a range starts with, after stripping a leading
/// range operator. `None` when the range does not start with digits.
fn leading_version(range: &str) -> Option<(u64, u64)> {
    let trimmed = range
        .trim()
        .trim_start_matches(['^', '~', '=', 'v', '>', '<', ' ']);
    let core = trimmed.split([' ', '|']).next().unwrap_or(trimmed);
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts
        .next()
        .and_then(|m| m.parse::<u64>().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// Every `name -> range` pair across the dependency sections. A later section
/// never overrides an earlier one, so a package listed as both a peer and a dev
/// dependency keeps its `dependencies`/`devDependencies` range.
fn dependency_entries(text: &str) -> FxHashMap<String, String> {
    let mut entries: FxHashMap<String, String> = FxHashMap::default();
    let Ok(Value::Object(manifest)) = serde_json::from_str::<Value>(text) else {
        return entries;
    };
    for section in DEPENDENCY_SECTIONS {
        let Some(Value::Object(deps)) = manifest.get(section) else {
            continue;
        };
        for (name, range) in deps {
            if let Value::String(range) = range {
                entries.entry(name.clone()).or_insert_with(|| range.clone());
            }
        }
    }
    entries
}

/// 1-based line of the first `"<name>":` key in `text`, `0` when absent.
fn entry_line(text: &str, name: &str) -> u32 {
    let needle = format!("\"{name}\"");
    text.lines()
        .position(|line| {
            line.trim_start()
                .strip_prefix(&needle)
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
        })
        .map_or(0, |index| u32::try_from(index + 1).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_and_major_bumped_entries_are_split_and_sorted() {
        let base = r#"{ "dependencies": { "react": "^18.2.0", "zod": "^3.0.0" } }"#;
        let head = "{\n  \"dependencies\": {\n    \"react\": \"^19.0.0\",\n    \"zod\": \"^3.4.0\",\n    \"dayjs\": \"^1.11.0\"\n  }\n}\n";
        let deltas = manifest_dependency_deltas(base, head);
        assert_eq!(
            deltas.added,
            vec![DependencyChange {
                name: "dayjs".to_string(),
                from: None,
                to: "^1.11.0".to_string(),
                line: 5,
            }]
        );
        assert_eq!(
            deltas.major_bumped,
            vec![DependencyChange {
                name: "react".to_string(),
                from: Some("^18.2.0".to_string()),
                to: "^19.0.0".to_string(),
                line: 3,
            }]
        );
    }

    #[test]
    fn non_numeric_ranges_and_minor_bumps_are_not_candidates() {
        assert!(!is_major_bump("^1.2.0", "^1.9.0"));
        assert!(!is_major_bump("workspace:*", "workspace:^"));
        assert!(!is_major_bump("^2.0.0", "file:../local"));
        assert!(!is_major_bump("latest", "^3.0.0"));
        assert!(is_major_bump("~1.4.0", "2.0.0"));
        assert!(is_major_bump(">=1.0.0 <2.0.0", "^3.0.0"));
    }

    #[test]
    fn zero_x_minor_move_counts_as_major() {
        assert!(is_major_bump("^0.3.1", "^0.4.0"));
        assert!(!is_major_bump("^0.3.1", "^0.3.9"));
    }

    #[test]
    fn dev_and_peer_sections_participate_without_overriding_runtime_ranges() {
        let base = r#"{ "devDependencies": { "vitest": "^1.0.0" } }"#;
        let head = r#"{ "dependencies": { "zod": "^3.0.0" }, "devDependencies": { "vitest": "^2.0.0", "zod": "^9.0.0" } }"#;
        let deltas = manifest_dependency_deltas(base, head);
        assert_eq!(deltas.major_bumped.len(), 1);
        assert_eq!(deltas.major_bumped[0].name, "vitest");
        assert_eq!(deltas.added.len(), 1);
        assert_eq!(deltas.added[0].to, "^3.0.0", "the runtime range wins");
    }

    #[test]
    fn malformed_manifest_yields_nothing() {
        assert_eq!(
            manifest_dependency_deltas("{", r#"{ "dependencies": { "a": "^1.0.0" } }"#),
            ManifestDependencyDeltas {
                added: vec![DependencyChange {
                    name: "a".to_string(),
                    from: None,
                    to: "^1.0.0".to_string(),
                    line: 0,
                }],
                major_bumped: Vec::new(),
            },
            "an unreadable base reads as empty, so every head entry is new"
        );
        assert_eq!(
            manifest_dependency_deltas(r#"{ "dependencies": { "a": "^1.0.0" } }"#, "{"),
            ManifestDependencyDeltas::default()
        );
    }

    #[test]
    fn manifest_paths_exclude_node_modules() {
        assert!(is_manifest_path("package.json"));
        assert!(is_manifest_path("packages/web/package.json"));
        assert!(!is_manifest_path("node_modules/react/package.json"));
        assert!(!is_manifest_path("packages/web/tsconfig.json"));
    }
}
