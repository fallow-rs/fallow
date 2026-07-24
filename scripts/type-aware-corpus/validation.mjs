const equalJson = (left, right) => JSON.stringify(left) === JSON.stringify(right);

const requireValid = (condition, message, fail) => {
  if (!condition) fail(message);
};

const sortedUniqueStrings = (values) => {
  if (!Array.isArray(values)) return false;
  const sorted = values.toSorted();
  return [equalJson(values, sorted), new Set(values).size === values.length].every(Boolean);
};

const validateSupplementalContext = (artifact, context, dependencies) => {
  const { fail, isObject } = dependencies;
  requireValid(
    [isObject(artifact), artifact.schema_version === 6].every(Boolean),
    "supplemental artifact schema_version must be 6",
    fail,
  );
  requireValid(
    [
      isObject(context),
      /^[0-9a-f]{64}$/.test(context.fallowSha256),
      /^[0-9a-f]{64}$/.test(context.sidecarSha256),
      typeof context.sourceRoot === "string",
    ].every(Boolean),
    "supplemental validation requires runtime hashes and a source root",
    fail,
  );
};

const validateSupplementalProvenance = (artifact, context, fail) => {
  requireValid(
    [
      artifact.artifacts.fallow_sha256 === context.fallowSha256,
      artifact.artifacts.sidecar_sha256 === context.sidecarSha256,
    ].every(Boolean),
    "supplemental artifact runtime hashes do not match the validated binaries",
    fail,
  );
  requireValid(
    equalJson(artifact.project.dependency_environment, context.dependencyEnvironment),
    "supplemental artifact dependency provenance does not match",
    fail,
  );
};

const supplementalReview = (decisions) =>
  decisions.supplemental_reviews.find(({ project_id: projectId }) => projectId === "vitest");

const validateSupplementalKeySets = (confirmedKeys, reviewedKeys, fail) => {
  requireValid(
    [sortedUniqueStrings(confirmedKeys), sortedUniqueStrings(reviewedKeys)].every(Boolean),
    "supplemental candidate key sets must be sorted and unique",
    fail,
  );
};

const supplementalReviewBindingsValid = (
  artifact,
  review,
  confirmedKeys,
  reviewedKeys,
  candidateSetDigest,
) =>
  [
    Boolean(review),
    artifact.project.commit === review.commit,
    artifact.independent_review.verdict === "approved",
    artifact.independent_review.reviewed_candidate_count === reviewedKeys.length,
    artifact.independent_review.reviewed_candidate_set_sha256 === candidateSetDigest(reviewedKeys),
    equalJson(reviewedKeys, review.candidate_keys),
  ].every(Boolean);

const supplementalResultCountsValid = (artifact, confirmedKeys, reviewedKeys, candidateSetDigest) =>
  [
    artifact.result.confirmed_used === confirmedKeys.length,
    artifact.result.confirmed_candidate_set_sha256 === candidateSetDigest(confirmedKeys),
    confirmedKeys.every((key) => reviewedKeys.includes(key)),
    artifact.independent_review.clean_run_is_subset === true,
    artifact.result.baseline_candidates - artifact.result.refined_candidates ===
      artifact.result.confirmed_used,
    artifact.result.refined_candidates ===
      artifact.result.unresolved_retained + artifact.result.abstained_retained,
  ].every(Boolean);

const validateSupplementalReview = (
  artifact,
  decisions,
  confirmedKeys,
  reviewedKeys,
  dependencies,
) => {
  const { candidateSetDigest, fail } = dependencies;
  const review = supplementalReview(decisions);
  requireValid(
    [
      supplementalReviewBindingsValid(
        artifact,
        review,
        confirmedKeys,
        reviewedKeys,
        candidateSetDigest,
      ),
      supplementalResultCountsValid(artifact, confirmedKeys, reviewedKeys, candidateSetDigest),
    ].every(Boolean),
    "supplemental artifact counts, hashes, or review binding are inconsistent",
    fail,
  );
};

const supplementalRunsValid = (runs) =>
  [
    runs.length === 2,
    equalJson(runs.map(({ iteration }) => iteration).toSorted(), [0, 1]),
    new Set(runs.map(({ normalized_sha256: digest }) => digest)).size === 1,
    runs.every(({ normalized_sha256: digest }) => /^[0-9a-f]{64}$/.test(digest)),
  ].every(Boolean);

