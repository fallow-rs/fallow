//! The shrink-only rule for a committed baseline.
//!
//! A baseline records debt that a repository still must pay. A change that adds
//! a finding can also re-save the baseline, and then the new finding is
//! suppressed and no stale entry exists. This module compares the baseline file
//! with the same file at a base ref and names each key that the base file does
//! not have. The comparison reads two file versions only. It needs no analysis
//! run, so it gives the same answer on a whole-project run and on a narrowed run.
//!
//! A key is compared as the file writes it. A renamed file or a moved line
//! gives a new key, so the rule counts it as growth. The rule is strict on
//! purpose: a reviewer approves each new key, or the change removes it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value};

use crate::baseline::BaselineKind;

/// The keys that a baseline has and its version at the base ref does not have.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaselineGrowth {
    /// One item per category that grew, sorted by category name.
    pub categories: Vec<GrownCategory>,
}

/// The new keys of one baseline category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrownCategory {
    /// The category as the baseline file names it, for example `unused_files`.
    pub category: String,
    /// The new keys, sorted.
    pub keys: Vec<String>,
}

impl BaselineGrowth {
    /// The number of new keys in all categories.
    #[must_use]
    pub fn added_entries(&self) -> usize {
        self.categories.iter().map(|grown| grown.keys.len()).sum()
    }

    /// True when the baseline has no key that the base does not have.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.categories.is_empty()
    }
}

/// Compare a baseline (`head`) with its version at the base ref (`base`).
///
/// Both values are the parsed JSON files. `kind` selects the categories that
/// the format of the command writes:
///
/// - `dead-code`: every top-level array of keys.
/// - `dupes`: the content fingerprints of the clone groups. A base file saved
///   before fingerprints existed is compared by its `clone_groups` keys.
/// - `health`: the legacy `findings` keys, the finding counts per file and
///   category (a higher count is growth), the runtime-coverage finding IDs and
///   the refactoring target keys. When both files carry per-function counts,
///   those counts replace the per-file counts.
#[must_use]
pub fn baseline_growth(kind: BaselineKind, base: &Value, head: &Value) -> BaselineGrowth {
    let empty = Map::new();
    let base = base.as_object().unwrap_or(&empty);
    let head = head.as_object().unwrap_or(&empty);
    let mut grown: BTreeMap<String, Vec<String>> = BTreeMap::new();
    match kind {
        BaselineKind::DeadCode => {
            for (category, value) in head {
                if value.is_array() {
                    add_new_keys(&mut grown, category, base, head);
                }
            }
        }
        BaselineKind::Dupes => {
            let category = if base.contains_key(DUPES_FINGERPRINTS) {
                DUPES_FINGERPRINTS
            } else {
                DUPES_LEGACY_GROUPS
            };
            add_new_keys(&mut grown, category, base, head);
        }
        BaselineKind::Health => {
            for category in HEALTH_KEY_CATEGORIES {
                add_new_keys(&mut grown, category, base, head);
            }
            let counts = if has_entries(base, HEALTH_IDENTITY_COUNTS)
                && has_entries(head, HEALTH_IDENTITY_COUNTS)
            {
                HEALTH_IDENTITY_COUNTS
            } else {
                HEALTH_FILE_COUNTS
            };
            add_grown_counts(&mut grown, counts, base, head);
        }
    }
    BaselineGrowth {
        categories: grown
            .into_iter()
            .filter(|(_, keys)| !keys.is_empty())
            .map(|(category, keys)| GrownCategory { category, keys })
            .collect(),
    }
}

const DUPES_FINGERPRINTS: &str = "normalized_clone_fingerprints";
const DUPES_LEGACY_GROUPS: &str = "clone_groups";
const HEALTH_KEY_CATEGORIES: [&str; 3] = ["findings", "runtime_coverage_findings", "target_keys"];
const HEALTH_FILE_COUNTS: &str = "finding_counts";
const HEALTH_IDENTITY_COUNTS: &str = "identity_finding_counts";

