//! Programmatic orchestration for local advisory similar-code discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_engine::session::AnalysisSession;
use fallow_engine::similar_code::{
    FunctionVector, SimilarCodeLimits as EngineLimits, SimilarCodeSelectionInput,
    SimilarCodeSkipReason as EngineSkipReason, evaluate_selected_similar_code,
    select_similar_code_corpus,
};
use fallow_engine::source::similar_code::{
    ExtractedSimilarCodeFunction, SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION,
    SimilarCodeExtractionLimits, SimilarCodeExtractionSkipReason,
};
use fallow_engine::{
    codeowners::CodeOwners,
    project_analysis::ProjectAnalysisArtifactOptions,
    trace::{trace_file, trace_impact_closure},
};
use fallow_output::{
    SimilarCodeAction, SimilarCodeActionType, SimilarCodeCacheStatus, SimilarCodeCacheSummary,
    SimilarCodeCandidate, SimilarCodeCandidateSnapshot, SimilarCodeCompletion,
    SimilarCodeCompletionStatus, SimilarCodeDiagnostic, SimilarCodeDiagnosticDomain,
    SimilarCodeDomainOutcome, SimilarCodeEnrichmentAvailability, SimilarCodeEnrichmentState,
    SimilarCodeGeneration, SimilarCodeGenerationParameters, SimilarCodeInspectOutput,
    SimilarCodeInspectPacket, SimilarCodeInspectSchemaVersion, SimilarCodeLimits,
    SimilarCodeLocation, SimilarCodeModelProvenance, SimilarCodeNamedReference, SimilarCodeOutput,
    SimilarCodePhase, SimilarCodePhaseCompletion, SimilarCodePhaseStatus, SimilarCodeProvider,
    SimilarCodeProviderProvenance, SimilarCodeReviewOutput, SimilarCodeReviewProvenance,
    SimilarCodeReviewSchemaVersion, SimilarCodeReviewedCandidate, SimilarCodeSchemaVersion,
    SimilarCodeScopeProvenance, SimilarCodeSideEffectHint, SimilarCodeSideEvidence,
    SimilarCodeSimilarityBand, SimilarCodeSkip, SimilarCodeSkipReason, SimilarCodeVerdictInput,
    SimilarCodeVerdictMatch, SimilarCodeVerificationStatus,
};
use fallow_types::envelope::{ElapsedMs, ToolVersion};
use globset::{Glob, GlobSet, GlobSetBuilder};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::analysis_context::{
    ProgrammaticAnalysisContext, changed_files_for_run,
    resolve_programmatic_analysis_context_deferred_workspace, workspace_roots_for_session,
};
use crate::similar_code::{
    self, EmbeddingInput, EmbeddingResult, ProviderError, ReadyProvider, SimilarCodeProviderStatus,
};
use crate::{ProgrammaticError, SimilarCodeInspectOptions, SimilarCodeOptions};

use super::ProgrammaticResult;

const MAX_FILES: usize = 20_000;
const MAX_RUN_TIMEOUT_MS: u64 = 15 * 60 * 1_000;
const HIGH_SIMILARITY: f64 = 0.88;
const VERY_HIGH_SIMILARITY: f64 = 0.95;
const MAX_REVIEW_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_RATIONALE_CHARS: usize = 4_000;
const MAX_SOURCE_WINDOW_CHARS: usize = 16_000;
const MAX_INSPECT_SOURCE_BYTES: u64 = fallow_config::DEFAULT_MAX_FILE_SIZE_BYTES;
const MAX_INSPECT_GRAPH_REFERENCES: usize = 50;
const MAX_INSPECT_RELATED_TESTS: usize = 50;
const MODULE_REFERENCE_NAME: &str = "<module>";
const INSPECT_CHURN_WINDOW: &str = "6 months ago";

#[derive(Clone, Copy)]
struct PhaseCompleteness {
    discovery: bool,
    extraction: bool,
    embedding: bool,
    comparison: bool,
}

trait RuntimeEmbedder {
    fn embed(
        &mut self,
        project_root: &Path,
        no_cache: bool,
        inputs: &[EmbeddingInput<'_>],
    ) -> Result<EmbeddingResult, ProviderError>;
}

struct VerifiedProviderEmbedder<'a> {
    provider: &'a ReadyProvider,
}

impl RuntimeEmbedder for VerifiedProviderEmbedder<'_> {
    fn embed(
        &mut self,
        project_root: &Path,
        no_cache: bool,
        inputs: &[EmbeddingInput<'_>],
    ) -> Result<EmbeddingResult, ProviderError> {
        similar_code::embed_selected(self.provider, project_root, no_cache, inputs)
    }
}

impl PhaseCompleteness {
    const fn all_complete(self) -> bool {
        self.discovery && self.extraction && self.embedding && self.comparison
    }
}

/// Run opt-in semantic similar-code discovery through the verified local provider.
///
/// Candidate presence never changes success status. Missing local setup uses
/// exit code 3, while invalid inputs and provider failures use exit code 2.
///
/// # Errors
///
/// Returns a structured error for invalid analysis options, unavailable local
/// setup, source discovery failures, or unusable provider output.
pub fn run_similar_code(options: &SimilarCodeOptions) -> ProgrammaticResult<SimilarCodeOutput> {
    validate_options(options)?;
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    let provider = options
        .adapter_provider_path
        .as_deref()
        .map_or_else(
            similar_code::ready_provider,
            similar_code::ready_provider_from_adapter_path,
        )
        .map_err(provider_error)?;
    resolved.install(|| run_similar_code_inner(options, &resolved, &provider))
}

/// Select one bounded candidate snapshot from raw discovery JSON.
///
/// # Errors
///
/// Returns an error for oversized or malformed discovery JSON, duplicate
/// candidate identities, or an unknown requested candidate.
pub fn select_similar_code_candidate_snapshot(
    candidate_json: &[u8],
    candidate_id: &str,
) -> ProgrammaticResult<SimilarCodeCandidateSnapshot> {
    if candidate_json.len() > MAX_REVIEW_INPUT_BYTES {
        return Err(candidate_input_error(
            "similar-code candidate input exceeded the 16 MiB limit",
        ));
    }
    if candidate_id.trim().is_empty() {
        return Err(candidate_input_error("candidate_id must not be empty"));
    }
    let raw = parse_candidate_document(candidate_json).map_err(|error| {
        candidate_input_error(format!(
            "invalid similar-code candidate document: {}",
            error.message
        ))
    })?;
    let mut candidates = raw
        .candidates
        .into_iter()
        .filter(|candidate| candidate.candidate_id == candidate_id);
    let candidate = candidates.next().ok_or_else(|| {
        candidate_input_error("candidate_id was not present in the discovery document")
    })?;
    if candidates.next().is_some() {
        return Err(candidate_input_error(
            "candidate document contains duplicate candidate_id values",
        ));
    }
    Ok(SimilarCodeCandidateSnapshot {
        schema_version: raw.schema_version,
        generation: raw.generation,
        candidate,
        completion: raw.completion,
        diagnostics: raw.diagnostics,
    })
}

/// Validate one inline bounded candidate snapshot.
///
/// # Errors
///
/// Returns an error for oversized, malformed, or identity-mismatched input.
pub fn parse_similar_code_candidate_snapshot(
    snapshot_json: &[u8],
    candidate_id: &str,
) -> ProgrammaticResult<SimilarCodeCandidateSnapshot> {
    if snapshot_json.len() > MAX_REVIEW_INPUT_BYTES {
        return Err(candidate_input_error(
            "similar-code candidate snapshot exceeded the 16 MiB limit",
        ));
    }
    let snapshot: SimilarCodeCandidateSnapshot = serde_json::from_slice(snapshot_json)
        .map_err(|error| candidate_input_error(format!("invalid candidate snapshot: {error}")))?;
    if candidate_id.trim().is_empty() || snapshot.candidate.candidate_id != candidate_id {
        return Err(candidate_input_error(
            "candidate snapshot identity does not match candidate_id",
        ));
    }
    Ok(snapshot)
}

/// Validate and inspect one exact candidate snapshot without rerunning global
/// provider retrieval or ranking.
///
/// # Errors
///
/// Returns an error when the candidate is stale or source cannot be reproduced.
pub fn inspect_similar_code(
    options: &SimilarCodeInspectOptions,
) -> ProgrammaticResult<SimilarCodeInspectOutput> {
    let started = Instant::now();
    let candidate = options.snapshot.candidate.clone();
    let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
    let root = resolved.root().to_path_buf();
    let mut left = inspect_side(&root, &candidate.left)?;
    let mut right = inspect_side(&root, &candidate.right)?;
    let session = load_session(&resolved)?;
    let enrichment = resolved.install(|| {
        enrich_inspect(
            &session,
            &candidate.left,
            &candidate.right,
            &mut left,
            &mut right,
        )
    });
    let mut diagnostics = options.snapshot.diagnostics.clone();
    diagnostics.extend(enrichment.diagnostics);
    Ok(SimilarCodeInspectOutput {
        schema_version: SimilarCodeInspectSchemaVersion::V1,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_owned()),
        elapsed_ms: ElapsedMs(duration_ms(started)),
        generation: options.snapshot.generation.clone(),
        packet: SimilarCodeInspectPacket {
            candidate_id: candidate.candidate_id.clone(),
            review_key: candidate.review_key.clone(),
            availability: enrichment.availability,
            graph_relationship: enrichment.graph_relationship,
            left,
            right,
        },
        candidate,
        completion: options.snapshot.completion.clone(),
        diagnostics,
    })
}

