//! Prune a saved baseline to the entries that still match a current finding.
//!
//! Each function keeps exactly the entries that `--baseline` matches for the
//! same findings, in the old file order. A pruned file therefore hides the same
//! findings as before, and `--fail-on-stale-baseline` finds no stale entry in
//! it. Prune never adds an entry: a new finding stays outside the baseline, and
//! the normal gates report it.

use rustc_hash::FxHashMap;
use std::path::Path;

use fallow_types::identity::IdentityPaths;

use super::{
    BASELINE_KEY_SCHEME, BaselineData, BaselineFileKind, BaselineKind, DuplicationBaselineData,
    HEALTH_FINDING_DIMENSIONS, HealthBaselineCount, HealthBaselineData, HealthBaselineMode,
    HealthFindingCategory, HealthFindingCountMap, canonical_keys, classify_baseline_value,
    clone_group_fingerprint_key, consume_baseline_key, health_finding_counts,
    moved_identity_bucket_remaps, severity_counts_for_dimension, severity_index,
};
use crate::duplicates::{CloneFingerprintSet, DuplicationReport};

/// One baseline entry that matched no current finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunedEntry {
    /// The baseline field that held the entry, for example `unused_exports`.
    /// A health count names the bucket field and the category, for example
    /// `finding_counts.complexity_high`.
    pub category: String,
    /// The entry key as the file stores it.
    pub key: String,
    /// The number of occurrences removed under this key.
    pub count: usize,
}

/// The result of pruning one baseline file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselinePrune {
    /// Entries in the file before the prune, counted as `--baseline` counts
    /// them for the stale-baseline check.
    pub entries_before: usize,
    /// Entries left after the prune.
    pub entries_after: usize,
    /// The removed entries, in file order.
    pub removed: Vec<PrunedEntry>,
    /// The new file content. `None` when nothing was removed, so the caller
    /// leaves the file as it is.
    pub content: Option<String>,
}

/// Why a baseline file cannot be pruned. The file stays as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselinePruneRefusal {
    /// The file is not valid JSON or not a valid baseline of this kind.
    Parse(String),
    /// Another command saved the file, or nothing in it names this kind.
    NotThisKind {
        /// The command that saved the file, when the file names one.
        saved_by: Option<BaselineKind>,
    },
    /// The file uses a key form from an older version, which prune cannot
    /// match entry by entry.
    LegacyKeys,
    /// The file uses a key scheme that this version does not know.
    UnknownKeyScheme(String),
    /// The file was saved with another analysis identity, for example a
    /// type-aware run. The fields that differ are listed.
    IncompatibleIdentity(Vec<&'static str>),
    /// The duplication baseline holds clone keys that older versions shared
    /// between unrelated groups (issue #3290).
    SharedCloneKeys,
    /// The file has fields that this version does not know, probably from a
    /// newer version. A rewrite would drop them.
    UnknownFields(Vec<String>),
}

impl BaselinePruneRefusal {
    /// A stable token for machine output.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Parse(_) => "parse-error",
            Self::NotThisKind { .. } => "not-this-kind",
            Self::LegacyKeys => "legacy-keys",
            Self::UnknownKeyScheme(_) => "unknown-key-scheme",
            Self::IncompatibleIdentity(_) => "incompatible-identity",
            Self::SharedCloneKeys => "shared-clone-keys",
            Self::UnknownFields(_) => "unknown-fields",
        }
    }

    /// A one-sentence reason for people.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Self::Parse(error) => format!("the file is not a valid baseline: {error}"),
            Self::NotThisKind {
                saved_by: Some(kind),
            } => format!("`fallow {}` saved this file", kind.as_str()),
            Self::NotThisKind { saved_by: None } => {
                "nothing in the file names this baseline kind".to_owned()
            }
            Self::LegacyKeys => {
                "the file uses an older key form that prune cannot match".to_owned()
            }
            Self::UnknownKeyScheme(scheme) => {
                format!("the file uses the key scheme `{scheme}`, which this version does not know")
            }
            Self::IncompatibleIdentity(fields) => format!(
                "the file was saved with another analysis mode ({})",
                fields.join(", ")
            ),
            Self::SharedCloneKeys => {
                "the file has clone keys that older versions shared between unrelated groups"
                    .to_owned()
            }
            Self::UnknownFields(fields) => format!(
                "the file has fields that this version does not know ({}); a newer fallow version probably saved it",
                fields.join(", ")
            ),
        }
    }
}

