const isValidDiscovery = (discovery, isObject) =>
  [
    isObject(discovery),
    Object(discovery).schema_version === 1,
    Array.isArray(Object(discovery).projects),
  ].every(Boolean);

const isLegacyLedger = (ledger, isObject) =>
  [
    isObject(ledger),
    Object(ledger).schema_version === 1,
    Array.isArray(Object(ledger).candidates),
  ].every(Boolean);

const isValidLedger = (ledger, isObject) =>
  [
    isObject(ledger),
    Object(ledger).schema_version === 2,
    Array.isArray(Object(ledger).candidates),
  ].every(Boolean);

export const evidenceLocationValid = (location, dependencies) => {
  const { isAbsolute, isObject } = dependencies;
  const value = Object(location);
  return [
    isObject(location),
    typeof value.path === "string",
    String(value.path).trim() !== "",
    !isAbsolute(String(value.path)),
    Number.isSafeInteger(value.line),
    value.line > 0,
    Number.isSafeInteger(value.col),
    value.col >= 0,
    typeof value.excerpt === "string",
    String(value.excerpt).trim() !== "",
  ].every(Boolean);
};

const featureBucketListValid = (buckets) =>
  [
    Array.isArray(buckets),
    buckets.length > 0,
    buckets.every((bucket) => [typeof bucket === "string", bucket.trim() !== ""].every(Boolean)),
    new Set(buckets).size === buckets.length,
  ].every(Boolean);

const optionalFeatureBucketListValid = (buckets) =>
  buckets === undefined ? true : featureBucketListValid(buckets);

const discoveryCandidates = (discovery) =>
  discovery.projects.flatMap((project) =>
    (project.candidates ?? []).map((candidate) => ({ candidate, projectId: project.id })),
  );

const describeError = (error) => (error instanceof Error ? error.message : String(error));

const indexExpectedCandidate = (expected, errors, item, candidateKey) => {
  const { candidate, projectId } = item;
  try {
    const computedKey = candidateKey(projectId, candidate);
    if (candidate.key !== computedKey) {
      errors.push(`${candidate.key}: discovery candidate key is not stable`);
    }
  } catch (error) {
    const key = candidate.key ?? "unknown";
    errors.push(`${key}: invalid discovery candidate: ${describeError(error)}`);
  }
  expected.set(candidate.key, { ...candidate, project_id: projectId });
};

const indexExpected = (discovery, errors, candidateKey) => {
  const expected = new Map();
  discoveryCandidates(discovery).forEach((item) =>
    indexExpectedCandidate(expected, errors, item, candidateKey),
  );
  return expected;
};

const indexActualEntry = (actual, errors, entry, isObject) => {
  if (![isObject(entry), typeof entry.key === "string"].every(Boolean)) {
    errors.push("ledger entry is missing a candidate key");
    return;
  }
  if (actual.has(entry.key)) errors.push(`${entry.key}: duplicate ledger entry`);
  actual.set(entry.key, entry);
};

const indexActual = (ledger, errors, isObject) => {
  const actual = new Map();
  ledger.candidates.forEach((entry) => indexActualEntry(actual, errors, entry, isObject));
  return actual;
};

const validateCandidateFields = (key, entry, candidate, errors, candidateFields) => {
  try {
    const fieldsMatch =
      JSON.stringify(candidateFields(entry.candidate)) ===
      JSON.stringify(candidateFields(candidate));
    if (!fieldsMatch) errors.push(`${key}: candidate fields do not match discovery`);
  } catch {
    errors.push(`${key}: ledger candidate fields are incomplete`);
  }
};

const validateEntryIdentity = (key, entry, candidate, errors, dependencies) => {
  if (entry.project_id !== candidate.project_id) {
    errors.push(`${key}: project_id does not match discovery`);
  }
  validateCandidateFields(key, entry, candidate, errors, dependencies.candidateFields);
  if (!dependencies.truthStatuses.has(entry.truth)) {
    errors.push(`${key}: truth must be used, preserved, unused, or indeterminate`);
  }
  if (entry.semantic_status !== candidate.semantic_status) {
    errors.push(`${key}: semantic_status does not match discovery`);
  }
  if (entry.semantic_decision !== candidate.semantic_decision) {
    errors.push(`${key}: semantic_decision does not match discovery`);
  }
};

