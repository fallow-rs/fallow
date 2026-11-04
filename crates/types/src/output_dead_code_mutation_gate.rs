use super::*;
use crate::extract::MemberKind;
use crate::results::DependencyLocation;
use std::path::PathBuf;

const BOTH: [ReachabilityCaveat; 2] = [
    ReachabilityCaveat::IncompleteFileAnalysis,
    ReachabilityCaveat::IncompleteImportGraph,
];

fn export(name: &str) -> UnusedExport {
    UnusedExport {
        path: PathBuf::from("/p/src/mod.ts"),
        export_name: name.to_string(),
        is_type_only: false,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn member(name: &str) -> UnusedMember {
    UnusedMember {
        path: PathBuf::from("/p/src/mod.ts"),
        parent_name: "Color".to_string(),
        member_name: name.to_string(),
        kind: MemberKind::EnumMember,
        line: 2,
        col: 2,
    }
}

fn class_member(name: &str) -> UnusedMember {
    UnusedMember {
        parent_name: "Widget".to_string(),
        kind: MemberKind::ClassMethod,
        ..member(name)
    }
}

fn store_member(name: &str) -> UnusedMember {
    UnusedMember {
        parent_name: "useCounterStore".to_string(),
        kind: MemberKind::StoreMember,
        ..member(name)
    }
}

fn dependency(name: &str) -> UnusedDependency {
    UnusedDependency {
        package_name: name.to_string(),
        location: DependencyLocation::Dependencies,
        path: PathBuf::from("/p/package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    }
}

/// One finding type: its name, the uncaveated finding, and the same
/// finding after the annotation pass stamped a caveat on it.
type GatedPair = (&'static str, Box<dyn Gated>, Box<dyn Gated>);

/// Every caveated finding type, boxed behind the one question the mutation
/// surfaces ask.
fn every_finding_type() -> Vec<GatedPair> {
    fn pair<T: Gated + Clone + 'static>(name: &'static str, clean: T) -> GatedPair {
        let mut caveated = clean.clone();
        caveated.stamp(BOTH.to_vec());
        (name, Box::new(clean), Box::new(caveated))
    }
    vec![
        pair(
            "unused_files",
            UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/p/src/orphan.ts"),
            }),
        ),
        pair(
            "unused_exports",
            UnusedExportFinding::with_actions(export("helper")),
        ),
        pair(
            "unused_types",
            UnusedTypeFinding::with_actions(export("Shape")),
        ),
        pair(
            "unused_enum_members",
            UnusedEnumMemberFinding::with_actions(member("Blue")),
        ),
        pair(
            "unused_class_members",
            UnusedClassMemberFinding::with_actions(class_member("legacyMethod")),
        ),
        pair(
            "unused_store_members",
            UnusedStoreMemberFinding::with_actions(store_member("onlyUsedInBigFile")),
        ),
        pair(
            "unused_dependencies",
            UnusedDependencyFinding::with_actions(dependency("lodash")),
        ),
        pair(
            "unused_dev_dependencies",
            UnusedDevDependencyFinding::with_actions(dependency("vitest")),
        ),
        pair(
            "unused_optional_dependencies",
            UnusedOptionalDependencyFinding::with_actions(dependency("fsevents")),
        ),
    ]
}

/// Erases the finding type down to what a mutation surface needs: the gate,
/// the actions it gates, and the annotation-pass write.
trait Gated {
    fn actions(&self) -> &[IssueAction];
    fn gate_allows_mutation(&self) -> bool;
    fn stamp(&mut self, caveats: Vec<ReachabilityCaveat>);
}

impl<T: MutationEvidence + CaveatedFinding + HasActions> Gated for T {
    fn actions(&self) -> &[IssueAction] {
        HasActions::actions(self)
    }
    fn gate_allows_mutation(&self) -> bool {
        self.may_auto_apply_mutation()
    }
    fn stamp(&mut self, caveats: Vec<ReachabilityCaveat>) {
        self.set_reachability_caveats(caveats);
    }
}

trait HasActions {
    fn actions(&self) -> &[IssueAction];
}

macro_rules! has_actions {
        ($($ty:ty),+ $(,)?) => { $( impl HasActions for $ty {
            fn actions(&self) -> &[IssueAction] { &self.actions }
        } )+ };
    }
has_actions!(
    UnusedFileFinding,
    UnusedExportFinding,
    UnusedTypeFinding,
    UnusedEnumMemberFinding,
    UnusedClassMemberFinding,
    UnusedStoreMemberFinding,
    UnusedDependencyFinding,
    UnusedDevDependencyFinding,
    UnusedOptionalDependencyFinding,
);

/// THE property. Not "the CLI withholds it" or "the LSP hides it": no
/// finding whose evidence the run itself flagged may advertise an
/// automatically applicable mutation, whichever surface is reading.
#[test]
fn every_auto_fixable_dead_code_mutation_is_gated() {
    for (name, _clean, caveated) in every_finding_type() {
        assert!(
            !caveated.gate_allows_mutation(),
            "{name}: a stamped finding must fail the gate"
        );
        for action in caveated.actions() {
            assert!(
                !action.is_auto_fixable(),
                "{name}: a caveated finding still advertises an auto-fixable action, so an \
                     agent following the documented actions contract would plan a removal \
                     `fallow fix` refuses"
            );
        }
    }
}

/// The other half, and the one a blunt fix breaks: the gate must not turn
/// every finding into a manual one. A run that read every file it
/// discovered keeps exactly the behavior it had.
#[test]
fn an_uncaveated_finding_keeps_its_auto_fix() {
    let auto_fixable_types = [
        "unused_exports",
        "unused_types",
        "unused_enum_members",
        "unused_dependencies",
        "unused_dev_dependencies",
        "unused_optional_dependencies",
    ];
    for (name, clean, _caveated) in every_finding_type() {
        assert!(
            clean.gate_allows_mutation(),
            "{name}: a finding with no caveat must pass the gate"
        );
        if auto_fixable_types.contains(&name) {
            assert!(
                clean.actions().iter().any(IssueAction::is_auto_fixable),
                "{name}: the gate must not withhold a mutation the run has the evidence for"
            );
        }
    }
}

/// The caveat is advisory about the FINDING and decisive only about the
/// MUTATION: the actions array keeps its shape so a consumer reading
/// `actions[0].type` is unaffected, and the suppress alternative stays.
#[test]
fn the_gate_downgrades_a_mutation_without_removing_it() {
    let clean = UnusedExportFinding::with_actions(export("helper"));
    let mut caveated = clean.clone();
    caveated.set_reachability_caveats(BOTH.to_vec());

    assert_eq!(caveated.actions.len(), clean.actions.len());
    let IssueAction::Fix(fix) = &caveated.actions[0] else {
        panic!("position 0 stays the fix action");
    };
    assert!(!fix.auto_fixable);
    assert_eq!(
        fix.note.as_deref(),
        Some(INCOMPLETE_EVIDENCE_NOTE),
        "the withheld action says why in its own note, not only in a sibling array"
    );
}

/// A pre-existing note is context the user still needs (the re-export
/// warning names a public-API risk the caveat says nothing about), so the
/// gate appends rather than overwrites.
#[test]
fn a_gated_mutation_keeps_the_note_it_already_had() {
    let mut re_export = export("helper");
    re_export.is_re_export = true;
    let mut finding = UnusedExportFinding::with_actions(re_export);
    finding.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);

    let IssueAction::Fix(fix) = &finding.actions[0] else {
        panic!("position 0 stays the fix action");
    };
    let note = fix.note.as_deref().expect("note present");
    assert!(note.contains("public API"), "the original note survives");
    assert!(
        note.contains("Evidence is incomplete"),
        "the caveat is added"
    );
}

/// The gate is complete only if every finding type that can ever expose an
/// auto-fixable mutation is in it. The one member type deliberately left
/// out is safe for a different reason, and this pins that reason rather
/// than trusting a comment: a store member exposes no fix action at all,
/// because reflective access via a Pinia plugin or `$onAction` is
/// invisible to syntactic analysis, so no evidence this run could gather
/// would open the removal. If it ever ships one, it needs
/// `reachability_caveats` and a row in `impl_caveated_finding!` first.
///
/// A class member used to sit here on the weaker argument that its removal
/// STARTS withheld. That argument covered only the syntactic finding: the
/// type-aware sidecar reopens the removal through
/// [`UnusedClassMemberFinding::set_semantic_decision`], and the review
/// formats rendered a one-click commit for it regardless of
/// `auto_fixable`. It is inside the gate now, so the assertion here is
/// only that the SYNTACTIC finding still ships no auto-fix; the reopening
/// path is pinned by `a_complete_semantic_verdict_cannot_reopen_a_
/// caveated_class_member`.
#[test]
fn a_store_member_exposes_no_mutation_at_all() {
    let store = UnusedStoreMemberFinding::with_actions(member("total"));
    assert!(
        !store.actions.iter().any(IssueAction::is_auto_fixable),
        "a store member must expose no automatically applicable mutation"
    );
    assert!(
        !store
            .actions
            .iter()
            .any(|action| matches!(action, IssueAction::Fix(_))),
        "and no fix action at all"
    );

    let class = UnusedClassMemberFinding::with_actions(class_member("helper"));
    assert!(
        !class.actions.iter().any(IssueAction::is_auto_fixable),
        "a class member's syntactic removal stays withheld until semantic evidence opens it"
    );
}

/// The semantic pass runs after the annotation pass and is the only code
/// path that RAISES `auto_fixable`. A `Complete` verdict must not re-open a
/// mutation the incomplete run already withheld.
#[test]
fn a_complete_semantic_verdict_cannot_reopen_a_caveated_mutation() {
    use crate::semantic::{
        SemanticCandidateDecision, SemanticCandidateDecisionKind, SemanticCompleteness,
        SemanticNamespace, SemanticSymbol,
    };

    let complete_negative = || SemanticCandidateDecision {
        query_id: 0,
        subject: SemanticSymbol {
            path: PathBuf::from("/p/src/mod.ts"),
            namespace: SemanticNamespace::Value,
            declaration_kind: "function".to_string(),
            exported_name: "helper".to_string(),
            local_name: "helper".to_string(),
            owner: None,
            line: 1,
            col: 0,
        },
        decision: SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
        status: SemanticCompleteness::Complete,
        owning_projects: Vec::new(),
        evidence: Vec::new(),
        contract: None,
        framework_contract: None,
        closed_world_eligible: false,
        edit_guard: None,
        reason_code: None,
        explanation: String::new(),
        actions: Vec::new(),
        total_evidence_count: 0,
        truncated: false,
        omissions: Vec::new(),
    };

    let mut clean = UnusedExportFinding::with_actions(export("helper"));
    clean.set_semantic_decision(complete_negative());
    assert!(
        clean.actions.iter().any(IssueAction::is_auto_fixable),
        "a complete negative verdict on a clean run still enables the fix"
    );

    let mut caveated = UnusedExportFinding::with_actions(export("helper"));
    caveated.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
    caveated.set_semantic_decision(complete_negative());
    assert!(
        !caveated.actions.iter().any(IssueAction::is_auto_fixable),
        "the semantic pass must ask the gate too"
    );
}

/// The class-member twin, on its own eligibility flag. `closed_world_
/// eligible` is proved over the program the sidecar could see, which is
/// the program this run parsed; a member whose only call site sits in a
/// file the run never opened is absent from that world for the same reason
/// it is absent from the syntactic verdict, so a `true` here is not
/// evidence the run lacks.
#[test]
fn a_complete_semantic_verdict_cannot_reopen_a_caveated_class_member() {
    use crate::semantic::{
        SemanticCandidateDecision, SemanticCandidateDecisionKind, SemanticCompleteness,
        SemanticNamespace, SemanticSymbol,
    };

    let eligible = || SemanticCandidateDecision {
        query_id: 0,
        subject: SemanticSymbol {
            path: PathBuf::from("/p/src/mod.ts"),
            namespace: SemanticNamespace::Value,
            declaration_kind: "method".to_string(),
            exported_name: "Widget".to_string(),
            local_name: "legacyMethod".to_string(),
            owner: Some("Widget".to_string()),
            line: 2,
            col: 2,
        },
        decision: SemanticCandidateDecisionKind::ConfirmedNoStaticReferences,
        status: SemanticCompleteness::Complete,
        owning_projects: Vec::new(),
        evidence: Vec::new(),
        contract: None,
        framework_contract: None,
        closed_world_eligible: true,
        edit_guard: None,
        reason_code: None,
        explanation: "closed world proved".to_string(),
        actions: Vec::new(),
        total_evidence_count: 0,
        truncated: false,
        omissions: Vec::new(),
    };

    let mut clean = UnusedClassMemberFinding::with_actions(class_member("legacyMethod"));
    clean.set_semantic_decision(eligible());
    assert!(
        clean.actions.iter().any(IssueAction::is_auto_fixable),
        "a closed-world verdict on a run that read every file still opens the removal"
    );

    let mut caveated = UnusedClassMemberFinding::with_actions(class_member("legacyMethod"));
    caveated.set_reachability_caveats(vec![ReachabilityCaveat::IncompleteImportGraph]);
    caveated.set_semantic_decision(eligible());
    assert!(
        !caveated.actions.iter().any(IssueAction::is_auto_fixable),
        "the class-member semantic pass must ask the gate too"
    );
    let IssueAction::Fix(fix) = &caveated.actions[0] else {
        panic!("position 0 stays the fix action");
    };
    assert_eq!(
        fix.note.as_deref(),
        Some(INCOMPLETE_EVIDENCE_NOTE),
        "the withheld action says why, rather than repeating a closed-world explanation \
             computed over a program the run did not fully read"
    );
}
