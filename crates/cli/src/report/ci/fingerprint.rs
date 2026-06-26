/// Fingerprint key used in SARIF partialFingerprints and other CI formats.
#[cfg(test)]
pub const FINGERPRINT_KEY: &str = fallow_output::SARIF_FINGERPRINT_KEY;

/// Compute a deterministic fingerprint hash from key fields.
#[must_use]
pub fn fingerprint_hash(parts: &[&str]) -> String {
    fallow_output::codeclimate_fingerprint_hash(parts)
}

#[cfg(test)]
#[must_use]
pub fn finding_fingerprint(rule_id: &str, path: &str, snippet: &str) -> String {
    fallow_output::sarif_finding_fingerprint(rule_id, path, snippet)
}

/// Stable fingerprint for the review envelope's top-level summary block.
#[must_use]
pub fn summary_fingerprint(body: &str) -> String {
    fingerprint_hash(&[body])
}

/// Composite fingerprint for v2 same-line merged comments (issue #528).
#[must_use]
pub fn composite_fingerprint(constituents: &[&str]) -> String {
    let mut sorted: Vec<&str> = constituents.to_vec();
    sorted.sort_unstable();
    let joined = sorted.join(":");
    format!("merged:{}", fingerprint_hash(&[joined.as_str()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_stable_for_whitespace_only_snippet_changes() {
        let a = finding_fingerprint(
            "fallow/unused-export",
            "src/a.ts",
            "  export const x = 1;  ",
        );
        let b = finding_fingerprint(
            "fallow/unused-export",
            "src/a.ts",
            "\nexport const x = 1;\n",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_parts_are_separated() {
        assert_ne!(
            fingerprint_hash(&["ab", "c"]),
            fingerprint_hash(&["a", "bc"])
        );
    }

    #[test]
    fn composite_fingerprint_shifts_when_constituents_change() {
        let three = composite_fingerprint(&["fp_a", "fp_b", "fp_c"]);
        let drop_b = composite_fingerprint(&["fp_a", "fp_c"]);
        let reordered = composite_fingerprint(&["fp_c", "fp_a", "fp_b"]);
        assert_ne!(three, drop_b);
        assert_eq!(three, reordered);
        assert!(three.starts_with("merged:"));
        assert_eq!(three.len(), 23);
    }

    #[test]
    fn summary_fingerprint_shifts_when_body_changes() {
        let a = summary_fingerprint("### Fallow check\n\n0 findings");
        let b = summary_fingerprint("### Fallow check\n\n1 finding");
        assert_ne!(a, b);
        assert_eq!(a, summary_fingerprint("### Fallow check\n\n0 findings"));
        assert_eq!(a.len(), 16);
    }
}
