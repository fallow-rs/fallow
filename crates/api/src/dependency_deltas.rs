//! Dependency deltas for the decision surface: what a changed `package.json`
//! adds or moves across a major version, read from the base and head manifest
//! text, projected onto [`DependencyAnchor`]s. Pure over its inputs; the CLI
//! reads base text through git and the typed runtime through the base
//! worktree, and both hand the pairs to [`dependency_anchors_from_manifests`]
//! so the two routes cannot drift.
//!
//! Firing precision mirrors the public-API rule R1: a manifest yields at most
//! one "added" candidate set and one "major bump" candidate set, never one
//! decision per package. Minor and patch bumps are not candidates; a range
//! that does not start with a numeric version (a workspace, catalog, file,
//! git, or tag specifier) is skipped rather than guessed at.

use fallow_engine::module_graph::PackageImporters;
use fallow_output::ReviewDeltas;
use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;

use crate::decision_surface::{DependencyAnchor, DependencyChangeKind, DependencyEntry};

/// The `package.json` sections a dependency entry can live in, in the
/// precedence order a duplicate name resolves to.
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
    /// The manifest section the entry lives in at head.
    pub section: String,
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

/// A changed manifest with its base and head text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPair {
    /// Root-relative, forward-slashed path of the manifest.
    pub manifest: String,
    /// The base text; `None` when the manifest did not exist at base.
    pub base: Option<String>,
    /// The head text.
    pub head: String,
}

/// Whether `rel_path` is a `package.json` manifest fallow should diff.
#[must_use]
pub fn is_manifest_path(rel_path: &str) -> bool {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    name == "package.json" && !rel_path.contains("node_modules/")
}

/// Diff the dependency sections of a manifest between `base` and `head` text.
/// Malformed JSON on either side reads as empty: an unreadable base makes every
/// head entry new, an unreadable head yields nothing.
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
        let (section, to) = &head_entries[name];
        let line = entry_line(head, section, name);
        let change = |from: Option<String>| DependencyChange {
            name: name.clone(),
            section: section.clone(),
            from,
            to: to.clone(),
            line,
        };
        match base_entries.get(name) {
            None => deltas.added.push(change(None)),
            Some((_, from)) if is_major_bump(from, to) => {
                deltas.major_bumped.push(change(Some(from.clone())));
            }
            Some(_) => {}
        }
    }
    deltas
}

/// Whether moving a range from `from` to `to` crosses a major version. Both
/// sides must start with a numeric version once the range operator is
/// stripped; anything else (workspace, catalog, file, git, tag, wildcard) is
/// not a bump fallow can classify. A `0.x` line treats a minor move as major,
/// the semver convention for pre-1.0 packages.
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

/// Project manifest pairs onto dependency anchors, one per manifest per kind,
/// in manifest order. Importer counts come from the graph's package usage; a
/// package the graph never saw counts zero.
#[must_use]
#[allow(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashMap; the importer map is always built by the engine with the fallow hasher"
)]
pub fn dependency_anchors_from_manifests(
    manifests: &[ManifestPair],
    package_importers: Option<&FxHashMap<String, PackageImporters>>,
) -> Vec<DependencyAnchor> {
    let mut anchors = Vec::new();
    for pair in manifests {
        let deltas = manifest_dependency_deltas(pair.base.as_deref().unwrap_or(""), &pair.head);
        if !deltas.added.is_empty() {
            anchors.push(to_anchor(
                &pair.manifest,
                DependencyChangeKind::Added,
                &deltas.added,
                package_importers,
            ));
        }
        if !deltas.major_bumped.is_empty() {
            anchors.push(to_anchor(
                &pair.manifest,
                DependencyChangeKind::MajorBump,
                &deltas.major_bumped,
                package_importers,
            ));
        }
    }
    anchors
}

/// The stable delta key for one dependency entry: `<manifest>::<name>` for an
/// added entry, `<manifest>::<name>@<from>-><to>` for a bump. The decision
/// candidate key joins these with `|`, so the brief's `deltas` and the
/// decision's `signal_key` name the same change.
#[must_use]
pub fn dependency_delta_key(
    manifest: &str,
    kind: DependencyChangeKind,
    entry: &DependencyEntry,
) -> String {
    match (kind, &entry.from) {
        (DependencyChangeKind::MajorBump, Some(from)) => {
            format!("{manifest}::{}@{from}->{}", entry.name, entry.to)
        }
        _ => format!("{manifest}::{}", entry.name),
    }
}

/// Mirror the dependency anchors onto the brief's `deltas` as stable keys so
/// the JSON envelope names what changed even when the cap collapses the
/// decision.
pub fn fill_dependency_delta_keys(deltas: &mut ReviewDeltas, anchors: &[DependencyAnchor]) {
    for anchor in anchors {
        for entry in &anchor.entries {
            let key = dependency_delta_key(&anchor.manifest, anchor.kind, entry);
            match anchor.kind {
                DependencyChangeKind::MajorBump => deltas.dependency_major_bumped.push(key),
                DependencyChangeKind::Added => deltas.dependency_added.push(key),
            }
        }
    }
    deltas.dependency_added.sort();
    deltas.dependency_major_bumped.sort();
}

