//! Deep-dive helpers for the `fallow dupes --trace` inspector: a stable
//! content fingerprint that addresses a clone group across runs, a group-level
//! refactoring suggestion, and a best-effort "dominant identifier" name for the
//! extracted function.
//!
//! These are pure functions over [`CloneInstance`] / [`CloneGroup`] so every
//! surface (human listing, `--trace dup:<fp>` lookup, the typed JSON wrappers,
//! and `trace_clone`) computes the same values without storing a field on the
//! core [`CloneGroup`] struct.

use std::path::Path;

use fallow_config::DetectionMode;
use rustc_hash::{FxHashMap, FxHashSet};
use xxhash_rust::xxh3::{Xxh3, xxh3_64};

use super::tokenize::{
    FragmentTokenizationKind, FragmentTokenizationStrategy, fragment_tokenization_kind,
};
use super::types::{CloneGroup, CloneInstance, RefactoringKind, RefactoringSuggestion};

/// Prefix marking a clone-group fingerprint addressable via `--trace`.
pub const FINGERPRINT_PREFIX: &str = "dup:";

/// Canonical identity for a clone group when assigning report-scoped handles.
///
/// Compact digests of canonically sorted fragments and locations make report
/// entries addressable and provide a report-order-independent collision-suffix
/// order without retaining a second copy of every source fragment and path.
/// The separately hashed, sorted, deduplicated normalized instance sequences
/// provide the public content identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CloneFingerprintKey {
    fragments_digest: u128,
    locations_digest: u128,
    token_count: usize,
    line_count: usize,
    instance_count: usize,
}

const _: () = assert!(std::mem::size_of::<CloneFingerprintKey>() <= 64);

impl CloneFingerprintKey {
    /// Build a fingerprint key from clone-group parts.
    #[must_use]
    fn from_parts(instances: &[CloneInstance], token_count: usize, line_count: usize) -> Self {
        Self {
            fragments_digest: hash_distinct_fragments(instances),
            locations_digest: hash_sorted_locations(instances),
            token_count,
            line_count,
            instance_count: instances.len(),
        }
    }

    fn from_group(group: &CloneGroup) -> Self {
        Self::from_parts(&group.instances, group.token_count, group.line_count)
    }
}

/// Report-scoped clone fingerprint assignment.
///
/// Most reports retain the short `dup:<8hex>` handle. If two report entries
/// collide on those low 32 bits, only the colliding entries widen to
/// `dup:<16hex>`. If a full 64-bit collision ever occurs inside one report,
/// every entry in that collision bucket receives a deterministic numeric suffix.
#[derive(Debug, Clone)]
pub struct CloneFingerprintSet {
    by_key: FxHashMap<CloneFingerprintKey, String>,
    key_by_fingerprint: FxHashMap<String, CloneFingerprintKey>,
}

impl CloneFingerprintSet {
    /// Assign collision-free fingerprints for the report's clone groups.
    #[must_use]
    pub fn from_groups(groups: &[CloneGroup]) -> Self {
        let entries: Vec<_> = groups
            .iter()
            .map(|group| {
                let key = CloneFingerprintKey::from_group(group);
                let hash = hash_instances(&group.instances);
                (key, hash)
            })
            .collect();
        Self::from_hashed_entries(&entries)
    }

    /// Return the assigned fingerprint for a clone group.
    #[must_use]
    pub fn fingerprint_for_group(&self, group: &CloneGroup) -> String {
        self.by_key
            .get(&CloneFingerprintKey::from_group(group))
            .cloned()
            .unwrap_or_else(|| clone_fingerprint(&group.instances))
    }

    /// Return the config key used by `duplicates.ignoredClones` for a group.
    #[must_use]
    pub fn ignored_clone_key_for_group(&self, group: &CloneGroup) -> String {
        format!(
            "{}:{}",
            self.fingerprint_for_group(group),
            group.instances.len()
        )
    }