const validateConfirmedTruth = (key, entry, errors) => {
  const confirmationMismatch = [
    entry.semantic_status === "confirmed-used",
    entry.truth !== "used",
  ].every(Boolean);
  if (confirmationMismatch) {
    errors.push(`${key}: every confirmed removal must be adjudicated used`);
  }
  const contractMismatch = [
    entry.semantic_status === "contract-preserved",
    entry.truth !== "preserved",
  ].every(Boolean);
  if (contractMismatch) {
    errors.push(`${key}: every contract-preserved removal must be adjudicated preserved`);
  }
};

const validateContract = (key, entry, errors, dependencies) => {
  const requiresContract = entry.semantic_status === "contract-preserved";
  const contract = Object(entry.contract);
  const validEvidence = [
    typeof contract.relation === "string",
    typeof contract.optional === "boolean",
    evidenceLocationValid(contract.declaration, dependencies),
  ].every(Boolean);
  const validRequiredContract = validEvidence && contract.optional === false;
  if ([requiresContract, !validRequiredContract].every(Boolean)) {
    errors.push(`${key}: contract-preserved candidates require exact contract evidence`);
  }
  if ([!requiresContract, entry.contract != null, !validEvidence].every(Boolean)) {
    errors.push(`${key}: retained contract evidence is invalid`);
  }
};

const validateDeclaration = (key, entry, candidate, errors, dependencies) => {
  const evidence = entry.source_evidence;
  const declaration = Object(evidence).declaration;
  const valid = [
    dependencies.isObject(evidence),
    evidenceLocationValid(declaration, dependencies),
  ].every(Boolean);
  if (!valid) {
    errors.push(`${key}: complete declaration source evidence is required`);
    return;
  }
  if (evidence.declaration.path !== candidate.path) {
    errors.push(`${key}: declaration evidence path must match the candidate path`);
  }
  if (evidence.declaration.line !== candidate.line) {
    errors.push(`${key}: declaration evidence line must match the candidate line`);
  }
};

const validateUses = (key, entry, errors, dependencies) => {
  const uses = entry.source_evidence?.uses;
  if (!Array.isArray(uses)) {
    errors.push(`${key}: source_evidence.uses must be an array`);
    return uses;
  }
  if (!uses.every((use) => evidenceLocationValid(use, dependencies))) {
    errors.push(`${key}: every use must have path, line, and excerpt`);
  }
  return uses;
};

const validateRequiredUses = (key, entry, uses, errors) => {
  const requiresUses = ["used", "confirmed-used"].includes(
    [entry.truth, entry.semantic_status].find((value) =>
      ["used", "confirmed-used"].includes(value),
    ),
  );
  const hasNoUses = Array.isArray(uses) ? uses.length === 0 : false;
  if ([requiresUses, hasNoUses].every(Boolean)) {
    errors.push(`${key}: used or confirmed candidates require concrete use evidence`);
  }
};

const validateRequiredNotes = (key, entry, errors) => {
  const requiresNotes = ["unused", "indeterminate"].includes(entry.truth);
  const notes = entry.source_evidence?.notes;
  const hasNotes = [typeof notes === "string", String(notes).trim() !== ""].every(Boolean);
  if ([requiresNotes, !hasNotes].every(Boolean)) {
    errors.push(`${key}: unused or indeterminate truth requires adjudication notes`);
  }
};

const validateFeatureBuckets = (key, entry, errors) => {
  if (!optionalFeatureBucketListValid(entry.suggested_feature_buckets)) {
    errors.push(`${key}: suggested feature buckets must be non-empty and unique when present`);
  }
  if (!featureBucketListValid(entry.adjudicated_feature_buckets)) {
    errors.push(`${key}: at least one explicitly adjudicated feature bucket is required`);
  }
};