/// The string members of the array `category`, as a set.
fn key_set<'a>(object: &'a Map<String, Value>, category: &str) -> BTreeSet<&'a str> {
    object
        .get(category)
        .and_then(Value::as_array)
        .map(|keys| keys.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

fn has_entries(object: &Map<String, Value>, category: &str) -> bool {
    object
        .get(category)
        .and_then(Value::as_object)
        .is_some_and(|buckets| !buckets.is_empty())
}

fn add_new_keys(
    grown: &mut BTreeMap<String, Vec<String>>,
    category: &str,
    base: &Map<String, Value>,
    head: &Map<String, Value>,
) {
    let known = key_set(base, category);
    let added: Vec<String> = key_set(head, category)
        .into_iter()
        .filter(|key| !known.contains(key))
        .map(display_key)
        .collect();
    if !added.is_empty() {
        grown.entry(category.to_owned()).or_default().extend(added);
    }
}

/// Health count buckets: `path -> finding category -> { count }`. A bucket
/// whose count is higher than at the base is growth, also when the base has no
/// such bucket.
fn add_grown_counts(
    grown: &mut BTreeMap<String, Vec<String>>,
    category: &str,
    base: &Map<String, Value>,
    head: &Map<String, Value>,
) {
    let Some(head_buckets) = head.get(category).and_then(Value::as_object) else {
        return;
    };
    let base_buckets = base.get(category).and_then(Value::as_object);
    let mut added = Vec::new();
    for (path, finding_counts) in head_buckets {
        let Some(finding_counts) = finding_counts.as_object() else {
            continue;
        };
        for (finding, count) in finding_counts {
            let now = bucket_count(Some(count));
            let before = bucket_count(
                base_buckets
                    .and_then(|buckets| buckets.get(path))
                    .and_then(|counts| counts.get(finding)),
            );
            if now > before {
                let key = display_key(path);
                added.push(if before == 0 {
                    format!("{key} {finding} (count {now})")
                } else {
                    format!("{key} {finding} (count {before} -> {now})")
                });
            }
        }
    }
    if !added.is_empty() {
        grown.entry(category.to_owned()).or_default().extend(added);
    }
}

fn bucket_count(bucket: Option<&Value>) -> u64 {
    bucket
        .and_then(|bucket| bucket.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Some formats join key parts with a NUL byte. Show it as `:`, so the
/// message stays on one line and prints in every terminal.
fn display_key(key: &str) -> String {
    key.replace('\0', ":")
}

/// Why the baseline at the base ref could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseBaselineError {
    /// `git` could not be started.
    GitMissing(String),
    /// The baseline is not in a git work tree.
    NotARepository,
    /// Git cannot resolve the ref to a commit, typically because a shallow
    /// clone did not fetch it.
    RefUnavailable,
    /// Git failed for another reason.
    GitFailed(String),
}

/// Read the version of the file at `path` that the commit `git_ref` has.
///
/// `Ok(None)` when the commit has no file at that path, which is a baseline
/// that the change adds. The path is resolved relative to the directory of the
/// file, so a baseline in a nested directory or under `--root` reads correctly.
pub fn read_baseline_at_ref(
    path: &Path,
    git_ref: &str,
) -> Result<Option<String>, BaseBaselineError> {
    let dir = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(BaseBaselineError::GitFailed(format!(
            "the baseline path {} has no file name",
            path.display()
        )));
    };
    let inside = run_git(dir, &["rev-parse", "--is-inside-work-tree"])?;
    if !inside.status.success() {
        return Err(BaseBaselineError::NotARepository);
    }
    let resolved = run_git(
        dir,
        &["rev-parse", "--verify", "--quiet", &peel_to_commit(git_ref)],
    )?;
    if !resolved.status.success() {
        return Err(BaseBaselineError::RefUnavailable);
    }
    let object = format!("{git_ref}:./{name}");
    let exists = run_git(dir, &["cat-file", "-e", &object])?;
    if !exists.status.success() {
        return Ok(None);
    }
    let shown = run_git(dir, &["show", &object])?;
    if !shown.status.success() {
        return Err(BaseBaselineError::GitFailed(
            String::from_utf8_lossy(&shown.stderr).trim().to_owned(),
        ));
    }
    Ok(Some(String::from_utf8_lossy(&shown.stdout).into_owned()))
}

/// True when `git_ref` names the commit that `HEAD` names in the repository
/// at `dir`. A base that resolves to `HEAD` compares a committed baseline with
/// itself, so only an uncommitted change can grow it.
#[must_use]
pub fn ref_is_head(dir: &Path, git_ref: &str) -> bool {
    let resolve = |rev: &str| {
        run_git(
            dir,
            &["rev-parse", "--verify", "--quiet", &peel_to_commit(rev)],
        )
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    match (resolve(git_ref), resolve("HEAD")) {
        (Some(base), Some(head)) => base == head,
        _ => false,
    }
}

/// `rev^{commit}`: git resolves it only when `rev` names a commit.
fn peel_to_commit(rev: &str) -> String {
    format!("{rev}^{{commit}}")
}

fn run_git(dir: &Path, args: &[&str]) -> Result<std::process::Output, BaseBaselineError> {
    crate::git_env::git_command()
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| BaseBaselineError::GitMissing(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn keys(growth: &BaselineGrowth) -> Vec<(String, Vec<String>)> {
        growth
            .categories
            .iter()
            .map(|grown| (grown.category.clone(), grown.keys.clone()))
            .collect()
    }

    #[test]
    fn dead_code_growth_is_each_new_key_per_category() {
        let base = json!({
            "kind": "dead-code",
            "unused_files": ["src/a.ts"],
            "boundary_violations": ["src/ui/a.ts->src/core/x.ts"],
        });
        let head = json!({
            "kind": "dead-code",
            "unused_files": ["src/a.ts", "src/b.ts"],
            "boundary_violations": [
                "src/ui/a.ts->src/core/x.ts",
                "src/ui/c.ts->src/core/z.ts"
            ],
            "unused_exports": ["src/c.ts:x"],
        });
        let growth = baseline_growth(BaselineKind::DeadCode, &base, &head);
        assert_eq!(
            keys(&growth),
            vec![
                (
                    "boundary_violations".to_owned(),
                    vec!["src/ui/c.ts->src/core/z.ts".to_owned()]
                ),
                ("unused_exports".to_owned(), vec!["src/c.ts:x".to_owned()]),
                ("unused_files".to_owned(), vec!["src/b.ts".to_owned()]),
            ]
        );
        assert_eq!(growth.added_entries(), 3);
    }

    #[test]
    fn a_removed_or_kept_key_is_not_growth() {
        let base = json!({ "unused_files": ["src/a.ts", "src/b.ts"] });
        let head = json!({ "unused_files": ["src/b.ts"] });
        assert!(baseline_growth(BaselineKind::DeadCode, &base, &head).is_empty());
    }

    #[test]
    fn dupes_growth_counts_one_key_per_clone_group() {
        let base = json!({
            "clone_groups": ["a.ts:1-9|b.ts:1-9"],
            "clone_fingerprints": ["f1"],
            "normalized_clone_fingerprints": ["n1"],
        });
        let head = json!({
            "clone_groups": ["a.ts:1-9|b.ts:1-9", "c.ts:1-9|d.ts:1-9"],
            "clone_fingerprints": ["f1", "f2"],
            "normalized_clone_fingerprints": ["n1", "n2"],
        });
        let growth = baseline_growth(BaselineKind::Dupes, &base, &head);
        assert_eq!(
            keys(&growth),
            vec![(DUPES_FINGERPRINTS.to_owned(), vec!["n2".to_owned()])]
        );
    }

    #[test]
    fn a_legacy_dupes_base_is_compared_by_clone_group_keys() {
        let base = json!({ "clone_groups": ["a.ts:1-9|b.ts:1-9"] });
        let head = json!({
            "clone_groups": ["a.ts:1-9|b.ts:1-9"],
            "normalized_clone_fingerprints": ["n1"],
        });
        assert!(baseline_growth(BaselineKind::Dupes, &base, &head).is_empty());
    }

    #[test]
    fn a_higher_health_count_is_growth() {
        let base = json!({
            "finding_counts": { "src/a.ts": { "cyclomatic": { "count": 1 } } },
        });
        let head = json!({
            "finding_counts": {
                "src/a.ts": { "cyclomatic": { "count": 2 } },
                "src/b.ts": { "cognitive": { "count": 1 } },
            },
            "target_keys": ["src/b.ts:split"],
        });
        let growth = baseline_growth(BaselineKind::Health, &base, &head);
        assert_eq!(
            keys(&growth),
            vec![
                (
                    HEALTH_FILE_COUNTS.to_owned(),
                    vec![
                        "src/a.ts cyclomatic (count 1 -> 2)".to_owned(),
                        "src/b.ts cognitive (count 1)".to_owned(),
                    ]
                ),
                ("target_keys".to_owned(), vec!["src/b.ts:split".to_owned()]),
            ]
        );
        assert_eq!(growth.added_entries(), 3);
    }

    #[test]
    fn identity_counts_replace_file_counts_when_both_files_have_them() {
        let base = json!({
            "finding_counts": { "src/a.ts": { "cyclomatic": { "count": 1 } } },
            "identity_finding_counts": { "src/a.ts\u{0}old": { "cyclomatic": { "count": 1 } } },
        });
        let head = json!({
            "finding_counts": { "src/a.ts": { "cyclomatic": { "count": 1 } } },
            "identity_finding_counts": { "src/a.ts\u{0}new": { "cyclomatic": { "count": 1 } } },
        });
        let growth = baseline_growth(BaselineKind::Health, &base, &head);
        assert_eq!(
            keys(&growth),
            vec![(
                HEALTH_IDENTITY_COUNTS.to_owned(),
                vec!["src/a.ts:new cyclomatic (count 1)".to_owned()]
            )]
        );
    }
}
