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
}