/// Join immutable candidate JSON with a separately authored verdict document.
///
/// # Errors
///
/// Returns a fail-closed error for malformed envelopes, invalid judgment
/// implications, duplicate or stale verdicts, or missing required verdicts.
#[expect(
    clippy::too_many_lines,
    reason = "the review join keeps fail-closed verdict validation in one auditable boundary"
)]
pub fn review_similar_code(
    candidate_json: &[u8],
    verdict_json: &[u8],
    require_verdict_for_each_candidate: bool,
) -> ProgrammaticResult<SimilarCodeReviewOutput> {
    let started = Instant::now();
    if candidate_json.len() > MAX_REVIEW_INPUT_BYTES || verdict_json.len() > MAX_REVIEW_INPUT_BYTES
    {
        return Err(review_error(
            "similar-code review input exceeded the 16 MiB limit",
        ));
    }
    let raw = parse_candidate_document(candidate_json)?;
    let verdicts: SimilarCodeVerdictInput = serde_json::from_slice(verdict_json)
        .map_err(|error| review_error(format!("invalid similar-code verdict document: {error}")))?;
    let mut by_candidate_id = FxHashMap::default();
    let mut by_review_key: FxHashMap<&str, Vec<usize>> = FxHashMap::default();
    for (index, candidate) in raw.candidates.iter().enumerate() {
        if by_candidate_id
            .insert(candidate.candidate_id.as_str(), index)
            .is_some()
        {
            return Err(review_error(
                "candidate document contains duplicate candidate_id values",
            ));
        }
        by_review_key
            .entry(candidate.review_key.as_str())
            .or_default()
            .push(index);
    }

    let mut matched = vec![None; raw.candidates.len()];
    let mut match_kind = vec![SimilarCodeVerdictMatch::Unverified; raw.candidates.len()];
    let mut diagnostics = raw.diagnostics.clone();
    let mut seen_candidate_ids = FxHashSet::default();
    let mut seen_review_keys = FxHashSet::default();
    for verdict in verdicts.verdicts {
        validate_verdict(&verdict)?;
        if !seen_candidate_ids.insert(verdict.candidate_id.clone())
            || !seen_review_keys.insert(verdict.review_key.clone())
        {
            return Err(review_error(
                "verdict document contains duplicate candidate or review identities",
            ));
        }
        if let Some(&index) = by_candidate_id.get(verdict.candidate_id.as_str()) {
            if raw.candidates[index].review_key != verdict.review_key {
                return Err(review_error(
                    "verdict review_key does not match its candidate_id",
                ));
            }
            matched[index] = Some(verdict);
            match_kind[index] = SimilarCodeVerdictMatch::CandidateId;
            continue;
        }
        let Some(indices) = by_review_key.get(verdict.review_key.as_str()) else {
            return Err(review_error(
                "verdict references a stale or unknown candidate",
            ));
        };
        if indices.len() == 1 {
            let index = indices[0];
            if matched[index].is_some() {
                return Err(review_error(
                    "multiple verdicts resolve to the same candidate",
                ));
            }
            matched[index] = Some(verdict);
            match_kind[index] = SimilarCodeVerdictMatch::ReviewKey;
        } else {
            for &index in indices {
                match_kind[index] = SimilarCodeVerdictMatch::AmbiguousReviewKey;
            }
            diagnostics.push(SimilarCodeDiagnostic {
                domain: SimilarCodeDiagnosticDomain::Review,
                code: "FALLOW_SIMILAR_CODE_REVIEW_KEY_AMBIGUOUS".to_owned(),
                message:
                    "a verdict review_key matched multiple current candidates and was not applied"
                        .to_owned(),
                path: None,
            });
        }
    }
    if require_verdict_for_each_candidate && matched.iter().any(Option::is_none) {
        return Err(review_error("a verdict is required for every candidate"));
    }
    let candidates = raw
        .candidates
        .into_iter()
        .zip(matched)
        .zip(match_kind)
        .map(|((candidate, verdict), verdict_match)| {
            let outcome = verdict
                .as_ref()
                .map_or(SimilarCodeDomainOutcome::NeedsHumanReview, |verdict| {
                    verdict.outcome
                });
            SimilarCodeReviewedCandidate {
                candidate,
                verdict,
                verdict_match,
                outcome,
            }
        })
        .collect();
    Ok(SimilarCodeReviewOutput {
        schema_version: SimilarCodeReviewSchemaVersion::V1,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_owned()),
        elapsed_ms: ElapsedMs(duration_ms(started)),
        generation: raw.generation,
        review: SimilarCodeReviewProvenance {
            candidates_sha256: sha256_hex(candidate_json),
            verdicts_sha256: sha256_hex(verdict_json),
        },
        candidates,
        completion: raw.completion,
        diagnostics,
    })
}

fn run_similar_code_inner(
    options: &SimilarCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    provider: &ReadyProvider,
) -> ProgrammaticResult<SimilarCodeOutput> {
    let mut embedder = VerifiedProviderEmbedder { provider };
    run_similar_code_inner_with_embedder(options, resolved, &provider.status, &mut embedder)
}

#[expect(
    clippy::too_many_lines,
    reason = "the orchestration keeps one auditable sequence of bounded analysis phases"
)]
fn run_similar_code_inner_with_embedder(
    options: &SimilarCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    provider: &SimilarCodeProviderStatus,
    embedder: &mut dyn RuntimeEmbedder,
) -> ProgrammaticResult<SimilarCodeOutput> {
    let started = Instant::now();
    let session = load_session(resolved)?;
    let threshold = options
        .threshold
        .unwrap_or_else(|| session.config().similar_code.threshold);
    let min_lines = options
        .min_lines
        .unwrap_or_else(|| session.config().similar_code.min_lines);
    validate_threshold(threshold)?;
    if min_lines == 0 {
        return Err(
            ProgrammaticError::new("`similar_code.min_lines` must be at least 1", 2)
                .with_code("FALLOW_INVALID_SIMILAR_CODE_MIN_LINES")
                .with_context("similarCode.minLines"),
        );
    }
    let changed_files = changed_files_for_run(resolved)?;
    let workspace_roots = workspace_roots_for_session(resolved, session.workspaces())?;
    let scope_active = similar_code_scope_active(
        options,
        resolved,
        changed_files.as_ref(),
        workspace_roots.as_deref(),
    );

    let ignore = build_ignore_set(&session.config().similar_code.ignore)?;
    let extraction_limits = SimilarCodeExtractionLimits::default();
    let mut functions = Vec::new();
    let mut extracted_source_bytes = 0usize;
    let mut extraction_skips = BTreeMap::new();
    let mut source_read_failures = 0usize;
    let mut diagnostics = Vec::new();
    let files = session.files();
    let mut eligible_files = files
        .iter()
        .filter_map(|file| {
            let relative = root_relative(session.root(), &file.path);
            (!ignore.is_match(&relative)).then_some((file, relative))
        })
        .collect::<Vec<_>>();
    eligible_files.sort_by_key(|(_, relative)| {
        usize::from(
            scope_active
                && !similar_code_path_in_scope(
                    relative,
                    options,
                    resolved,
                    changed_files.as_ref(),
                    workspace_roots.as_deref(),
                ),
        )
    });
    let total_eligible_files = eligible_files.len();
    let admitted_files = total_eligible_files.min(MAX_FILES);
    let omitted_files = total_eligible_files.saturating_sub(admitted_files);
    eligible_files.truncate(admitted_files);
    let mut effective_scope_paths = if scope_active {
        eligible_files
            .iter()
            .filter(|(_, relative)| {
                similar_code_path_in_scope(
                    relative,
                    options,
                    resolved,
                    changed_files.as_ref(),
                    workspace_roots.as_deref(),
                )
            })
            .map(|(_, relative)| relative.clone())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    effective_scope_paths.sort();
    effective_scope_paths.dedup();
    for (file_index, (file, relative)) in eligible_files.iter().enumerate() {
        let remaining_functions = extraction_limits
            .max_functions
            .saturating_sub(functions.len());
        let remaining_bytes = extraction_limits
            .max_total_source_bytes
            .saturating_sub(extracted_source_bytes);
        if let Some(reason) = exhausted_extraction_limit(remaining_functions, remaining_bytes) {
            add_skip(
                &mut extraction_skips,
                reason,
                remaining_extraction_inputs(eligible_files.len(), file_index),
            );
            break;
        }
        let source = match std::fs::read_to_string(&file.path) {
            Ok(source) => source,
            Err(error) => {
                source_read_failures = source_read_failures.saturating_add(1);
                diagnostics.push(SimilarCodeDiagnostic {
                    domain: SimilarCodeDiagnosticDomain::Extraction,
                    code: "FALLOW_SIMILAR_CODE_SOURCE_READ_FAILED".to_owned(),
                    message: format!("failed to read source: {error}"),
                    path: Some(relative.clone()),
                });
                continue;
            }
        };
        let extracted = fallow_engine::source::similar_code::extract(
            Path::new(relative),
            &source,
            SimilarCodeExtractionLimits {
                max_functions: remaining_functions,
                max_source_bytes_per_function: extraction_limits.max_source_bytes_per_function,
                max_total_source_bytes: remaining_bytes,
            },
        );
        for skip in extracted.skipped {
            add_skip(
                &mut extraction_skips,
                map_extraction_skip(skip.reason),
                skip.count,
            );
        }
        for function in extracted.functions {
            let lines = function
                .location
                .end_line
                .saturating_sub(function.location.start_line)
                .saturating_add(1) as usize;
            if lines < min_lines {
                add_skip(
                    &mut extraction_skips,
                    SimilarCodeSkipReason::BelowMinimumLines,
                    1,
                );
            } else {
                extracted_source_bytes =
                    extracted_source_bytes.saturating_add(function.source.len());
                functions.push(function);
            }
        }
    }

    let engine_limits = EngineLimits::for_dimensions(provider.dimensions);
    let selection_inputs = functions
        .iter()
        .map(|function| SimilarCodeSelectionInput {
            location: &function.location,
            source_sha256: function.source_sha256,
            in_scope: !scope_active
                || similar_code_path_in_scope(
                    &function.location.file,
                    options,
                    resolved,
                    changed_files.as_ref(),
                    workspace_roots.as_deref(),
                ),
        })
        .collect::<Vec<_>>();
    let selection =
        select_similar_code_corpus(&selection_inputs, engine_limits).map_err(engine_error)?;
    let scoped_functions = selection_inputs
        .iter()
        .filter(|function| function.in_scope)
        .count();
    let selected_scoped_functions = selection
        .selected_in_scope
        .iter()
        .filter(|&&value| value)
        .count();
    if scope_active && selected_scoped_functions < scoped_functions {
        diagnostics.push(SimilarCodeDiagnostic {
            domain: SimilarCodeDiagnosticDomain::Workspace,
            code: "FALLOW_SIMILAR_CODE_SCOPE_PARTIAL".to_owned(),
            message: format!(
                "scope limits admitted {selected_scoped_functions} of {scoped_functions} eligible scoped functions"
            ),
            path: None,
        });
    }
    let selected = selection
        .selected_indices
        .iter()
        .map(|index| &functions[*index])
        .collect::<Vec<_>>();
    let embedding_inputs = selected
        .iter()
        .map(|function| EmbeddingInput {
            source_sha256: function.source_sha256,
            source: &function.source,
        })
        .collect::<Vec<_>>();
    let embedding = embedder
        .embed(session.root(), resolved.no_cache(), &embedding_inputs)
        .map_err(provider_error)?;

    let mut vectors = Vec::new();
    let mut effective_scope = Vec::new();
    for (selected_index, values) in embedding.vectors.into_iter().enumerate() {
        if let Some(values) = values {
            let function = selected[selected_index];
            vectors.push(FunctionVector {
                location: function.location.clone(),
                source_sha256: function.source_sha256,
                extraction_semantics_version: SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION,
                values,
            });
            effective_scope.push(selection.selected_in_scope[selected_index]);
        }
    }
    if vectors.len() < 2 && selected.len() >= 2 {
        return Err(ProgrammaticError::new(
            embedding.provider_problem.unwrap_or_else(|| {
                "the local similar-code provider returned fewer than two usable vectors".to_owned()
            }),
            2,
        )
        .with_code("FALLOW_SIMILAR_CODE_PROVIDER_FAILED")
        .with_context("similarCode.provider"));
    }

    let effective_selection = fallow_engine::similar_code::SimilarCodeCorpusSelection {
        selected_indices: (0..vectors.len()).collect(),
        selected_in_scope: effective_scope,
        skipped: selection.skipped.clone(),
    };
    let evaluation = evaluate_selected_similar_code(
        &vectors,
        &effective_selection,
        threshold,
        engine_limits,
        SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION,
    )
    .map_err(engine_error)?;
    let metadata = functions
        .iter()
        .map(|function| (location_key(&function.location), function))
        .collect::<FxHashMap<_, _>>();
    let mut candidates = evaluation
        .candidates
        .into_iter()
        .filter(|candidate| {
            !scope_active
                || similar_code_path_in_scope(
                    &candidate.left.file,
                    options,
                    resolved,
                    changed_files.as_ref(),
                    workspace_roots.as_deref(),
                )
                || similar_code_path_in_scope(
                    &candidate.right.file,
                    options,
                    resolved,
                    changed_files.as_ref(),
                    workspace_roots.as_deref(),
                )
        })
        .filter_map(|candidate| map_candidate(candidate, &metadata))
        .collect::<Vec<_>>();
    if let Some(top) = options.top {
        candidates.truncate(top);
    }

    let extraction_complete = extraction_is_complete(&extraction_skips, source_read_failures);
    let mut skips = extraction_skips
        .into_iter()
        .map(|(reason, count)| SimilarCodeSkip {
            phase: SimilarCodePhase::Extraction,
            reason,
            count: usize_to_u64(count),
        })
        .collect::<Vec<_>>();
    if omitted_files > 0 {
        skips.push(SimilarCodeSkip {
            phase: SimilarCodePhase::Discovery,
            reason: SimilarCodeSkipReason::InputLimit,
            count: usize_to_u64(omitted_files),
        });
    }
    skips.extend(
        evaluation
            .completion
            .skipped
            .iter()
            .map(|skip| SimilarCodeSkip {
                phase: SimilarCodePhase::Comparison,
                reason: map_engine_skip(skip.reason),
                count: usize_to_u64(skip.count),
            }),
    );
    let missing_vectors = selected.len().saturating_sub(vectors.len());
    if missing_vectors > 0 {
        skips.push(SimilarCodeSkip {
            phase: SimilarCodePhase::Embedding,
            reason: if embedding
                .provider_problem
                .as_deref()
                .is_some_and(|problem| problem.contains("timed out"))
            {
                SimilarCodeSkipReason::Timeout
            } else {
                SimilarCodeSkipReason::ProviderFailure
            },
            count: usize_to_u64(missing_vectors),
        });
    }
    if embedding.truncated_functions > 0 {
        skips.push(SimilarCodeSkip {
            phase: SimilarCodePhase::Embedding,
            reason: SimilarCodeSkipReason::TokenTruncation,
            count: usize_to_u64(embedding.truncated_functions),
        });
    }
    skips.sort_by_key(|skip| (skip.phase as u8, skip.reason as u8));
    if let Some(problem) = embedding.provider_problem {
        diagnostics.push(SimilarCodeDiagnostic {
            domain: SimilarCodeDiagnosticDomain::Provider,
            code: "FALLOW_SIMILAR_CODE_PROVIDER_PARTIAL".to_owned(),
            message: problem,
            path: None,
        });
    }
    if let Some(problem) = embedding.cache_problem {
        diagnostics.push(SimilarCodeDiagnostic {
            domain: SimilarCodeDiagnosticDomain::Cache,
            code: "FALLOW_SIMILAR_CODE_CACHE_ADVISORY".to_owned(),
            message: problem,
            path: None,
        });
    }
    let phase_completeness = PhaseCompleteness {
        discovery: omitted_files == 0,
        extraction: extraction_complete,
        embedding: missing_vectors == 0 && embedding.truncated_functions == 0,
        comparison: evaluation.completion.status
            == fallow_engine::similar_code::SimilarCodeCompletionStatus::Complete,
    };
    let complete = phase_completeness.all_complete()
        && diagnostics
            .iter()
            .all(|diagnostic| diagnostic.domain == SimilarCodeDiagnosticDomain::Cache);
    let cache_status = cache_status(
        embedding.cache_disabled,
        embedding.cache_hits,
        embedding.cache_misses,
    );
    let completion = SimilarCodeCompletion {
        status: if complete {
            SimilarCodeCompletionStatus::Complete
        } else {
            SimilarCodeCompletionStatus::Partial
        },
        phases: phases(
            admitted_files,
            total_eligible_files,
            functions.len(),
            selected.len(),
            vectors.len(),
            evaluation.completion.comparisons_performed,
            phase_completeness,
            missing_vectors,
            embedding.truncated_functions,
            source_read_failures,
        ),
        limits: output_limits(engine_limits, extraction_limits),
        skips,
        cache: SimilarCodeCacheSummary {
            status: cache_status,
            hits: usize_to_u64(embedding.cache_hits),
            misses: usize_to_u64(embedding.cache_misses),
            writes: usize_to_u64(embedding.cache_writes),
            invalid_entries: usize_to_u64(embedding.cache_invalid_entries),
        },
        provider_inference_ms: finite_f64_to_u64(embedding.inference_ms),
    };

    Ok(SimilarCodeOutput {
        schema_version: SimilarCodeSchemaVersion::V1,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_owned()),
        elapsed_ms: ElapsedMs(duration_ms(started)),
        generation: generation(
            provider,
            threshold,
            min_lines,
            SimilarCodeScopeProvenance {
                active: scope_active,
                paths: effective_scope_paths,
            },
        ),
        candidates,
        completion,
        diagnostics,
    })
}

