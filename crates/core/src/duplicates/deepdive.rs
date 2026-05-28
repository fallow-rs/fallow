//! Deep-dive helpers for the `fallow dupes --trace` inspector: a stable
//! content fingerprint that addresses a clone group across runs, a group-level
//! refactoring suggestion, and a best-effort "dominant identifier" name for the
//! extracted function.
//!
//! These are pure functions over [`CloneInstance`] / [`CloneGroup`] so every
//! surface (human listing, `--trace dup:<fp>` lookup, the typed JSON wrappers,
//! and `trace_clone`) computes the same values without storing a field on the
//! core [`CloneGroup`] struct.

use xxhash_rust::xxh3::xxh3_64;

use super::types::{CloneGroup, CloneInstance, RefactoringKind, RefactoringSuggestion};

/// Prefix marking a clone-group fingerprint addressable via `--trace`.
pub const FINGERPRINT_PREFIX: &str = "dup:";

/// Compute the stable content fingerprint for a clone group from its instances.
///
/// The fingerprint is derived from the representative instance's raw source
/// fragment (the first instance after [`super::types::DuplicationReport::sort`],
/// which orders instances by `(file, line)`), so it is:
///
/// - content-derived, not line-derived (moving a clone down a file does not
///   change it),
/// - sibling-stable (editing one clone group never changes another group's
///   fingerprint, since each hashes only its own content),
/// - collision-free within a report (two groups with identical representative
///   content would have clustered into one group).
///
/// Hashes the empty string for an empty group (never produced by the detector,
/// which guarantees `>= 2` instances), so the result is still a well-formed
/// `dup:<8hex>` handle.
#[must_use]
pub fn clone_fingerprint(instances: &[CloneInstance]) -> String {
    let representative = instances.first().map_or("", |inst| inst.fragment.as_str());
    fingerprint_for_fragment(representative)
}

/// Compute the fingerprint directly from a representative source fragment.
///
/// Use when the instances are wrapped (e.g. `--group-by` attributed instances)
/// but the representative fragment is the same as the bare clone group's, so the
/// fingerprint matches the top-level `clone_groups[].fingerprint` for the clone.
#[must_use]
pub fn fingerprint_for_fragment(fragment: &str) -> String {
    let hash = xxh3_64(fragment.as_bytes());
    // Low 32 bits give an 8-hex handle: ~4e9 space, ample for a single report's
    // clone-group count while staying short enough to type after `--trace`.
    format!("{FINGERPRINT_PREFIX}{:08x}", hash as u32)
}

/// Build a per-group `ExtractFunction` refactoring suggestion.
///
/// Mirrors the per-group branch of [`super::families`]'s `generate_suggestions`:
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
    let mut counts: rustc_hash::FxHashMap<&str, usize> = rustc_hash::FxHashMap::default();
    for word in identifier_words(fragment) {
        if is_generic_identifier(word) {
            continue;
        }
        *counts.entry(word).or_insert(0) += 1;
    }

    let mut best: Option<(&str, usize)> = None;
    let mut tie = false;
    for (word, count) in counts {
        match best {
            Some((_, best_count)) if count > best_count => {
                best = Some((word, count));
                tie = false;
            }
            Some((_, best_count)) if count == best_count => tie = true,
            None => best = Some((word, count)),
            Some(_) => {}
        }
    }

    match best {
        Some((word, count)) if count >= 2 && !tie => Some(word.to_string()),
        _ => None,
    }
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
fn is_generic_identifier(word: &str) -> bool {
    // Single-character names are never useful as an extracted-function name:
    // loop counters (`i`, `n`), lambda params (`x`), and generic type params
    // (`T`, `U`, `K`, `V`) all collapse here regardless of case.
    if word.chars().count() == 1 {
        return true;
    }
    matches!(
        word,
        // generic value / collection names
        "data" | "result" | "results" | "item" | "items" | "value" | "values" | "val"
            | "obj" | "object" | "arr" | "array" | "list" | "map" | "set" | "key" | "keys"
            | "tmp" | "temp" | "acc" | "cur" | "curr" | "prev" | "next" | "node" | "el"
            | "elem" | "element" | "args" | "arg" | "opts" | "options" | "params" | "param"
            | "props" | "ctx" | "context" | "res" | "req" | "err" | "error" | "fn" | "cb"
            | "callback" | "out" | "input" | "output" | "name" | "id" | "index" | "idx"
            // single-letter loop / lambda vars
            | "x" | "y" | "z" | "i" | "j" | "k" | "n" | "m" | "a" | "b" | "c" | "e" | "_"
            // JS / TS keywords that appear as bare words in fragments
            | "const" | "let" | "var" | "function" | "return" | "if" | "else" | "for"
            | "while" | "do" | "switch" | "case" | "break" | "continue" | "new" | "this"
            | "true" | "false" | "null" | "undefined" | "void" | "typeof" | "instanceof"
            | "in" | "of" | "class" | "extends" | "super" | "import" | "export" | "from"
            | "default" | "async" | "await" | "yield" | "type" | "interface" | "enum"
            | "as" | "is" | "keyof" | "readonly" | "public" | "private" | "protected"
            | "static" | "get" | "delete" | "throw" | "try" | "catch" | "finally"
            // TS primitive / utility type keywords (dominate type-heavy code like
            // schema libraries, where they would otherwise win the frequency count)
            | "string" | "number" | "boolean" | "any" | "unknown" | "never" | "bigint"
            | "symbol"
            // common JS globals that are never a useful extracted-function name
            | "Math" | "JSON" | "Object" | "Array" | "Promise" | "BigInt" | "Number"
            | "String" | "Boolean" | "Symbol" | "RegExp" | "Date"
    )
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
    fn fingerprint_is_sibling_stable() {
        // Editing group B's content must not change group A's fingerprint.
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
        // Type-heavy code (schema libraries) repeats `string`/`number`/`any`;
        // they must never become the proposed name. The real domain identifier
        // wins instead.
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
        // A generic type param repeated many times must not become the name.
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
}