    /// Return the assigned fingerprint for clone-group parts.
    #[must_use]
    pub fn fingerprint_for_parts(
        &self,
        instances: &[CloneInstance],
        token_count: usize,
        line_count: usize,
    ) -> String {
        let key = CloneFingerprintKey::from_parts(instances, token_count, line_count);
        self.by_key
            .get(&key)
            .cloned()
            .unwrap_or_else(|| clone_fingerprint(instances))
    }

    /// Find the group addressed by an assigned fingerprint.
    ///
    /// Ambiguous short handles created by low-32 collisions are intentionally
    /// absent from the lookup table, so callers get `None` instead of the first
    /// matching group.
    #[must_use]
    pub fn find_group<'a>(
        &self,
        groups: &'a [CloneGroup],
        fingerprint: &str,
    ) -> Option<&'a CloneGroup> {
        let key = self.key_by_fingerprint.get(fingerprint)?;
        groups
            .iter()
            .find(|group| CloneFingerprintKey::from_group(group) == *key)
    }

    fn from_hashed_entries(entries: &[(CloneFingerprintKey, u64)]) -> Self {
        let mut short_counts: FxHashMap<u32, usize> = FxHashMap::default();
        let mut full_counts: FxHashMap<u64, usize> = FxHashMap::default();
        for (_, hash) in entries {
            *short_counts.entry(*hash as u32).or_insert(0) += 1;
            *full_counts.entry(*hash).or_insert(0) += 1;
        }

        let mut sorted_entries = entries.to_vec();
        sorted_entries.sort_unstable_by(|(left_key, left_hash), (right_key, right_hash)| {
            left_key
                .cmp(right_key)
                .then_with(|| left_hash.cmp(right_hash))
        });

        let mut full_ordinals: FxHashMap<u64, usize> = FxHashMap::default();
        let mut ambiguous_short_handles: FxHashSet<String> = FxHashSet::default();
        let mut by_key = FxHashMap::default();
        let mut key_by_fingerprint = FxHashMap::default();

        for (key, hash) in &sorted_entries {
            let short = *hash as u32;
            let short_handle = format!("{FINGERPRINT_PREFIX}{short:08x}");
            let fingerprint = if short_counts.get(&short).copied().unwrap_or(0) == 1 {
                short_handle
            } else {
                ambiguous_short_handles.insert(short_handle);
                let full_handle = format!("{FINGERPRINT_PREFIX}{hash:016x}");
                if full_counts.get(hash).copied().unwrap_or(0) == 1 {
                    full_handle
                } else {
                    let ordinal = full_ordinals.entry(*hash).or_insert(0);
                    *ordinal += 1;
                    format!("{full_handle}-{ordinal}")
                }
            };

            key_by_fingerprint.insert(fingerprint.clone(), key.clone());
            by_key.insert(key.clone(), fingerprint);
        }

        for handle in ambiguous_short_handles {
            key_by_fingerprint.remove(&handle);
        }

        Self {
            by_key,
            key_by_fingerprint,
        }
    }
}

/// Compute a mode-independent normalized short content fingerprint for a clone
/// group from all distinct instance token sequences.
///
/// Whitespace, comments, and line endings are absent from the normalized token
/// hashes. Sorting and deduplicating sequences makes the fingerprint independent
/// of instance order while ensuring a token edit in any distinct instance
/// changes the group identity. Instance count is deliberately excluded and is
/// appended separately by [`CloneFingerprintSet::ignored_clone_key_for_group`].
///
/// Use [`CloneFingerprintSet`] for user-facing report output, since it widens
/// only the rare colliding handles while preserving this short form for the
/// common case.
///
/// Hashes the empty string for an empty group (never produced by the detector,
/// which guarantees `>= 2` instances), so the result is still a well-formed
/// `dup:<8hex>` handle.
#[must_use]
pub fn clone_fingerprint(instances: &[CloneInstance]) -> String {
    fingerprint_for_hash(hash_instances(instances))
}

