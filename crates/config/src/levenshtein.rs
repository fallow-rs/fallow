//! Levenshtein-distance helpers for typo detection across config surfaces.
//!
//! Shared between `RulesConfig` key validation (`closest_known_rule_name` in
//! `crates/config/src/config/rules.rs`) and the plugin enabler-typo path
//! (`detect_enabler_typos` in `crates/core/src/plugins/registry/mod.rs`) so
//! both consumers share one algorithm and one distance/length policy.

/// Levenshtein edit distance between two ASCII-leaning strings.
///
/// Uses two `Vec<usize>` rows so the working set stays bounded; the inputs we
/// match against (rule names, package names) are short and allocation cost is
/// negligible at config-load time.
#[must_use]
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let (a_len, b_len) = (a_bytes.len(), b_bytes.len());

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            curr[j] = (prev[j] + 1) // deletion
                .min(curr[j - 1] + 1) // insertion
                .min(prev[j - 1] + cost); // substitution
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find the closest candidate to `input` when it is plausibly a typo.
///
/// Returns the best match when the Levenshtein distance is at most 2 AND the
/// input is long enough that the match is not coincidental
/// (`input.len() / 2 > distance`). Returns `None` when no candidate clears the
/// bar so callers stay silent on completely novel strings rather than emitting
/// a misleading suggestion.
///
/// Input is lowercased before comparison; callers should pass canonical-case
/// candidates (kebab-case for rule names, original-case for package names).
pub fn closest_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let input_lower = input.to_ascii_lowercase();
    let mut best: Option<(&'a str, usize)> = None;

    for candidate in candidates {
        let d = levenshtein(&input_lower, &candidate.to_ascii_lowercase());
        if best.is_none_or(|(_, b_dist)| d < b_dist) {
            best = Some((candidate, d));
        }
    }

    best.filter(|&(_, d)| d > 0 && d <= 2 && input_lower.len() / 2 > d)
        .map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_identical() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn levenshtein_one_insertion() {
        assert_eq!(levenshtein("abc", "abcd"), 1);
    }

    #[test]
    fn levenshtein_empty_pair() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn closest_match_finds_typo() {
        let candidates = ["@vue/core", "react", "svelte"];
        assert_eq!(
            closest_match("@vue/cor", candidates.iter().copied()),
            Some("@vue/core")
        );
    }

    #[test]
    fn closest_match_returns_none_for_novel_input() {
        let candidates = ["react", "vue"];
        assert_eq!(
            closest_match("acme-magic", candidates.iter().copied()),
            None
        );
    }

    #[test]
    fn closest_match_returns_none_when_input_too_short() {
        let candidates = ["react"];
        // input.len() / 2 (1) must be > distance, so a one-char typo on a
        // 3-char input ("rea" vs "react", dist 2) is rejected as coincidental
        assert_eq!(closest_match("rea", candidates.iter().copied()), None);
    }

    #[test]
    fn closest_match_skips_exact_match() {
        let candidates = ["react"];
        assert_eq!(closest_match("react", candidates.iter().copied()), None);
    }

    #[test]
    fn closest_match_is_case_insensitive() {
        let candidates = ["@vue/core"];
        assert_eq!(
            closest_match("@VUE/CORE", candidates.iter().copied()),
            None,
            "exact match (ignoring case) should not produce a suggestion"
        );
        assert_eq!(
            closest_match("@VUE/CORX", candidates.iter().copied()),
            Some("@vue/core")
        );
    }

    #[test]
    fn closest_match_empty_candidates() {
        let candidates: [&str; 0] = [];
        assert_eq!(closest_match("anything", candidates.iter().copied()), None);
    }
}
