//! The `cloud` evidence section of `fallow inspect --file <path> --cloud`.
//!
//! The section joins the functions of the local file with the production
//! data that Fallow Cloud holds for the deployed file. Local functions come
//! from the same inventory walk that `fallow coverage upload-inventory` runs,
//! so a local function and its deployed function share one `stable_id` when
//! the function did not move. The pairing:
//!
//! 1. `stable_id` (the protocol recipe over the repo-relative path, the name
//!    and the start line);
//! 2. else the name plus the ordinal among the same-name functions of the
//!    file, when both sides have the same count of that name. This is the
//!    rule of the cloud for a function that moved to another line.
//!
//! A local function without a pair has no production data: it is new, or it
//! moved in a way that the rules above cannot follow. The section never
//! guesses a pair by name alone, because that gives wrong numbers.

use std::path::Path;

use fallow_cov_protocol::function_identity_id;
use fallow_output::{
    InspectCloudErrors, InspectCloudEvidence, InspectCloudFunction, InspectCloudMatch,
    InspectCloudStatus, InspectCloudSummary, InspectCloudUnpairedFunction,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::coverage::credentials;
use crate::coverage::file_context::{
    FileContext, FileContextFailure, FileContextFunction, fetch_file_context, resolve_cloud_repo,
};

/// Note on a local function without production data.
pub const UNPAIRED_NOTE: &str = "no production data (new or moved since deploy)";
/// Note on a local function without a pair when the cloud withheld the
/// never_called rows. The function can be deployed and tracked.
pub const WITHHELD_NOTE: &str =
    "no pair in this read (Fallow Cloud withheld never_called rows; not actionable yet)";
/// Message of the `no_key` status.
const NO_KEY_MESSAGE: &str =
    "no Fallow Cloud API key. Run `fallow cloud login`, or set FALLOW_API_KEY.";

/// One function of the local file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFunction {
    pub name: String,
    pub line: u32,
    pub stable_id: String,
}

/// The functions of the local file, in the shape of the uploaded inventory.
pub fn local_functions(root: &Path, file: &str) -> Vec<LocalFunction> {
    let Ok(source) = std::fs::read_to_string(root.join(file)) else {
        return Vec::new();
    };
    let mut seen: FxHashSet<(String, u32)> = FxHashSet::default();
    let mut functions: Vec<LocalFunction> =
        fallow_engine::source::inventory::walk_source(Path::new(file), &source)
            .into_iter()
            .filter(|entry| seen.insert((entry.name.clone(), entry.line)))
            .map(|entry| LocalFunction {
                stable_id: function_identity_id(file, &entry.name, entry.line),
                name: entry.name,
                line: entry.line,
            })
            .collect();
    functions.sort_by(|a, b| a.line.cmp(&b.line).then_with(|| a.name.cmp(&b.name)));
    functions
}

/// Build the `cloud` section for the file target `file` (relative to `root`).
pub fn collect_cloud_section(root: &Path, file: &str, repo: Option<&str>) -> InspectCloudEvidence {
    let repo = resolve_cloud_repo(repo, root);
    let Some(key) = credentials::resolve_api_key(None, None) else {
        return InspectCloudEvidence::status_only(
            InspectCloudStatus::NoKey,
            NO_KEY_MESSAGE.to_owned(),
            repo.ok(),
        );
    };
    let repo = match repo {
        Ok(repo) => repo,
        Err(message) => {
            return InspectCloudEvidence::status_only(
                InspectCloudStatus::Unavailable,
                message,
                None,
            );
        }
    };
    match fetch_file_context(&key.key, &repo, file) {
        Ok(context) => build_section(&repo, &context, &local_functions(root, file)),
        Err(failure) => failure_section(failure, repo),
    }
}

fn failure_section(failure: FileContextFailure, repo: String) -> InspectCloudEvidence {
    let (status, message) = match failure {
        FileContextFailure::KeyRefused(message) => (InspectCloudStatus::NoKey, message),
        FileContextFailure::PlanRequired(message) => (InspectCloudStatus::PlanRequired, message),
        FileContextFailure::Unavailable(message) => (InspectCloudStatus::Unavailable, message),
    };
    InspectCloudEvidence::status_only(status, message, Some(repo))
}

