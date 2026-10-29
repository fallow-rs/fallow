//! Attribution keys of styling findings.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::relative_key_path;

/// Attribution key set for every styling finding in a health report.
pub fn styling_keys(report: &fallow_output::HealthReport, root: &Path) -> FxHashSet<String> {
    styling_ledger_keys(&report.styling_findings, root)
        .into_iter()
        .collect()
}

/// Identity key for one styling finding:
/// `styling:<code>:<sub kind>:<path>:<value>`, plus `:<selector hash>` when
/// the finding has the selector of its rule.
///
/// Line numbers are excluded, so a finding survives unrelated edits above it
/// (issue #3956). A selector-complexity value is only a metric, such as
/// `specificity 1-2-0`. The selector hash keeps a new rule with an equal
/// metric apart from a removed rule. The key holds a hash, not the selector,
/// because a selector can contain the `:` and `|` key separators. Use
/// [`styling_ledger_keys`] to tell repeated findings apart.
pub fn styling_finding_key(finding: &fallow_output::StylingFinding, root: &Path) -> String {
    let path = relative_key_path(Path::new(&finding.path), root);
    let base = format!(
        "styling:{}:{}:{path}:{}",
        finding.code, finding.sub_kind, finding.value
    );
    match finding.internal.rule_selector.as_deref() {
        Some(selector) if !selector.is_empty() => {
            format!(
                "{base}:{:016x}",
                xxhash_rust::xxh3::xxh3_64(selector.as_bytes())
            )
        }
        _ => base,
    }
}

/// Attribution keys of styling findings, in the order of `findings`.
///
/// Findings with the same identity key get an occurrence suffix (`#2`, `#3`)
/// in source line order. Thus a file with two equal findings keeps two keys,
/// and a line shift does not change which key a finding gets.
pub fn styling_ledger_keys(findings: &[fallow_output::StylingFinding], root: &Path) -> Vec<String> {
    styling_ledger_keys_with_diff(findings, root, None)
}

/// Attribution keys of styling findings, like [`styling_ledger_keys`].
///
/// With a `diff`, a finding on an added line gets its occurrence number after
/// the findings on unchanged lines. Equal findings have the same identity key,
/// so the base snapshot cannot tell them apart. The diff shows which of them
/// the change wrote, and that finding gets the occurrence that the base does
/// not have.
pub fn styling_ledger_keys_with_diff(
    findings: &[fallow_output::StylingFinding],
    root: &Path,
    diff: Option<&fallow_output::DiffIndex>,
) -> Vec<String> {
    let mut keys: Vec<String> = findings
        .iter()
        .map(|finding| styling_finding_key(finding, root))
        .collect();
    let on_added_line = |finding: &fallow_output::StylingFinding| -> bool {
        diff.is_some_and(|diff| {
            diff.key_for(&root.join(&finding.path), root)
                .is_some_and(|path| diff.line_is_added(&path, u64::from(finding.line)))
        })
    };
    let mut order: Vec<usize> = (0..findings.len()).collect();
    order.sort_by_cached_key(|&index| (on_added_line(&findings[index]), findings[index].line));
    let mut seen: FxHashMap<String, usize> = FxHashMap::default();
    for index in order {
        let occurrence = seen.entry(keys[index].clone()).or_default();
        *occurrence += 1;
        if *occurrence > 1 {
            keys[index] = format!("{}#{occurrence}", keys[index]);
        }
    }
    keys
}