const validateSupplementalSourceFile = (run, mode, seenPaths, context, dependencies) => {
  const { fail, normalizedRelativePath, resolve, relative, sep, ensureFile, normalizedFileDigest } =
    dependencies;
  const runPath = normalizedRelativePath(run.path, `supplemental ${mode} source path`);
  requireValid(!seenPaths.has(runPath), `duplicate supplemental source path: ${runPath}`, fail);
  seenPaths.add(runPath);
  const absolutePath = resolve(context.sourceRoot, runPath);
  const relativeToRoot = relative(context.sourceRoot, absolutePath);
  requireValid(
    [!relativeToRoot.startsWith(`..${sep}`), relativeToRoot !== ".."].every(Boolean),
    `supplemental source path escapes its root: ${runPath}`,
    fail,
  );
  ensureFile(absolutePath, `supplemental ${mode} source run`);
  requireValid(
    normalizedFileDigest(absolutePath) === run.normalized_sha256,
    `supplemental ${mode} source hash does not match ${runPath}`,
    fail,
  );
};

const validateSupplementalMode = (sourceRuns, mode, seenPaths, context, dependencies) => {
  const matching = sourceRuns.filter((run) => run.mode === mode);
  requireValid(
    supplementalRunsValid(matching),
    `supplemental ${mode} source runs are incomplete or nondeterministic`,
    dependencies.fail,
  );
  matching.forEach((run) =>
    validateSupplementalSourceFile(run, mode, seenPaths, context, dependencies),
  );
};

export const validateSupplementalArtifactData = (artifact, decisions, context, dependencies) => {
  validateSupplementalContext(artifact, context, dependencies);
  validateSupplementalProvenance(artifact, context, dependencies.fail);
  const confirmedKeys = artifact.result.confirmed_candidate_keys;
  const reviewedKeys = artifact.independent_review.reviewed_candidate_keys;
  validateSupplementalKeySets(confirmedKeys, reviewedKeys, dependencies.fail);
  validateSupplementalReview(artifact, decisions, confirmedKeys, reviewedKeys, dependencies);
  const sourceRuns = artifact.artifacts.source_runs;
  requireValid(
    [Array.isArray(sourceRuns), sourceRuns.length === 4].every(Boolean),
    "supplemental artifact requires two baseline and two refined source runs",
    dependencies.fail,
  );
  const seenPaths = new Set();
  ["baseline", "refined"].forEach((mode) =>
    validateSupplementalMode(sourceRuns, mode, seenPaths, context, dependencies),
  );
  return artifact;
};

const validateCapabilitiesHeader = (artifact, context, dependencies) => {
  const { fail, isObject, requiredCapabilities } = dependencies;
  requireValid(
    [isObject(artifact), artifact.schema_version === 3].every(Boolean),
    "semantic capabilities artifact schema_version must be 3",
    fail,
  );
  requireValid(
    [
      artifact.artifacts.fallow_sha256 === context.fallowSha256,
      artifact.artifacts.sidecar_sha256 === context.sidecarSha256,
    ].every(Boolean),
    "semantic capabilities artifact runtime hashes do not match",
    fail,
  );
  requireValid(
    equalJson(artifact.excludes, ["compiler-diagnostics", "syntax-and-style-lint-rules"]),
    "semantic capabilities artifact must exclude tsc and Oxlint responsibilities",
    fail,
  );
  requireValid(
    [
      equalJson(artifact.coverage.capability_ids, requiredCapabilities),
      artifact.coverage.repository_count === 2,
      artifact.coverage.all_capabilities_proven_on_each_repository === true,
    ].every(Boolean),
    "semantic capabilities artifact does not cover all five capabilities on both projects",
    fail,
  );
};

const programsValid = (programs, expectedReuse) =>
  [
    programs.length > 0,
    programs.every((program) =>
      [
        typeof program.config === "string",
        program.status === "complete",
        Number.isSafeInteger(program.source_file_count),
        program.source_file_count > 0,
        program.program_reused === expectedReuse,
      ].every(Boolean),
    ),
  ].every(Boolean);

const validateCapabilityRepositoryHeader = (repository, context, dependencies) => {
  const { fail, requiredCapabilities } = dependencies;
  const root = context.roots[repository.id];
  requireValid(
    [
      typeof root === "string",
      repository.commit === context.commits[repository.id],
      repository.tracked_source_clean === true,
      equalJson(repository.dependency_environment, context.dependencyEnvironments[repository.id]),
    ].every(Boolean),
    `${repository.id} capability provenance does not match its pinned clean source`,
    fail,
  );
  requireValid(
    equalJson(Object.keys(repository.capabilities), requiredCapabilities),
    `${repository.id} does not contain all five semantic capabilities`,
    fail,
  );
  requireValid(
    programsValid(repository.programs.inspect, true) &&
      programsValid(repository.programs.coupling, false),
    `${repository.id} Program reuse metadata does not match its semantic query batches`,
    fail,
  );
  return root;
};

const validateRefinementCapability = (repository, root, dependencies) => {
  const capability = repository.capabilities["dead-code-refinement"];
  requireValid(
    [
      capability.assertion === "confirmed-used",
      capability.confirmed_used_count > 0,
      capability.reviewed === true,
    ].every(Boolean),
    `${repository.id} has no reviewed dead-code refinement proof`,
    dependencies.fail,
  );
  dependencies.validateEvidence(
    root,
    capability.source_evidence,
    `${repository.id} dead-code refinement`,
  );
};

