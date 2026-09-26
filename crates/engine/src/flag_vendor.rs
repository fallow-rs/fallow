//! Vendor flag state for `fallow flags --retirement --flag-state <FILE>`.
//!
//! The export is a local JSON file in one vendor-neutral schema. Fallow reads
//! it offline: no credentials, no network calls and no vendor clients. The
//! docs give `jq` recipes that turn the export of each vendor into this
//! schema.
//!
//! The export adds four reasons to the report. `fully-rolled-out` and
//! `archived-in-vendor` come from the vendor state of a flag in the code.
//! `missing-in-vendor` marks a flag in the code whose key is not in the
//! export. `vendor-only` adds a row for a key in the export that no code
//! reads.

use std::io::Read;
use std::path::Path;

use fallow_types::flag_retirement::{
    FlagSiteRole, RetirementEvidence, RetirementFlag, RetirementFlagKind, RetirementReason,
    RetirementVendor, RetirementVendorState, VendorFlagState,
};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;

/// The schema version of the export that this build reads.
pub const FLAG_STATE_SCHEMA_VERSION: u32 = 1;

/// Largest export that Fallow reads, in bytes.
pub const MAX_FLAG_STATE_BYTES: u64 = 16 * 1024 * 1024;

/// An export older than this many days gets a warning.
pub const STALE_EXPORT_DAYS: u64 = 30;

/// Seconds in one day.
const SECS_PER_DAY: u64 = 86_400;

/// Hint for every invalid export.
const FLAG_STATE_HELP: &str = "See https://docs.fallow.tools/cli/flags#vendor-flag-state for the schema and a jq recipe for each vendor.";

/// Why the export cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagStateError {
    /// What is wrong with the export.
    pub message: String,
    /// How to make a valid export.
    pub help: &'static str,
}

impl FlagStateError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            help: FLAG_STATE_HELP,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagStateFile {
    schema_version: u32,
    source: String,
    exported_at: String,
    flags: Vec<FlagStateEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagStateEntry {
    key: String,
    state: VendorFlagState,
    #[serde(default)]
    serves_single_variation: Option<bool>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    last_evaluated_at: Option<String>,
}

/// One flag of the export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorFlag {
    /// The vendor state of the flag.
    pub state: RetirementVendor,
    /// 1-based line of the key in the export file.
    pub line: u32,
}

/// A valid vendor export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorExport {
    /// The vendor name, for example `launchdarkly`.
    pub source: String,
    /// When the export was made, as the file gives it.
    pub exported_at: String,
    /// The export path for evidence: relative to the root when the file is
    /// inside it.
    pub display_path: String,
    /// The flags, in file order.
    pub flags: Vec<VendorFlag>,
}

/// Read and check the export at `path`.
///
/// # Errors
///
/// Returns an error when the file cannot be read, is larger than
/// [`MAX_FLAG_STATE_BYTES`], or does not match the schema.
pub fn load_flag_state(path: &Path, root: &Path) -> Result<VendorExport, FlagStateError> {
    let file = std::fs::File::open(path).map_err(|error| {
        FlagStateError::new(format!(
            "cannot read the flag state file {}: {error}",
            path.display()
        ))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_FLAG_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            FlagStateError::new(format!(
                "cannot read the flag state file {}: {error}",
                path.display()
            ))
        })?;
    if bytes.len() as u64 > MAX_FLAG_STATE_BYTES {
        return Err(FlagStateError::new(format!(
            "the flag state file {} is larger than {} MiB",
            path.display(),
            MAX_FLAG_STATE_BYTES / (1024 * 1024)
        )));
    }
    parse_flag_state(&bytes, display_path(path, root))
}

