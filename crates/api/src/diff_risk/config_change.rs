//! Changes of the Fallow config that make the analysis less strict.
//!
//! The detector resolves the config chain of the base and the head (see
//! [`super::config_chain`]) and compares the resolved values:
//!
//! - a lower rule severity gives the `rule-severity` kind;
//! - a new or broader `ignorePatterns` or `ignoreFindings` entry, or a removed
//!   `!` exception, gives the `ignore-pattern` kind.
//!
//! A finding goes on the changed file that sets the new value, on the line
//! that the change added. When no changed file sets it, the finding goes on
//! the root config file.

use std::path::{Component, Path, PathBuf};

use fallow_config::Severity;
use fallow_output::WeakenedTestsKind;
use globset::GlobMatcher;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;

use super::config_chain::{ConfigChain, Lookup};
use super::rule_severity::{key_names, names_key};
use super::text::added_lines;

/// The config keys whose entries remove files or findings from the analysis.
const IGNORE_KEYS: &[&str] = &["ignorePatterns", "ignoreFindings"];

/// A path segment that no real glob names as a literal. A head glob whose
/// sample path, with each wildcard replaced by this segment, matches a base
/// glob is not broader than that base glob.
const SAMPLE_SEGMENT: &str = "__fallow_sample__";

/// One change of the config that makes the analysis less strict.
pub struct Weakening {
    pub kind: WeakenedTestsKind,
    pub evidence: String,
    /// The head file that sets the new value, `None` for a default.
    pub head_file: Option<PathBuf>,
    /// The base file that set the old value, `None` for a default.
    pub base_file: Option<PathBuf>,
    pub line_match: LineMatch,
}

/// How to find the added line of a weakening.
pub enum LineMatch {
    /// A line that names the rule, preferably with the new severity.
    Rule { key: String, head: Severity },
    /// A line that holds this quoted text.
    Quoted(String),
    /// No added line shows a removal.
    None,
}

/// One weakening with its location.
pub struct ConfigHit {
    pub kind: WeakenedTestsKind,
    /// Path relative to the analysis root.
    pub path: String,
    pub line: Option<u32>,
    pub evidence: String,
}

/// The base and head chains of one change.
pub struct ConfigSides {
    pub base: ConfigChain,
    pub head: ConfigChain,
}

impl ConfigSides {
    /// Whether the change touches a file of either chain.
    #[must_use]
    pub fn touched_by(&self, changed: &FxHashSet<PathBuf>) -> bool {
        self.base
            .files()
            .chain(self.head.files())
            .any(|file| changed.contains(file))
    }

    /// Every weakening of the change, with its location. `read_base` reads
    /// the base text of a head file that the base chain did not read.
    pub fn hits(
        &self,
        root: &Path,
        read_base: &mut dyn FnMut(&Path) -> Option<String>,
    ) -> Vec<ConfigHit> {
        let mut weakenings = super::rule_severity::rule_weakenings(&self.base, &self.head);
        weakenings.extend(ignore_weakenings(&self.base, &self.head));
        let mut used: FxHashMap<PathBuf, FxHashSet<u32>> = FxHashMap::default();
        weakenings
            .into_iter()
            .filter_map(|weakening| self.locate(root, weakening, read_base, &mut used))
            .collect()
    }

    fn locate(
        &self,
        root: &Path,
        weakening: Weakening,
        read_base: &mut dyn FnMut(&Path) -> Option<String>,
        used: &mut FxHashMap<PathBuf, FxHashSet<u32>>,
    ) -> Option<ConfigHit> {
        let fallback = self.head.root().or_else(|| self.base.root());
        let target = [
            weakening.head_file.as_deref(),
            weakening.base_file.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|file| self.head.text(file).is_some() && self.changed(file, read_base))
        .or(fallback);
        // A side without a config file has no chain file, so the change
        // touches a chain only when one side has a root file.
        let target = target?;
        let head_text = self.head.text(target).unwrap_or("");
        let base_text = self.base_text(target, read_base);
        let added = added_lines(&base_text, head_text);
        let used_lines = used.entry(target.to_path_buf()).or_default();
        let mut line = find_line(&added, &weakening.line_match, used_lines);
        if line.is_none() && Some(target) == self.head.root() {
            line = added
                .iter()
                .find(|(line, text)| !used_lines.contains(line) && names_key(text, "extends"))
                .map(|(line, _)| *line);
        }
        if let Some(line) = line {
            used_lines.insert(line);
        }
        Some(ConfigHit {
            kind: weakening.kind,
            path: rel_path(target, root),
            line,
            evidence: weakening.evidence,
        })
    }