fn parse_own_kind(
    content: &str,
    kind: BaselineKind,
) -> Result<serde_json::Value, BaselinePruneRefusal> {
    let parsed = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|error| BaselinePruneRefusal::Parse(error.to_string()))?;
    match classify_baseline_value(&parsed, kind) {
        BaselineFileKind::Own | BaselineFileKind::NotAnObject => Ok(parsed),
        other @ (BaselineFileKind::Foreign(_) | BaselineFileKind::Unrecognised) => {
            Err(BaselinePruneRefusal::NotThisKind {
                saved_by: other.saved_by(),
            })
        }
    }
}

/// Parse the file and refuse it when a rewrite would drop a field with data.
///
/// The structs skip empty fields on save, so a field that is absent from the
/// round trip is unknown only when the file holds data in it.
fn parse_value<T: serde::de::DeserializeOwned + serde::Serialize>(
    value: &serde_json::Value,
) -> Result<T, BaselinePruneRefusal> {
    let parsed =
        T::deserialize(value).map_err(|error| BaselinePruneRefusal::Parse(error.to_string()))?;
    let round_trip = serde_json::to_value(&parsed)
        .map_err(|error| BaselinePruneRefusal::Parse(error.to_string()))?;
    if let (Some(original), Some(known)) = (value.as_object(), round_trip.as_object()) {
        let unknown: Vec<String> = original
            .iter()
            .filter(|(key, field)| {
                key.as_str() != "kind" && !known.contains_key(key.as_str()) && !is_empty_json(field)
            })
            .map(|(key, _)| key.clone())
            .collect();
        if !unknown.is_empty() {
            return Err(BaselinePruneRefusal::UnknownFields(unknown));
        }
    }
    Ok(parsed)
}

fn is_empty_json(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.is_empty(),
        serde_json::Value::Object(fields) => fields.is_empty(),
        serde_json::Value::String(text) => text.is_empty(),
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
    }
}

fn serialize<T: serde::Serialize>(
    baseline: &T,
    trailing_newline: bool,
) -> Result<String, BaselinePruneRefusal> {
    let mut json = serde_json::to_string_pretty(baseline)
        .map_err(|error| BaselinePruneRefusal::Parse(error.to_string()))?;
    if trailing_newline {
        json.push('\n');
    }
    Ok(json)
}

/// Keep the saved keys that a current key matches, one saved occurrence for
/// each current occurrence, and record the rest.
fn retain_matched(
    saved: &mut Vec<String>,
    current: &[String],
    category: &str,
    removed: &mut Vec<PrunedEntry>,
) {
    let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
    for key in current {
        *remaining.entry(key.as_str()).or_default() += 1;
    }
    saved.retain(|key| {
        if consume_baseline_key(&mut remaining, key) {
            return true;
        }
        removed.push(PrunedEntry {
            category: category.to_owned(),
            key: key.clone(),
            count: 1,
        });
        false
    });
}

/// Prune a dead-code baseline (the format `fallow dead-code --save-baseline`
/// writes) against the results of a whole-project run.
///
/// # Errors
///
/// Returns [`BaselinePruneRefusal`] when the file is not a canonical dead-code
/// baseline of the same analysis identity.
pub fn prune_dead_code_baseline(
    content: &str,
    results: &crate::results::AnalysisResults,
    root: &Path,
    identity: &fallow_types::semantic::SemanticAnalysisIdentity,
) -> Result<BaselinePrune, BaselinePruneRefusal> {
    let mut baseline: BaselineData =
        parse_value(&parse_own_kind(content, BaselineKind::DeadCode)?)?;
    match baseline.identity.as_deref() {
        None => return Err(BaselinePruneRefusal::LegacyKeys),
        Some(scheme) if scheme != BASELINE_KEY_SCHEME => {
            return Err(BaselinePruneRefusal::UnknownKeyScheme(scheme.to_owned()));
        }
        Some(_) => {}
    }
    let incompatible = baseline.analysis_identity().incompatible_fields(identity);
    if !incompatible.is_empty() {
        return Err(BaselinePruneRefusal::IncompatibleIdentity(incompatible));
    }

    let entries_before = baseline.total_entries();
    let paths = IdentityPaths::new(root);
    let mut removed = Vec::new();
    macro_rules! prune {
        ($($field:ident),* $(,)?) => {
            $(retain_matched(
                &mut baseline.$field,
                &canonical_keys(&results.$field, &paths),
                stringify!($field),
                &mut removed,
            );)*
        };
    }
    with_baseline_fields!(prune);

    let content = if removed.is_empty() {
        None
    } else {
        baseline.kind = Some(BaselineKind::DeadCode);
        Some(serialize(&baseline, true)?)
    };
    Ok(BaselinePrune {
        entries_before,
        entries_after: baseline.total_entries(),
        removed,
        content,
    })
}