fn inspect_side(
    root: &Path,
    location: &SimilarCodeLocation,
) -> ProgrammaticResult<SimilarCodeSideEvidence> {
    let relative = Path::new(&location.path);
    if location.path.trim().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(candidate_input_error(
            "candidate source paths must be normalized project-root-relative paths",
        ));
    }
    let path = dunce::canonicalize(root.join(relative)).map_err(|error| {
        ProgrammaticError::new(
            format!(
                "failed to resolve inspected source {}: {error}",
                location.path
            ),
            2,
        )
        .with_code("FALLOW_SIMILAR_CODE_INSPECT_SOURCE_FAILED")
        .with_context("similarCode.inspect")
    })?;
    if !path.starts_with(root) {
        return Err(candidate_input_error(
            "candidate source path resolves outside the project root",
        ));
    }
    let source = read_inspect_source(&path, &location.path)?;
    let extracted = fallow_engine::source::similar_code::extract(
        Path::new(&location.path),
        &source,
        SimilarCodeExtractionLimits::default(),
    );
    let function = extracted
        .functions
        .into_iter()
        .find(|function| function_matches_snapshot_location(function, location))
        .ok_or_else(|| {
            ProgrammaticError::new(
                "inspected function no longer matches the candidate snapshot",
                2,
            )
            .with_code("FALLOW_SIMILAR_CODE_CANDIDATE_STALE")
            .with_context("similarCode.inspect")
        })?;
    Ok(SimilarCodeSideEvidence {
        source_window: Some(bound_source_window(&function.source)),
        parameter_count: Some(function.param_count),
        is_async: Some(function.is_async),
        is_generator: Some(function.is_generator),
        has_await: Some(function.has_await),
        has_throw: Some(function.has_throw),
        side_effect_hint: Some(match function.side_effect_hint {
            fallow_engine::source::similar_code::SimilarCodeSideEffectHint::PureLooking => {
                SimilarCodeSideEffectHint::PureLooking
            }
            fallow_engine::source::similar_code::SimilarCodeSideEffectHint::MayHaveSideEffects => {
                SimilarCodeSideEffectHint::MayHaveSideEffects
            }
            fallow_engine::source::similar_code::SimilarCodeSideEffectHint::Unknown => {
                SimilarCodeSideEffectHint::Unknown
            }
            _ => SimilarCodeSideEffectHint::Unknown,
        }),
        entry_point_reachable: None,
        callers: Vec::new(),
        callees: Vec::new(),
        owners: Vec::new(),
        churn_commits: None,
        tests: Vec::new(),
        deterministic_clone_coverage: None,
        runtime_observations: None,
    })
}

fn function_matches_snapshot_location(
    function: &ExtractedSimilarCodeFunction,
    location: &SimilarCodeLocation,
) -> bool {
    function.location.file == location.path
        && function.name == location.name
        && function.location.start_line == location.start_line
        && function.location.start_column_utf8.saturating_add(1) == location.start_column
        && function.location.end_line == location.end_line
        && function.location.end_column_utf8.saturating_add(1) == location.end_column
        && hex(function.source_sha256.as_bytes()) == location.source_sha256
}

#[expect(
    clippy::filetype_is_file,
    reason = "exact inspect accepts only regular source files and rejects every special file"
)]
fn read_inspect_source(path: &Path, display_path: &str) -> ProgrammaticResult<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        inspect_source_error(format!("failed to inspect source {display_path}: {error}"))
    })?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_INSPECT_SOURCE_BYTES {
        return Err(stale_candidate_error(format!(
            "inspected source {display_path} exceeded the {} MiB per-file limit",
            MAX_INSPECT_SOURCE_BYTES / (1024 * 1024)
        )));
    }

    let file = std::fs::File::open(path).map_err(|error| {
        inspect_source_error(format!(
            "failed to read inspected source {display_path}: {error}"
        ))
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_INSPECT_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            inspect_source_error(format!(
                "failed to read inspected source {display_path}: {error}"
            ))
        })?;
    if bytes.len() as u64 > MAX_INSPECT_SOURCE_BYTES {
        return Err(stale_candidate_error(format!(
            "inspected source {display_path} exceeded the {} MiB per-file limit",
            MAX_INSPECT_SOURCE_BYTES / (1024 * 1024)
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        inspect_source_error(format!(
            "failed to read inspected source {display_path}: {error}"
        ))
    })
}

fn inspect_source_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_SIMILAR_CODE_INSPECT_SOURCE_FAILED")
        .with_context("similarCode.inspect")
}

fn stale_candidate_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_SIMILAR_CODE_CANDIDATE_STALE")
        .with_context("similarCode.inspect")
}

struct InspectEnrichment {
    availability: SimilarCodeEnrichmentAvailability,
    graph_relationship: Option<String>,
    diagnostics: Vec<SimilarCodeDiagnostic>,
}

fn enrich_inspect(
    session: &AnalysisSession,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
    left: &mut SimilarCodeSideEvidence,
    right: &mut SimilarCodeSideEvidence,
) -> InspectEnrichment {
    let mut result = InspectEnrichment {
        availability: unavailable_inspect_enrichment(),
        graph_relationship: None,
        diagnostics: Vec::new(),
    };

    if session.files().len() > MAX_FILES {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_ENRICHMENT_INPUT_LIMIT",
            format!(
                "graph and deterministic clone enrichment require at most {MAX_FILES} discovered files"
            ),
            None,
        ));
    } else {
        enrich_graph_and_clones(
            session,
            left_location,
            right_location,
            left,
            right,
            &mut result,
        );
    }

    enrich_ownership(
        session,
        left_location,
        right_location,
        left,
        right,
        &mut result,
    );
    enrich_churn(
        session,
        left_location,
        right_location,
        left,
        right,
        &mut result,
    );
    result
}