    /// The base text that a head file compares with. The head root config
    /// compares with the base root config, so a rename from `.fallowrc.json`
    /// to `fallow.toml` compares the two texts.
    fn base_text(&self, file: &Path, read_base: &mut dyn FnMut(&Path) -> Option<String>) -> String {
        if let Some(text) = self.base.text(file) {
            return text.to_owned();
        }
        if Some(file) == self.head.root()
            && let Some(text) = self.base.root().and_then(|root| self.base.text(root))
        {
            return text.to_owned();
        }
        read_base(file).unwrap_or_default()
    }

    fn changed(&self, file: &Path, read_base: &mut dyn FnMut(&Path) -> Option<String>) -> bool {
        self.head.text(file) != Some(self.base_text(file, read_base).as_str())
    }
}

/// The path of `file` relative to `root`. The loader also reads a config
/// file in a parent directory of the root, which gets a `../` path.
fn rel_path(file: &Path, root: &Path) -> String {
    if file.starts_with(root) {
        return fallow_engine::module_graph::relative_key_path(file, root);
    }
    let file_parts: Vec<Component<'_>> = file.components().collect();
    let root_parts: Vec<Component<'_>> = root.components().collect();
    let common = file_parts
        .iter()
        .zip(&root_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_owned(); root_parts.len() - common];
    parts.extend(
        file_parts[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// The first free added line that matches.
fn find_line(added: &[(u32, &str)], line_match: &LineMatch, used: &FxHashSet<u32>) -> Option<u32> {
    let free = |line: &u32| !used.contains(line);
    match line_match {
        LineMatch::Rule { key, head } => {
            let names = key_names(key);
            let names_rule = |text: &str| names.iter().any(|name| names_key(text, name));
            let head_value = format!("\"{head}\"");
            added
                .iter()
                .find(|(line, text)| free(line) && names_rule(text) && text.contains(&head_value))
                .or_else(|| {
                    added
                        .iter()
                        .find(|(line, text)| free(line) && names_rule(text))
                })
                .map(|(line, _)| *line)
        }
        LineMatch::Quoted(entry) => {
            // The comparison strips a leading `./`, so the line can hold
            // either form.
            let (negation, body) = entry
                .strip_prefix('!')
                .map_or(("", entry.as_str()), |body| ("!", body));
            let forms = [
                format!("\"{entry}\""),
                format!("'{entry}'"),
                format!("\"{negation}./{body}\""),
                format!("'{negation}./{body}'"),
            ];
            added
                .iter()
                .find(|(line, text)| free(line) && forms.iter().any(|form| text.contains(form)))
                .map(|(line, _)| *line)
        }
        LineMatch::None => None,
    }
}

/// The entries of one ignore key, `None` when an opaque source can set it.
fn ignore_entries(chain: &ConfigChain, key: &str) -> Option<(Vec<String>, Option<PathBuf>)> {
    match chain.lookup(|map| map.get(key)) {
        Lookup::Unknown => None,
        Lookup::Unset => Some((Vec::new(), None)),
        Lookup::Set { value, abs } => Some((
            value
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(Value::as_str)
                        .map(normalize)
                        .collect()
                })
                .unwrap_or_default(),
            Some(abs.to_path_buf()),
        )),
    }
}

fn normalize(entry: &str) -> String {
    match entry.strip_prefix('!') {
        Some(body) => format!("!{}", body.strip_prefix("./").unwrap_or(body)),
        None => entry.strip_prefix("./").unwrap_or(entry).to_owned(),
    }
}

/// New or broader ignore entries, and removed `!` exceptions.
fn ignore_weakenings(base: &ConfigChain, head: &ConfigChain) -> Vec<Weakening> {
    let mut changes = Vec::new();
    for key in IGNORE_KEYS {
        let (Some((base_entries, base_file)), Some((head_entries, head_file))) =
            (ignore_entries(base, key), ignore_entries(head, key))
        else {
            continue;
        };
        let base_list = IgnoreList::new(key, &base_entries);
        let head_list = IgnoreList::new(key, &head_entries);
        changes.extend(list_changes(key, &base_list, &head_list).into_iter().map(
            |(evidence, line_match)| Weakening {
                kind: WeakenedTestsKind::IgnorePattern,
                evidence,
                head_file: head_file.clone(),
                base_file: base_file.clone(),
                line_match,
            },
        ));
    }
    changes
}

/// The entries of one ignore list.
struct IgnoreList<'a> {
    /// The positive entries.
    hidden: Vec<&'a str>,
    /// The bodies of the `!` entries.
    kept: Vec<&'a str>,
    /// An `ignoreFindings` list of only `!` entries hides every path outside
    /// them.
    hides_rest: bool,
}

