use super::*;

/// The token-side helpers must render the same words as the typed ones, so
/// one finding reads identically whether a surface holds the findings or
/// re-reads them off a serialized envelope.
#[test]
fn token_labels_match_the_typed_labels() {
    let typed = [
        ReachabilityCaveat::IncompleteFileAnalysis,
        ReachabilityCaveat::IncompleteImportGraph,
    ];
    let tokens: Vec<&str> = typed.iter().map(|c| c.token()).collect();

    assert_eq!(
        caveat_labels_for_tokens(tokens.iter().copied()),
        caveat_labels(&typed)
    );
    assert_eq!(
        caveat_suffix_for_tokens(tokens.iter().copied()),
        caveat_suffix(&typed)
    );
}

#[test]
fn no_tokens_means_nothing_to_say() {
    assert_eq!(caveat_labels_for_tokens(std::iter::empty()), None);
    assert_eq!(caveat_suffix_for_tokens(std::iter::empty()), None);
}

/// The CI review formats can only see the rendered description, so the
/// recogniser and the renderer have to stay one pair. A description with
/// no caveat must not match, or the review formats would withhold the
/// suggestion block on every finding in a clean run.
#[test]
fn a_rendered_suffix_is_recognised_by_the_marker() {
    for caveats in [
        &[ReachabilityCaveat::IncompleteImportGraph][..],
        &[
            ReachabilityCaveat::IncompleteFileAnalysis,
            ReachabilityCaveat::IncompleteImportGraph,
        ][..],
    ] {
        let suffix = caveat_suffix(caveats).expect("a caveat renders a suffix");
        assert!(
            description_carries_caveat(&format!("Something is never referenced{suffix}")),
            "the marker must match what caveat_suffix writes: {suffix}"
        );
    }
    assert!(
        description_carries_caveat(&format!(
            "Something is never referenced{}",
            caveat_suffix_for_tokens(["some-future-cause"]).expect("token suffix")
        )),
        "the token-side renderer writes the same marker"
    );
    assert!(
        !description_carries_caveat("Class member 'Widget.helper' is never referenced"),
        "a clean description must not read as caveated"
    );
}

/// A caveat is a RUN-level condition covering several ways a file goes
/// unread: a degraded parse, an unreadable file, and three kinds of file
/// discovery skipped before opening. A message naming only the parse case
/// told a reader whose run was degraded by the size guard to go fix parse
/// errors that do not exist, which is the same overclaiming the caveat
/// itself exists to prevent.
#[test]
fn no_caveat_message_names_a_single_cause() {
    for caveat in [
        ReachabilityCaveat::IncompleteFileAnalysis,
        ReachabilityCaveat::IncompleteImportGraph,
    ] {
        let message = caveat.message();
        assert!(
            !message.contains("parse cleanly") && !message.contains("parse error"),
            "{} names the parse cause alone, but a size-skipped or unreadable \
                 file reaches the same caveat: {message}",
            caveat.token()
        );
        assert!(
            message.contains("workspace_diagnostics"),
            "{} must point at the list that names the actual files: {message}",
            caveat.token()
        );
    }
}

/// The value set is open. A token a consumer build does not recognise still
/// means the evidence is incomplete, so it must survive into the rendered
/// hedge rather than being dropped back into a confident-looking finding.
#[test]
fn an_unrecognised_token_still_renders_as_a_caveat() {
    let suffix = caveat_suffix_for_tokens(["some-future-cause"])
        .expect("an unknown token is still a caveat");

    assert_eq!(suffix, " (caveat: some future cause)");
}
