//! The clause a rendered CI body carries when the target cannot group.
//!
//! `--group-by` is carried by the JSON, human, SARIF and CodeClimate targets
//! and by no other. Until issue #2691 the remaining targets rendered a flat
//! document and said so on stderr for three of them and nowhere at all for the
//! six that a CI integration actually uses, so a consumer that asked for groups
//! received a document that is valid, complete and not what it asked for.
//!
//! Deliberately not a `request_outcomes` member: the fallback is decided at
//! RENDER time, and on `--format json`, the only format with an envelope,
//! grouping is supported. An entry for it would be emitted precisely never.
//! The fact can only live where it is produced: the stderr note and the
//! rendered body.

/// One sentence for a body whose target dropped the requested grouping.
///
/// The caller decides whether to render it at all, so a run that asked for no
/// grouping keeps a body byte-identical to one produced before this clause
/// existed.
///
/// `fallow report --from` reaches the same function with the mode taken from
/// the resolver it built for the saved envelope, rather than by reading
/// `grouped_by` back off the envelope: the saved grouped dead-code envelope is
/// flattened before rendering and loses that field, while its resolver carries
/// the same label the live render uses. Live and saved bodies must be
/// byte-identical for one envelope, which `the_live_and_saved_notes_agree`
/// pins.
pub fn dropped_grouping_clause(mode: &str) -> String {
    format!(
        "Grouping: --group-by {mode} was requested and this format renders one flat document; \
         run --format json for the grouped envelope."
    )
}
