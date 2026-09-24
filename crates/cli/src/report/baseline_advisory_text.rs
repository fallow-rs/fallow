//! One reader for the envelope's `baseline_staleness`, shared by every review
//! surface that states an advisory.
//!
//! The gate inventory line already tells a reviewer that `stale-baseline`
//! failed or stood down. It does not say what went stale, and the counts that
//! answer that are on the envelope: the step log and the job summary print them
//! and the sticky comment did not, so a repository whose baseline had rotted
//! saw it on the surface nobody opens and not on the one people read (#2675).
//!
//! The fact clauses are word for word the job summary's
//! (`action/scripts/summary.sh`), which is what a reader comparing the two
//! surfaces gets from having one advisory. The remedy clause is deliberately
//! channel-free: one Rust renderer serves GitHub and GitLab and cannot know
//! whether the reader re-saves through the `save-baseline` input,
//! `FALLOW_SAVE_BASELINE` or `--save-baseline`, so it names the run rather than
//! the knob. Do not "fix" that divergence by naming one channel here.
//!
//! The envelope arrives untyped, exactly as `fallow report --from` holds it,
//! and the live path routes its typed object through the same formatter so the
//! saved render stays byte-identical to the direct one.

use serde_json::Value;

/// The advisory for every loaded baseline that earned one, or `None` when the
/// run loaded no baseline or every baseline is fresh enough to say nothing.
///
/// Keyed on `warning` and `gate_trips`, the two members that exist so a
/// renderer does not infer which advisory applies from the counts. A
/// change-scoped run says nothing here on purpose: it cannot judge a
/// whole-project baseline, and both members are false by construction, which
/// is the stood-down verdict the gate inventory line already reports.
///
/// A multi-section envelope carries up to three baselines, so this reports one
/// sentence per site and names the analysis, rather than reporting the first
/// one's rot under the counts of whichever section came first.
pub fn advisory_line(envelope: &Value) -> Option<String> {
    let root = envelope.as_object()?;
    let sentences = fallow_types::envelope_sites::baseline_staleness_objects(root)
        .filter_map(|(staleness, analysis)| advisory_sentence(staleness, analysis))
        .collect::<Vec<_>>();
    if sentences.is_empty() {
        return None;
    }
    Some(sentences.join(" "))
}

/// [`advisory_line`] for a live run holding one staleness object typed.
///
/// Routed through the same formatter on purpose: `fallow report --from` must
/// render byte-identically to the direct `--format` run, which is a contract
/// with its own parity suite, so the live and saved paths cannot each format
/// the advisory their own way. The root site is unlabelled, which is where
/// `dead-code` and `dupes` publish and how `health`'s `summary` site is also
/// reported, so a single-baseline command renders one unqualified sentence.
pub fn advisory_line_for_staleness(
    staleness: Option<&fallow_output::BaselineStaleness>,
) -> Option<String> {
    let staleness = staleness?;
    advisory_line(&serde_json::json!({ "baseline_staleness": staleness }))
}

/// One sentence for one loaded baseline, or `None` when it earned no advisory.
fn advisory_sentence(staleness: &Value, analysis: Option<&str>) -> Option<String> {
    let warning = staleness
        .get("warning")
        .and_then(Value::as_str)
        .unwrap_or("none");
    let gate_trips = staleness
        .get("gate_trips")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let entries = count(staleness, "baseline_entries");
    let stale = count(staleness, "stale_entries");
    let subject = subject(analysis);
    // Checked before the advisory arms, and before the `gate_trips` fallback the
    // same file now reaches: its counts are all zero, so that arm would render
    // "0 of 0 saved entries matched nothing this run" next to a baseline whose
    // problem is that nothing read it. Word for word the job summary's line for a
    // baseline whose path the Action never saw. The summary names the file when it
    // has one. This renderer never can, because the envelope carries no path.
    if staleness
        .get("unrecognised_format")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if let Some(writer) = saved_by(staleness) {
            return Some(format!(
                "**{subject} recognises nothing.** `fallow {writer}` saved this baseline, so \
                 this command reads nothing from it and it suppresses nothing."
            ));
        }
        return Some(format!(
            "**{subject} recognises nothing.** The baseline has no entries this command \
             recognises. It may be a baseline saved by another command, or an empty file. Either \
             way it suppresses nothing."
        ));
    }
    match warning {
        "partial" => Some(format!(
            "**{subject} is partially stale.** {stale} of {entries} saved entries matched \
             nothing this run, so the baseline protects less than what was saved. Re-save it \
             from a whole-project run."
        )),
        "zero-overlap" => Some(format!(
            "**{subject} matched nothing.** All {entries} saved entries went unmatched. Paths \
             may have changed, or the baseline was saved elsewhere. Re-save it from a \
             whole-project run."
        )),
        // "none", and any advisory this build does not recognise: an unknown
        // value must not invent a sentence, but `gate_trips` is a separate
        // member and a run whose gate rule holds still has something to say.
        _ if gate_trips => Some(format!(
            "**{subject} has stale entries.** {stale} of {entries} saved entries matched \
             nothing this run. The project may be clean, or the baseline may no longer \
             describe it."
        )),
        _ => None,
    }
}