/// Insert `"introduced": bool` into each `styling_findings` object in
/// serialized health JSON, matching entries to `findings` by array position.
///
/// The typed flag of the audit run wins. A finding without the typed flag
/// falls back to key membership in `base`.
pub(super) fn annotate_styling_json(
    json: &mut serde_json::Value,
    findings: &[fallow_output::StylingFinding],
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("styling_findings")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    let keys = styling_ledger_keys(findings, root);
    for ((item, key), finding) in items.iter_mut().zip(&keys).zip(findings) {
        if let serde_json::Value::Object(map) = item {
            let introduced = finding
                .internal
                .introduced
                .unwrap_or_else(|| !base.contains(key));
            map.insert("introduced".to_string(), serde_json::json!(introduced));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{styling_finding_key, styling_ledger_keys, styling_ledger_keys_with_diff};

    fn styling_finding(path: &str, line: u32, value: &str) -> fallow_output::StylingFinding {
        fallow_output::StylingFinding {
            internal: fallow_output::StylingFindingInternal::default(),
            code: "css-selector-complexity".to_string(),
            sub_kind: "high-specificity".to_string(),
            path: path.to_string(),
            line,
            value: value.to_string(),
            effective_severity: fallow_output::StylingFindingSeverity::Warn,
            blast_radius: None,
            confidence: None,
            agent_disposition: None,
            nearest_token: None,
            fix_hint: None,
            actions: Vec::new(),
        }
    }

    /// Issue #3956: a line shift keeps the styling keys, and repeated
    /// findings get occurrence suffixes in source line order.
    #[test]
    fn styling_ledger_keys_ignore_line_shifts_and_count_repeats() {
        let root = PathBuf::from("/project");
        let base = [
            styling_finding("src/a.css", 1, "specificity 1-2-0"),
            styling_finding("src/a.css", 4, "specificity 1-2-0"),
            styling_finding("src/b.css", 2, "specificity 1-2-0"),
        ];
        let head = [
            styling_finding("src/b.css", 2, "specificity 1-2-0"),
            styling_finding("src/a.css", 7, "specificity 1-2-0"),
            styling_finding("src/a.css", 3, "specificity 1-2-0"),
        ];

        let base_keys = styling_ledger_keys(&base, &root);
        let head_keys = styling_ledger_keys(&head, &root);

        assert_eq!(
            base_keys,
            vec![
                "styling:css-selector-complexity:high-specificity:src/a.css:specificity 1-2-0",
                "styling:css-selector-complexity:high-specificity:src/a.css:specificity 1-2-0#2",
                "styling:css-selector-complexity:high-specificity:src/b.css:specificity 1-2-0",
            ]
        );
        assert_eq!(head_keys[1], base_keys[1]);
        assert_eq!(head_keys[2], base_keys[0]);
        assert_eq!(head_keys[0], base_keys[2]);
    }

    /// Issue #3956: a new equal finding on an added line above an unchanged
    /// finding takes the last occurrence, so the unchanged finding keeps the
    /// key of the base.
    #[test]
    fn styling_ledger_keys_with_diff_give_added_lines_the_last_occurrence() {
        let root = PathBuf::from("/project");
        let base = [styling_finding("src/a.css", 4, "specificity 1-2-0")];
        let head = [
            styling_finding("src/a.css", 1, "specificity 1-2-0"),
            styling_finding("src/a.css", 5, "specificity 1-2-0"),
        ];
        let diff = fallow_output::DiffIndex::from_unified_diff(
            "diff --git a/src/a.css b/src/a.css\n--- a/src/a.css\n+++ b/src/a.css\n@@ -0,0 +1,1 @@\n+#nav .menu .item { color: green; }\n",
        );

        let base_keys = styling_ledger_keys(&base, &root);
        let head_keys = styling_ledger_keys_with_diff(&head, &root, Some(&diff));

        assert_eq!(head_keys[1], base_keys[0]);
        assert_eq!(head_keys[0], format!("{}#2", base_keys[0]));
    }

    /// Issue #3956: the rule selector separates findings with an equal
    /// metric value, and the line does not change the key.
    #[test]
    fn styling_finding_key_uses_the_rule_selector() {
        let root = PathBuf::from("/project");
        let with_selector = |line: u32, selector: &str| {
            let mut finding = styling_finding("src/a.css", line, "specificity 1-2-0");
            finding.internal.rule_selector = Some(selector.to_string());
            finding
        };

        let legacy = styling_finding_key(&with_selector(4, "#app .legacy .title"), &root);
        let moved = styling_finding_key(&with_selector(9, "#app .legacy .title"), &root);
        let nav = styling_finding_key(&with_selector(6, "#nav .menu .item"), &root);

        assert_eq!(legacy, moved);
        assert_ne!(legacy, nav);
        assert!(!legacy.contains("#app"), "{legacy}");
    }
}
