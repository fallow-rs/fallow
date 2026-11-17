//! New-only attribution of unused code and clone groups that a change moved
//! to another file.
//!
//! Each attribution key holds the path of its finding, and git reports a
//! rename only for a whole file. A refactor that splits a file gives the
//! moved code a new path, so the head key has no base key and the finding
//! reads as introduced. The functions here find the base key that a moved
//! finding left behind. Complexity findings have their own step, see
//! [`crate::audit_complexity::classify_complexity_attribution_with_moves`].
//!
//! A base key is a candidate only when both conditions are true:
//!
//! - no head finding has that key, so the finding is gone from its base path;
//! - its path is a file that the change touched or deleted. The head report
//!   covers the changed files, so a finding in an unchanged file still
//!   exists, and a finding of the same name in another file is new.
//!
//! Each base key matches at most one head finding, so a copy of moved code is
//! introduced. Dead-code findings have two more conditions, see
//! [`moved_dead_code_keys`].

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::Severity;

use crate::audit_complexity::ChangedPaths;
use crate::audit_keys::DeadCodeAuditLedger;

/// Stands for the path in a key, so two keys that differ only in the path
/// compare as equal. A key part never holds this character.
const PATH_SLOT: &str = "\0";

/// The dead-code rules whose key names a symbol that a file declares:
/// `<rule>:<path>:...:<symbol>`. Only a finding of these rules can move with
/// its code. The key of another rule names something that is not code in the
/// file (a dependency of a manifest, a suppression comment, a boundary
/// target), so equal keys under two paths are two findings.
const MOVABLE_RULES: &[&str] = &[
    "unused-export",
    "unused-type",
    "unused-enum-member",
    "unused-class-member",
    "unused-protected-class-member",
    "unused-namespace-member",
    "unused-store-member",
    "unrendered-component",
    "unused-component-prop",
    "unused-component-emit",
    "unused-component-input",
    "unused-component-output",
    "unused-svelte-event",
    "unused-effect-layer",
    "unused-tagged-error",
    "unused-server-action",
    "unused-load-data-key",
];

/// The parts of a movable dead-code key.
struct MovableKey<'a> {
    /// The key with [`PATH_SLOT`] in place of the path.
    pathless: String,
    /// The path part.
    path: &'a str,
    /// The symbol part, as the key holds it.
    symbol: &'a str,
}

/// Split a dead-code key (`<rule>:<part>:<part>...`) of a rule in
/// [`MOVABLE_RULES`] into its parts.
///
/// `None` for another rule, and when the key does not have exactly one part
/// that is a changed path, with the symbol after it.
fn movable_key<'a>(key: &'a str, changed: &FxHashSet<String>) -> Option<MovableKey<'a>> {
    let mut parts = key.split(':');
    let rule = parts.next()?;
    if !MOVABLE_RULES.contains(&rule) {
        return None;
    }
    let parts: Vec<&str> = parts.collect();
    let mut paths = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| changed.contains(**part));
    let (index, path) = paths.next()?;
    if paths.next().is_some() {
        return None;
    }
    // An occurrence part (`~1`) follows the symbol of a repeated key.
    let symbol = parts[index + 1..]
        .iter()
        .rev()
        .find(|part| !part.starts_with('~'))?;
    let mut pathless = String::with_capacity(key.len());
    pathless.push_str(rule);
    for (position, part) in parts.iter().enumerate() {
        pathless.push(':');
        pathless.push_str(if position == index { PATH_SLOT } else { part });
    }
    Some(MovableKey {
        pathless,
        path,
        symbol,
    })
}

/// A key part as the finding holds it: `%3A` is `:` and `%25` is `%`.
fn unescape_key_part(part: &str) -> String {
    part.replace("%3A", ":").replace("%25", "%")
}

/// A lower rank is inherited first, so the finding that gates stays
/// introduced when two findings compete for one base key.
const fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Off => 0,
        Severity::Warn => 1,
        Severity::Error => 2,
    }
}

