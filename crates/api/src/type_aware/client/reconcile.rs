//! Atomic dead-code reconciliation for validated semantic responses.

use super::*;

pub(super) fn apply_dead_code_response(
    root: &Path,
    results: &mut AnalysisResults,
    reconciliation: DeadCodeReconciliation<'_>,
    response: SemanticResponse,
    requested_capabilities: Vec<SemanticCapability>,
) -> Result<SemanticDeadCodeOutcome, TypeAwareError> {
    let plan = build_dead_code_response_plan(
        root,
        results,
        reconciliation,
        response,
        requested_capabilities,
    )?;
    Ok(plan.commit(results))
}

struct DeadCodeResponsePlan {
    class_decisions: Vec<(usize, SemanticCandidateDecision)>,
    export_decisions: Vec<(usize, SemanticCandidateDecision)>,
    type_decisions: Vec<(usize, SemanticCandidateDecision)>,
    remove_class: BTreeSet<usize>,
    remove_exports: BTreeSet<usize>,
    remove_types: BTreeSet<usize>,
    api_surface: Option<ApiSurfaceMutation>,
    outcome: SemanticDeadCodeOutcome,
}

impl DeadCodeResponsePlan {
    fn commit(self, results: &mut AnalysisResults) -> SemanticDeadCodeOutcome {
        for (index, decision) in self.class_decisions {
            results.unused_class_members[index].set_semantic_decision(decision);
        }
        for (index, decision) in self.export_decisions {
            results.unused_exports[index].set_semantic_decision(decision);
        }
        for (index, decision) in self.type_decisions {
            results.unused_types[index].set_semantic_decision(decision);
        }
        if let Some(mutation) = self.api_surface {
            apply_api_surface_mutation(results, mutation);
        }
        retain_unconfirmed(&mut results.unused_class_members, &self.remove_class);
        retain_unconfirmed(&mut results.unused_exports, &self.remove_exports);
        retain_unconfirmed(&mut results.unused_types, &self.remove_types);
        self.outcome
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "the transaction plans every capability delta before its infallible commit"
)]
fn build_dead_code_response_plan(
    root: &Path,
    results: &AnalysisResults,
    reconciliation: DeadCodeReconciliation<'_>,
    response: SemanticResponse,
    requested_capabilities: Vec<SemanticCapability>,
) -> Result<DeadCodeResponsePlan, TypeAwareError> {
    let DeadCodeReconciliation {
        request,
        targets,
        capacity: local_capacity,
    } = reconciliation;
    let mut confirmed_class = BTreeSet::new();
    let mut confirmed_exports = BTreeSet::new();
    let mut confirmed_types = BTreeSet::new();
    let mut class_decisions = Vec::new();
    let mut export_decisions = Vec::new();
    let mut type_decisions = Vec::new();
    let mut api_surface = None;
    let mut api_surface_mutation = None;
    let mut type_coupling = None;
    let mut query_summaries = Vec::new();
    let mut candidate_decisions = Vec::new();
    let mut decision_stats = CandidateDecisionStats::default();
    let mut source_cache = FxHashMap::default();

    for result in &response.results {
        let Some(target) = targets.get(&result.query_id) else {
            continue;
        };
        let query = request
            .queries
            .iter()
            .find(|query| query.id() == result.query_id)
            .ok_or_else(|| {
                TypeAwareError::from(format!(
                    "type-aware response query {} had no matching request",
                    result.query_id
                ))
            })?;
        let mut summary = query_summary(result);
        match target {
            QueryTarget::ClassMember(index) => {
                let fix_supported = query_supports_guarded_fix(query);
                let decision = decode_candidate_decision(
                    root,
                    query,
                    result,
                    fix_supported,
                    &mut source_cache,
                )?;
                let finding = results.unused_class_members.get(*index).ok_or_else(|| {
                    TypeAwareError::from(
                        "type-aware class-member target no longer exists".to_string(),
                    )
                })?;
                let semantic_only = finding.semantic_only_candidate;
                record_candidate_decision(
                    &decision,
                    *index,
                    &mut confirmed_class,
                    &mut decision_stats,
                );
                if semantic_only_candidate_stays_hidden(semantic_only, decision.decision) {
                    confirmed_class.insert(*index);
                }
                class_decisions.push((*index, decision.clone()));
                candidate_decisions.push(decision);
            }
            QueryTarget::UnusedExport(index) => {
                let decision =
                    decode_candidate_decision(root, query, result, false, &mut source_cache)?;
                results.unused_exports.get(*index).ok_or_else(|| {
                    TypeAwareError::from("type-aware export target no longer exists".to_string())
                })?;
                record_candidate_decision(
                    &decision,
                    *index,
                    &mut confirmed_exports,
                    &mut decision_stats,
                );
                export_decisions.push((*index, decision.clone()));
                candidate_decisions.push(decision);
            }
            QueryTarget::UnusedType(index) => {
                let decision =
                    decode_candidate_decision(root, query, result, false, &mut source_cache)?;
                results.unused_types.get(*index).ok_or_else(|| {
                    TypeAwareError::from("type-aware type target no longer exists".to_string())
                })?;
                record_candidate_decision(
                    &decision,
                    *index,
                    &mut confirmed_types,
                    &mut decision_stats,
                );
                type_decisions.push((*index, decision.clone()));
                candidate_decisions.push(decision);
            }
            QueryTarget::ApiSurface {
                unrequested_candidate_count,
            } => {
                let (mut surface, mutation) = plan_api_surface(root, query, result)?;
                if *unrequested_candidate_count > 0 {
                    add_capacity_gap(&mut summary, &mut surface, *unrequested_candidate_count);
                }
                api_surface = Some(surface);
                api_surface_mutation = Some(mutation);
            }
            QueryTarget::TypeCoupling => {
                type_coupling = Some(decode_type_coupling(&response, request, result)?);
            }
        }
        query_summaries.push(summary);
    }
    if local_capacity.unrequested_symbol_count > 0
        && let Some(summary) = query_summaries
            .iter_mut()
            .rev()
            .find(|summary| summary.capability == SemanticCapability::SymbolUse)
    {
        add_summary_capacity_gap(summary, local_capacity.unrequested_symbol_count);
    }

    let planned_class_decisions = class_decisions
        .iter()
        .map(|(index, _)| *index)
        .collect::<BTreeSet<_>>();
    for (index, finding) in results.unused_class_members.iter().enumerate() {
        if finding.semantic_only_candidate
            && finding.semantic.is_none()
            && !planned_class_decisions.contains(&index)
        {
            confirmed_class.insert(index);
        }
    }

    let mut completeness = aggregate_completeness(&response.results);
    if completeness == SemanticCompleteness::Complete
        && (local_capacity.unrequested_symbol_count > 0
            || targets.values().any(|target| {
                matches!(
                    target,
                    QueryTarget::ApiSurface {
                        unrequested_candidate_count: 1..
                    }
                )
            }))
    {
        completeness = SemanticCompleteness::Partial;
    }
    let capabilities = requested_capabilities
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let candidate_count = request
        .queries
        .iter()
        .filter(|query| matches!(query, SemanticQuery::SymbolUse { .. }))
        .count()
        + local_capacity.unrequested_symbol_count;
    decision_stats.abstained += local_capacity.unrequested_symbol_count;
    decision_stats.abstention_reasons.capacity += local_capacity.unrequested_symbol_count;
    let warning_count = response.warnings.len();
    let warnings = response.warnings.clone();
    let identity = SemanticAnalysisIdentity {
        mode: SemanticAnalysisMode::TypeAware,
        semantic_schema_version: SEMANTIC_SCHEMA_VERSION,
        capabilities,
        project_config_hash: response_project_config_hash(&response.projects),
        backend_family: response.backend.clone(),
        completeness,
    };
    let projects = response
        .projects
        .into_iter()
        .map(|project| TypeAwareProjectMeta {
            config: project.config,
            source: project.source,
            status: project.status,
            candidate_count: project.candidate_count,
            confirmed_used_count: project.confirmed_used_count,
            contract_preserved_count: project.contract_preserved_count,
            no_static_references_count: project.no_static_references_count,
            fix_eligible_count: project.fix_eligible_count,
            unresolved_count: project.unresolved_count,
            abstained_count: project.abstained_count,
            blocking_diagnostic_count: project.blocking_diagnostic_count,
            source_file_count: project.source_file_count,
            program_reused: Some(project.program_reused),
            program_shared_across_queries: Some(project.program_reused),
            program_reused_from_previous_snapshot: project.program_reused_from_previous_snapshot,
            snapshot_revision: project.snapshot_revision,
            invalidation_kind: project.invalidation_kind,
            reason_code: project.reason_code,
            abstain_reason: None,
        })
        .collect();
    let type_aware = TypeAwareOutcome {
        meta: TypeAwareMeta {
            identity: Some(identity),
            required_completeness: None,
            queries: query_summaries,
            candidate_decisions,
            symbol_traces: Vec::new(),
            api_surface,
            symbol_impacts: Vec::new(),
            type_coupling: type_coupling.clone(),
            executed: true,
            protocol_version: response.protocol_version,
            sidecar_version: Some(response.sidecar_version),
            backend: response.backend,
            backend_version: Some(response.backend_version),
            selected_tsconfigs: response.selected_tsconfigs,
            candidate_count,
            confirmed_used_count: decision_stats.confirmed_used,
            contract_preserved_count: decision_stats.contract_preserved,
            no_static_references_count: decision_stats.no_static_references,
            fix_eligible_count: decision_stats.fix_eligible,
            unresolved_count: decision_stats.unresolved,
            abstained_count: decision_stats.abstained,
            abstention_reasons: decision_stats.abstention_reasons,
            projects,
            warning_count,
            warnings: warnings.clone(),
            elapsed_ms: response.elapsed_ms,
            phase_timings_ms: TypeAwarePhaseTimings {
                project_setup: response.phase_timings_ms.project_setup,
                diagnostics: response.phase_timings_ms.diagnostics,
                symbol_scan: response.phase_timings_ms.semantic_queries,
            },
        },
        warnings,
    };
    Ok(DeadCodeResponsePlan {
        class_decisions,
        export_decisions,
        type_decisions,
        remove_class: confirmed_class,
        remove_exports: confirmed_exports,
        remove_types: confirmed_types,
        api_surface: api_surface_mutation,
        outcome: SemanticDeadCodeOutcome {
            type_aware,
            type_coupling,
        },
    })
}

pub(super) const fn semantic_only_candidate_stays_hidden(
    semantic_only: bool,
    decision: SemanticCandidateDecisionKind,
) -> bool {
    semantic_only
        && !matches!(
            decision,
            SemanticCandidateDecisionKind::ConfirmedNoStaticReferences
        )
}