/// Compute the fingerprint directly from a representative source fragment.
///
/// Use when the instances are wrapped (e.g. `--group-by` attributed instances)
/// but the representative fragment is the same as the bare clone group's, so the
/// fingerprint matches the top-level `clone_groups[].fingerprint` for the clone.
#[must_use]
pub fn fingerprint_for_fragment(fragment: &str) -> String {
    let sequence = normalized_fragment_sequence(Path::new("fragment.ts"), fragment);
    fingerprint_for_hash(hash_normalized_sequences(&[sequence]))
}

fn hash_distinct_fragments(instances: &[CloneInstance]) -> u128 {
    let mut fragments = instances
        .iter()
        .map(|instance| instance.fragment.as_str())
        .collect::<Vec<_>>();
    fragments.sort_unstable();
    fragments.dedup();

    let mut hasher = Xxh3::new();
    for fragment in fragments {
        update_hash_bytes(&mut hasher, fragment.as_bytes());
    }
    hasher.digest128()
}

fn hash_sorted_locations(instances: &[CloneInstance]) -> u128 {
    let mut locations = instances
        .iter()
        .map(|instance| {
            (
                instance.file.as_path(),
                instance.start_line,
                instance.end_line,
            )
        })
        .collect::<Vec<_>>();
    locations.sort_unstable();

    let mut hasher = Xxh3::new();
    for (path, start_line, end_line) in locations {
        update_hash_bytes(&mut hasher, path.as_os_str().as_encoded_bytes());
        hasher.update(&start_line.to_le_bytes());
        hasher.update(&end_line.to_le_bytes());
    }
    hasher.digest128()
}

fn update_hash_bytes(hasher: &mut Xxh3, bytes: &[u8]) {
    hasher.update(&bytes.len().to_le_bytes());
    hasher.update(bytes);
}

fn hash_instances(instances: &[CloneInstance]) -> u64 {
    let mut sequences = distinct_fragment_inputs(instances)
        .into_iter()
        .map(|(kind, fragment)| normalized_fragment_sequence(kind.path(), fragment))
        .collect::<Vec<_>>();
    sequences.sort_unstable();
    sequences.dedup();
    hash_normalized_sequences(&sequences)
}

fn distinct_fragment_inputs(instances: &[CloneInstance]) -> Vec<(FragmentTokenizationKind, &str)> {
    let mut fragments = instances
        .iter()
        .map(|instance| {
            (
                fragment_tokenization_kind(
                    &instance.file,
                    FragmentTokenizationStrategy::Fingerprint,
                ),
                instance.fragment.as_str(),
            )
        })
        .collect::<Vec<_>>();
    fragments.sort_unstable();
    fragments.dedup();
    fragments
}

fn normalized_fragment_sequence(path: &Path, fragment: &str) -> Vec<u64> {
    let tokens = super::tokenize::tokenize_file(path, fragment, false);
    super::normalize::normalize_and_hash(&tokens.tokens, DetectionMode::Strict)
        .into_iter()
        .map(|token| token.hash)
        .collect()
}

fn hash_normalized_sequences(sequences: &[Vec<u64>]) -> u64 {
    let byte_len = sequences
        .iter()
        .map(|sequence| sequence.len().saturating_add(1))
        .sum::<usize>()
        .saturating_mul(std::mem::size_of::<u64>());
    let mut bytes = Vec::with_capacity(byte_len);
    for sequence in sequences {
        bytes.extend_from_slice(&(sequence.len() as u64).to_le_bytes());
        for hash in sequence {
            bytes.extend_from_slice(&hash.to_le_bytes());
        }
    }
    xxh3_64(&bytes)
}

fn fingerprint_for_hash(hash: u64) -> String {
    format!("{FINGERPRINT_PREFIX}{:08x}", hash as u32)
}