const validateEntry = (key, entry, candidate, errors, dependencies) => {
  validateEntryIdentity(key, entry, candidate, errors, dependencies);
  validateConfirmedTruth(key, entry, errors);
  validateContract(key, entry, errors, dependencies);
  validateDeclaration(key, entry, candidate, errors, dependencies);
  const uses = validateUses(key, entry, errors, dependencies);
  validateRequiredUses(key, entry, uses, errors);
  validateRequiredNotes(key, entry, errors);
  validateFeatureBuckets(key, entry, errors);
};

const validateExpectedEntries = (expected, actual, errors, dependencies) => {
  expected.forEach((candidate, key) => {
    const entry = actual.get(key);
    if (!entry) {
      errors.push(`${key}: missing ledger entry`);
      return;
    }
    validateEntry(key, entry, candidate, errors, dependencies);
  });
};

const validateNoStaleEntries = (expected, actual, errors) => {
  actual.forEach((_entry, key) => {
    if (!expected.has(key)) errors.push(`${key}: stale ledger entry is not present in discovery`);
  });
};

export const verifyLedgerData = (discovery, ledger, dependencies) => {
  if (!isValidDiscovery(discovery, dependencies.isObject)) {
    return ["discovery artifact is invalid"];
  }
  if (isLegacyLedger(ledger, dependencies.isObject)) {
    return [
      "ledger schema_version 1 is outdated; run `npm run type-aware:corpus -- evidence` to migrate it",
    ];
  }
  if (!isValidLedger(ledger, dependencies.isObject)) return ["ledger artifact is invalid"];
  const errors = [];
  const expected = indexExpected(discovery, errors, dependencies.candidateKey);
  const actual = indexActual(ledger, errors, dependencies.isObject);
  validateExpectedEntries(expected, actual, errors, dependencies);
  validateNoStaleEntries(expected, actual, errors);
  return errors.toSorted();
};

const validRefreshLedger = (previous, isObject) =>
  [
    isObject(previous),
    [1, 2].includes(previous.schema_version),
    Array.isArray(previous.candidates),
  ].every(Boolean);

const indexRefreshEntry = (previousByKey, entry, recovery, dependencies) => {
  const { fail, isObject } = dependencies;
  if (![isObject(entry), typeof entry.key === "string", entry.key.trim() !== ""].every(Boolean)) {
    fail(`every existing evidence ledger candidate must have a non-empty key. ${recovery}`);
  }
  if (previousByKey.has(entry.key)) {
    fail(
      `existing evidence ledger contains duplicate candidate key ${JSON.stringify(entry.key)}. ${recovery}`,
    );
  }
  previousByKey.set(entry.key, entry);
};

export const indexLedgerForRefresh = (previous, discoveredKeys, recovery, dependencies) => {
  if (previous === null) return new Map();
  if (!validRefreshLedger(previous, dependencies.isObject)) {
    dependencies.fail(
      `existing evidence ledger must use schema_version 1 or 2 and contain a candidates array. ${recovery}`,
    );
  }
  const previousByKey = new Map();
  previous.candidates.forEach((entry) =>
    indexRefreshEntry(previousByKey, entry, recovery, dependencies),
  );
  previousByKey.forEach((_entry, key) => {
    if (!discoveredKeys.has(key)) {
      dependencies.fail(
        `existing evidence ledger candidate key ${JSON.stringify(key)} is missing from the current discovery. ${recovery}`,
      );
    }
  });
  return previousByKey;
};

export const candidateFeatureBucketFields = (
  projectFeatureBuckets,
  previousEntry,
  previousSchemaVersion,
) => {
  const previous = Object(previousEntry);
  const legacySuggestions = previousSchemaVersion === 1 ? previous.feature_buckets : undefined;
  const suggestions = [
    previous.suggested_feature_buckets,
    legacySuggestions,
    projectFeatureBuckets,
  ].find((value) => value !== undefined);
  const adjudicated =
    previousSchemaVersion === 2 ? arrayOrEmpty(previous.adjudicated_feature_buckets) : [];
  return {
    suggested_feature_buckets: [...suggestions],
    adjudicated_feature_buckets: [...adjudicated],
  };
};

