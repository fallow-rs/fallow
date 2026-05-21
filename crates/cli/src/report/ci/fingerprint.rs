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

/// Composite fingerprint for v2 same-line merged comments (issue #528).
/// Hashes the sorted list of constituent per-finding fingerprints (joined
/// by `:`) and prefixes the resulting 16-char FNV-1a hash with `merged:`
/// so consumers can discriminate the merged shape from a single-finding
/// fingerprint by string inspection. The hash changes when constituent
/// findings change membership across runs; the bundled wrappers
/// (`action/scripts/review.sh`, `ci/scripts/review.sh`) and
/// `fallow ci reconcile-review` consume only the primary fingerprint, so
/// content-change yielding a new fingerprint cleanly re-posts on the next
/// run rather than silently keeping a stale body. External consumers that
/// want update-in-place reconciliation implement their own identity
/// tracking via `marker_regex`.
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
        // Hash incorporates the sorted constituent fingerprints, so adding
        // or removing one shifts the merged identity. Idempotent on equal
        // input regardless of insertion order (sort stabilises it). The
        // wire shape is `merged:<16-char hex>` so consumers can detect
        // compositeness by prefix without re-hashing.
        let three = composite_fingerprint(&["fp_a", "fp_b", "fp_c"]);
        let drop_b = composite_fingerprint(&["fp_a", "fp_c"]);
        let reordered = composite_fingerprint(&["fp_c", "fp_a", "fp_b"]);
        assert_ne!(three, drop_b);
        assert_eq!(three, reordered);
        assert!(three.starts_with("merged:"));
        // 7 chars prefix + 16 hex = 23 total.
        assert_eq!(three.len(), 23);
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