/// Build a per-group `ExtractFunction` refactoring suggestion.
///
/// Mirrors the per-group branch of the families suggestion generator:
/// the savings is `(instances - 1)` copies of the group's line count, since one
/// copy survives as the extracted function and the rest collapse to call sites.
#[must_use]
pub fn group_refactoring_suggestion(group: &CloneGroup) -> RefactoringSuggestion {
    let estimated_savings = group.line_count * group.instances.len().saturating_sub(1);
    RefactoringSuggestion {
        kind: RefactoringKind::ExtractFunction,
        description: format!(
            "Extract the shared {}-line block into one function and call it from {} sites",
            group.line_count,
            group.instances.len(),
        ),
        estimated_savings,
    }
}

/// Best-effort name for the extracted function, derived from the most frequent
/// non-generic identifier in the representative fragment.
///
/// Returns `None` when the dominant identifier is generic (`data`, `result`,
/// loop counters), appears only once, or ties with another, so absence is the
/// low-confidence signal for both human and agent consumers. This is a
/// lexical heuristic over the raw fragment, not an AST analysis; it is advisory
/// and consumers should verify before applying.
#[must_use]
pub fn dominant_identifier(group: &CloneGroup) -> Option<String> {
    let fragment = group.instances.first().map(|inst| inst.fragment.as_str())?;
    let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
    for word in identifier_words(fragment) {
        if is_generic_identifier(word) {
            continue;
        }
        *counts.entry(word).or_insert(0) += 1;
    }

    let mut candidates: Vec<_> = counts
        .into_iter()
        .map(|(word, count)| IdentifierCandidate {
            word,
            count,
            score: identifier_score(word, count),
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| a.word.cmp(b.word))
    });

    let best = candidates.first()?;
    if best.count < 2 {
        return None;
    }

    let runner_up = candidates.get(1);
    if runner_up.is_some_and(|next| best.score.saturating_sub(next.score) < 2) {
        return None;
    }

    if is_plain_single_token(best.word) {
        let next_count = runner_up.map_or(0, |candidate| candidate.count);
        if best.count < 3 || best.count < next_count + 2 {
            return None;
        }
    }

    Some(best.word.to_string())
}

#[derive(Debug)]
struct IdentifierCandidate<'a> {
    word: &'a str,
    count: usize,
    score: usize,
}

fn identifier_score(word: &str, count: usize) -> usize {
    let quality_bonus = if has_identifier_separator_or_case_transition(word) {
        5
    } else if word.chars().count() >= 8 {
        2
    } else {
        0
    };
    count * 5 + quality_bonus
}

fn is_plain_single_token(word: &str) -> bool {
    !has_identifier_separator_or_case_transition(word) && word.chars().count() < 8
}

fn has_identifier_separator_or_case_transition(word: &str) -> bool {
    if word.contains('_') || word.contains('$') {
        return true;
    }

    let mut previous = None;
    for ch in word.chars() {
        if previous.is_some_and(|prev: char| prev.is_ascii_lowercase() && ch.is_ascii_uppercase()) {
            return true;
        }
        previous = Some(ch);
    }
    false
}

/// Yield identifier-like words (`[A-Za-z_$][A-Za-z0-9_$]*`) from raw source.
fn identifier_words(source: &str) -> impl Iterator<Item = &str> {
    source
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .filter(|word| {
            !word.is_empty()
                && word
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        })
}