const arrayOrEmpty = (value) => (Array.isArray(value) ? value : []);

const validHash = (value) =>
  [typeof value === "string", /^[0-9a-f]{64}$/u.test(String(value))].every(Boolean);

const validateProducerSidecar = (producer, discovery, errors) => {
  if (!validHash(producer.sidecar_sha256)) {
    errors.push("ledger evidence producer sidecar hash is invalid");
    return;
  }
  if (producer.sidecar_sha256 !== Object(Object(discovery.provenance).sidecar).sha256) {
    errors.push("ledger evidence producer does not match discovery sidecar provenance");
  }
};

const validateProducerDigest = (producer, field, expected, label, errors) => {
  if (!validHash(producer[field])) {
    errors.push(`ledger evidence producer ${label} hash is invalid`);
    return;
  }
  const mismatched = [expected !== undefined, producer[field] !== expected].every(Boolean);
  if (mismatched) {
    errors.push(
      `ledger evidence producer ${label} hash does not match ${label === "request-set" ? "the corpus requests" : "stored responses"}`,
    );
  }
};

export const evidenceProducerErrors = (
  discovery,
  producer,
  expectedRequestSetSha256,
  expectedResponseSetSha256,
  dependencies,
) => {
  if (!dependencies.isObject(producer)) return ["ledger evidence producer is missing"];
  const errors = [];
  if (producer.protocol_version !== dependencies.protocolVersion) {
    errors.push(`ledger evidence producer must use protocol ${dependencies.protocolVersion}`);
  }
  validateProducerSidecar(producer, discovery, errors);
  validateProducerDigest(
    producer,
    "request_set_sha256",
    expectedRequestSetSha256,
    "request-set",
    errors,
  );
  validateProducerDigest(
    producer,
    "response_set_sha256",
    expectedResponseSetSha256,
    "response-set",
    errors,
  );
  return errors;
};

const confirmedEntriesByProject = (allEntries) => {
  const grouped = new Map();
  allEntries
    .filter(({ semantic_status: status }) =>
      ["confirmed-used", "contract-preserved"].includes(status),
    )
    .forEach((entry) => {
      const entries = grouped.get(entry.project_id) ?? [];
      entries.push(entry);
      grouped.set(entry.project_id, entries);
    });
  return grouped;
};

const reviewProjectId = (review) => Object(review).project_id;
const reviewProjectLabel = (projectId) => (typeof projectId === "string" ? projectId : "missing");

const validateReview = (review, confirmed, seen, errors, dependencies) => {
  const projectId = reviewProjectId(review);
  if (![typeof projectId === "string", confirmed.has(projectId)].every(Boolean)) {
    errors.push(
      `independent review references an unconfirmed project: ${reviewProjectLabel(projectId)}`,
    );
    return;
  }
  if (seen.has(projectId)) {
    errors.push(`independent review is duplicated for ${projectId}`);
    return;
  }
  seen.add(projectId);
  const entries = confirmed.get(projectId);
  const keys = entries.map(({ key }) => key).toSorted();
  const valid = [
    review.verdict === "approved",
    review.candidate_count === keys.length,
    review.candidate_set_sha256 === dependencies.candidateSetDigest(keys),
    review.evidence_sha256 === dependencies.independentReviewDigest(entries),
  ].every(Boolean);
  if (!valid) errors.push(`independent review does not match confirmed evidence for ${projectId}`);
};

export const independentReviewErrors = (allEntries, reviews, dependencies) => {
  if (!Array.isArray(reviews)) return ["independent reviews must be an array"];
  const errors = [];
  const confirmed = confirmedEntriesByProject(allEntries);
  const seen = new Set();
  reviews.forEach((review) => validateReview(review, confirmed, seen, errors, dependencies));
  confirmed.forEach((_entries, projectId) => {
    if (!seen.has(projectId)) {
      errors.push(`confirmed candidates for ${projectId} have no independent approved review`);
    }
  });
  return errors.toSorted();
};
