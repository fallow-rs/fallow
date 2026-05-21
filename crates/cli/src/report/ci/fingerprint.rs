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

/// Stable fingerprint for the review envelope's top-level summary block
/// (issue #528 / v2). Hashes the rendered summary body so consumers can
/// reconcile a single sticky PR/MR summary comment by fingerprint match
/// without invoking fallow twice. Stable across runs that produce the same
/// summary content; the hash shifts when finding counts or section headers
/// change, so consumers detect content change cheaply.
#[must_use]
pub fn summary_fingerprint(body: &str) -> String {
    fingerprint_hash(&[body])
}

/// Stable per-(path, line) fingerprint used by v2 same-line comment merging
/// (issue #528). Returns `linecomp:<16-char hex>` so consumers can
/// discriminate the merged shape from a single-finding fingerprint by string
/// inspection, and the key stays stable even when constituent findings
/// change membership across runs. The stability is what enables
/// update-in-place reconciliation (PATCH the body, preserve reviewer reply
/// threads) instead of delete-and-recreate on every membership change.
#[must_use]
pub fn linecomp_fingerprint(path: &str, line: u64) -> String {
    let key = format!("{path}:{line}");
    format!("linecomp:{}", fingerprint_hash(&[&key]))
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
    fn linecomp_fingerprint_is_stable_per_path_line_pair() {
        // Per-line key is independent of constituent findings; same
        // (path, line) input yields the same hash, two different inputs
        // yield different hashes. This is the load-bearing invariant that
        // lets v2 consumers PATCH-in-place instead of delete-and-recreate
        // when constituent findings change membership.
        let a = linecomp_fingerprint("src/foo.ts", 42);
        let b = linecomp_fingerprint("src/foo.ts", 42);
        let c = linecomp_fingerprint("src/foo.ts", 43);
        let d = linecomp_fingerprint("src/bar.ts", 42);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert!(a.starts_with("linecomp:"));
        // 9 chars prefix + 16 hex = 25 total.
        assert_eq!(a.len(), 25);
    }

    #[test]
    fn summary_fingerprint_shifts_when_body_changes() {
        let a = summary_fingerprint("### Fallow check\n\n0 findings");
        let b = summary_fingerprint("### Fallow check\n\n1 finding");
        assert_ne!(a, b);
        // Idempotent.
        assert_eq!(a, summary_fingerprint("### Fallow check\n\n0 findings"));
        // 16 hex chars, no prefix.
        assert_eq!(a.len(), 16);
    }
}