/// Identifiers too generic to make a useful extracted-function name, plus the
/// reserved words that show up as bare tokens in a fragment.
const GENERIC_IDENTIFIERS: &[&str] = &[
    "data",
    "result",
    "results",
    "item",
    "items",
    "value",
    "values",
    "val",
    "obj",
    "object",
    "arr",
    "array",
    "list",
    "map",
    "set",
    "key",
    "keys",
    "tmp",
    "temp",
    "acc",
    "cur",
    "curr",
    "prev",
    "next",
    "node",
    "el",
    "elem",
    "element",
    "args",
    "arg",
    "opts",
    "options",
    "params",
    "param",
    "props",
    "ctx",
    "context",
    "res",
    "req",
    "err",
    "error",
    "fn",
    "cb",
    "callback",
    "out",
    "input",
    "output",
    "name",
    "id",
    "index",
    "idx",
    "x",
    "y",
    "z",
    "i",
    "j",
    "k",
    "n",
    "m",
    "a",
    "b",
    "c",
    "e",
    "_",
    "const",
    "let",
    "var",
    "function",
    "return",
    "if",
    "else",
    "for",
    "while",
    "do",
    "switch",
    "case",
    "break",
    "continue",
    "new",
    "this",
    "true",
    "false",
    "null",
    "undefined",
    "void",
    "typeof",
    "instanceof",
    "in",
    "of",
    "class",
    "extends",
    "super",
    "import",
    "export",
    "from",
    "default",
    "async",
    "await",
    "yield",
    "type",
    "interface",
    "enum",
    "as",
    "is",
    "keyof",
    "readonly",
    "public",
    "private",
    "protected",
    "static",
    "get",
    "delete",
    "throw",
    "try",
    "catch",
    "finally",
    "string",
    "number",
    "boolean",
    "any",
    "unknown",
    "never",
    "bigint",
    "symbol",
    "Math",
    "JSON",
    "Object",
    "Array",
    "Promise",
    "BigInt",
    "Number",
    "String",
    "Boolean",
    "Symbol",
    "RegExp",
    "Date",
];