/// Prune a duplication baseline against the report of a whole-project run.
///
/// The three key arrays of the format hold one row for each clone group. A row
/// stays when its normalized key matches a current group, and the older arrays
/// lose the same rows so they stay aligned.
///
/// # Errors
///
/// Returns [`BaselinePruneRefusal`] when the file is not a duplication baseline
/// with normalized keys, or when it holds the shared clone keys of issue #3290.
pub fn prune_dupes_baseline(
    content: &str,
    report: &DuplicationReport,
) -> Result<BaselinePrune, BaselinePruneRefusal> {
    let mut baseline: DuplicationBaselineData =
        parse_value(&parse_own_kind(content, BaselineKind::Dupes)?)?;
    if baseline.has_unparsed_collision_keys() {
        return Err(BaselinePruneRefusal::SharedCloneKeys);
    }
    let entries_before = baseline.entry_count();
    if baseline.normalized_clone_fingerprints.is_empty() {
        if entries_before == 0 {
            return Ok(BaselinePrune {
                entries_before,
                entries_after: 0,
                removed: Vec::new(),
                content: None,
            });
        }
        return Err(BaselinePruneRefusal::LegacyKeys);
    }

    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    let current: Vec<String> = report
        .clone_groups
        .iter()
        .map(|group| clone_group_fingerprint_key(group, &fingerprints))
        .collect();
    let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
    for key in &current {
        *remaining.entry(key.as_str()).or_default() += 1;
    }
    let keep: Vec<bool> = baseline
        .normalized_clone_fingerprints
        .iter()
        .map(|key| consume_baseline_key(&mut remaining, key))
        .collect();
    let removed: Vec<PrunedEntry> = baseline
        .normalized_clone_fingerprints
        .iter()
        .zip(&keep)
        .filter(|(_, kept)| !**kept)
        .map(|(key, _)| PrunedEntry {
            category: "normalized_clone_fingerprints".to_owned(),
            key: key.clone(),
            count: 1,
        })
        .collect();
    if removed.is_empty() {
        return Ok(BaselinePrune {
            entries_before,
            entries_after: entries_before,
            removed,
            content: None,
        });
    }

    let rows = keep.len();
    retain_rows(&mut baseline.normalized_clone_fingerprints, &keep);
    // A hand-edited file can break the row alignment. Older binaries read the
    // older arrays only, so leave a misaligned array as it is.
    if baseline.clone_groups.len() == rows {
        retain_rows(&mut baseline.clone_groups, &keep);
    }
    if baseline.clone_fingerprints.len() == rows {
        retain_rows(&mut baseline.clone_fingerprints, &keep);
    }
    baseline.kind = Some(BaselineKind::Dupes);
    Ok(BaselinePrune {
        entries_before,
        entries_after: baseline.entry_count(),
        removed,
        content: Some(serialize(&baseline, false)?),
    })
}

fn retain_rows(rows: &mut Vec<String>, keep: &[bool]) {
    let mut index = 0;
    rows.retain(|_| {
        let kept = keep[index];
        index += 1;
        kept
    });
}

