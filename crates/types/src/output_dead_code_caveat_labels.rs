//! The compact caveat labels that one-line human renderers append to a
//! finding.

use super::ReachabilityCaveat;

/// The compact labels of `caveats`, joined for a one-line renderer, or `None`
/// when there is nothing to say.
#[must_use]
pub fn caveat_labels(caveats: &[ReachabilityCaveat]) -> Option<String> {
    if caveats.is_empty() {
        return None;
    }
    let labels: Vec<&str> = caveats
        .iter()
        .map(|caveat| ReachabilityCaveat::short_label(*caveat))
        .collect();
    Some(labels.join(", "))
}

/// The compact parenthetical a one-line human renderer appends to a finding
/// carrying `caveats`, or `None` when there is nothing to say. Shared by every
/// dead-code section so the suffix reads the same everywhere.
#[must_use]
pub fn caveat_suffix(caveats: &[ReachabilityCaveat]) -> Option<String> {
    caveat_labels(caveats).map(|labels| format!("{CAVEAT_SUFFIX_MARKER}{labels})"))
}

/// The opening of the parenthetical [`caveat_suffix`] renders. Public because
/// one consumer can only see the rendered description: the CI review formats
/// build their comments from CodeClimate issues, whose `description` is the
/// only place the caveat survives (the CodeClimate wire is a published
/// contract with no field for it). Recognising the marker is what lets those
/// formats withhold a one-click mutation.
pub const CAVEAT_SUFFIX_MARKER: &str = " (caveat: ";

/// Whether a rendered finding description already carries a caveat
/// parenthetical, for a surface holding the string rather than the typed
/// finding.
///
/// Lives here, next to the renderer, so the producer and the recogniser cannot
/// drift; `a_rendered_suffix_is_recognised_by_the_marker` pins the pair.
#[must_use]
pub fn description_carries_caveat(description: &str) -> bool {
    description.contains(CAVEAT_SUFFIX_MARKER)
}

/// The compact label for one wire token, for a renderer that reads
/// `reachability_caveats[]` back off a serialized envelope instead of holding
/// the typed findings.
///
/// The value set is OPEN, exactly as the wire documentation says: a token this
/// build does not recognise is still a caveat, so it is rendered as itself with
/// its separators relaxed into spaces rather than dropped. Dropping it would
/// turn a finding whose evidence is incomplete back into a confident one,
/// which is the failure this whole mechanism exists to prevent.
#[must_use]
pub fn caveat_label_for_token(token: &str) -> String {
    match token {
        "incomplete-file-analysis" => {
            ReachabilityCaveat::short_label(ReachabilityCaveat::IncompleteFileAnalysis).to_owned()
        }
        "incomplete-import-graph" => {
            ReachabilityCaveat::short_label(ReachabilityCaveat::IncompleteImportGraph).to_owned()
        }
        other => other.replace('-', " "),
    }
}

/// The joined compact labels for wire tokens, or `None` when there are none.
/// The token-side twin of [`caveat_labels`], for renderers driven by a
/// serialized envelope rather than by typed findings.
#[must_use]
pub fn caveat_labels_for_tokens<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let labels: Vec<String> = tokens.into_iter().map(caveat_label_for_token).collect();
    if labels.is_empty() {
        return None;
    }
    Some(labels.join(", "))
}

/// The token-side twin of [`caveat_suffix`], so an envelope-driven renderer
/// appends the same parenthetical as a findings-driven one.
#[must_use]
pub fn caveat_suffix_for_tokens<'a>(tokens: impl IntoIterator<Item = &'a str>) -> Option<String> {
    caveat_labels_for_tokens(tokens).map(|labels| format!("{CAVEAT_SUFFIX_MARKER}{labels})"))
}