/// The command that saved a foreign baseline, from `saved_by`.
///
/// The value set is open, so any command-shaped token renders. A value that is
/// not a plain kebab-case token renders nothing, so the sentence falls back to
/// the hedged wording instead of quoting unexpected text into Markdown.
fn saved_by(staleness: &Value) -> Option<&str> {
    staleness
        .get("saved_by")
        .and_then(Value::as_str)
        .filter(|token| {
            !token.is_empty()
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        })
}

/// What the sentence calls the baseline it is about.
///
/// Unqualified on the single-analysis shapes, where the surface already names
/// the command that ran. Qualified on the multi-section ones, because "the
/// baseline" is ambiguous when a run loaded three.
fn subject(analysis: Option<&str>) -> String {
    let Some(analysis) = analysis else {
        return "Baseline".to_owned();
    };
    format!("{} baseline", capitalize(analysis))
}

/// Upper-case the first character, leaving the rest alone so `dead-code` reads
/// as `Dead-code` rather than losing its hyphen.
fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn count(staleness: &Value, key: &str) -> u64 {
    staleness.get(key).and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{BaselineScopeReasons, BaselineStaleness, BaselineStalenessAdvisory};

    fn staleness(warning: &str, entries: u64, matched: u64, gate_trips: bool) -> Value {
        serde_json::json!({
            "baseline_entries": entries,
            "matched_entries": matched,
            "stale_entries": entries - matched,
            "current_findings": 4,
            "change_scoped": false,
            "stale": warning != "none",
            "warning": warning,
            "gate_trips": gate_trips,
            "moved_entries": 0
        })
    }

    fn envelope(staleness: &Value) -> Value {
        serde_json::json!({ "kind": "dead-code", "baseline_staleness": staleness })
    }

    #[test]
    fn an_envelope_without_a_baseline_says_nothing() {
        assert!(advisory_line(&serde_json::json!({ "kind": "dead-code" })).is_none());
    }

    #[test]
    fn a_partially_stale_baseline_names_both_counts() {
        assert_eq!(
            advisory_line(&envelope(&staleness("partial", 8, 3, true)))
                .expect("a baseline was loaded"),
            "**Baseline is partially stale.** 5 of 8 saved entries matched nothing this run, \
             so the baseline protects less than what was saved. Re-save it from a whole-project \
             run."
        );
    }

    #[test]
    fn a_baseline_that_matched_nothing_says_so() {
        assert_eq!(
            advisory_line(&envelope(&staleness("zero-overlap", 8, 0, true)))
                .expect("a baseline was loaded"),
            "**Baseline matched nothing.** All 8 saved entries went unmatched. Paths may have \
             changed, or the baseline was saved elsewhere. Re-save it from a whole-project run."
        );
    }

    /// The case the advisory alone cannot report: a rotted baseline on a
    /// cleaned project produces no findings to compare, so `stale` stays false
    /// while the gate rule holds. Without this arm the surface that says the
    /// gate failed would carry no reason it did.
    #[test]
    fn a_tripped_gate_reports_even_with_no_advisory() {
        assert_eq!(
            advisory_line(&envelope(&staleness("none", 8, 0, true))).expect("the gate rule held"),
            "**Baseline has stale entries.** 8 of 8 saved entries matched nothing this run. \
             The project may be clean, or the baseline may no longer describe it."
        );
    }

    #[test]
    fn a_fresh_baseline_says_nothing() {
        assert!(advisory_line(&envelope(&staleness("none", 8, 8, false))).is_none());
    }

    /// A file this command could not read as its own now trips the gate, so
    /// without its own arm the `gate_trips` fallback would render "0 of 0 saved
    /// entries matched nothing this run" for a baseline whose problem is that
    /// nothing read it. Word for word the job summary's pathless line; the
    /// summary names the file when the Action published a path for it.
    #[test]
    fn a_baseline_nothing_recognises_gets_its_own_sentence() {
        let mut object = staleness("none", 0, 0, true);
        object["unrecognised_format"] = Value::Bool(true);
        assert_eq!(
            advisory_line(&envelope(&object)).expect("the file was not this command's"),
            "**Baseline recognises nothing.** The baseline has no entries this command \
             recognises. It may be a baseline saved by another command, or an empty file. Either \
             way it suppresses nothing."
        );
    }

    /// `saved_by` names the writer when the file names a known one. A value
    /// that is not a kebab-case token falls back to the hedged sentence.
    #[test]
    fn a_foreign_baseline_names_the_command_that_saved_it() {
        let mut object = staleness("none", 0, 0, true);
        object["unrecognised_format"] = Value::Bool(true);
        object["saved_by"] = Value::String("health".to_owned());
        assert_eq!(
            advisory_line(&envelope(&object)).expect("the file was not this command's"),
            "**Baseline recognises nothing.** `fallow health` saved this baseline, so this \
             command reads nothing from it and it suppresses nothing."
        );
        object["saved_by"] = Value::String("<b>x</b>".to_owned());
        assert!(
            advisory_line(&envelope(&object))
                .expect("the file was not this command's")
                .contains("It may be a baseline saved by another command"),
        );
    }

    /// The counts are all zero on such a file, so the arm has to be keyed on the
    /// member rather than on them: a baseline saved from a project that had
    /// nothing to record carries the same zeros and is not a mistake.
    #[test]
    fn an_empty_baseline_of_this_commands_own_says_nothing() {
        assert!(advisory_line(&envelope(&staleness("none", 0, 0, false))).is_none());
    }

    /// A multi-section run names which of its baselines was the wrong file.
    #[test]
    fn a_multi_section_envelope_names_the_baseline_nothing_recognises() {
        let mut object = staleness("none", 0, 0, true);
        object["unrecognised_format"] = Value::Bool(true);
        let value = serde_json::json!({
            "kind": "audit",
            "complexity": { "summary": { "baseline_staleness": object } }
        });
        assert!(
            advisory_line(&value)
                .expect("a baseline was loaded")
                .starts_with("**Complexity baseline recognises nothing.**"),
            "{value}"
        );
    }

    /// A narrowed run compares a whole-project baseline against a slice of it,
    /// so both members are false by construction and the advisory must not
    /// invent rot the run could not measure.
    #[test]
    fn a_change_scoped_run_says_nothing() {
        let mut object = staleness("none", 8, 0, false);
        object["change_scoped"] = Value::Bool(true);
        assert!(advisory_line(&envelope(&object)).is_none());
    }

    /// An advisory value from a newer build must not produce an invented
    /// sentence, and must not silence the gate rule either.
    #[test]
    fn an_unrecognised_advisory_falls_back_to_the_gate_rule() {
        let mut object = staleness("none", 8, 2, true);
        object["warning"] = Value::String("some-future-advisory".to_owned());
        assert!(advisory_line(&envelope(&object)).is_some());

        object["gate_trips"] = Value::Bool(false);
        assert!(advisory_line(&envelope(&object)).is_none());
    }

    #[test]
    fn health_publishes_inside_summary_and_reads_the_same() {
        let value = serde_json::json!({
            "kind": "health",
            "summary": { "baseline_staleness": staleness("partial", 4, 1, true) }
        });
        assert!(
            advisory_line(&value)
                .expect("a baseline was loaded")
                .starts_with("**Baseline is partially stale.**")
        );
    }

    /// A run that loaded three baselines must not report the first one's rot
    /// under another's counts, so each sentence names its own analysis.
    #[test]
    fn a_multi_section_envelope_names_each_baseline() {
        let value = serde_json::json!({
            "kind": "audit",
            "dead_code": { "baseline_staleness": staleness("zero-overlap", 3, 0, true) },
            "complexity": {
                "summary": { "baseline_staleness": staleness("partial", 8, 2, true) }
            }
        });

        let line = advisory_line(&value).expect("two baselines were loaded");

        assert!(
            line.contains("**Dead-code baseline matched nothing.**"),
            "{line}"
        );
        assert!(
            line.contains("**Complexity baseline is partially stale.**"),
            "{line}"
        );
    }

    /// The live path holds the object typed and the saved path reads it off the
    /// envelope. They must produce the same string, or `report --from` stops
    /// being byte-identical to a direct render.
    #[test]
    fn the_live_and_saved_advisories_agree() {
        for (warning, matched, gate_trips, unrecognised_format) in [
            (BaselineStalenessAdvisory::Partial, 3, true, false),
            (BaselineStalenessAdvisory::ZeroOverlap, 0, true, false),
            (BaselineStalenessAdvisory::None, 0, true, false),
            (BaselineStalenessAdvisory::None, 8, false, false),
            (BaselineStalenessAdvisory::None, 8, true, true),
        ] {
            let typed = BaselineStaleness {
                baseline_entries: 8,
                matched_entries: matched,
                stale_entries: 8 - matched,
                current_findings: 4,
                change_scoped: false,
                stale: !matches!(warning, BaselineStalenessAdvisory::None),
                warning,
                gate_trips,
                moved_entries: 0,
                unrecognised_format,
                saved_by: None,
                scope_reasons: BaselineScopeReasons::empty(),
            };
            let envelope = serde_json::json!({ "baseline_staleness": typed });

            assert_eq!(
                advisory_line_for_staleness(Some(&typed)),
                advisory_line(&envelope),
                "{warning:?} must render the same from both paths"
            );
        }
    }

    #[test]
    fn a_live_run_without_a_baseline_says_nothing() {
        assert!(advisory_line_for_staleness(None).is_none());
    }
}