/// Build the section from a cloud answer and the local functions.
pub fn build_section(
    repo: &str,
    context: &FileContext,
    local: &[LocalFunction],
) -> InspectCloudEvidence {
    let summary = InspectCloudSummary {
        functions: context.summary.functions,
        called: context.summary.called,
        never_called: context.summary.never_called,
        untracked: context.summary.untracked,
        calls: context.summary.calls,
        error_groups: context.summary.error_groups,
        error_events: context.summary.error_events,
        text: context.summary.text.clone(),
    };
    let repo = if context.repo.is_empty() {
        repo.to_owned()
    } else {
        context.repo.clone()
    };
    let mut section = InspectCloudEvidence {
        status: InspectCloudStatus::Ok,
        message: None,
        repo: Some(repo),
        latest_sha: context.latest_sha.clone(),
        period_days: context.period_days,
        summary: Some(summary),
        functions: Vec::new(),
        unpaired_local: Vec::new(),
    };
    let withheld = context
        .never_called_withheld
        .then(|| withheld_message(context.actionability_reason.as_deref()));
    if context.state == "untracked" {
        section.status = InspectCloudStatus::Untracked;
        section.message = Some(match withheld {
            Some(message) => message,
            None if context.summary.text.is_empty() => {
                "Fallow Cloud has no production data for this file".to_owned()
            }
            None => context.summary.text.clone(),
        });
        return section;
    }
    let truncated = context.truncation.as_ref().map(|truncation| {
        format!(
            "Fallow Cloud left out {} functions to fit its size limit; a local function without a pair can still have production data",
            truncation.omitted_functions
        )
    });
    let messages: Vec<String> = withheld.iter().chain(truncated.iter()).cloned().collect();
    if !messages.is_empty() {
        section.message = Some(messages.join("; "));
    }
    let unpaired_note = if context.never_called_withheld {
        WITHHELD_NOTE
    } else {
        UNPAIRED_NOTE
    };
    let pairs = pair_functions(local, &context.functions);
    for (local_fn, pair) in local.iter().zip(pairs) {
        match pair {
            Some((index, match_kind)) => section.functions.push(paired_function(
                local_fn,
                &context.functions[index],
                match_kind,
            )),
            None => section.unpaired_local.push(InspectCloudUnpairedFunction {
                name: local_fn.name.clone(),
                line: local_fn.line,
                note: unpaired_note.to_owned(),
            }),
        }
    }
    section
}

/// The section message when the cloud withheld the never_called rows.
fn withheld_message(reason: Option<&str>) -> String {
    let reason = reason
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("the read is not actionable yet");
    format!(
        "Fallow Cloud withheld the never_called rows ({reason}); a local function without a pair can still be deployed and tracked"
    )
}

fn paired_function(
    local: &LocalFunction,
    cloud: &FileContextFunction,
    match_kind: InspectCloudMatch,
) -> InspectCloudFunction {
    let errors = cloud
        .errors
        .as_ref()
        .filter(|errors| errors.state == "tracked")
        .and_then(|errors| {
            Some(InspectCloudErrors {
                groups: errors.groups?,
                events: errors.events?,
            })
        });
    let cpu_ms_per_hour = cloud
        .cpu
        .as_ref()
        .filter(|cpu| cpu.state == "tracked")
        .and_then(|cpu| cpu.self_ms_per_hour);
    let never_run_ranges = cloud
        .branches
        .as_ref()
        .filter(|branches| branches.state == "observed")
        .and_then(|branches| branches.ranges_never_run);
    InspectCloudFunction {
        name: local.name.clone(),
        line: local.line,
        stable_id: cloud.stable_id.clone(),
        match_kind,
        state: cloud.state.clone(),
        period_state: cloud.period_state.clone(),
        calls: if cloud.state == "untracked" {
            None
        } else {
            cloud.calls
        },
        errors,
        cpu_ms_per_hour,
        never_run_ranges,
        summary: cloud.summary.clone(),
    }
}