const validateTraceCapability = (repository, root, dependencies) => {
  const capability = repository.capabilities["semantic-symbol-trace"];
  requireValid(
    [
      capability.assertion === "references-found",
      ["complete", "partial"].includes(capability.status),
      capability.total_reference_count > 0,
      capability.checker_evidence_count > 0,
    ].every(Boolean),
    `${repository.id} has no semantic symbol trace proof`,
    dependencies.fail,
  );
  dependencies.validateEvidence(root, capability.source_evidence, `${repository.id} symbol trace`);
};

const validateApiCapability = (repository, root, dependencies) => {
  const capability = repository.capabilities["public-api-surface"];
  requireValid(
    [
      ["leak-confirmed", "no-leak-confirmed"].includes(capability.assertion),
      ["complete", "partial"].includes(capability.status),
      capability.public_entry_sample_count > 0,
      Number.isSafeInteger(capability.private_type_leak_sample_count),
      Array.isArray(capability.omissions),
    ].every(Boolean),
    `${repository.id} has no public API surface proof`,
    dependencies.fail,
  );
  dependencies.validateEvidence(root, capability.source_evidence, `${repository.id} API surface`);
};

const validateImpactCapability = (repository, root, dependencies) => {
  const capability = repository.capabilities["semantic-impact-targeted-tests"];
  requireValid(
    [
      capability.assertion === "consumers-found",
      ["complete", "partial"].includes(capability.status),
      capability.direct_consumer_count > 0,
      capability.targeted_test_count > 0,
      Array.isArray(capability.targeted_tests),
      capability.targeted_tests.length > 0,
    ].every(Boolean),
    `${repository.id} has no impact or targeted-test proof`,
    dependencies.fail,
  );
  dependencies.validateEvidence(root, capability.source_evidence, `${repository.id} impact`);
};

const couplingSummaryValid = (summary) =>
  [
    summary.scope === "project-local-public-signatures",
    summary.direction === "directed",
    summary.project_size > 0,
    summary.distinct_coupled_files > 0,
    summary.edge_count > 0,
    Number.isFinite(summary.coupled_file_pct),
    Number.isFinite(summary.p50_distinct_connections),
    Number.isFinite(summary.p90_distinct_connections),
    Number.isFinite(summary.concentration),
  ].every(Boolean);

const validateCouplingCapability = (repository, root, dependencies) => {
  const capability = repository.capabilities["public-type-coupling"];
  requireValid(
    [
      capability.assertion === "coupling-found",
      ["complete", "partial"].includes(capability.status),
      couplingSummaryValid(capability.summary),
      Array.isArray(capability.top_contributors),
      capability.top_contributors.length > 0,
      Array.isArray(capability.cycles),
      capability.cycles.length > 0,
    ].every(Boolean),
    `${repository.id} has no rich public type-coupling proof`,
    dependencies.fail,
  );
  dependencies.validateEvidence(root, capability.source_evidence, `${repository.id} type coupling`);
};

const validateCapabilityRepository = (repository, context, dependencies) => {
  const root = validateCapabilityRepositoryHeader(repository, context, dependencies);
  validateRefinementCapability(repository, root, dependencies);
  validateTraceCapability(repository, root, dependencies);
  validateApiCapability(repository, root, dependencies);
  validateImpactCapability(repository, root, dependencies);
  validateCouplingCapability(repository, root, dependencies);
};

export const validateCapabilitiesArtifactData = (artifact, context, dependencies) => {
  validateCapabilitiesHeader(artifact, context, dependencies);
  const repositories = artifact.repositories;
  requireValid(
    [
      Array.isArray(repositories),
      equalJson(
        repositories.map(({ id }) => id),
        ["astro", "vitest"],
      ),
    ].every(Boolean),
    "semantic capabilities artifact requires Astro and Vitest in stable order",
    dependencies.fail,
  );
  repositories.forEach((repository) =>
    validateCapabilityRepository(repository, context, dependencies),
  );
  return artifact;
};

const validateMeasurementHeader = (discovery, measurements, fail, isObject) => {
  requireValid(
    [isObject(measurements), measurements.schema_version === 1].every(Boolean),
    "measurement artifact schema_version must be 1",
    fail,
  );
  requireValid(
    equalJson(discovery.corpus, measurements.corpus),
    "measurement corpus identity does not match discovery; rerun measure",
    fail,
  );
  requireValid(
    [Number.isSafeInteger(measurements.warmups), measurements.warmups >= 1].every(Boolean),
    "measurements require at least one warmup pair",
    fail,
  );
  requireValid(
    [Number.isSafeInteger(measurements.measured_pairs), measurements.measured_pairs >= 4].every(
      Boolean,
    ),
    "measurements require at least four measured pairs",
    fail,
  );
};