fn to_anchor(
    manifest: &str,
    kind: DependencyChangeKind,
    entries: &[DependencyChange],
    package_importers: Option<&FxHashMap<String, PackageImporters>>,
) -> DependencyAnchor {
    // Union, not sum: a module importing two packages of the same batch is one
    // importer, and the count is both displayed and used for ranking.
    let mut importer_ids: FxHashSet<FileId> = FxHashSet::default();
    let mut out_of_diff_ids: FxHashSet<FileId> = FxHashSet::default();
    for entry in entries {
        if let Some(counts) = package_importers.and_then(|map| map.get(&entry.name)) {
            importer_ids.extend(counts.importers.iter().copied());
            out_of_diff_ids.extend(counts.out_of_diff.iter().copied());
        }
    }
    let importers = importer_ids.len() as u64;
    let out_of_diff_importers = out_of_diff_ids.len() as u64;
    DependencyAnchor {
        manifest: manifest.to_string(),
        kind,
        entries: entries
            .iter()
            .map(|entry| DependencyEntry {
                name: entry.name.clone(),
                section: entry.section.clone(),
                from: entry.from.clone(),
                to: entry.to.clone(),
            })
            .collect(),
        importers,
        out_of_diff_importers,
        line: entries
            .iter()
            .map(|entry| entry.line)
            .find(|line| *line > 0)
            .unwrap_or(0),
    }
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

/// Every `name -> (section, range)` pair across the dependency sections. A
/// later section never overrides an earlier one, so a package listed as both
/// a peer and a runtime dependency keeps its `dependencies` range.
fn dependency_entries(text: &str) -> FxHashMap<String, (String, String)> {
    let mut entries: FxHashMap<String, (String, String)> = FxHashMap::default();
    let Ok(Value::Object(manifest)) = serde_json::from_str::<Value>(text) else {
        return entries;
    };
    for section in DEPENDENCY_SECTIONS {
        let Some(Value::Object(deps)) = manifest.get(section) else {
            continue;
        };
        for (name, range) in deps {
            if let Value::String(range) = range {
                entries
                    .entry(name.clone())
                    .or_insert_with(|| (section.to_string(), range.clone()));
            }
        }
    }
    entries
}

/// 1-based line of the `"<name>":` key inside the `"<section>":` block of
/// `text`, `0` when absent. The scan starts at the section header so a
/// package listed in two sections anchors on the section that won.
fn entry_line(text: &str, section: &str, name: &str) -> u32 {
    let is_key = |line: &str, key: &str| {
        line.trim_start()
            .strip_prefix(&format!("\"{key}\""))
            .is_some_and(|rest| rest.trim_start().starts_with(':'))
    };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| is_key(line, section))
        .unwrap_or(0);
    lines[start..]
        .iter()
        .position(|line| is_key(line, name))
        .map_or(0, |offset| {
            u32::try_from(start + offset + 1).unwrap_or(u32::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        name: &str,
        section: &str,
        from: Option<&str>,
        to: &str,
        line: u32,
    ) -> DependencyChange {
        DependencyChange {
            name: name.to_string(),
            section: section.to_string(),
            from: from.map(str::to_string),
            to: to.to_string(),
            line,
        }
    }

    #[test]
    fn added_and_major_bumped_entries_are_split_and_sorted() {
        let base = r#"{ "dependencies": { "react": "^18.2.0", "zod": "^3.0.0" } }"#;
        let head = "{\n  \"dependencies\": {\n    \"react\": \"^19.0.0\",\n    \"zod\": \"^3.4.0\",\n    \"dayjs\": \"^1.11.0\"\n  }\n}\n";
        let deltas = manifest_dependency_deltas(base, head);
        assert_eq!(
            deltas.added,
            vec![change("dayjs", "dependencies", None, "^1.11.0", 5)]
        );
        assert_eq!(
            deltas.major_bumped,
            vec![change(
                "react",
                "dependencies",
                Some("^18.2.0"),
                "^19.0.0",
                3
            )]
        );
    }

    #[test]
    fn non_numeric_ranges_and_minor_bumps_are_not_candidates() {
        assert!(!is_major_bump("^1.2.0", "^1.9.0"));
        assert!(!is_major_bump("workspace:*", "workspace:^"));
        assert!(!is_major_bump("catalog:", "catalog:react19"));
        assert!(!is_major_bump("^2.0.0", "file:../local"));
        assert!(!is_major_bump("latest", "^3.0.0"));
        assert!(!is_major_bump("npm:foo@1.0.0", "npm:foo@2.0.0"));
        assert!(!is_major_bump("1.0.0-beta.2", "1.0.0"));
        assert!(is_major_bump("~1.4.0", "2.0.0"));
        assert!(is_major_bump(">=1.0.0 <2.0.0", "^3.0.0"));
        assert!(is_major_bump("1.x || 2.x", "3.0.0"));
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
        assert_eq!(deltas.major_bumped[0].section, "devDependencies");
        assert_eq!(deltas.added.len(), 1);
        assert_eq!(deltas.added[0].to, "^3.0.0", "the runtime range wins");
        assert_eq!(deltas.added[0].section, "dependencies");
    }

    #[test]
    fn malformed_manifest_yields_nothing_or_all_new() {
        let all_new = manifest_dependency_deltas("{", r#"{ "dependencies": { "a": "^1.0.0" } }"#);
        assert_eq!(
            all_new.added,
            vec![change("a", "dependencies", None, "^1.0.0", 0)]
        );
        assert!(all_new.major_bumped.is_empty());
        assert_eq!(
            manifest_dependency_deltas(r#"{ "dependencies": { "a": "^1.0.0" } }"#, "{"),
            ManifestDependencyDeltas::default()
        );
    }

    #[test]
    fn batched_importers_are_a_union_not_a_sum() {
        let pairs = vec![ManifestPair {
            manifest: "package.json".to_string(),
            base: Some(
                r#"{ "dependencies": { "react": "^18.0.0", "react-dom": "^18.0.0" } }"#.to_string(),
            ),
            head: r#"{ "dependencies": { "react": "^19.0.0", "react-dom": "^19.0.0" } }"#
                .to_string(),
        }];
        let mut importers = FxHashMap::default();
        for name in ["react", "react-dom"] {
            importers.insert(
                name.to_string(),
                PackageImporters {
                    importers: vec![FileId(1), FileId(2)],
                    out_of_diff: vec![FileId(2)],
                },
            );
        }
        let anchors = dependency_anchors_from_manifests(&pairs, Some(&importers));
        assert_eq!(anchors.len(), 1);
        assert_eq!(anchors[0].entries.len(), 2);
        assert_eq!(anchors[0].importers, 2, "two modules import both packages");
        assert_eq!(anchors[0].out_of_diff_importers, 1);
    }

    #[test]
    fn entry_line_follows_the_winning_section() {
        let head = "{\n  \"devDependencies\": {\n    \"zod\": \"^9.0.0\"\n  },\n  \"dependencies\": {\n    \"zod\": \"^3.0.0\"\n  }\n}\n";
        let deltas = manifest_dependency_deltas("{}", head);
        assert_eq!(deltas.added.len(), 1);
        assert_eq!(deltas.added[0].section, "dependencies");
        assert_eq!(
            deltas.added[0].line, 6,
            "the runtime entry, not the dev one above it"
        );
    }

    #[test]
    fn manifest_paths_exclude_node_modules() {
        assert!(is_manifest_path("package.json"));
        assert!(is_manifest_path("packages/web/package.json"));
        assert!(!is_manifest_path("node_modules/react/package.json"));
        assert!(!is_manifest_path("packages/web/tsconfig.json"));
    }

    #[test]
    fn anchors_and_delta_keys_agree_with_the_candidate_key() {
        let pairs = vec![ManifestPair {
            manifest: "packages/web/package.json".to_string(),
            base: Some(r#"{ "dependencies": { "react": "^18.0.0" } }"#.to_string()),
            head: r#"{ "dependencies": { "react": "^19.0.0", "dayjs": "^1.0.0" } }"#.to_string(),
        }];
        let ids = |range: std::ops::Range<u32>| range.map(FileId).collect::<Vec<_>>();
        let mut importers = FxHashMap::default();
        importers.insert(
            "react".to_string(),
            PackageImporters {
                importers: ids(0..40),
                out_of_diff: ids(2..40),
            },
        );
        let anchors = dependency_anchors_from_manifests(&pairs, Some(&importers));
        assert_eq!(anchors.len(), 2);
        assert_eq!(anchors[0].kind, DependencyChangeKind::Added);
        assert_eq!(anchors[0].importers, 0);
        assert_eq!(anchors[1].kind, DependencyChangeKind::MajorBump);
        assert_eq!(anchors[1].importers, 40);
        assert_eq!(anchors[1].out_of_diff_importers, 38);

        let mut deltas = ReviewDeltas::default();
        fill_dependency_delta_keys(&mut deltas, &anchors);
        assert_eq!(
            deltas.dependency_added,
            vec!["packages/web/package.json::dayjs"]
        );
        assert_eq!(
            deltas.dependency_major_bumped,
            vec!["packages/web/package.json::react@^18.0.0->^19.0.0"]
        );
        let surface = crate::decision_surface::extract_decision_surface(
            &crate::decision_surface::DecisionInputs {
                deltas: &deltas,
                boundary_anchors: &[],
                coordination: &[],
                dependency_anchors: &anchors,
                public_api_anchor_line: 0,
                affected_not_shown: 0,
                routing: &fallow_output::RoutingFacts::default(),
                head_source: &|_: &str| None,
                rename_old_path: &|_: &str| None,
                internal_consumers: &|_: &str| 0,
                cap: 4,
            },
        );
        let keys: Vec<&str> = surface
            .decisions
            .iter()
            .map(|d| d.signal_key.as_str())
            .collect();
        assert!(keys.contains(&"packages/web/package.json::react@^18.0.0->^19.0.0"));
        assert!(keys.contains(&"packages/web/package.json::dayjs"));
    }
}