/// Pair each local function with at most one cloud function. The result has
/// one entry per local function: the index of the cloud function and how it
/// was paired, or `None`.
pub fn pair_functions(
    local: &[LocalFunction],
    cloud: &[FileContextFunction],
) -> Vec<Option<(usize, InspectCloudMatch)>> {
    let mut result: Vec<Option<(usize, InspectCloudMatch)>> = vec![None; local.len()];
    let mut cloud_taken = vec![false; cloud.len()];

    let local_keys = ordinal_keys(local.iter().map(|f| (f.name.as_str(), Some(f.line))));
    let cloud_keys = ordinal_keys(cloud.iter().map(|f| (f.name.as_str(), f.line)));

    let mut by_stable_id: FxHashMap<&str, usize> = FxHashMap::default();
    for (index, function) in cloud.iter().enumerate() {
        if let Some(stable_id) = function.stable_id.as_deref() {
            by_stable_id.entry(stable_id).or_insert(index);
        }
    }
    for (local_index, function) in local.iter().enumerate() {
        if let Some(&cloud_index) = by_stable_id.get(function.stable_id.as_str())
            && !cloud_taken[cloud_index]
            && !is_line_collision(local_keys[local_index], cloud_keys[cloud_index])
        {
            cloud_taken[cloud_index] = true;
            result[local_index] = Some((cloud_index, InspectCloudMatch::StableId));
        }
    }

    let mut cloud_by_key: FxHashMap<(&str, usize, usize), usize> = FxHashMap::default();
    for (index, key) in cloud_keys.iter().enumerate() {
        if let Some(key) = key
            && !cloud_taken[index]
        {
            cloud_by_key.insert(*key, index);
        }
    }
    for (local_index, key) in local_keys.iter().enumerate() {
        if result[local_index].is_some() {
            continue;
        }
        let Some(key) = key else { continue };
        if let Some(cloud_index) = cloud_by_key.remove(key) {
            cloud_taken[cloud_index] = true;
            result[local_index] = Some((cloud_index, InspectCloudMatch::NameOrdinal));
        }
    }
    result
}

/// True when a stable id match on one line pairs two different functions: a
/// moved function landed on the old line of another same-name function. Both
/// sides have the same count of same-name functions and the ordinals differ.
/// Fallow Cloud drops this match too (`isLineCollision`), so the name ordinal
/// step pairs these functions.
fn is_line_collision(
    local: Option<(&str, usize, usize)>,
    cloud: Option<(&str, usize, usize)>,
) -> bool {
    match (local, cloud) {
        (
            Some((local_name, local_ordinal, local_count)),
            Some((cloud_name, cloud_ordinal, cloud_count)),
        ) => {
            local_name == cloud_name && local_count == cloud_count && local_ordinal != cloud_ordinal
        }
        _ => false,
    }
}

