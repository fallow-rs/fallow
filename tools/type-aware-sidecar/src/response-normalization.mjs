const copyArray = (value) => [...(value ?? [])];

const evidenceCount = (result, evidence) => result.totalEvidenceCount ?? evidence.length;

export const normalizePhaseTimings = (phaseTimings) =>
  Object.fromEntries(
    Object.entries(phaseTimings).map(([name, duration]) => [
      name,
      Math.max(0, Math.round(duration)),
    ]),
  );

export const normalizeSemanticResult = (result) => {
  const evidence = copyArray(result.evidence);
  return {
    query_id: result.queryId,
    operation: result.operation,
    assertion: result.assertion,
    status: result.status,
    reason_code: result.reasonCode ?? null,
    actions: copyArray(result.actions).slice(0, 3),
    evidence,
    total_evidence_count: evidenceCount(result, evidence),
    truncated: Boolean(result.truncated),
    omissions: copyArray(result.omissions),
    data: result.data ?? {},
  };
};