/// The inputs of [`moved_dead_code_keys`] next to the ledger.
pub struct MovedDeadCode<'a> {
    /// The base keys that the ledger compared with.
    pub base: &'a FxHashSet<String>,
    /// The files of the change.
    pub changed: &'a ChangedPaths,
    /// Head keys that the ledger does not hold but the head still reports,
    /// for example the syntactic keys of a degraded type-aware comparison. A
    /// base key in this set is not left behind.
    pub also_reported: Option<&'a FxHashSet<String>>,
    /// Whether the change wrote a line with `symbol` in the head file `path`.
    /// A finding without such a line is not moved code: the change made
    /// existing code unused.
    pub added_line_names: &'a dyn Fn(&str, &str) -> bool,
}

/// The keys of the introduced dead-code findings that the change moved to
/// another file. A finding is moved when all conditions are true:
///
/// - its rule is in [`MOVABLE_RULES`];
/// - the base holds the same rule and the same symbol under another changed
///   path, and no head finding has that base key;
/// - the change wrote a line with the symbol in the head file.
///
/// The match does not compare the code. A change that removes an unused
/// export and writes a new unused export of the same name in another file
/// gets one inherited finding.
#[must_use]
pub fn moved_dead_code_keys(
    ledger: &DeadCodeAuditLedger,
    input: &MovedDeadCode<'_>,
) -> FxHashSet<String> {
    let changed = &input.changed.changed;
    let mut moved = FxHashSet::default();
    let mut introduced: Vec<(u8, &str, MovableKey<'_>)> = ledger
        .records()
        .iter()
        .filter(|record| record.introduced)
        .filter_map(|record| {
            Some((
                severity_rank(record.effective_severity),
                record.stable_key.as_str(),
                movable_key(&record.stable_key, changed)?,
            ))
        })
        .collect();
    if introduced.is_empty() {
        return moved;
    }
    introduced.sort_by_key(|(rank, _, _)| *rank);
    let wanted: FxHashSet<&str> = introduced
        .iter()
        .map(|(_, _, key)| key.pathless.as_str())
        .collect();
    let mut left_behind: FxHashMap<String, Vec<&str>> = FxHashMap::default();
    for key in input.base {
        if ledger.keys().contains(key)
            || input
                .also_reported
                .is_some_and(|reported| reported.contains(key))
        {
            continue;
        }
        if let Some(base_key) = movable_key(key, changed)
            && wanted.contains(base_key.pathless.as_str())
        {
            left_behind
                .entry(base_key.pathless)
                .or_default()
                .push(base_key.path);
        }
    }
    for paths in left_behind.values_mut() {
        paths.sort_unstable();
    }
    for (_, key, head) in introduced {
        let Some(paths) = left_behind.get_mut(&head.pathless) else {
            continue;
        };
        let Some(position) = paths.iter().position(|base_path| *base_path != head.path) else {
            continue;
        };
        if (input.added_line_names)(head.path, &unescape_key_part(head.symbol)) {
            paths.remove(position);
            moved.insert(key.to_string());
        }
    }
    moved
}

/// Split a clone-group key
/// (`dupe:<files joined by |>:<tokens>:<lines>:<fragment hash>`) into its
/// file list and the part that does not depend on the files.
fn dupe_key_parts(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix("dupe:")?;
    let mut cut = rest.len();
    for _ in 0..3 {
        cut = rest[..cut].rfind(':')?;
    }
    Some((&rest[..cut], &rest[cut + 1..]))
}

/// The keys of the introduced clone groups that the change moved to other
/// files: the base holds a group with the same token count, line count and
/// fragment hash under other files, each of those files is a changed path,
/// and no head group has that base key.
///
/// The fragment hash covers the text of each instance, so only a group with
/// unchanged text matches. `head` holds the key of each head group, and
/// `introduced` the keys of the introduced ones.
#[must_use]
pub fn moved_dupe_group_keys<'a>(
    introduced: impl IntoIterator<Item = &'a str>,
    head: &FxHashSet<&str>,
    base: &FxHashSet<String>,
    changed: &ChangedPaths,
) -> FxHashSet<String> {
    let mut introduced: Vec<(&str, &str)> = introduced
        .into_iter()
        .filter_map(|key| Some((key, dupe_key_parts(key)?.1)))
        .collect();
    let mut moved = FxHashSet::default();
    if introduced.is_empty() {
        return moved;
    }
    introduced.sort_unstable();
    let mut left_behind: FxHashMap<&str, usize> = FxHashMap::default();
    for key in base {
        if head.contains(key.as_str()) {
            continue;
        }
        if let Some((files, content)) = dupe_key_parts(key)
            && files.split('|').all(|file| changed.changed.contains(file))
        {
            *left_behind.entry(content).or_default() += 1;
        }
    }
    for (key, content) in introduced {
        if let Some(count) = left_behind.get_mut(content)
            && *count > 0
        {
            *count -= 1;
            moved.insert(key.to_string());
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(paths: &[&str]) -> ChangedPaths {
        ChangedPaths {
            changed: paths.iter().map(|path| (*path).to_string()).collect(),
            deleted: FxHashSet::default(),
        }
    }

    fn keys(keys: &[&str]) -> FxHashSet<String> {
        keys.iter().map(|key| (*key).to_string()).collect()
    }

    #[test]
    fn movable_key_needs_a_symbol_rule_and_one_changed_path() {
        let paths = changed(&["src/a.ts", "src/b.ts", "package.json"]).changed;
        let key = movable_key("unused-export:src/a.ts:helper", &paths).expect("a movable key");
        assert_eq!(key.pathless, "unused-export:\0:helper");
        assert_eq!((key.path, key.symbol), ("src/a.ts", "helper"));

        let member =
            movable_key("unused-class-member:src/a.ts:Box:size:~1", &paths).expect("a movable key");
        assert_eq!(member.pathless, "unused-class-member:\0:Box:size:~1");
        assert_eq!(member.symbol, "size");

        // The path is the whole identity.
        assert!(movable_key("unused-file:src/a.ts", &paths).is_none());
        // No changed path.
        assert!(movable_key("unused-export:src/c.ts:helper", &paths).is_none());
        // The key names no symbol of the file.
        assert!(movable_key("unused-dependency:package.json:lodash", &paths).is_none());
        assert!(movable_key("boundary-violation:src/a.ts:src/c.ts", &paths).is_none());
        assert!(movable_key("stale-suppression:src/a.ts:comment:*", &paths).is_none());
    }

    #[test]
    fn key_part_unescapes_the_separator() {
        assert_eq!(unescape_key_part("a%3Ab%2525"), "a:b%25");
    }

    #[test]
    fn dupe_key_parts_split_from_the_right() {
        assert_eq!(
            dupe_key_parts("dupe:src/a.ts|src/b.ts:120:14:9f3c"),
            Some(("src/a.ts|src/b.ts", "120:14:9f3c"))
        );
        assert_eq!(dupe_key_parts("dupe:120:14"), None);
        assert_eq!(dupe_key_parts("complexity:src/a.ts:f"), None);
    }

    fn moved_dupes(
        introduced: &[&str],
        head: &[&str],
        base: &[&str],
        paths: &[&str],
    ) -> Vec<String> {
        let mut moved: Vec<String> = moved_dupe_group_keys(
            introduced.iter().copied(),
            &head.iter().copied().collect(),
            &keys(base),
            &changed(paths),
        )
        .into_iter()
        .collect();
        moved.sort();
        moved
    }

    #[test]
    fn moved_clone_group_matches_the_base_group_with_the_same_content() {
        let head = "dupe:src/one.ts|src/two.ts:120:14:9f3c";
        let base = "dupe:src/old.ts:120:14:9f3c";
        let all = ["src/old.ts", "src/one.ts", "src/two.ts"];
        assert_eq!(moved_dupes(&[head], &[head], &[base], &all), [head]);
        // Other content.
        assert!(moved_dupes(&[head], &[head], &["dupe:src/old.ts:120:14:aaaa"], &all).is_empty());
        // The base group is in a file that the change did not touch.
        assert!(moved_dupes(&[head], &[head], &[base], &["src/one.ts", "src/two.ts"]).is_empty());
        // A head group still has the base key.
        assert!(moved_dupes(&[head], &[head, base], &[base], &all).is_empty());
    }

    #[test]
    fn one_base_clone_group_covers_one_head_group() {
        let first = "dupe:src/one.ts|src/two.ts:120:14:9f3c";
        let second = "dupe:src/three.ts|src/four.ts:120:14:9f3c";
        let base = "dupe:src/old.ts:120:14:9f3c";
        let all = [
            "src/old.ts",
            "src/one.ts",
            "src/two.ts",
            "src/three.ts",
            "src/four.ts",
        ];
        assert_eq!(
            moved_dupes(&[first, second], &[first, second], &[base], &all).len(),
            1
        );
    }
}