fn is_generic_identifier(word: &str) -> bool {
    word.chars().count() == 1 || GENERIC_IDENTIFIERS.contains(&word)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn instance(fragment: &str) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from("a.ts"),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 0,
            fragment: fragment.to_string(),
        }
    }

    fn group(fragments: &[&str], line_count: usize) -> CloneGroup {
        CloneGroup {
            instances: fragments.iter().map(|f| instance(f)).collect(),
            token_count: 40,
            line_count,
            similarity: None,
        }
    }

    #[test]
    fn fingerprint_is_stable_and_prefixed() {
        let g = group(&["foo(bar)", "foo(baz)"], 3);
        let fp1 = clone_fingerprint(&g.instances);
        let fp2 = clone_fingerprint(&g.instances);
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("dup:"));
        assert_eq!(fp1.len(), "dup:".len() + 8);
    }

    #[test]
    fn compact_fingerprint_key_is_independent_of_instance_order() {
        let mut original = group(&["alpha()", "beta()"], 3);
        original.instances[0].file = PathBuf::from("src/a.ts");
        original.instances[0].start_line = 2;
        original.instances[1].file = PathBuf::from("src/b.ts");
        original.instances[1].start_line = 8;
        let mut reordered = original.clone();
        reordered.instances.reverse();

        assert_eq!(
            CloneFingerprintKey::from_group(&original),
            CloneFingerprintKey::from_group(&reordered)
        );
    }

    #[test]
    fn duplicate_fragments_are_deduplicated_before_tokenization() {
        let mut clones = group(&["alpha()", "alpha()", "alpha()"], 2);
        clones.instances[0].file = PathBuf::from("src/a.ts");
        clones.instances[1].file = PathBuf::from("src/b.ts");
        clones.instances[2].file = PathBuf::from("src/c.ts");
        assert_eq!(distinct_fragment_inputs(&clones.instances).len(), 1);

        clones.instances[2].file = PathBuf::from("src/c.css");
        assert_eq!(
            distinct_fragment_inputs(&clones.instances).len(),
            2,
            "equal source still needs separate tokenization when syntax differs"
        );
    }

    #[test]
    fn fingerprint_is_sibling_stable() {
        let group_a = group(&["computeInvoiceTotal(order)", "computeInvoiceTotal(o)"], 4);
        let before = clone_fingerprint(&group_a.instances);
        let _group_b_edited = group(&["totallyDifferentBody()"], 2);
        let after = clone_fingerprint(&group_a.instances);
        assert_eq!(before, after);
    }

    #[test]
    fn fingerprint_differs_for_different_content() {
        let a = group(&["alpha()"], 2);
        let b = group(&["beta()"], 2);
        assert_ne!(
            clone_fingerprint(&a.instances),
            clone_fingerprint(&b.instances)
        );
    }

    #[test]
    fn fingerprint_ignores_formatting_comments_and_line_endings() {
        let compact = group(
            &["const total = left + right;", "const total = left + right;"],
            2,
        );
        let formatted = group(
            &[
                "const  total=left + right; // reviewed\r\n",
                "/* reviewed */\nconst total = left + right;",
            ],
            3,
        );

        assert_eq!(
            clone_fingerprint(&compact.instances),
            clone_fingerprint(&formatted.instances)
        );
    }

    #[test]
    fn fingerprint_changes_when_any_distinct_instance_changes_tokens() {
        let reviewed = group(&["alpha()", "alpha()"], 2);
        let edited = group(&["alpha()", "beta()"], 2);

        assert_ne!(
            clone_fingerprint(&reviewed.instances),
            clone_fingerprint(&edited.instances)
        );
    }

    #[test]
    fn fingerprint_is_independent_of_instance_order_and_count() {
        let two = group(&["alpha()", "beta()"], 2);
        let three_reordered = group(&["beta()", "alpha()", "alpha()"], 2);

        assert_eq!(
            clone_fingerprint(&two.instances),
            clone_fingerprint(&three_reordered.instances)
        );

        let two_set = CloneFingerprintSet::from_groups(std::slice::from_ref(&two));
        let three_set = CloneFingerprintSet::from_groups(std::slice::from_ref(&three_reordered));
        assert_ne!(
            two_set.ignored_clone_key_for_group(&two),
            three_set.ignored_clone_key_for_group(&three_reordered)
        );
    }

    #[test]
    fn fingerprint_set_widens_only_colliding_short_handles() {
        let a = group(&["alpha()"], 2);
        let b = group(&["beta()"], 2);
        let c = group(&["gamma()"], 2);
        let entries = vec![
            (
                CloneFingerprintKey::from_group(&a),
                0x0000_0001_1234_5678_u64,
            ),
            (
                CloneFingerprintKey::from_group(&b),
                0x0000_0002_1234_5678_u64,
            ),
            (
                CloneFingerprintKey::from_group(&c),
                0x0000_0003_8765_4321_u64,
            ),
        ];

        let fingerprints = CloneFingerprintSet::from_hashed_entries(&entries);

        assert_eq!(
            fingerprints.fingerprint_for_group(&a),
            "dup:0000000112345678"
        );
        assert_eq!(
            fingerprints.fingerprint_for_group(&b),
            "dup:0000000212345678"
        );
        assert_eq!(fingerprints.fingerprint_for_group(&c), "dup:87654321");
        assert!(
            fingerprints
                .find_group(&[a.clone(), b.clone(), c.clone()], "dup:12345678")
                .is_none()
        );
        assert_eq!(
            fingerprints
                .find_group(&[a, b, c], "dup:0000000212345678")
                .and_then(|group| group.instances.first())
                .map(|inst| inst.fragment.as_str()),
            Some("beta()")
        );
    }

    #[test]
    fn fingerprint_set_suffixes_full_hash_collisions() {
        let a = group(&["alpha()"], 2);
        let b = group(&["beta()"], 2);
        let mut entries = vec![
            (
                CloneFingerprintKey::from_group(&a),
                0x0000_0001_1234_5678_u64,
            ),
            (
                CloneFingerprintKey::from_group(&b),
                0x0000_0001_1234_5678_u64,
            ),
        ];

        let fingerprints = CloneFingerprintSet::from_hashed_entries(&entries);
        entries.reverse();
        let reversed_fingerprints = CloneFingerprintSet::from_hashed_entries(&entries);

        let a_fingerprint = fingerprints.fingerprint_for_group(&a);
        let b_fingerprint = fingerprints.fingerprint_for_group(&b);
        assert_eq!(
            a_fingerprint,
            reversed_fingerprints.fingerprint_for_group(&a)
        );
        assert_eq!(
            b_fingerprint,
            reversed_fingerprints.fingerprint_for_group(&b)
        );
        let mut assigned = vec![a_fingerprint, b_fingerprint];
        assigned.sort_unstable();
        assert_eq!(
            assigned,
            ["dup:0000000112345678-1", "dup:0000000112345678-2"]
        );
        assert!(
            fingerprints
                .find_group(&[a.clone(), b.clone()], "dup:12345678")
                .is_none()
        );
        assert!(
            fingerprints
                .find_group(&[a, b], "dup:0000000112345678")
                .is_none()
        );
    }

    #[test]
    fn group_suggestion_savings_is_lines_times_extra_copies() {
        let g = group(&["x", "x", "x"], 10); // 3 instances, 10 lines
        let suggestion = group_refactoring_suggestion(&g);
        assert_eq!(suggestion.kind, RefactoringKind::ExtractFunction);
        assert_eq!(suggestion.estimated_savings, 20); // 10 * (3 - 1)
    }

    #[test]
    fn dominant_identifier_picks_repeated_domain_name() {
        let g = group(
            &["function buildInvoice(invoice) { return invoice.total + invoice.tax; }"],
            3,
        );
        assert_eq!(dominant_identifier(&g).as_deref(), Some("invoice"));
    }

    #[test]
    fn dominant_identifier_none_on_generic() {
        let g = group(&["const data = result.map((item) => item.value);"], 3);
        assert_eq!(dominant_identifier(&g), None);
    }

    #[test]
    fn dominant_identifier_skips_ts_primitive_keywords_and_globals() {
        let g = group(
            &["const parseUser = z.string(); parseUser(z.number()); parseUser.or(z.string());"],
            4,
        );
        assert_eq!(dominant_identifier(&g).as_deref(), Some("parseUser"));
        let only_keywords = group(&["const x: string = y as string; return x as any;"], 3);
        assert_eq!(dominant_identifier(&only_keywords), None);
        let g_global = group(&["Math.max(Math.floor(Math.abs(v)), 0)"], 3);
        assert_eq!(dominant_identifier(&g_global), None);
    }

    #[test]
    fn dominant_identifier_none_on_single_letter_type_param() {
        let g = group(
            &["function id<T>(x: T): T { const a: T = x; return a as T; }"],
            3,
        );
        assert_eq!(dominant_identifier(&g), None);
    }

    #[test]
    fn dominant_identifier_none_on_tie() {
        let g = group(&["alpha(); beta();"], 2); // each appears once, no count >= 2
        assert_eq!(dominant_identifier(&g), None);
    }

    #[test]
    fn dominant_identifier_prefers_structured_names() {
        let g = group(
            &["parseSchema(input); parseSchema(cache); helper(); helper();"],
            3,
        );
        assert_eq!(dominant_identifier(&g).as_deref(), Some("parseSchema"));
    }

    #[test]
    fn dominant_identifier_requires_plain_token_margin() {
        let low_signal = group(&["schema(); schema(); parseUser();"], 3);
        assert_eq!(dominant_identifier(&low_signal), None);

        let strong = group(&["schema(); schema(); schema(); schema(); parseUser();"], 3);
        assert_eq!(dominant_identifier(&strong).as_deref(), Some("schema"));
    }

    #[test]
    fn dominant_identifier_is_stable_across_word_order() {
        let first = group(
            &["helper(); parseSchema(input); helper(); parseSchema(cache);"],
            3,
        );
        let second = group(
            &["parseSchema(input); helper(); parseSchema(cache); helper();"],
            3,
        );

        assert_eq!(dominant_identifier(&first), dominant_identifier(&second));
        assert_eq!(dominant_identifier(&first).as_deref(), Some("parseSchema"));
    }
}