fn enrich_graph_and_clones(
    session: &AnalysisSession,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
    left: &mut SimilarCodeSideEvidence,
    right: &mut SimilarCodeSideEvidence,
    result: &mut InspectEnrichment,
) {
    let artifacts = match session.analyze_project_with_artifacts(
        &session.config().duplicates,
        ProjectAnalysisArtifactOptions {
            retain_graph: true,
            ..ProjectAnalysisArtifactOptions::default()
        },
    ) {
        Ok(artifacts) => artifacts,
        Err(error) => {
            result.diagnostics.push(enrichment_diagnostic(
                "FALLOW_SIMILAR_CODE_ANALYSIS_ENRICHMENT_UNAVAILABLE",
                format!("graph and deterministic clone enrichment failed: {error}"),
                None,
            ));
            return;
        }
    };

    left.deterministic_clone_coverage = Some(deterministic_clone_coverage(
        &artifacts.duplication,
        session.root(),
        left_location,
    ));
    right.deterministic_clone_coverage = Some(deterministic_clone_coverage(
        &artifacts.duplication,
        session.root(),
        right_location,
    ));
    result.availability.deterministic_clone_coverage = SimilarCodeEnrichmentState::Available;

    let Some(graph) = artifacts.dead_code.graph.as_ref() else {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_GRAPH_ENRICHMENT_UNAVAILABLE",
            "retained module graph was unavailable",
            None,
        ));
        return;
    };
    let Some(left_trace) = trace_file(graph, session.root(), &left_location.path) else {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_GRAPH_TARGET_UNAVAILABLE",
            "left candidate module was absent from the retained graph",
            Some(left_location.path.clone()),
        ));
        return;
    };
    let Some(right_trace) = trace_file(graph, session.root(), &right_location.path) else {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_GRAPH_TARGET_UNAVAILABLE",
            "right candidate module was absent from the retained graph",
            Some(right_location.path.clone()),
        ));
        return;
    };

    let left_impact = trace_impact_closure(graph, session.root(), &left_location.path);
    let right_impact = trace_impact_closure(graph, session.root(), &right_location.path);
    apply_graph_evidence(
        &left_trace,
        &right_trace,
        left_location,
        right_location,
        left_impact
            .as_ref()
            .map_or(&[][..], |impact| impact.affected_not_shown.as_slice()),
        right_impact
            .as_ref()
            .map_or(&[][..], |impact| impact.affected_not_shown.as_slice()),
        left,
        right,
        result,
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "the helper applies symmetric evidence for both immutable candidate sides"
)]
fn apply_graph_evidence(
    left_trace: &fallow_engine::trace::FileTrace,
    right_trace: &fallow_engine::trace::FileTrace,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
    left_impact_paths: &[String],
    right_impact_paths: &[String],
    left: &mut SimilarCodeSideEvidence,
    right: &mut SimilarCodeSideEvidence,
    result: &mut InspectEnrichment,
) {
    left.entry_point_reachable = Some(left_trace.is_reachable);
    right.entry_point_reachable = Some(right_trace.is_reachable);

    let (left_callers, left_callers_truncated) =
        bounded_module_references(&left_trace.imported_by, MAX_INSPECT_GRAPH_REFERENCES);
    let (left_callees, left_callees_truncated) =
        bounded_module_references(&left_trace.imports_from, MAX_INSPECT_GRAPH_REFERENCES);
    let (right_callers, right_callers_truncated) =
        bounded_module_references(&right_trace.imported_by, MAX_INSPECT_GRAPH_REFERENCES);
    let (right_callees, right_callees_truncated) =
        bounded_module_references(&right_trace.imports_from, MAX_INSPECT_GRAPH_REFERENCES);
    left.callers = left_callers;
    left.callees = left_callees;
    right.callers = right_callers;
    right.callees = right_callees;

    let (left_tests, left_tests_truncated) =
        bounded_related_tests(left_impact_paths, MAX_INSPECT_RELATED_TESTS);
    let (right_tests, right_tests_truncated) =
        bounded_related_tests(right_impact_paths, MAX_INSPECT_RELATED_TESTS);
    left.tests = left_tests;
    right.tests = right_tests;

    result.graph_relationship = Some(module_relationship(
        left_trace,
        right_trace,
        left_location,
        right_location,
    ));
    result.availability.graph_relationship = SimilarCodeEnrichmentState::Available;
    result.availability.entry_point_reachability = SimilarCodeEnrichmentState::Available;
    result.availability.callers = SimilarCodeEnrichmentState::Available;
    result.availability.callees = SimilarCodeEnrichmentState::Available;
    result.availability.tests = SimilarCodeEnrichmentState::Available;
    result.diagnostics.push(enrichment_diagnostic(
        "FALLOW_SIMILAR_CODE_GRAPH_REFERENCES_MODULE_LEVEL",
        "callers and callees are direct module import relationships; <module> at line 1 is a module anchor, not a function callsite",
        None,
    ));
    if left_callers_truncated
        || left_callees_truncated
        || right_callers_truncated
        || right_callees_truncated
        || left_tests_truncated
        || right_tests_truncated
    {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_GRAPH_ENRICHMENT_TRUNCATED",
            "graph references or related tests exceeded inspect output limits",
            None,
        ));
    }
}

fn bounded_module_references(
    paths: &[PathBuf],
    limit: usize,
) -> (Vec<SimilarCodeNamedReference>, bool) {
    let mut paths = paths
        .iter()
        .map(|path| normalize_path(path))
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let truncated = paths.len() > limit;
    paths.truncate(limit);
    (
        paths
            .into_iter()
            .map(|path| SimilarCodeNamedReference {
                path,
                name: MODULE_REFERENCE_NAME.to_owned(),
                line: 1,
            })
            .collect(),
        truncated,
    )
}

fn bounded_related_tests(paths: &[String], limit: usize) -> (Vec<String>, bool) {
    let mut tests = paths
        .iter()
        .map(|path| path.replace('\\', "/"))
        .filter(|path| is_test_path(path))
        .collect::<Vec<_>>();
    tests.sort();
    tests.dedup();
    let truncated = tests.len() > limit;
    tests.truncate(limit);
    (tests, truncated)
}

fn is_test_path(path: &str) -> bool {
    let surrounded = format!("/{}/", path.trim_matches('/'));
    surrounded.contains("/__tests__/")
        || surrounded.contains("/__mocks__/")
        || surrounded.contains("/test/")
        || surrounded.contains("/tests/")
        || path.contains(".test.")
        || path.contains(".spec.")
}

fn module_relationship(
    left_trace: &fallow_engine::trace::FileTrace,
    right_trace: &fallow_engine::trace::FileTrace,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
) -> String {
    if left_location.path == right_location.path {
        return "same-module".to_owned();
    }
    let left_imports_right = trace_imports_path(left_trace, &right_location.path);
    let right_imports_left = trace_imports_path(right_trace, &left_location.path);
    if left_imports_right && right_imports_left {
        return "mutual-direct-module-import".to_owned();
    }
    if left_imports_right {
        return "left-directly-imports-right".to_owned();
    }
    if right_imports_left {
        return "right-directly-imports-left".to_owned();
    }

    let left_callers = left_trace
        .imported_by
        .iter()
        .map(|path| normalize_path(path))
        .collect::<BTreeSet<_>>();
    if right_trace
        .imported_by
        .iter()
        .map(|path| normalize_path(path))
        .any(|path| left_callers.contains(&path))
    {
        "shared-direct-importer".to_owned()
    } else {
        "no-direct-module-relationship".to_owned()
    }
}

fn trace_imports_path(trace: &fallow_engine::trace::FileTrace, target: &str) -> bool {
    trace
        .imports_from
        .iter()
        .any(|path| normalize_path(path) == target)
}

fn enrich_ownership(
    session: &AnalysisSession,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
    left: &mut SimilarCodeSideEvidence,
    right: &mut SimilarCodeSideEvidence,
    result: &mut InspectEnrichment,
) {
    match CodeOwners::load(session.root(), session.config().codeowners.as_deref()) {
        Ok(codeowners) => {
            left.owners = primary_owner(&codeowners, &left_location.path);
            right.owners = primary_owner(&codeowners, &right_location.path);
            result.availability.ownership = SimilarCodeEnrichmentState::Available;
        }
        Err(error) => result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_OWNERSHIP_UNAVAILABLE",
            error,
            None,
        )),
    }
}

fn primary_owner(codeowners: &CodeOwners, path: &str) -> Vec<String> {
    codeowners
        .owner_of(Path::new(path))
        .map(|owner| vec![owner.to_owned()])
        .unwrap_or_default()
}

fn enrich_churn(
    session: &AnalysisSession,
    left_location: &SimilarCodeLocation,
    right_location: &SimilarCodeLocation,
    left: &mut SimilarCodeSideEvidence,
    right: &mut SimilarCodeSideEvidence,
    result: &mut InspectEnrichment,
) {
    if !fallow_engine::churn::is_git_repo(session.root()) {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_CHURN_UNAVAILABLE",
            "git repository unavailable at project root",
            None,
        ));
        return;
    }
    let since = fallow_engine::churn::SinceDuration {
        git_after: INSPECT_CHURN_WINDOW.to_owned(),
        display: "6 months".to_owned(),
    };
    let Some((churn, _cache_hit)) = fallow_engine::churn::analyze_churn_cached(
        session.root(),
        &since,
        &session.config().cache_dir,
        session.config().no_cache,
    ) else {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_CHURN_UNAVAILABLE",
            "git churn analysis failed",
            None,
        ));
        return;
    };
    left.churn_commits = Some(churn_commits_for(
        &churn,
        &session.root().join(&left_location.path),
    ));
    right.churn_commits = Some(churn_commits_for(
        &churn,
        &session.root().join(&right_location.path),
    ));
    result.availability.churn = SimilarCodeEnrichmentState::Available;
    if churn.shallow_clone {
        result.diagnostics.push(enrichment_diagnostic(
            "FALLOW_SIMILAR_CODE_CHURN_SHALLOW_HISTORY",
            "git churn counts may undercount history because the repository is shallow",
            None,
        ));
    }
}

fn churn_commits_for(churn: &fallow_engine::churn::ChurnResult, path: &Path) -> u64 {
    if let Some(file) = churn.files.get(path) {
        return u64::from(file.commits);
    }
    let target = normalize_path(path);
    churn
        .files
        .iter()
        .find(|(candidate, _)| normalize_path(candidate) == target)
        .map_or(0, |(_, file)| u64::from(file.commits))
}

fn deterministic_clone_coverage(
    report: &fallow_engine::duplicates::DuplicationReport,
    root: &Path,
    location: &SimilarCodeLocation,
) -> f64 {
    let start = usize::try_from(location.start_line).unwrap_or(usize::MAX);
    let end = usize::try_from(location.end_line).unwrap_or(0);
    if start > end {
        return 0.0;
    }
    let mut covered = BTreeSet::new();
    for group in &report.clone_groups {
        if !matches!(
            group.kind(),
            fallow_engine::duplicates::CloneGroupKind::Exact
        ) {
            continue;
        }
        for instance in &group.instances {
            if root_relative(root, &instance.file) != location.path {
                continue;
            }
            let overlap_start = start.max(instance.start_line);
            let overlap_end = end.min(instance.end_line);
            if overlap_start <= overlap_end {
                covered.extend(overlap_start..=overlap_end);
            }
        }
    }
    let total = end.saturating_sub(start).saturating_add(1);
    covered.len() as f64 / total as f64
}