impl<'a> IgnoreList<'a> {
    fn new(key: &str, entries: &'a [String]) -> Self {
        let (hidden, kept) = split(entries);
        let hides_rest = key == "ignoreFindings" && hidden.is_empty() && !kept.is_empty();
        Self {
            hidden,
            kept,
            hides_rest,
        }
    }
}

/// The evidence and the line match of each change that hides more paths.
fn list_changes(
    key: &str,
    base: &IgnoreList<'_>,
    head: &IgnoreList<'_>,
) -> Vec<(String, LineMatch)> {
    if head.hides_rest && !base.hides_rest {
        let first = head.kept[0];
        return vec![(
            format!("{key}: + !{first} (hides every other path)"),
            LineMatch::Quoted(format!("!{first}")),
        )];
    }
    // The built-in defaults of `ignorePatterns` hide their paths on both
    // sides. A new entry that a default covers hides nothing new, and a
    // removed `!` exception of a default path hides that path again.
    let defaults: &[&str] = if key == "ignorePatterns" {
        fallow_config::DEFAULT_IGNORE_PATTERNS
    } else {
        &[]
    };
    let mut changes = Vec::new();
    if !base.hides_rest {
        let base_matchers = matchers(&base.hidden, defaults);
        changes.extend(
            head.hidden
                .iter()
                .filter(|entry| !base.hidden.contains(entry) && !is_covered(entry, &base_matchers))
                .map(|entry| {
                    (
                        format!("{key}: + {entry}"),
                        LineMatch::Quoted((*entry).to_owned()),
                    )
                }),
        );
    }
    // A removed `!` exception hides its paths only when a head entry or a
    // default still matches them.
    let head_matchers = matchers(&head.hidden, defaults);
    changes.extend(
        base.kept
            .iter()
            .filter(|entry| !head.kept.contains(entry))
            .filter(|entry| head.hides_rest || is_covered(entry, &head_matchers))
            .map(|entry| (format!("{key}: - !{entry}"), LineMatch::None)),
    );
    changes
}

fn matchers(globs: &[&str], defaults: &[&str]) -> Vec<GlobMatcher> {
    globs
        .iter()
        .chain(defaults)
        .filter_map(|glob| fallow_config::compile_path_glob(glob).ok())
        .map(|glob| glob.compile_matcher())
        .collect()
}

/// The positive entries and the bodies of the `!` entries.
fn split(entries: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut hidden = Vec::new();
    let mut kept = Vec::new();
    for entry in entries {
        match entry.strip_prefix('!') {
            Some(body) => kept.push(body),
            None => hidden.push(entry.as_str()),
        }
    }
    (hidden, kept)
}