/// The export path relative to the root when the file is inside it. Both
/// sides are canonical, so a relative `--flag-state` path and a symlinked
/// root still match.
fn display_path(path: &Path, root: &Path) -> String {
    let canonical_path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    canonical_path
        .strip_prefix(&canonical_root)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Parse and check the bytes of an export.
///
/// # Errors
///
/// Returns an error when the bytes are not valid JSON in the schema, when
/// `schema_version` is not [`FLAG_STATE_SCHEMA_VERSION`], when `exported_at`
/// has no `YYYY-MM-DD` date, or when a key is empty or occurs two times.
pub fn parse_flag_state(
    bytes: &[u8],
    display_path: String,
) -> Result<VendorExport, FlagStateError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FlagStateError::new(format!("{display_path} is not UTF-8 text")))?;
    let file: FlagStateFile = serde_json::from_str(text)
        .map_err(|error| FlagStateError::new(format!("{display_path} is not valid: {error}")))?;
    if file.schema_version != FLAG_STATE_SCHEMA_VERSION {
        return Err(FlagStateError::new(format!(
            "{display_path} has schema_version {}, but this build reads schema_version {FLAG_STATE_SCHEMA_VERSION}",
            file.schema_version
        )));
    }
    if file.source.trim().is_empty() {
        return Err(FlagStateError::new(format!(
            "{display_path} has an empty source"
        )));
    }
    if date_epoch(&file.exported_at).is_none() {
        return Err(FlagStateError::new(format!(
            "{display_path} has exported_at {:?}, which does not start with a YYYY-MM-DD date",
            file.exported_at
        )));
    }
    let lines = key_lines(text);
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    for entry in &file.flags {
        if entry.key.is_empty() {
            return Err(FlagStateError::new(format!(
                "{display_path} has a flag with an empty key"
            )));
        }
        if !seen.insert(entry.key.as_str()) {
            return Err(FlagStateError::new(format!(
                "{display_path} has the key {:?} more than one time",
                entry.key
            )));
        }
    }
    let flags = file
        .flags
        .into_iter()
        .map(|entry| VendorFlag {
            line: lines.get(entry.key.as_str()).copied().unwrap_or(1),
            state: RetirementVendor {
                key: entry.key,
                state: entry.state,
                serves_single_variation: entry.serves_single_variation,
                created_at: entry.created_at,
                last_evaluated_at: entry.last_evaluated_at,
            },
        })
        .collect();
    Ok(VendorExport {
        source: file.source,
        exported_at: file.exported_at,
        display_path,
        flags,
    })
}

/// The line of the first `"key": "<value>"` pair for each value.
fn key_lines(text: &str) -> FxHashMap<String, u32> {
    const KEY_TOKEN: &str = "\"key\"";
    let mut lines = FxHashMap::default();
    let mut from = 0;
    while let Some(offset) = text[from..].find(KEY_TOKEN) {
        let start = from + offset;
        from = start + KEY_TOKEN.len();
        let rest = text[from..].trim_start();
        let Some(value) = rest.strip_prefix(':') else {
            continue;
        };
        let mut values =
            serde_json::Deserializer::from_str(value.trim_start()).into_iter::<String>();
        let Some(Ok(key)) = values.next() else {
            continue;
        };
        let line = u32::try_from(text[..start].matches('\n').count() + 1).unwrap_or(u32::MAX);
        lines.entry(key).or_insert(line);
    }
    lines
}

/// Epoch of the UTC midnight of the `YYYY-MM-DD` date at the start of `text`.
fn date_epoch(text: &str) -> Option<u64> {
    crate::clock::utc_midnight_epoch(text.get(..10)?)
}

/// Inputs to match an export with the rows of the report.
pub struct VendorMatch<'a> {
    /// The export.
    pub export: &'a VendorExport,
    /// `flags.vendorKeyPrefix`: removed from each vendor key before the
    /// match.
    pub key_prefix: Option<&'a str>,
    /// Every flag name in the project, also outside the scope of the run.
    pub code_flag_names: &'a FxHashSet<String>,
    /// Whether to add `vendor-only` rows. A run narrowed to part of the
    /// project cannot tell that no code reads a key, so it adds none.
    pub add_vendor_only: bool,
    /// The analysis clock, in unix seconds.
    pub clock_epoch_secs: u64,
}

/// Add the vendor reasons to `rows`, and add a row for each `vendor-only`
/// key. Returns the export summary for the report.
///
/// Only SDK rows match the export. When an SDK label in the project matches
/// the export `source` (for example `LaunchDarkly` and `launchdarkly`), only
/// the rows of that SDK, and the SDK rows without a label, match. Thus an
/// export of one vendor does not mark the flags of another SDK as
/// `missing-in-vendor`.
pub fn apply_vendor_state(
    rows: &mut Vec<RetirementFlag>,
    input: &VendorMatch<'_>,
) -> RetirementVendorState {
    let export = input.export;
    let by_name: FxHashMap<&str, &VendorFlag> = export
        .flags
        .iter()
        .map(|flag| (code_name(&flag.state.key, input.key_prefix), flag))
        .collect();
    let source = normalize_label(&export.source);
    let source_is_project_sdk = rows.iter().any(|row| sdk_matches_source(row, &source));
    for row in rows.iter_mut() {
        if row.kind != RetirementFlagKind::SdkCall
            || (source_is_project_sdk
                && row.sdk_name.is_some()
                && !sdk_matches_source(row, &source))
        {
            continue;
        }
        match by_name.get(row.flag_name.as_str()) {
            Some(flag) => add_state_reasons(row, flag, export),
            None => add_missing_reason(row, export),
        }
    }
    if input.add_vendor_only {
        for flag in &export.flags {
            let name = code_name(&flag.state.key, input.key_prefix);
            if !input.code_flag_names.contains(name) {
                rows.push(vendor_only_row(name, flag, export));
            }
        }
    }
    RetirementVendorState {
        source: export.source.clone(),
        exported_at: export.exported_at.clone(),
        export_age_days: date_epoch(&export.exported_at)
            .map(|epoch| input.clock_epoch_secs.saturating_sub(epoch) / SECS_PER_DAY),
        flags: export.flags.len(),
    }
}