/// Prune a health baseline against the findings of a whole-project run.
///
/// `finding_counts` and `identity_finding_counts` lose the slots that matched
/// no current finding. Identity buckets follow a moved file as `--baseline`
/// does and keep their saved key. An emptied identity bucket stays as an empty
/// map when the run reports its key, so the move matching does not change. The runtime-coverage keys and the refactoring
/// target keys stay as they are, because this run does not compute them.
///
/// # Errors
///
/// Returns [`BaselinePruneRefusal`] when the file is not a health baseline with
/// count buckets.
pub fn prune_health_baseline(
    content: &str,
    findings: &[fallow_output::ComplexityViolation],
    root: &Path,
) -> Result<BaselinePrune, BaselinePruneRefusal> {
    let mut baseline: HealthBaselineData =
        parse_value(&parse_own_kind(content, BaselineKind::Health)?)?;
    if baseline.finding_counts.is_empty() && !baseline.findings.is_empty() {
        return Err(BaselinePruneRefusal::LegacyKeys);
    }
    let entries_before = baseline.finding_entry_count();

    let mut removed = Vec::new();
    let current_counts = health_finding_counts(findings, root, HealthBaselineMode::Count);
    prune_count_buckets(
        &mut baseline.finding_counts,
        &current_counts,
        &FxHashMap::default(),
        "finding_counts",
        &mut removed,
    );
    baseline
        .finding_counts
        .retain(|_, counts| !counts.is_empty());
    if !baseline.identity_finding_counts.is_empty() {
        let current_identity = health_finding_counts(findings, root, HealthBaselineMode::Identity);
        let remaps: FxHashMap<String, String> = moved_identity_bucket_remaps(
            &baseline.identity_finding_counts,
            &current_identity,
            root,
        )
        .into_iter()
        .collect();
        prune_count_buckets(
            &mut baseline.identity_finding_counts,
            &current_identity,
            &remaps,
            "identity_finding_counts",
            &mut removed,
        );
        // A saved identity key that the run also reports is never a move
        // candidate. Dropping its emptied bucket would make it one, and a
        // second candidate with the same function name stops another bucket
        // from following its move. So the key stays, with no slots.
        baseline
            .identity_finding_counts
            .retain(|key, counts| !counts.is_empty() || current_identity.contains_key(key));
    }

    let content = if removed.is_empty() {
        None
    } else {
        baseline.kind = Some(BaselineKind::Health);
        Some(serialize(&baseline, false)?)
    };
    Ok(BaselinePrune {
        entries_before,
        entries_after: baseline.finding_entry_count(),
        removed,
        content,
    })
}

/// Reduce each saved bucket to the slots that the `--baseline` match uses.
fn prune_count_buckets(
    saved: &mut HealthFindingCountMap,
    current: &HealthFindingCountMap,
    remaps: &FxHashMap<String, String>,
    field: &str,
    removed: &mut Vec<PrunedEntry>,
) {
    for (bucket, saved_counts) in saved.iter_mut() {
        let current_key = remaps.get(bucket).unwrap_or(bucket);
        let current_counts = current.get(current_key);
        for dimension in HEALTH_FINDING_DIMENSIONS {
            let baseline = severity_counts_for_dimension(Some(saved_counts), dimension);
            let used = consumed_severity_slots(
                severity_counts_for_dimension(current_counts, dimension),
                baseline,
            );
            for severity in [
                fallow_output::FindingSeverity::Moderate,
                fallow_output::FindingSeverity::High,
                fallow_output::FindingSeverity::Critical,
            ] {
                let index = severity_index(severity);
                if used[index] == baseline[index] {
                    continue;
                }
                let category = HealthFindingCategory {
                    dimension,
                    severity,
                }
                .key();
                removed.push(PrunedEntry {
                    category: format!("{field}.{category}"),
                    key: bucket.clone(),
                    count: baseline[index] - used[index],
                });
                if used[index] == 0 {
                    saved_counts.remove(category);
                } else {
                    saved_counts.insert(
                        category.to_owned(),
                        HealthBaselineCount { count: used[index] },
                    );
                }
            }
        }
    }
}

/// The saved slots per severity that the greedy match of
/// [`super::overflowing_severities`] uses for the current findings.
///
/// A saved slot covers a current finding of the same or a lower severity. The
/// match takes the lowest compatible slot first, so keeping exactly the used
/// slots leaves every match, and every overflow, as it was.
fn consumed_severity_slots(current: [usize; 3], baseline: [usize; 3]) -> [usize; 3] {
    let mut available = baseline;
    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }
    [
        baseline[0] - available[0],
        baseline[1] - available[1],
        baseline[2] - available[2],
    ]
}

#[cfg(test)]
mod tests;