const validateMeasurementRunShape = (projectId, run, measurements, dependencies) => {
  const { fail, isObject, platform } = dependencies;
  requireValid(
    [isObject(run), typeof run.warmup === "boolean"].every(Boolean),
    `${projectId} measurement warmup must be a boolean`,
    fail,
  );
  requireValid(
    ["baseline", "refined"].includes(run.mode),
    `${projectId} measurement mode must be baseline or refined`,
    fail,
  );
  const iterationLimit = run.warmup ? measurements.warmups : measurements.measured_pairs;
  requireValid(
    [Number.isSafeInteger(run.iteration), run.iteration >= 0, run.iteration < iterationLimit].every(
      Boolean,
    ),
    `${projectId} measurement iteration is outside its expected range`,
    fail,
  );
  requireValid(
    [Number.isFinite(run.wall_ms), run.wall_ms > 0].every(Boolean),
    `${projectId} measurement has an invalid wall time`,
    fail,
  );
  const rssValid = [
    Number.isFinite(run.peak_process_tree_rss_kb),
    run.peak_process_tree_rss_kb > 0,
  ].every(Boolean);
  requireValid(
    platform === "win32" ? true : rssValid,
    `${projectId} measurement has no process-tree RSS sample`,
    fail,
  );
};

const recordMeasurementPair = (projectId, run, pairs, fail) => {
  const kind = run.warmup ? "warmup" : "measured";
  const key = `${kind}:${run.iteration}`;
  const modes = pairs.get(key);
  const currentModes = modes instanceof Set ? modes : new Set();
  requireValid(
    !currentModes.has(run.mode),
    `${projectId} measurements contain a duplicate ${key}:${run.mode} run`,
    fail,
  );
  currentModes.add(run.mode);
  pairs.set(key, currentModes);
};

const validateMeasurementRun = (project, discovered, run, measurements, pairs, dependencies) => {
  validateMeasurementRunShape(project.id, run, measurements, dependencies);
  const counts = {
    baseline: discovered.baseline_candidate_count,
    refined: discovered.refined_candidate_count,
  };
  requireValid(
    run.candidate_count === counts[run.mode],
    `${project.id} ${run.mode} candidate count drifted during measurement`,
    dependencies.fail,
  );
  recordMeasurementPair(project.id, run, pairs, dependencies.fail);
};

const completeMeasurementPair = (modes) =>
  [modes.size === 2, modes.has("baseline"), modes.has("refined")].every(Boolean);

const validateMeasurementProject = (project, discovered, measurements, dependencies) => {
  const expectedRunCount = 2 * (measurements.warmups + measurements.measured_pairs);
  requireValid(
    [Array.isArray(project.runs), project.runs.length === expectedRunCount].every(Boolean),
    `${project.id} measurements have an incomplete run matrix`,
    dependencies.fail,
  );
  const pairs = new Map();
  project.runs.forEach((run) =>
    validateMeasurementRun(project, discovered, run, measurements, pairs, dependencies),
  );
  requireValid(
    [
      pairs.size === measurements.warmups + measurements.measured_pairs,
      [...pairs.values()].every(completeMeasurementPair),
    ].every(Boolean),
    `${project.id} measurements are missing a baseline or refined pair`,
    dependencies.fail,
  );
};

const comparableProvenance = (provenance) => ({
  manifest_sha256: provenance.manifest_sha256,
  fallow: provenance.fallow,
  sidecar: provenance.sidecar,
  runtime: provenance.runtime,
  dependency_environments: provenance.dependency_environments,
  fixtures: provenance.fixtures,
});

export const validateMeasurements = (discovery, measurements, dependencies) => {
  validateMeasurementHeader(discovery, measurements, dependencies.fail, dependencies.isObject);
  const expectedProjects = new Map(discovery.projects.map((project) => [project.id, project]));
  requireValid(
    [
      Array.isArray(measurements.projects),
      measurements.projects.length === expectedProjects.size,
    ].every(Boolean),
    "measurements must contain every discovery project exactly once",
    dependencies.fail,
  );
  const seen = new Set();
  measurements.projects.forEach((project) => {
    const discovered = expectedProjects.get(project.id);
    requireValid(
      [Boolean(discovered), !seen.has(project.id)].every(Boolean),
      `invalid measurement project ${project.id}`,
      dependencies.fail,
    );
    seen.add(project.id);
    validateMeasurementProject(project, discovered, measurements, dependencies);
  });
  requireValid(
    equalJson(
      comparableProvenance(discovery.provenance),
      comparableProvenance(measurements.provenance),
    ),
    "discovery and measurement provenance differ; rerun both with the same artifacts",
    dependencies.fail,
  );
};