/// The name that the code uses for a vendor key.
fn code_name<'k>(key: &'k str, prefix: Option<&str>) -> &'k str {
    prefix
        .filter(|prefix| !prefix.is_empty())
        .and_then(|prefix| key.strip_prefix(prefix))
        .filter(|name| !name.is_empty())
        .unwrap_or(key)
}

/// Lowercase ASCII letters and digits only, so `LaunchDarkly`,
/// `launch-darkly` and `launchdarkly` compare equal.
fn normalize_label(label: &str) -> String {
    label
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn sdk_matches_source(row: &RetirementFlag, source: &str) -> bool {
    if row.kind != RetirementFlagKind::SdkCall || source.is_empty() {
        return false;
    }
    row.sdk_name.as_deref().is_some_and(|sdk| {
        let sdk = normalize_label(sdk);
        !sdk.is_empty() && (sdk.starts_with(source) || source.starts_with(&sdk))
    })
}

/// The code site that evidence of a vendor reason points at: the first read
/// site, else the first site.
fn evidence_site(row: &RetirementFlag) -> Option<(String, u32)> {
    row.sites
        .iter()
        .find(|site| site.role == FlagSiteRole::Read)
        .or_else(|| row.sites.first())
        .map(|site| (site.path.clone(), site.line))
}

fn add_state_reasons(row: &mut RetirementFlag, flag: &VendorFlag, export: &VendorExport) {
    row.vendor = Some(flag.state.clone());
    let Some((path, line)) = evidence_site(row) else {
        return;
    };
    let state = &flag.state;
    let single = state.serves_single_variation == Some(true);
    if state.state == VendorFlagState::RolledOut || single {
        let mut detail = format!("{} state {}", export.source, state_code(state.state));
        if single {
            detail.push_str(", serves one variation");
        }
        push_reason(row, RetirementReason::FullyRolledOut, &path, line, detail);
    }
    if state.state == VendorFlagState::Archived {
        let detail = format!("{} state archived", export.source);
        push_reason(row, RetirementReason::ArchivedInVendor, &path, line, detail);
    }
}

fn add_missing_reason(row: &mut RetirementFlag, export: &VendorExport) {
    let Some((path, line)) = evidence_site(row) else {
        return;
    };
    let detail = format!(
        "the key is not in the {} export ({})",
        export.source, export.display_path
    );
    push_reason(row, RetirementReason::MissingInVendor, &path, line, detail);
}

fn push_reason(
    row: &mut RetirementFlag,
    reason: RetirementReason,
    path: &str,
    line: u32,
    detail: String,
) {
    if !row.reasons.contains(&reason) {
        row.reasons.push(reason);
    }
    row.evidence.push(RetirementEvidence {
        reason,
        path: path.to_string(),
        line,
        detail,
    });
}

fn vendor_only_row(name: &str, flag: &VendorFlag, export: &VendorExport) -> RetirementFlag {
    RetirementFlag {
        flag_name: name.to_string(),
        kind: RetirementFlagKind::VendorExport,
        sdk_name: None,
        workspace: None,
        sites: Vec::new(),
        read_sites: 0,
        test_only: false,
        first_seen: None,
        oldest_surviving_site: None,
        last_touched: None,
        age_days: None,
        reasons: vec![RetirementReason::VendorOnly],
        evidence: vec![RetirementEvidence {
            reason: RetirementReason::VendorOnly,
            path: export.display_path.clone(),
            line: flag.line,
            detail: format!(
                "the key is in the {} export, but no code reads it",
                export.source
            ),
        }],
        actions: Vec::new(),
        vendor: Some(flag.state.clone()),
    }
}

const fn state_code(state: VendorFlagState) -> &'static str {
    match state {
        VendorFlagState::On => "on",
        VendorFlagState::Off => "off",
        VendorFlagState::RolledOut => "rolled_out",
        VendorFlagState::Archived => "archived",
        VendorFlagState::Experiment => "experiment",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::flag_retirement::RetirementSite;

    const EXPORT: &str = r#"{
  "schema_version": 1,
  "source": "launchdarkly",
  "exported_at": "2026-09-20T00:00:00Z",
  "flags": [
    { "key": "web.rolled", "state": "rolled_out" },
    { "key": "web.single", "state": "off", "serves_single_variation": true },
    { "key": "web.archived", "state": "archived" },
    { "key": "web.live", "state": "on", "serves_single_variation": false },
    { "key": "web.orphan", "state": "on" }
  ]
}"#;

    /// 2026-09-25T00:00:00Z.
    const CLOCK: u64 = 1_790_294_400;

    fn export() -> VendorExport {
        parse_flag_state(EXPORT.as_bytes(), "flag-state.json".to_string()).expect("valid export")
    }

    fn sdk_row(name: &str, sdk: Option<&str>) -> RetirementFlag {
        RetirementFlag {
            flag_name: name.to_string(),
            kind: RetirementFlagKind::SdkCall,
            sdk_name: sdk.map(str::to_string),
            workspace: None,
            sites: vec![RetirementSite {
                path: "src/app.ts".to_string(),
                line: 3,
                col: 2,
                role: FlagSiteRole::Read,
                in_test: false,
            }],
            read_sites: 1,
            test_only: false,
            first_seen: None,
            oldest_surviving_site: None,
            last_touched: None,
            age_days: None,
            reasons: Vec::new(),
            evidence: Vec::new(),
            actions: Vec::new(),
            vendor: None,
        }
    }

    fn apply(rows: &mut Vec<RetirementFlag>, add_vendor_only: bool) -> RetirementVendorState {
        let export = export();
        let names: FxHashSet<String> = rows.iter().map(|row| row.flag_name.clone()).collect();
        apply_vendor_state(
            rows,
            &VendorMatch {
                export: &export,
                key_prefix: Some("web."),
                code_flag_names: &names,
                add_vendor_only,
                clock_epoch_secs: CLOCK,
            },
        )
    }

    fn reasons_of<'r>(rows: &'r [RetirementFlag], name: &str) -> &'r [RetirementReason] {
        &rows
            .iter()
            .find(|row| row.flag_name == name)
            .unwrap_or_else(|| panic!("no row {name}"))
            .reasons
    }

    #[test]
    fn vendor_states_give_the_vendor_reasons() {
        let mut rows = vec![
            sdk_row("rolled", Some("LaunchDarkly")),
            sdk_row("single", Some("LaunchDarkly")),
            sdk_row("archived", Some("LaunchDarkly")),
            sdk_row("live", Some("LaunchDarkly")),
            sdk_row("typo", Some("LaunchDarkly")),
        ];
        let state = apply(&mut rows, true);
        assert_eq!(
            reasons_of(&rows, "rolled"),
            [RetirementReason::FullyRolledOut]
        );
        assert_eq!(
            reasons_of(&rows, "single"),
            [RetirementReason::FullyRolledOut]
        );
        assert_eq!(
            reasons_of(&rows, "archived"),
            [RetirementReason::ArchivedInVendor]
        );
        assert!(reasons_of(&rows, "live").is_empty());
        assert_eq!(
            reasons_of(&rows, "typo"),
            [RetirementReason::MissingInVendor]
        );
        assert_eq!(reasons_of(&rows, "orphan"), [RetirementReason::VendorOnly]);

        let single = rows
            .iter()
            .find(|row| row.flag_name == "single")
            .expect("row");
        assert_eq!(
            single.evidence[0].detail,
            "launchdarkly state off, serves one variation"
        );
        assert_eq!(
            single.vendor.as_ref().map(|v| v.key.as_str()),
            Some("web.single")
        );

        let orphan = rows
            .iter()
            .find(|row| row.flag_name == "orphan")
            .expect("row");
        assert_eq!(orphan.kind, RetirementFlagKind::VendorExport);
        assert!(orphan.sites.is_empty());
        assert_eq!(orphan.evidence[0].path, "flag-state.json");
        assert_eq!(orphan.evidence[0].line, 10, "the line of the orphan key");

        assert_eq!(state.source, "launchdarkly");
        assert_eq!(state.flags, 5);
        assert_eq!(state.export_age_days, Some(5));
    }

    #[test]
    fn a_narrowed_run_adds_no_vendor_only_rows() {
        let mut rows = vec![sdk_row("rolled", Some("LaunchDarkly"))];
        apply(&mut rows, false);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn a_key_that_the_code_reads_under_another_kind_is_not_vendor_only() {
        let mut rows = vec![RetirementFlag {
            kind: RetirementFlagKind::EnvironmentVariable,
            ..sdk_row("orphan", None)
        }];
        apply(&mut rows, true);
        assert_eq!(
            rows.len(),
            1 + 4,
            "the env row and four keys that no code reads"
        );
        assert!(
            reasons_of(&rows, "orphan").is_empty(),
            "an env row never matches the export"
        );
    }

    #[test]
    fn an_export_of_one_vendor_leaves_other_sdks_alone() {
        let mut rows = vec![
            sdk_row("typo", Some("LaunchDarkly")),
            sdk_row("other", Some("Statsig")),
            sdk_row("custom", None),
        ];
        apply(&mut rows, false);
        assert_eq!(
            reasons_of(&rows, "typo"),
            [RetirementReason::MissingInVendor]
        );
        assert!(reasons_of(&rows, "other").is_empty());
        assert_eq!(
            reasons_of(&rows, "custom"),
            [RetirementReason::MissingInVendor],
            "an SDK row without a label can belong to the vendor"
        );
    }

    #[test]
    fn an_export_of_an_unknown_vendor_matches_every_sdk_row() {
        let mut rows = vec![sdk_row("typo", Some("Statsig"))];
        let export = parse_flag_state(
            br#"{"schema_version":1,"source":"in-house","exported_at":"2026-09-20","flags":[]}"#,
            "state.json".to_string(),
        )
        .expect("valid");
        apply_vendor_state(
            &mut rows,
            &VendorMatch {
                export: &export,
                key_prefix: None,
                code_flag_names: &FxHashSet::default(),
                add_vendor_only: true,
                clock_epoch_secs: CLOCK,
            },
        );
        assert_eq!(
            reasons_of(&rows, "typo"),
            [RetirementReason::MissingInVendor]
        );
    }

    #[test]
    fn invalid_exports_are_rejected() {
        let cases: [(&str, &str); 6] = [
            ("not json", "is not valid"),
            (
                r#"{"schema_version":2,"source":"x","exported_at":"2026-01-01","flags":[]}"#,
                "schema_version 2",
            ),
            (
                r#"{"schema_version":1,"source":"x","exported_at":"yesterday","flags":[]}"#,
                "YYYY-MM-DD",
            ),
            (
                r#"{"schema_version":1,"source":"x","exported_at":"2026-01-01","flags":[{"key":"a","state":"paused"}]}"#,
                "unknown variant",
            ),
            (
                r#"{"schema_version":1,"source":"x","exported_at":"2026-01-01","flags":[{"key":"a","state":"on"},{"key":"a","state":"off"}]}"#,
                "more than one time",
            ),
            (
                r#"{"schema_version":1,"source":"x","exported_at":"2026-01-01","flags":[{"key":"a","state":"on","enabled":true}]}"#,
                "unknown field",
            ),
        ];
        for (input, expected) in cases {
            let error =
                parse_flag_state(input.as_bytes(), "state.json".to_string()).expect_err(input);
            assert!(
                error.message.contains(expected),
                "{input}: {}",
                error.message
            );
        }
    }

    #[test]
    fn an_oversized_export_is_rejected_before_parse() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.json");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_FLAG_STATE_BYTES + 1).expect("grow");
        let error = load_flag_state(&path, dir.path()).expect_err("too large");
        assert!(
            error.message.contains("larger than 16 MiB"),
            "{}",
            error.message
        );
    }

    #[test]
    fn the_display_path_is_relative_to_the_root_only_inside_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let inside = dir.path().join("state.json");
        std::fs::write(&inside, "{}").expect("write");
        assert_eq!(display_path(&inside, dir.path()), "state.json");
        let other = tempfile::tempdir().expect("temp dir");
        let outside = other.path().join("state.json");
        std::fs::write(&outside, "{}").expect("write");
        assert_eq!(
            display_path(&outside, dir.path()),
            outside.to_string_lossy().replace('\\', "/")
        );
    }

    #[test]
    fn a_prefix_that_is_the_whole_key_keeps_the_key() {
        assert_eq!(code_name("web.", Some("web.")), "web.");
        assert_eq!(code_name("web.a", Some("")), "web.a");
        assert_eq!(code_name("app.a", Some("web.")), "app.a");
    }
}
