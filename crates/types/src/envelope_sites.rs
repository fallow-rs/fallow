//! Where a fact sits on each JSON envelope shape, for the surfaces that read
//! an envelope back rather than holding the run's typed state.
//!
//! Shared because two surfaces have to agree on the baseline sites: the MCP's
//! verdict warnings and the CLI's pull-request comment advisory both read a
//! saved envelope they did not produce, and a multi-section envelope carries
//! up to three baselines. Two copies of the list would let one surface learn
//! about a new section while the other reported one baseline's rot under
//! another's counts.

use serde_json::{Map, Value};

/// One place a run publishes `baseline_staleness`, and which analysis the
/// object belongs to on the shapes that carry more than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineSite {
    /// Key path from the envelope root to the staleness object.
    pub path: &'static [&'static str],
    /// The analysis the baseline belongs to, `None` on the single-analysis
    /// shapes where the envelope root already says which command ran.
    pub analysis: Option<&'static str>,
}

/// Every site a fallow envelope can carry a staleness object at, in the order
/// a consumer reports them.
///
/// Every row is a path a run actually emits, measured rather than inferred.
/// `dead-code` and `dupes` publish it at the root and `health` inside
/// `summary`; the combined envelope repeats those under `check`, `dupes` and
/// `health`. `audit` publishes one per baseline it loaded, at the root of its
/// `dead_code` and `duplication` sections and inside its `complexity`
/// section's own `summary`, so all three of its baselines have a row.
///
/// The label is the section the envelope actually uses, not the command the
/// baseline came from, because that is what a reader goes looking for.
pub const BASELINE_STALENESS_SITES: &[BaselineSite] = &[
    BaselineSite {
        path: &["baseline_staleness"],
        analysis: None,
    },
    BaselineSite {
        path: &["summary", "baseline_staleness"],
        analysis: None,
    },
    BaselineSite {
        path: &["check", "baseline_staleness"],
        analysis: Some("dead-code"),
    },
    BaselineSite {
        path: &["dupes", "baseline_staleness"],
        analysis: Some("duplication"),
    },
    BaselineSite {
        path: &["health", "summary", "baseline_staleness"],
        analysis: Some("health"),
    },
    BaselineSite {
        path: &["dead_code", "baseline_staleness"],
        analysis: Some("dead-code"),
    },
    BaselineSite {
        path: &["duplication", "baseline_staleness"],
        analysis: Some("duplication"),
    },
    BaselineSite {
        path: &["complexity", "summary", "baseline_staleness"],
        analysis: Some("complexity"),
    },
];

/// Resolve a key path against an envelope root, `None` when any segment is
/// absent.
#[must_use]
pub fn lookup<'a>(root: &'a Map<String, Value>, path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut current = root.get(*first)?;
    for key in rest {
        current = current.get(key)?;
    }
    Some(current)
}

/// Every staleness object `root` carries, paired with its analysis label, in
/// site order.
///
/// An envelope that loaded no baseline yields nothing, which is every run
/// produced before the object existed and every run that passed no baseline.
pub fn baseline_staleness_objects(
    root: &Map<String, Value>,
) -> impl Iterator<Item = (&Value, Option<&'static str>)> {
    BASELINE_STALENESS_SITES
        .iter()
        .filter_map(move |site| Some((lookup(root, site.path)?, site.analysis)))
}

#[cfg(test)]
mod tests {
    use super::{BASELINE_STALENESS_SITES, baseline_staleness_objects, lookup};

    #[test]
    fn every_site_has_a_distinct_path() {
        let mut paths = BASELINE_STALENESS_SITES
            .iter()
            .map(|site| site.path)
            .collect::<Vec<_>>();
        let total = paths.len();
        paths.sort_unstable();
        paths.dedup();
        assert_eq!(paths.len(), total, "a duplicated site would report twice");
    }

    #[test]
    fn every_site_path_ends_at_the_staleness_key() {
        for site in BASELINE_STALENESS_SITES {
            assert_eq!(site.path.last(), Some(&"baseline_staleness"));
        }
    }

    #[test]
    fn a_missing_segment_resolves_to_nothing() {
        let envelope = serde_json::json!({ "summary": { "score": 90 } });
        let root = envelope.as_object().expect("object");
        assert!(lookup(root, &["summary", "baseline_staleness"]).is_none());
        assert!(lookup(root, &["check", "baseline_staleness"]).is_none());
        assert_eq!(baseline_staleness_objects(root).count(), 0);
    }

    #[test]
    fn a_multi_section_envelope_reports_each_baseline_with_its_own_label() {
        let envelope = serde_json::json!({
            "kind": "audit",
            "dead_code": { "baseline_staleness": { "baseline_entries": 3 } },
            "duplication": { "baseline_staleness": { "baseline_entries": 4 } },
            "complexity": { "summary": { "baseline_staleness": { "baseline_entries": 5 } } }
        });
        let root = envelope.as_object().expect("object");

        let found = baseline_staleness_objects(root)
            .map(|(staleness, analysis)| (staleness["baseline_entries"].as_u64(), analysis))
            .collect::<Vec<_>>();

        assert_eq!(
            found,
            vec![
                (Some(3), Some("dead-code")),
                (Some(4), Some("duplication")),
                (Some(5), Some("complexity")),
            ]
        );
    }
}
