/// Fingerprint key used in SARIF partialFingerprints and other CI formats.
pub const FINGERPRINT_KEY: &str = "tools.fallow.fingerprint/v1";

/// Conventional SARIF key consumed by GitHub Code Scanning's alert-correlation
/// engine. Emitted in addition to `FINGERPRINT_KEY` so GHAS deduplicates fallow
/// alerts across pushes.
pub const GHAS_FINGERPRINT_KEY: &str = "primaryLocationLineHash/v1";

#[must_use]
pub fn normalize_snippet(snippet: &str) -> String {
    snippet
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compute a deterministic fingerprint hash from key fields.
///
/// Uses FNV-1a (64-bit) for guaranteed cross-version stability.
/// `DefaultHasher` is explicitly not specified across Rust versions.
#[must_use]
pub fn fingerprint_hash(parts: &[&str]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV offset basis
    for part in parts {
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3); // FNV prime
        }
        // Separator between parts to avoid "ab"+"c" == "a"+"bc"
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[must_use]
pub fn finding_fingerprint(rule_id: &str, path: &str, snippet: &str) -> String {
    let normalized = normalize_snippet(snippet);
    fingerprint_hash(&[rule_id, path, &normalized])
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
