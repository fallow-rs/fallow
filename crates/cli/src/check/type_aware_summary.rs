//! The human status line of the type-aware pass.

pub(super) fn format_type_aware_summary(meta: &fallow_types::envelope::TypeAwareMeta) -> String {
    let candidates = count_noun(meta.candidate_count, "candidate", "candidates");
    let confirmed = count_noun(meta.confirmed_used_count, "confirmed use", "confirmed uses");
    let contracts = count_noun(
        meta.contract_preserved_count,
        "preserved contract",
        "preserved contracts",
    );
    let no_static_references = count_noun(
        meta.no_static_references_count,
        "candidate without static references",
        "candidates without static references",
    );
    let fixable = count_noun(meta.fix_eligible_count, "guarded fix", "guarded fixes");
    let unresolved = count_noun(
        meta.unresolved_count,
        "unresolved finding",
        "unresolved findings",
    );
    let abstained = count_noun(
        meta.abstained_count,
        "abstained finding",
        "abstained findings",
    );
    let summary = format!(
        "Type-aware refinement: {candidates}, {confirmed}, {contracts}, \
         {no_static_references} ({fixable}), {unresolved}, {abstained}"
    );
    match &meta.call_evidence {
        Some(evidence) => {
            let checked = count_noun(
                evidence.default_candidates + evidence.method_candidates,
                "constant-parameter candidate",
                "constant-parameter candidates",
            );
            let proved = count_noun(evidence.findings, "finding", "findings");
            format!("{summary}; {checked} checked, {proved} with type evidence")
        }
        None => summary,
    }
}

fn count_noun(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}