/// `(name, ordinal among the same-name functions by line, count of those
/// functions)` per function. A function without a line has no key.
fn ordinal_keys<'a>(
    functions: impl Iterator<Item = (&'a str, Option<u32>)>,
) -> Vec<Option<(&'a str, usize, usize)>> {
    let functions: Vec<(&str, Option<u32>)> = functions.collect();
    let mut by_name: FxHashMap<&str, Vec<(u32, usize)>> = FxHashMap::default();
    for (index, (name, line)) in functions.iter().enumerate() {
        if let Some(line) = line {
            by_name.entry(name).or_default().push((*line, index));
        }
    }
    let mut keys = vec![None; functions.len()];
    for (name, mut entries) in by_name {
        entries.sort_unstable();
        let count = entries.len();
        for (ordinal, (_, index)) in entries.into_iter().enumerate() {
            keys[index] = Some((name, ordinal, count));
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, line: u32) -> LocalFunction {
        LocalFunction {
            name: name.to_owned(),
            line,
            stable_id: function_identity_id("src/a.ts", name, line),
        }
    }

    fn cloud(name: &str, line: Option<u32>) -> FileContextFunction {
        FileContextFunction {
            name: name.to_owned(),
            line,
            stable_id: line.map(|line| function_identity_id("src/a.ts", name, line)),
            state: "called".to_owned(),
            period_state: Some("called".to_owned()),
            calls: Some(1),
            errors: None,
            cpu: None,
            branches: None,
            summary: String::new(),
        }
    }

    #[test]
    fn stable_id_pairs_before_the_name_ordinal() {
        let pairs = pair_functions(
            &[local("a", 1), local("b", 5)],
            &[cloud("b", Some(9)), cloud("a", Some(1))],
        );
        assert_eq!(pairs[0], Some((1, InspectCloudMatch::StableId)));
        assert_eq!(pairs[1], Some((0, InspectCloudMatch::NameOrdinal)));
    }

    #[test]
    fn a_stable_id_on_the_old_line_of_another_function_is_not_a_pair() {
        // Deployed: cb@2 (A), cb@10 (B). Local: A moved to line 10, B to line 18.
        // The stable id of local cb@10 equals the one of deployed B, but the
        // ordinals differ, so the name ordinal step pairs A with A and B with B.
        let pairs = pair_functions(
            &[local("cb", 10), local("cb", 18)],
            &[cloud("cb", Some(2)), cloud("cb", Some(10))],
        );
        assert_eq!(pairs[0], Some((0, InspectCloudMatch::NameOrdinal)));
        assert_eq!(pairs[1], Some((1, InspectCloudMatch::NameOrdinal)));
    }

    #[test]
    fn a_changed_same_name_count_gives_no_pair() {
        let pairs = pair_functions(&[local("cb", 3), local("cb", 7)], &[cloud("cb", Some(10))]);
        assert_eq!(pairs, vec![None, None]);
    }

    #[test]
    fn same_name_functions_pair_by_ordinal() {
        let pairs = pair_functions(
            &[local("cb", 3), local("cb", 7)],
            &[cloud("cb", Some(12)), cloud("cb", Some(4))],
        );
        assert_eq!(pairs[0], Some((1, InspectCloudMatch::NameOrdinal)));
        assert_eq!(pairs[1], Some((0, InspectCloudMatch::NameOrdinal)));
    }

    #[test]
    fn a_cloud_function_without_a_line_never_pairs_by_name() {
        let pairs = pair_functions(&[local("a", 3)], &[cloud("a", None)]);
        assert_eq!(pairs, vec![None]);
    }

    #[test]
    fn a_cloud_cut_is_named_in_the_message() {
        let body = r#"{"repo":"a/b","state":"observed","latest_sha":"s","period_days":30,"summary":{"functions":2,"called":2,"never_called":0,"untracked":0,"calls":2,"error_groups":null,"error_events":null,"text":"t"},"functions":[],"truncation":{"max_bytes":1000,"original_bytes":2000,"omitted_functions":2,"hint":"h"}}"#;
        let context: FileContext = serde_json::from_str(body).unwrap();
        let section = build_section("a/b", &context, &[local("a", 1)]);
        assert_eq!(section.status, InspectCloudStatus::Ok);
        assert!(section.message.unwrap().contains("left out 2 functions"));
        assert_eq!(section.unpaired_local.len(), 1);
    }

    #[test]
    fn withheld_never_called_rows_change_the_note_and_the_message() {
        let body = r#"{"repo":"a/b","state":"observed","latest_sha":"s","period_days":30,"actionable":false,"actionability_reason":"deployment warm-up","never_called_withheld":true,"summary":{"functions":1,"called":1,"never_called":0,"untracked":0,"calls":2,"error_groups":null,"error_events":null,"text":"t"},"functions":[]}"#;
        let context: FileContext = serde_json::from_str(body).unwrap();
        let section = build_section("a/b", &context, &[local("a", 1)]);
        assert_eq!(section.status, InspectCloudStatus::Ok);
        let message = section.message.unwrap();
        assert!(message.contains("withheld"), "{message}");
        assert!(message.contains("deployment warm-up"), "{message}");
        assert_eq!(section.unpaired_local[0].note, WITHHELD_NOTE);
        assert_ne!(section.unpaired_local[0].note, UNPAIRED_NOTE);
    }

    #[test]
    fn the_period_state_reaches_the_section() {
        let mut deployed = cloud("a", Some(1));
        deployed.state = "never_called".to_owned();
        let context = FileContext {
            repo: "a/b".to_owned(),
            state: "observed".to_owned(),
            latest_sha: None,
            period_days: Some(30),
            summary: serde_json::from_str(r#"{"text":"t"}"#).unwrap(),
            functions: vec![deployed],
            truncation: None,
            never_called_withheld: false,
            actionability_reason: None,
        };
        let section = build_section("a/b", &context, &[local("a", 1)]);
        assert_eq!(section.functions[0].period_state.as_deref(), Some("called"));
    }

    #[test]
    fn one_cloud_function_pairs_at_most_once() {
        let mut duplicate = cloud("a", Some(1));
        duplicate.name = "a".to_owned();
        let pairs = pair_functions(&[local("a", 1)], &[cloud("a", Some(1)), duplicate]);
        assert_eq!(pairs, vec![Some((0, InspectCloudMatch::StableId))]);
    }
}