fn unavailable_inspect_enrichment() -> SimilarCodeEnrichmentAvailability {
    SimilarCodeEnrichmentAvailability {
        graph_relationship: SimilarCodeEnrichmentState::Unavailable,
        entry_point_reachability: SimilarCodeEnrichmentState::Unavailable,
        callers: SimilarCodeEnrichmentState::Unavailable,
        callees: SimilarCodeEnrichmentState::Unavailable,
        ownership: SimilarCodeEnrichmentState::Unavailable,
        churn: SimilarCodeEnrichmentState::Unavailable,
        tests: SimilarCodeEnrichmentState::Unavailable,
        deterministic_clone_coverage: SimilarCodeEnrichmentState::Unavailable,
        runtime: SimilarCodeEnrichmentState::NotRequested,
    }
}

fn enrichment_diagnostic(
    code: &'static str,
    message: impl Into<String>,
    path: Option<String>,
) -> SimilarCodeDiagnostic {
    SimilarCodeDiagnostic {
        domain: SimilarCodeDiagnosticDomain::Enrichment,
        code: code.to_owned(),
        message: message.into(),
        path,
    }
}

fn parse_candidate_document(bytes: &[u8]) -> ProgrammaticResult<SimilarCodeOutput> {
    let mut value: Value = serde_json::from_slice(bytes)
        .map_err(|error| review_error(format!("invalid similar-code candidate JSON: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| review_error("similar-code candidate document must be a JSON object"))?;
    let kind = object
        .remove("kind")
        .and_then(|kind| kind.as_str().map(str::to_owned))
        .ok_or_else(|| review_error("similar-code candidate document is missing kind"))?;
    if kind != "similar-code" {
        return Err(review_error(
            "candidate document kind must be `similar-code`",
        ));
    }
    serde_json::from_value(value)
        .map_err(|error| review_error(format!("invalid similar-code candidate envelope: {error}")))
}

fn validate_verdict(verdict: &fallow_output::SimilarCodeVerdict) -> ProgrammaticResult<()> {
    verdict
        .validate()
        .map_err(|error| review_error(format!("invalid verdict implication: {error}")))?;
    let rationale_chars = verdict.rationale.chars().count();
    if rationale_chars == 0 || rationale_chars > MAX_RATIONALE_CHARS {
        return Err(review_error(
            "verdict rationale must contain 1 through 4000 characters",
        ));
    }
    if verdict.rationale.chars().any(char::is_control) {
        return Err(review_error(
            "verdict rationale must not contain control characters",
        ));
    }
    Ok(())
}

fn review_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_SIMILAR_CODE_REVIEW_INVALID")
        .with_context("similarCode.review")
}

fn candidate_input_error(message: impl Into<String>) -> ProgrammaticError {
    ProgrammaticError::new(message, 2)
        .with_code("FALLOW_SIMILAR_CODE_CANDIDATE_INPUT_INVALID")
        .with_context("similarCode.candidates")
}

fn bound_source_window(source: &str) -> String {
    let mut chars = source.chars();
    let bounded = chars
        .by_ref()
        .take(MAX_SOURCE_WINDOW_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}\n/* source window truncated */")
    } else {
        bounded
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn finite_f64_to_u64(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= u64::MAX as f64 {
        u64::MAX
    } else {
        value.round() as u64
    }
}

fn load_session(resolved: &ProgrammaticAnalysisContext) -> ProgrammaticResult<AnalysisSession> {
    let mut project = fallow_engine::project_config::config_for_project_with_load_options(
        resolved.root(),
        resolved.config_path().as_deref(),
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: resolved.allow_remote_extends(),
        },
    )
    .map_err(|error| {
        ProgrammaticError::new(format!("failed to load config: {error}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    project.config.no_cache = resolved.no_cache();
    project.config.threads = resolved.threads();
    Ok(AnalysisSession::from_config(project))
}

fn build_ignore_set(patterns: &[String]) -> ProgrammaticResult<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern).map_err(|error| {
            ProgrammaticError::new(
                format!("invalid `similarCode.ignore` pattern `{pattern}`: {error}"),
                2,
            )
            .with_code("FALLOW_INVALID_SIMILAR_CODE_IGNORE")
            .with_context("similarCode.ignore")
        })?);
    }
    builder.build().map_err(|error| {
        ProgrammaticError::new(
            format!("failed to compile `similarCode.ignore`: {error}"),
            2,
        )
        .with_code("FALLOW_INVALID_SIMILAR_CODE_IGNORE")
        .with_context("similarCode.ignore")
    })
}

fn map_candidate(
    candidate: fallow_engine::similar_code::SimilarCodeCandidate,
    metadata: &FxHashMap<(String, u32, u32), &ExtractedSimilarCodeFunction>,
) -> Option<SimilarCodeCandidate> {
    let left = metadata.get(&location_key(&candidate.left))?;
    let right = metadata.get(&location_key(&candidate.right))?;
    Some(SimilarCodeCandidate {
        candidate_id: candidate.candidate_id,
        review_key: candidate.review_key,
        left: output_location(left),
        right: output_location(right),
        similarity: candidate.similarity,
        similarity_band: similarity_band(candidate.similarity),
        verification_status: SimilarCodeVerificationStatus::Unverified,
        enrichment: raw_enrichment_availability(),
        actions: vec![
            SimilarCodeAction {
                action: SimilarCodeActionType::Inspect,
                description: "Inspect bounded source, graph, ownership, churn, test, and deterministic clone evidence".to_owned(),
                read_only: true,
            },
            SimilarCodeAction {
                action: SimilarCodeActionType::Review,
                description: "Join this immutable candidate with a separate evidence-grounded verdict".to_owned(),
                read_only: true,
            },
        ],
    })
}

fn output_location(function: &ExtractedSimilarCodeFunction) -> SimilarCodeLocation {
    SimilarCodeLocation {
        path: function.location.file.clone(),
        name: function.name.clone(),
        start_line: function.location.start_line,
        start_column: function.location.start_column_utf8.saturating_add(1),
        end_line: function.location.end_line,
        end_column: function.location.end_column_utf8.saturating_add(1),
        source_sha256: hex(function.source_sha256.as_bytes()),
    }
}

fn similar_code_scope_active(
    options: &SimilarCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
    workspace_roots: Option<&[PathBuf]>,
) -> bool {
    !options.files.is_empty()
        || changed_files.is_some()
        || resolved.diff_index().is_some()
        || workspace_roots.is_some()
}

fn similar_code_path_in_scope(
    path: &str,
    options: &SimilarCodeOptions,
    resolved: &ProgrammaticAnalysisContext,
    changed_files: Option<&FxHashSet<PathBuf>>,
    workspace_roots: Option<&[PathBuf]>,
) -> bool {
    if !options.files.is_empty()
        && !options
            .files
            .iter()
            .any(|filter| normalize_path(filter) == path)
    {
        return false;
    }
    if let Some(changed_files) = changed_files
        && !changed_files.contains(Path::new(path))
        && !changed_files.contains(&resolved.root().join(path))
    {
        return false;
    }
    if let Some(diff) = resolved.diff_index()
        && !diff.touches_file(&diff.key_for_root_relative(path))
    {
        return false;
    }
    if let Some(workspace_roots) = workspace_roots
        && !workspace_roots
            .iter()
            .any(|workspace| resolved.root().join(path).starts_with(workspace))
    {
        return false;
    }
    true
}

#[expect(
    clippy::too_many_arguments,
    reason = "each argument maps directly to one public completion-accounting field"
)]
fn phases(
    admitted_files: usize,
    total_files: usize,
    extracted_functions: usize,
    selected_functions: usize,
    embedded_functions: usize,
    comparisons: usize,
    completeness: PhaseCompleteness,
    missing_vectors: usize,
    truncated_functions: usize,
    source_read_failures: usize,
) -> Vec<SimilarCodePhaseCompletion> {
    vec![
        phase(
            SimilarCodePhase::Discovery,
            phase_status(completeness.discovery),
            admitted_files,
            Some(total_files),
            (!completeness.discovery)
                .then(|| "the file admission limit omitted source files".to_owned()),
        ),
        phase(
            SimilarCodePhase::Extraction,
            phase_status(completeness.extraction),
            extracted_functions,
            None,
            (!completeness.extraction).then(|| {
                if source_read_failures > 0 {
                    "one or more admitted source files could not be read".to_owned()
                } else {
                    "one or more function forms or source fragments were outside extraction limits"
                        .to_owned()
                }
            }),
        ),
        phase(
            SimilarCodePhase::Cache,
            SimilarCodePhaseStatus::Complete,
            selected_functions,
            Some(selected_functions),
            None,
        ),
        phase(
            SimilarCodePhase::Embedding,
            phase_status(completeness.embedding),
            embedded_functions,
            Some(selected_functions),
            (!completeness.embedding).then(|| {
                if missing_vectors > 0 {
                    "the provider did not return every selected vector".to_owned()
                } else if truncated_functions > 0 {
                    "the provider truncated one or more admitted functions".to_owned()
                } else {
                    "embedding did not complete its admitted scope".to_owned()
                }
            }),
        ),
        phase(
            SimilarCodePhase::Validation,
            SimilarCodePhaseStatus::Complete,
            embedded_functions,
            Some(embedded_functions),
            None,
        ),
        phase(
            SimilarCodePhase::Comparison,
            phase_status(completeness.comparison),
            comparisons,
            None,
            (!completeness.comparison)
                .then(|| "comparison limits omitted candidate pairs".to_owned()),
        ),
        phase(
            SimilarCodePhase::Enrichment,
            SimilarCodePhaseStatus::Skipped,
            0,
            None,
            Some("raw discovery defers source-grounded enrichment to inspect".to_owned()),
        ),
    ]
}

const fn phase_status(complete: bool) -> SimilarCodePhaseStatus {
    if complete {
        SimilarCodePhaseStatus::Complete
    } else {
        SimilarCodePhaseStatus::Partial
    }
}

fn phase(
    phase: SimilarCodePhase,
    status: SimilarCodePhaseStatus,
    processed: usize,
    total: Option<usize>,
    reason: Option<String>,
) -> SimilarCodePhaseCompletion {
    SimilarCodePhaseCompletion {
        phase,
        status,
        processed: usize_to_u64(processed),
        total: total.map(usize_to_u64),
        reason,
    }
}

fn output_limits(
    engine: EngineLimits,
    extraction: SimilarCodeExtractionLimits,
) -> SimilarCodeLimits {
    SimilarCodeLimits {
        max_files: usize_to_u64(MAX_FILES),
        max_functions: usize_to_u64(engine.max_functions),
        max_source_bytes: usize_to_u64(extraction.max_total_source_bytes),
        max_function_bytes: usize_to_u64(extraction.max_source_bytes_per_function),
        max_batch_size: usize_to_u64(similar_code::embedding_batch_size()),
        max_vector_bytes: usize_to_u64(engine.max_vector_bytes),
        max_comparisons: usize_to_u64(engine.max_comparisons),
        max_candidates: usize_to_u64(engine.max_candidates),
        max_neighbors_per_function: usize_to_u64(engine.max_neighbors_per_function),
        timeout_ms: MAX_RUN_TIMEOUT_MS,
    }
}

