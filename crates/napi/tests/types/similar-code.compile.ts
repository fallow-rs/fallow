import {
  detectSimilarCode,
  type SimilarCodeAction,
  type SimilarCodeDiagnostic,
  type SimilarCodeGeneration,
  type SimilarCodeReport,
  type SimilarCodeSkip,
} from "../../types/index.js";

const consumeSimilarCode = async (): Promise<void> => {
  const report: SimilarCodeReport = await detectSimilarCode({
    root: ".",
    files: ["src/index.ts"],
    threshold: 0.8,
    minLines: 3,
    top: 10,
  });
  const generation: SimilarCodeGeneration = report.generation;
  const embeddingSemanticsVersion: number = generation.embedding_semantics_version;
  const extractionSemanticsVersion: number = generation.extraction_semantics_version;
  const provider: "official-local-companion" = generation.provider.provider;
  const sourceStayedLocal: false = generation.provider.source_left_machine;
  const companionVersion: string = generation.provider.companion_version;
  const protocolVersion: number = generation.provider.protocol_version;
  const modelId: string = generation.model.model_id;
  const modelRevision: string = generation.model.revision;
  const modelDigest: string = generation.model.artifact_sha256;
  const modelLicense: string = generation.model.license;
  const modelDimensions: number = generation.model.dimensions;
  const dtype: string = generation.parameters.dtype;
  const pooling: string = generation.parameters.pooling;
  const normalized: boolean = generation.parameters.normalized;
  const batchSize: number = generation.parameters.batch_size;
  const maxTokens: number = generation.parameters.max_tokens;
  const parameterDigest: string = generation.parameters.parameter_sha256;
  const scopeActive: boolean = generation.scope.active;
  const scopePaths: string[] = generation.scope.paths;
  const threshold: number = generation.threshold;
  const minLines: number = generation.min_lines;
  const skip: SimilarCodeSkip | undefined = report.completion.skips[0];
  const diagnostic: SimilarCodeDiagnostic | undefined = report.diagnostics[0];
  const action: SimilarCodeAction | undefined = report.candidates[0]?.actions[0];

  if (skip !== undefined) {
    const skippedPhase: typeof skip.phase = skip.phase;
    const skippedReason: typeof skip.reason = skip.reason;
    const skippedCount: number = skip.count;
    void [skippedPhase, skippedReason, skippedCount];
  }
  if (diagnostic !== undefined) {
    const domain: typeof diagnostic.domain = diagnostic.domain;
    const code: string = diagnostic.code;
    const message: string = diagnostic.message;
    const path: string | null = diagnostic.path;
    void [domain, code, message, path];
  }
  if (action !== undefined) {
    const actionType: "inspect" | "review" = action.action;
    const description: string = action.description;
    const readOnly: true = action.read_only;
    void [actionType, description, readOnly];
  }
  const candidate = report.candidates[0];
  if (candidate !== undefined) {
    const candidateId: string = candidate.candidate_id;
    const reviewKey: string = candidate.review_key;
    const score: number = candidate.similarity;
    const band: "moderate" | "high" | "very-high" = candidate.similarity_band;
    const verification: "unverified" = candidate.verification_status;
    const leftPath: string = candidate.left.path;
    const rightDigest: string = candidate.right.source_sha256;
    const enrichmentState: typeof candidate.enrichment.callers = candidate.enrichment.callers;
    void [
      candidateId,
      reviewKey,
      score,
      band,
      verification,
      leftPath,
      rightDigest,
      enrichmentState,
    ];
  }
  const phase = report.completion.phases[0];
  if (phase !== undefined) {
    const phaseName: typeof phase.phase = phase.phase;
    const phaseStatus: typeof phase.status = phase.status;
    const processed: number = phase.processed;
    const total: number | null = phase.total;
    const reason: string | null = phase.reason;
    void [phaseName, phaseStatus, processed, total, reason];
  }
  const completionStatus: "complete" | "partial" = report.completion.status;
  const cacheStatus: typeof report.completion.cache.status = report.completion.cache.status;
  const cacheHits: number = report.completion.cache.hits;
  const cacheMisses: number = report.completion.cache.misses;
  const cacheWrites: number = report.completion.cache.writes;
  const invalidEntries: number = report.completion.cache.invalid_entries;
  const maxFiles: number = report.completion.limits.max_files;
  const maxFunctions: number = report.completion.limits.max_functions;
  const maxSourceBytes: number = report.completion.limits.max_source_bytes;
  const maxFunctionBytes: number = report.completion.limits.max_function_bytes;
  const maxBatchSize: number = report.completion.limits.max_batch_size;
  const maxVectorBytes: number = report.completion.limits.max_vector_bytes;
  const maxComparisons: number = report.completion.limits.max_comparisons;
  const maxCandidates: number = report.completion.limits.max_candidates;
  const maxNeighbors: number = report.completion.limits.max_neighbors_per_function;
  const timeoutMs: number = report.completion.limits.timeout_ms;
  const providerInferenceMs: number = report.completion.provider_inference_ms;

  void [
    report.kind,
    report.schema_version,
    report.version,
    report.elapsed_ms,
    embeddingSemanticsVersion,
    extractionSemanticsVersion,
    provider,
    sourceStayedLocal,
    companionVersion,
    protocolVersion,
    modelId,
    modelRevision,
    modelDigest,
    modelLicense,
    modelDimensions,
    dtype,
    pooling,
    normalized,
    batchSize,
    maxTokens,
    parameterDigest,
    scopeActive,
    scopePaths,
    threshold,
    minLines,
    completionStatus,
    cacheStatus,
    cacheHits,
    cacheMisses,
    cacheWrites,
    invalidEntries,
    maxFiles,
    maxFunctions,
    maxSourceBytes,
    maxFunctionBytes,
    maxBatchSize,
    maxVectorBytes,
    maxComparisons,
    maxCandidates,
    maxNeighbors,
    timeoutMs,
    providerInferenceMs,
  ];
};

void consumeSimilarCode;