/// Whether one glob of `globs` matches every path of `entry`. The check
/// builds sample paths from `entry`: a `**` becomes zero, one and two sample
/// segments, and a `*` becomes one sample segment. A glob that matches every
/// sample covers the entry. The check is a heuristic. An entry with other
/// glob syntax is never covered, so it reports.
fn is_covered(entry: &str, globs: &[GlobMatcher]) -> bool {
    if entry.contains(['?', '[', ']', '{', '}', '\\']) {
        return false;
    }
    let one = SAMPLE_SEGMENT.to_owned();
    let two = format!("{SAMPLE_SEGMENT}/{SAMPLE_SEGMENT}");
    let mut samples: Vec<String> = [one.as_str(), two.as_str()]
        .iter()
        .map(|segments| entry.replace("**", segments).replace('*', SAMPLE_SEGMENT))
        .collect();
    if entry.contains("**/") {
        samples.push(entry.replace("**/", "").replace('*', SAMPLE_SEGMENT));
    }
    globs
        .iter()
        .any(|glob| samples.iter().all(|sample| glob.is_match(sample)))
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// The hits of a change of one config file, with line 0 for no line.
    pub fn single_file_hits(base: &str, head: &str) -> Vec<(u32, String)> {
        let root = Path::new("/r");
        let file = root.join(".fallowrc.json");
        let (Some(base), Some(head)) = (
            ConfigChain::from_text(&file, base),
            ConfigChain::from_text(&file, head),
        ) else {
            return Vec::new();
        };
        let sides = ConfigSides { base, head };
        sides
            .hits(root, &mut |_| None)
            .into_iter()
            .map(|hit| (hit.line.unwrap_or(0), hit.evidence))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::test_support::single_file_hits;

    #[test]
    fn a_file_above_the_root_gets_a_parent_path() {
        assert_eq!(
            super::rel_path(
                Path::new("/repo/.fallowrc.json"),
                Path::new("/repo/packages/app")
            ),
            "../../.fallowrc.json"
        );
        assert_eq!(
            super::rel_path(Path::new("/repo/app/cfg/a.json"), Path::new("/repo/app")),
            "cfg/a.json"
        );
    }

    #[test]
    fn a_new_ignore_entry_reports_on_its_line() {
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["dist/**"]}"#,
                "{\n  \"ignorePatterns\": [\"dist/**\",\n    \"src/a.test.ts\"]\n}\n"
            ),
            vec![(3, "ignorePatterns: + src/a.test.ts".to_owned())]
        );
    }

    #[test]
    fn a_narrower_equal_or_removed_entry_does_not_report() {
        let base = r#"{"ignorePatterns": ["src/**", "lib/*.ts"], "ignoreFindings": ["a/**"]}"#;
        for head in [
            r#"{"ignorePatterns": ["src/legacy/**", "lib/*.ts"], "ignoreFindings": ["a/**"]}"#,
            r#"{"ignorePatterns": ["./src/**", "src/a/*.test.ts"], "ignoreFindings": ["a/b/**"]}"#,
            r#"{"ignorePatterns": [], "ignoreFindings": ["a/**", "!a/keep.ts"]}"#,
            "{}",
        ] {
            assert!(single_file_hits(base, head).is_empty(), "{head}");
        }
    }

    #[test]
    fn a_dot_slash_entry_reports_on_its_line() {
        assert_eq!(
            single_file_hits("{}", "{\"ignorePatterns\": [\n\"./src/a.test.ts\"\n]}"),
            vec![(2, "ignorePatterns: + src/a.test.ts".to_owned())]
        );
    }

    #[test]
    fn a_broader_entry_reports() {
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["src/*"]}"#,
                r#"{"ignorePatterns": ["src/**"]}"#
            ),
            vec![(1, "ignorePatterns: + src/**".to_owned())]
        );
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["src/a.ts"]}"#,
                r#"{"ignorePatterns": ["src/*.ts"]}"#
            ),
            vec![(1, "ignorePatterns: + src/*.ts".to_owned())]
        );
    }

    #[test]
    fn a_new_entry_that_a_default_covers_does_not_report() {
        assert!(
            single_file_hits(
                "{}",
                r#"{"ignorePatterns": ["dist/**", "src/**/*.min.js"]}"#
            )
            .is_empty()
        );
        assert_eq!(
            single_file_hits("{}", r#"{"ignoreFindings": ["dist/**"]}"#),
            vec![(1, "ignoreFindings: + dist/**".to_owned())]
        );
    }

    #[test]
    fn a_glob_that_misses_a_shallow_path_does_not_cover() {
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["src/*/*"]}"#,
                r#"{"ignorePatterns": ["src/**"]}"#
            ),
            vec![(1, "ignorePatterns: + src/**".to_owned())]
        );
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["src/*/**"]}"#,
                r#"{"ignorePatterns": ["src/**/*.ts"]}"#
            ),
            vec![(1, "ignorePatterns: + src/**/*.ts".to_owned())]
        );
    }

    #[test]
    fn an_exception_removed_with_its_entry_does_not_report() {
        for head in [
            r#"{"ignorePatterns": []}"#,
            r#"{"ignorePatterns": ["lib/**"]}"#,
            r#"{"ignoreFindings": ["lib/**"]}"#,
        ] {
            let base = r#"{"ignorePatterns": ["src/**", "!src/keep/**"], "ignoreFindings": ["src/**", "!src/keep/**"]}"#;
            let hits = single_file_hits(base, head);
            assert!(
                hits.iter().all(|(_, evidence)| !evidence.contains(" - !")),
                "{head}: {hits:?}"
            );
        }
    }

    #[test]
    fn a_removed_exception_of_a_default_or_an_only_exceptions_list_reports() {
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["!src/coverage/**"]}"#,
                r#"{"ignorePatterns": []}"#
            ),
            vec![(0, "ignorePatterns: - !src/coverage/**".to_owned())]
        );
        assert_eq!(
            single_file_hits(
                r#"{"ignoreFindings": ["!src/a/**", "!src/b/**"]}"#,
                r#"{"ignoreFindings": ["!src/a/**"]}"#
            ),
            vec![(0, "ignoreFindings: - !src/b/**".to_owned())]
        );
    }

    #[test]
    fn a_removed_exception_reports_without_a_line() {
        assert_eq!(
            single_file_hits(
                r#"{"ignorePatterns": ["src/**", "!src/keep/**"]}"#,
                r#"{"ignorePatterns": ["src/**"]}"#
            ),
            vec![(0, "ignorePatterns: - !src/keep/**".to_owned())]
        );
    }

    #[test]
    fn an_ignore_findings_list_of_only_exceptions_hides_the_rest() {
        assert_eq!(
            single_file_hits("{}", "{\"ignoreFindings\": [\n\"!src/core/**\"\n]}"),
            vec![(
                2,
                "ignoreFindings: + !src/core/** (hides every other path)".to_owned()
            )]
        );
        assert!(
            single_file_hits(
                r#"{"ignoreFindings": ["!src/core/**"]}"#,
                r#"{"ignoreFindings": ["!src/core/**", "!src/api/**"]}"#
            )
            .is_empty()
        );
    }

    #[test]
    fn an_opaque_extends_target_hides_the_ignore_list() {
        assert!(
            single_file_hits(
                r#"{"extends": "npm:@acme/c"}"#,
                r#"{"extends": "npm:@acme/c", "rules": {}}"#
            )
            .is_empty()
        );
        assert_eq!(
            single_file_hits(
                r#"{"extends": "npm:@acme/c", "ignorePatterns": []}"#,
                r#"{"extends": "npm:@acme/c", "ignorePatterns": ["gen/**"]}"#
            ),
            vec![(1, "ignorePatterns: + gen/**".to_owned())]
        );
    }
}