fn generation(
    provider: &SimilarCodeProviderStatus,
    threshold: f64,
    min_lines: usize,
    scope: SimilarCodeScopeProvenance,
) -> SimilarCodeGeneration {
    SimilarCodeGeneration {
        extraction_semantics_version: SIMILAR_CODE_EXTRACTION_SEMANTICS_VERSION,
        embedding_semantics_version: similar_code::embedding_semantics_version(),
        provider: SimilarCodeProviderProvenance {
            provider: SimilarCodeProvider::OfficialLocalCompanion,
            companion_version: provider.sidecar_version.clone(),
            protocol_version: provider.protocol_version,
            source_left_machine: false,
        },
        model: SimilarCodeModelProvenance {
            model_id: provider.model_id.clone(),
            revision: provider.model_revision.clone(),
            artifact_sha256: similar_code::model_artifact_sha256().to_owned(),
            license: provider.license.clone(),
            dimensions: u32::try_from(provider.dimensions).unwrap_or(u32::MAX),
        },
        parameters: SimilarCodeGenerationParameters {
            dtype: "f32".to_owned(),
            pooling: "mean".to_owned(),
            normalized: true,
            batch_size: u32::try_from(similar_code::embedding_batch_size()).unwrap_or(u32::MAX),
            max_tokens: u32::try_from(provider.max_tokens).unwrap_or(u32::MAX),
            parameter_sha256: similar_code::parameter_sha256(),
        },
        scope,
        threshold,
        min_lines: usize_to_u64(min_lines),
    }
}

fn raw_enrichment_availability() -> SimilarCodeEnrichmentAvailability {
    SimilarCodeEnrichmentAvailability {
        graph_relationship: SimilarCodeEnrichmentState::NotRequested,
        entry_point_reachability: SimilarCodeEnrichmentState::NotRequested,
        callers: SimilarCodeEnrichmentState::NotRequested,
        callees: SimilarCodeEnrichmentState::NotRequested,
        ownership: SimilarCodeEnrichmentState::NotRequested,
        churn: SimilarCodeEnrichmentState::NotRequested,
        tests: SimilarCodeEnrichmentState::NotRequested,
        deterministic_clone_coverage: SimilarCodeEnrichmentState::NotRequested,
        runtime: SimilarCodeEnrichmentState::NotRequested,
    }
}

fn map_extraction_skip(reason: SimilarCodeExtractionSkipReason) -> SimilarCodeSkipReason {
    match reason {
        SimilarCodeExtractionSkipReason::GeneratedSource => SimilarCodeSkipReason::GeneratedSource,
        SimilarCodeExtractionSkipReason::SourceBytesPerFunctionLimit => {
            SimilarCodeSkipReason::FunctionTooLarge
        }
        SimilarCodeExtractionSkipReason::FunctionLimit => SimilarCodeSkipReason::InputLimit,
        SimilarCodeExtractionSkipReason::TotalSourceBytesLimit => {
            SimilarCodeSkipReason::SourceBytesLimit
        }
        _ => SimilarCodeSkipReason::UnsupportedFunction,
    }
}

fn map_engine_skip(reason: EngineSkipReason) -> SimilarCodeSkipReason {
    match reason {
        EngineSkipReason::VectorMemoryLimit => SimilarCodeSkipReason::VectorMemoryLimit,
        EngineSkipReason::ComparisonLimit => SimilarCodeSkipReason::ComparisonLimit,
        EngineSkipReason::CandidateLimit => SimilarCodeSkipReason::CandidateLimit,
        EngineSkipReason::NeighborLimit => SimilarCodeSkipReason::NeighborLimit,
        _ => SimilarCodeSkipReason::InputLimit,
    }
}

fn cache_status(disabled: bool, hits: usize, misses: usize) -> SimilarCodeCacheStatus {
    if disabled {
        SimilarCodeCacheStatus::Disabled
    } else if misses == 0 {
        SimilarCodeCacheStatus::Hit
    } else if hits == 0 {
        SimilarCodeCacheStatus::Miss
    } else {
        SimilarCodeCacheStatus::Mixed
    }
}

fn similarity_band(similarity: f64) -> SimilarCodeSimilarityBand {
    if similarity >= VERY_HIGH_SIMILARITY {
        SimilarCodeSimilarityBand::VeryHigh
    } else if similarity >= HIGH_SIMILARITY {
        SimilarCodeSimilarityBand::High
    } else {
        SimilarCodeSimilarityBand::Moderate
    }
}

fn validate_options(options: &SimilarCodeOptions) -> ProgrammaticResult<()> {
    if let Some(threshold) = options.threshold {
        validate_threshold(threshold)?;
    }
    if options.min_lines == Some(0) {
        return Err(ProgrammaticError::new("`min_lines` must be at least 1", 2)
            .with_code("FALLOW_INVALID_SIMILAR_CODE_MIN_LINES")
            .with_context("similarCode.minLines"));
    }
    if options.top == Some(0) {
        return Err(ProgrammaticError::new("`top` must be at least 1", 2)
            .with_code("FALLOW_INVALID_SIMILAR_CODE_TOP")
            .with_context("similarCode.top"));
    }
    for path in &options.files {
        if path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(ProgrammaticError::new(
                "`file` paths must be project-root-relative and must not contain `..`",
                2,
            )
            .with_code("FALLOW_INVALID_SIMILAR_CODE_FILE")
            .with_context("similarCode.files"));
        }
    }
    Ok(())
}

fn validate_threshold(threshold: f64) -> ProgrammaticResult<()> {
    if !threshold.is_finite() || !(0.0..=1.0).contains(&threshold) {
        return Err(
            ProgrammaticError::new("`threshold` must be finite and between 0 and 1", 2)
                .with_code("FALLOW_INVALID_SIMILAR_CODE_THRESHOLD")
                .with_context("similarCode.threshold"),
        );
    }
    Ok(())
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "map_err supplies owned provider failures and the mapper consumes that boundary"
)]
fn provider_error(error: ProviderError) -> ProgrammaticError {
    let exit_code = if matches!(error, ProviderError::NotReady(_)) {
        3
    } else {
        2
    };
    let code = if exit_code == 3 {
        "FALLOW_SIMILAR_CODE_NOT_READY"
    } else {
        "FALLOW_SIMILAR_CODE_PROVIDER_FAILED"
    };
    ProgrammaticError::new(error.message(), exit_code)
        .with_code(code)
        .with_context("similarCode.provider")
}

fn engine_error(error: impl std::fmt::Display) -> ProgrammaticError {
    ProgrammaticError::new(format!("invalid similar-code evaluation: {error}"), 2)
        .with_code("FALLOW_SIMILAR_CODE_EVALUATION_FAILED")
        .with_context("similarCode.evaluation")
}

fn location_key(
    location: &fallow_engine::source::similar_code::SimilarCodeFunctionLocation,
) -> (String, u32, u32) {
    (
        location.file.clone(),
        location.start_byte,
        location.end_byte,
    )
}

fn root_relative(root: &Path, path: &Path) -> String {
    normalize_path(path.strip_prefix(root).unwrap_or(path))
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn add_skip(
    skips: &mut BTreeMap<SimilarCodeSkipReason, usize>,
    reason: SimilarCodeSkipReason,
    count: usize,
) {
    *skips.entry(reason).or_default() += count;
}

fn extraction_is_complete(
    skips: &BTreeMap<SimilarCodeSkipReason, usize>,
    source_read_failures: usize,
) -> bool {
    source_read_failures == 0
        && skips.iter().all(|(reason, count)| {
            *count == 0 || matches!(reason, SimilarCodeSkipReason::BelowMinimumLines)
        })
}

const fn remaining_extraction_inputs(total: usize, current_index: usize) -> usize {
    total.saturating_sub(current_index)
}

const fn exhausted_extraction_limit(
    remaining_functions: usize,
    remaining_source_bytes: usize,
) -> Option<SimilarCodeSkipReason> {
    if remaining_functions == 0 {
        Some(SimilarCodeSkipReason::InputLimit)
    } else if remaining_source_bytes == 0 {
        Some(SimilarCodeSkipReason::SourceBytesLimit)
    } else {
        None
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut output, byte| {
            let _ = write!(output, "{byte:02x}");
            output
        },
    )
}

fn duration_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    clippy::unwrap_used,
    reason = "deterministic fixtures fail immediately and ratios have exact binary representations"
)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::similar_code::{
        EmbeddingBatch, EmbeddingBatchVector, EmbeddingSession, EmbeddingSessionFactory,
    };

    #[derive(Default)]
    struct FakeProviderState {
        spawns: usize,
        batches: usize,
        complete_batches: Option<usize>,
    }

    struct FakeEmbeddingSession {
        state: Arc<Mutex<FakeProviderState>>,
        dimensions: usize,
    }

    impl EmbeddingSession for FakeEmbeddingSession {
        fn embed(&mut self, functions: &[(u32, &str)]) -> Result<EmbeddingBatch, String> {
            let should_return_partial = {
                let mut state = self.state.lock().unwrap();
                state.batches += 1;
                state
                    .complete_batches
                    .is_some_and(|limit| state.batches > limit)
            };
            if should_return_partial {
                return Ok(EmbeddingBatch {
                    vectors: Vec::new(),
                    inference_ms: 0.0,
                    problem: Some("fixture provider returned a bounded partial batch".to_owned()),
                });
            }
            let vectors = functions
                .iter()
                .map(|(key, _)| {
                    let mut values = vec![0.0; self.dimensions];
                    values[0] = 1.0;
                    EmbeddingBatchVector {
                        key: *key,
                        values,
                        truncated: false,
                    }
                })
                .collect();
            Ok(EmbeddingBatch {
                vectors,
                inference_ms: 0.25,
                problem: None,
            })
        }
    }

    struct FakeEmbeddingFactory {
        state: Arc<Mutex<FakeProviderState>>,
        dimensions: usize,
    }

    impl EmbeddingSessionFactory for FakeEmbeddingFactory {
        fn spawn(&mut self) -> Result<Box<dyn EmbeddingSession>, String> {
            self.state.lock().unwrap().spawns += 1;
            Ok(Box::new(FakeEmbeddingSession {
                state: Arc::clone(&self.state),
                dimensions: self.dimensions,
            }))
        }
    }

    struct FixtureEmbedder {
        provider_cache_dir: PathBuf,
        run_timeout: Duration,
        factory: FakeEmbeddingFactory,
    }

    impl RuntimeEmbedder for FixtureEmbedder {
        fn embed(
            &mut self,
            project_root: &Path,
            no_cache: bool,
            inputs: &[EmbeddingInput<'_>],
        ) -> Result<EmbeddingResult, ProviderError> {
            similar_code::embed_selected_with_factory(
                &self.provider_cache_dir,
                project_root,
                no_cache,
                inputs,
                self.run_timeout,
                &mut self.factory,
            )
        }
    }

    fn similar_code_fixture() -> (tempfile::TempDir, PathBuf, SimilarCodeProviderStatus) {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let cache_root = temp.path().join("user-cache");
        let provider_cache_dir = cache_root.join("models").join("fixture-model");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::create_dir_all(&cache_root).unwrap();
        std::fs::write(
            project.join("package.json"),
            r#"{"name":"similar-code-runtime-fixture","private":true}"#,
        )
        .unwrap();
        for (name, value) in [("a", 1), ("b", 2), ("c", 3)] {
            std::fs::write(
                project.join("src").join(format!("{name}.ts")),
                format!(
                    "export function {name}(input: number) {{\n  const adjusted = input + {value};\n  return adjusted * 2;\n}}\n"
                ),
            )
            .unwrap();
        }
        let (model_id, model_revision, dimensions, license) = similar_code::provider_identity();
        let status = SimilarCodeProviderStatus {
            protocol_version: 2,
            embedding_semantics_version: similar_code::embedding_semantics_version(),
            sidecar_version: env!("CARGO_PKG_VERSION").to_owned(),
            model_ready: true,
            model_id: model_id.to_owned(),
            model_revision: model_revision.to_owned(),
            dimensions,
            max_tokens: 512,
            license: license.to_owned(),
            cache_dir: provider_cache_dir.to_string_lossy().into_owned(),
            download_bytes: similar_code::model_download_bytes(),
            analysis_offline: true,
            integrity_verified: true,
            problem: None,
            downloaded: None,
        };
        (temp, project, status)
    }

    fn fixture_options(project: &Path) -> SimilarCodeOptions {
        SimilarCodeOptions {
            analysis: crate::AnalysisOptions {
                root: Some(project.to_path_buf()),
                ..crate::AnalysisOptions::default()
            },
            threshold: Some(0.9),
            min_lines: Some(2),
            ..SimilarCodeOptions::default()
        }
    }

    fn run_with_fixture(
        options: &SimilarCodeOptions,
        status: &SimilarCodeProviderStatus,
        embedder: &mut FixtureEmbedder,
    ) -> ProgrammaticResult<SimilarCodeOutput> {
        let resolved = resolve_programmatic_analysis_context_deferred_workspace(&options.analysis)?;
        resolved
            .install(|| run_similar_code_inner_with_embedder(options, &resolved, status, embedder))
    }

    fn find_cache_file(root: &Path) -> Option<PathBuf> {
        for entry in std::fs::read_dir(root).ok()? {
            let path = entry.ok()?.path();
            if path.file_name().is_some_and(|name| name == "vectors.bin") {
                return Some(path);
            }
            if path.is_dir()
                && let Some(found) = find_cache_file(&path)
            {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn similar_code_runtime_covers_cold_warm_corrupt_cache_scope_and_output_contract() {
        let (_temp, project, status) = similar_code_fixture();
        let provider_cache_dir = PathBuf::from(&status.cache_dir);
        let state = Arc::new(Mutex::new(FakeProviderState::default()));
        let mut embedder = FixtureEmbedder {
            provider_cache_dir: provider_cache_dir.clone(),
            run_timeout: Duration::from_secs(5),
            factory: FakeEmbeddingFactory {
                state: Arc::clone(&state),
                dimensions: status.dimensions,
            },
        };
        let mut options = fixture_options(&project);
        options.files = vec![PathBuf::from("src/a.ts")];

        let cold = run_with_fixture(&options, &status, &mut embedder).unwrap();
        assert!(!cold.candidates.is_empty());
        assert!(cold.candidates.iter().all(|candidate| {
            candidate.left.path == "src/a.ts" || candidate.right.path == "src/a.ts"
        }));
        assert_eq!(
            cold.completion.status,
            SimilarCodeCompletionStatus::Complete
        );
        assert!(cold.completion.cache.misses > 0);
        assert!(cold.completion.cache.writes > 0);
        let cold_spawns = state.lock().unwrap().spawns;
        let cold_ids = cold
            .candidates
            .iter()
            .map(|candidate| candidate.candidate_id.clone())
            .collect::<Vec<_>>();
        let json = serde_json::to_value(&cold).unwrap();
        assert_eq!(json["generation"]["embedding_semantics_version"], 1);
        assert_eq!(json["generation"]["provider"]["source_left_machine"], false);
        assert_eq!(json["generation"]["scope"]["active"], true);
        assert_eq!(
            json["generation"]["scope"]["paths"],
            serde_json::json!(["src/a.ts"])
        );
        assert!(json["completion"]["cache"].is_object());

        let warm = run_with_fixture(&options, &status, &mut embedder).unwrap();
        assert_eq!(state.lock().unwrap().spawns, cold_spawns);
        assert!(warm.completion.cache.hits > 0);
        assert_eq!(warm.completion.cache.writes, 0);
        assert_eq!(
            warm.candidates
                .iter()
                .map(|candidate| candidate.candidate_id.clone())
                .collect::<Vec<_>>(),
            cold_ids
        );

        let cache_root = provider_cache_dir.parent().and_then(Path::parent).unwrap();
        let cache_file = find_cache_file(cache_root).unwrap();
        std::fs::write(&cache_file, b"corrupt cache fixture").unwrap();
        let recovered = run_with_fixture(&options, &status, &mut embedder).unwrap();
        assert_eq!(recovered.completion.cache.invalid_entries, 1);
        assert!(recovered.completion.cache.writes > 0);
        assert!(state.lock().unwrap().spawns > cold_spawns);
    }

    #[test]
    fn snapshot_inspect_survives_ranking_crowd_out_and_rejects_stale_source() {
        let (_temp, project, status) = similar_code_fixture();
        let state = Arc::new(Mutex::new(FakeProviderState::default()));
        let mut embedder = FixtureEmbedder {
            provider_cache_dir: PathBuf::from(&status.cache_dir),
            run_timeout: Duration::from_secs(5),
            factory: FakeEmbeddingFactory {
                state,
                dimensions: status.dimensions,
            },
        };
        let discovery =
            run_with_fixture(&fixture_options(&project), &status, &mut embedder).unwrap();
        let candidate_id = discovery.candidates.last().unwrap().candidate_id.clone();
        let tagged = fallow_output::serialize_similar_code_json_output(
            discovery,
            fallow_output::RootEnvelopeMode::Tagged,
        )
        .unwrap();
        let snapshot = select_similar_code_candidate_snapshot(
            &serde_json::to_vec(&tagged).unwrap(),
            &candidate_id,
        )
        .unwrap();

        let mut legacy_options = fixture_options(&project);
        legacy_options.files = vec![
            PathBuf::from(&snapshot.candidate.left.path),
            PathBuf::from(&snapshot.candidate.right.path),
        ];
        legacy_options.top = Some(1);
        let endpoint_reranked = run_with_fixture(&legacy_options, &status, &mut embedder).unwrap();
        assert_eq!(endpoint_reranked.candidates.len(), 1);
        assert!(
            endpoint_reranked
                .candidates
                .iter()
                .all(|candidate| candidate.candidate_id != candidate_id),
            "the endpoint-only legacy rerank must reproduce the crowd-out condition"
        );

        let inspect_options = SimilarCodeInspectOptions {
            analysis: crate::AnalysisOptions {
                root: Some(project.clone()),
                ..crate::AnalysisOptions::default()
            },
            snapshot: snapshot.clone(),
        };
        let inspected = inspect_similar_code(&inspect_options).unwrap();
        assert_eq!(inspected.candidate.candidate_id, candidate_id);

        let stale_path = project.join(&snapshot.candidate.left.path);
        let stale_source = std::fs::read_to_string(&stale_path).unwrap();
        std::fs::write(
            &stale_path,
            stale_source.replace("return adjusted * 2", "return adjusted * 3"),
        )
        .unwrap();
        let error = inspect_similar_code(&inspect_options).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_SIMILAR_CODE_CANDIDATE_STALE")
        );
    }

    #[test]
    fn snapshot_inspect_rejects_oversized_endpoint_before_source_allocation() {
        let temp = tempfile::tempdir().unwrap();
        let project = dunce::canonicalize(temp.path()).unwrap();
        let source_path = project.join("src/endpoint.ts");
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        std::fs::write(
            &source_path,
            "export function candidate() {\n  return true;\n}\n",
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&source_path)
            .unwrap()
            .set_len(MAX_INSPECT_SOURCE_BYTES + 1)
            .unwrap();

        let error = inspect_side(&project, &location("src/endpoint.ts", 1, 3)).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_SIMILAR_CODE_CANDIDATE_STALE")
        );
        assert!(error.message.contains("5 MiB per-file limit"));
    }

    #[test]
    fn snapshot_inspect_does_not_rebind_an_identical_same_line_function() {
        let temp = tempfile::tempdir().unwrap();
        let project = dunce::canonicalize(temp.path()).unwrap();
        let relative = Path::new("src/duplicates.js");
        let source_path = project.join(relative);
        std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
        let function = "function duplicate() { return 1; }";
        let original = format!("{function} {function}\n");
        std::fs::write(&source_path, &original).unwrap();

        let extracted = fallow_engine::source::similar_code::extract(
            relative,
            &original,
            SimilarCodeExtractionLimits::default(),
        );
        assert_eq!(extracted.functions.len(), 2);
        let snapshot = output_location(&extracted.functions[0]);
        assert_eq!(
            snapshot.source_sha256,
            output_location(&extracted.functions[1]).source_sha256
        );
        assert_ne!(
            snapshot.start_column,
            output_location(&extracted.functions[1]).start_column
        );

        let second_start = function.len() + 1;
        std::fs::write(
            &source_path,
            format!("{}{function}\n", " ".repeat(second_start)),
        )
        .unwrap();

        let error = inspect_side(&project, &snapshot).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_SIMILAR_CODE_CANDIDATE_STALE")
        );
    }

    #[test]
    fn similar_code_runtime_reports_partial_provider_output_and_bounded_timeout() {
        let (_temp, project, status) = similar_code_fixture();
        let provider_cache_dir = PathBuf::from(&status.cache_dir);
        let partial_state = Arc::new(Mutex::new(FakeProviderState {
            complete_batches: Some(2),
            ..FakeProviderState::default()
        }));
        let mut partial_embedder = FixtureEmbedder {
            provider_cache_dir: provider_cache_dir.clone(),
            run_timeout: Duration::from_secs(5),
            factory: FakeEmbeddingFactory {
                state: partial_state,
                dimensions: status.dimensions,
            },
        };
        let mut options = fixture_options(&project);
        options.analysis.no_cache = true;

        let partial = run_with_fixture(&options, &status, &mut partial_embedder).unwrap();
        assert_eq!(
            partial.completion.status,
            SimilarCodeCompletionStatus::Partial
        );
        assert!(partial.completion.skips.iter().any(|skip| {
            skip.phase == SimilarCodePhase::Embedding
                && skip.reason == SimilarCodeSkipReason::ProviderFailure
        }));
        assert!(
            partial
                .diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "FALLOW_SIMILAR_CODE_PROVIDER_PARTIAL" })
        );

        let timeout_state = Arc::new(Mutex::new(FakeProviderState::default()));
        let mut timeout_embedder = FixtureEmbedder {
            provider_cache_dir,
            run_timeout: Duration::ZERO,
            factory: FakeEmbeddingFactory {
                state: Arc::clone(&timeout_state),
                dimensions: status.dimensions,
            },
        };
        let error = run_with_fixture(&options, &status, &mut timeout_embedder).unwrap_err();
        assert_eq!(
            error.code.as_deref(),
            Some("FALLOW_SIMILAR_CODE_PROVIDER_FAILED")
        );
        assert_eq!(timeout_state.lock().unwrap().spawns, 0);
    }

    fn location(path: &str, start_line: u32, end_line: u32) -> SimilarCodeLocation {
        SimilarCodeLocation {
            path: path.to_owned(),
            name: "candidate".to_owned(),
            start_line,
            start_column: 1,
            end_line,
            end_column: 1,
            source_sha256: "00".repeat(32),
        }
    }

    fn file_trace(imports_from: &[&str], imported_by: &[&str]) -> fallow_engine::trace::FileTrace {
        fallow_engine::trace::FileTrace {
            file: PathBuf::from("src/current.ts"),
            is_reachable: true,
            is_entry_point: false,
            exports: Vec::new(),
            imports_from: imports_from.iter().map(PathBuf::from).collect(),
            imported_by: imported_by.iter().map(PathBuf::from).collect(),
            re_exports: Vec::new(),
        }
    }

    fn side_evidence() -> SimilarCodeSideEvidence {
        SimilarCodeSideEvidence {
            source_window: None,
            parameter_count: None,
            is_async: None,
            is_generator: None,
            has_await: None,
            has_throw: None,
            side_effect_hint: None,
            entry_point_reachable: None,
            callers: Vec::new(),
            callees: Vec::new(),
            owners: Vec::new(),
            churn_commits: None,
            tests: Vec::new(),
            deterministic_clone_coverage: None,
            runtime_observations: None,
        }
    }

    #[test]
    fn similar_code_phase_statuses_identify_only_the_incomplete_phase() {
        let phases = phases(
            5,
            5,
            3,
            3,
            3,
            3,
            PhaseCompleteness {
                discovery: true,
                extraction: false,
                embedding: true,
                comparison: true,
            },
            0,
            0,
            0,
        );

        assert_eq!(phases[0].status, SimilarCodePhaseStatus::Complete);
        assert_eq!(phases[1].status, SimilarCodePhaseStatus::Partial);
        assert!(phases[1].reason.is_some());
        assert_eq!(phases[3].status, SimilarCodePhaseStatus::Complete);
        assert_eq!(phases[4].status, SimilarCodePhaseStatus::Complete);
        assert_eq!(phases[5].status, SimilarCodePhaseStatus::Complete);
    }

    #[test]
    fn similar_code_extraction_completion_counts_limits_and_read_failures_honestly() {
        let mut skips = BTreeMap::from([(SimilarCodeSkipReason::BelowMinimumLines, 2)]);
        assert!(extraction_is_complete(&skips, 0));
        assert!(!extraction_is_complete(&skips, 1));

        skips.insert(SimilarCodeSkipReason::InputLimit, 3);
        assert!(!extraction_is_complete(&skips, 0));
        assert_eq!(remaining_extraction_inputs(7, 3), 4);
    }

    #[test]
    fn similar_code_exhausted_extraction_budget_uses_the_specific_skip_reason() {
        assert_eq!(
            exhausted_extraction_limit(0, 1),
            Some(SimilarCodeSkipReason::InputLimit)
        );
        assert_eq!(
            exhausted_extraction_limit(1, 0),
            Some(SimilarCodeSkipReason::SourceBytesLimit)
        );
        assert_eq!(exhausted_extraction_limit(1, 1), None);
    }

    #[test]
    fn similar_code_scope_requires_one_endpoint_to_match_every_active_filter() {
        let root = tempfile::tempdir().unwrap();
        let resolved =
            resolve_programmatic_analysis_context_deferred_workspace(&crate::AnalysisOptions {
                root: Some(root.path().to_path_buf()),
                ..crate::AnalysisOptions::default()
            })
            .unwrap();
        let options = SimilarCodeOptions {
            files: vec![PathBuf::from("src/file-scoped.ts")],
            ..SimilarCodeOptions::default()
        };
        let changed = FxHashSet::from_iter([PathBuf::from("src/changed.ts")]);

        assert!(similar_code_scope_active(
            &options,
            &resolved,
            Some(&changed),
            None,
        ));
        assert!(!similar_code_path_in_scope(
            "src/file-scoped.ts",
            &options,
            &resolved,
            Some(&changed),
            None,
        ));
        assert!(!similar_code_path_in_scope(
            "src/changed.ts",
            &options,
            &resolved,
            Some(&changed),
            None,
        ));

        let changed = FxHashSet::from_iter([PathBuf::from("src/file-scoped.ts")]);
        assert!(similar_code_path_in_scope(
            "src/file-scoped.ts",
            &options,
            &resolved,
            Some(&changed),
            None,
        ));
    }

    #[test]
    fn similar_code_module_references_are_sorted_deduplicated_and_bounded() {
        let paths = vec![
            PathBuf::from("src/z.ts"),
            PathBuf::from("src/a.ts"),
            PathBuf::from("src/a.ts"),
        ];

        let (references, truncated) = bounded_module_references(&paths, 1);

        assert!(truncated);
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].path, "src/a.ts");
        assert_eq!(references[0].name, MODULE_REFERENCE_NAME);
        assert_eq!(references[0].line, 1);
    }

    #[test]
    fn similar_code_related_tests_are_transitive_path_filtered_and_bounded() {
        let paths = vec![
            "src/helper.ts".to_owned(),
            "tests/z.spec.ts".to_owned(),
            "src/a.test.ts".to_owned(),
            "src/a.test.ts".to_owned(),
        ];

        let (tests, truncated) = bounded_related_tests(&paths, 1);

        assert!(truncated);
        assert_eq!(tests, vec!["src/a.test.ts"]);
    }

    #[test]
    fn similar_code_module_relationship_uses_direct_edges_then_shared_importers() {
        let left_location = location("src/left.ts", 1, 3);
        let right_location = location("src/right.ts", 1, 3);
        let left = file_trace(&["src/right.ts"], &["src/shared.ts"]);
        let right = file_trace(&[], &["src/shared.ts"]);

        assert_eq!(
            module_relationship(&left, &right, &left_location, &right_location),
            "left-directly-imports-right"
        );

        let left = file_trace(&[], &["src/shared.ts"]);
        assert_eq!(
            module_relationship(&left, &right, &left_location, &right_location),
            "shared-direct-importer"
        );
    }

    #[test]
    fn similar_code_primary_owner_uses_the_codeowners_winning_rule() {
        let codeowners = CodeOwners::parse("/src/* @team/base\n/src/special.ts @team/special")
            .expect("CODEOWNERS parses");

        assert_eq!(
            primary_owner(&codeowners, "src/special.ts"),
            vec!["@team/special"]
        );
        assert!(primary_owner(&codeowners, "test/a.ts").is_empty());
    }

    #[test]
    fn similar_code_churn_lookup_normalizes_path_separators_and_defaults_to_zero() {
        let mut files = FxHashMap::default();
        files.insert(
            PathBuf::from("C:/repo/src/a.ts"),
            fallow_engine::churn::FileChurn {
                path: PathBuf::from("C:/repo/src/a.ts"),
                commits: 7,
                weighted_commits: 0.0,
                lines_added: 0,
                lines_deleted: 0,
                trend: fallow_engine::churn::ChurnTrend::Stable,
                authors: FxHashMap::default(),
            },
        );
        let churn = fallow_engine::churn::ChurnResult {
            files,
            shallow_clone: false,
            author_pool: Vec::new(),
        };

        assert_eq!(churn_commits_for(&churn, Path::new(r"C:\repo\src\a.ts")), 7);
        assert_eq!(churn_commits_for(&churn, Path::new("src/missing.ts")), 0);
    }

    #[test]
    fn similar_code_clone_coverage_counts_unique_exact_lines_only() {
        let root = Path::new("/repo");
        let exact = fallow_engine::duplicates::CloneGroup {
            instances: vec![
                clone_instance("/repo/src/a.ts", 2, 4),
                clone_instance("/repo/src/a.ts", 4, 6),
            ],
            token_count: 10,
            line_count: 3,
            similarity: None,
        };
        let near = fallow_engine::duplicates::CloneGroup {
            instances: vec![clone_instance("/repo/src/a.ts", 7, 9)],
            token_count: 10,
            line_count: 3,
            similarity: Some(0.9),
        };
        let report = fallow_engine::duplicates::DuplicationReport {
            clone_groups: vec![exact, near],
            ..fallow_engine::duplicates::DuplicationReport::default()
        };

        assert_eq!(
            deterministic_clone_coverage(&report, root, &location("src/a.ts", 1, 10)),
            0.5
        );
    }

    #[test]
    fn similar_code_project_artifacts_enrich_both_sides_deterministically() {
        let project = tempfile::tempdir().expect("temporary project");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).expect("source directory");
        std::fs::create_dir_all(root.join("tests")).expect("test directory");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"inspect-enrichment","main":"src/index.ts"}"#,
        )
        .expect("package manifest");
        let repeated = "export function candidate(values: number[]) {\n  const positive = values.filter((value) => value > 0);\n  const doubled = positive.map((value) => value * 2);\n  const total = doubled.reduce((sum, value) => sum + value, 0);\n  return { total, count: doubled.length };\n}\n";
        std::fs::write(root.join("src/left.ts"), repeated).expect("left source");
        std::fs::write(root.join("src/right.ts"), repeated).expect("right source");
        std::fs::write(
            root.join("src/index.ts"),
            "import { candidate as left } from './left';\nimport { candidate as right } from './right';\nconsole.log(left([1]), right([2]));\n",
        )
        .expect("entry source");
        std::fs::write(
            root.join("tests/left.test.ts"),
            "import { candidate } from '../src/left';\ntest('candidate', () => expect(candidate([1])).toBeTruthy());\n",
        )
        .expect("test source");

        let session = AnalysisSession::load_with_config(root, None, |config| {
            config.duplicates.min_tokens = 5;
            config.duplicates.min_lines = 2;
        })
        .expect("analysis session");
        let left_location = location("src/left.ts", 1, 6);
        let right_location = location("src/right.ts", 1, 6);
        let mut left = side_evidence();
        let mut right = side_evidence();
        let mut result = InspectEnrichment {
            availability: unavailable_inspect_enrichment(),
            graph_relationship: None,
            diagnostics: Vec::new(),
        };

        enrich_graph_and_clones(
            &session,
            &left_location,
            &right_location,
            &mut left,
            &mut right,
            &mut result,
        );

        assert_eq!(
            result.graph_relationship.as_deref(),
            Some("shared-direct-importer")
        );
        assert_eq!(
            result.availability.entry_point_reachability,
            SimilarCodeEnrichmentState::Available
        );
        assert_eq!(left.entry_point_reachable, Some(true));
        assert!(
            left.callers
                .iter()
                .any(|reference| reference.path == "src/index.ts")
        );
        assert_eq!(left.tests, vec!["tests/left.test.ts"]);
        assert!(
            left.deterministic_clone_coverage
                .is_some_and(|value| value > 0.0)
        );
        assert!(
            right
                .deterministic_clone_coverage
                .is_some_and(|value| value > 0.0)
        );
    }

    fn clone_instance(
        file: &str,
        start_line: usize,
        end_line: usize,
    ) -> fallow_engine::duplicates::CloneInstance {
        fallow_engine::duplicates::CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }
}
