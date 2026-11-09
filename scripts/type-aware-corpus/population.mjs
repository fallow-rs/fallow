const fail = (message) => {
  throw new Error(message);
};
const REMOVALS = new Set(["confirmed-used", "contract-preserved"]);
const RETENTIONS = new Set([
  "confirmed-no-static-references",
  "retained-abstained",
  "retained-unresolved",
]);

const describeCandidate = (projectId, candidate, decisions) => {
  const decision = decisions.get(candidate.key);
  if (!decision) fail(`${projectId} emitted no semantic decision for ${candidate.key}`);
  if (!REMOVALS.has(decision.decision) && !RETENTIONS.has(decision.decision)) {
    fail(`${projectId} emitted an unknown semantic decision for ${candidate.key}`);
  }
  return {
    ...candidate,
    semantic_status: REMOVALS.has(decision.decision) ? decision.decision : "retained",
    semantic_decision: decision.decision,
    semantic_completeness: decision.status,
    owning_projects: decision.owning_projects ?? [],
    contract: decision.contract ?? null,
  };
};

/** Keep baseline confirmations and complete semantic-only promotions in separate cohorts. */
export const comparePopulation = (projectId, baseline, refined, decisions) => {
  const baselineKeys = new Set(baseline.map(({ key }) => key));
  const refinedKeys = new Set(refined.map(({ key }) => key));
  const candidates = baseline.map((candidate) => {
    const result = describeCandidate(projectId, candidate, decisions);
    if (REMOVALS.has(result.semantic_decision) === refinedKeys.has(candidate.key)) {
      fail(
        `${projectId} semantic decision ${result.semantic_decision} disagrees with the refined finding set for ${candidate.key}`,
      );
    }
    return result;
  });
  const promotions = refined
    .filter(({ key }) => !baselineKeys.has(key))
    .map((candidate) => {
      const result = describeCandidate(projectId, candidate, decisions);
      if (
        result.semantic_decision !== "confirmed-no-static-references" ||
        result.semantic_completeness !== "complete"
      ) {
        fail(
          `${projectId} semantic-only promotion lacks complete negative evidence for ${candidate.key}`,
        );
      }
      return result;
    });
  for (const [key, decision] of decisions) {
    if (
      !baselineKeys.has(key) &&
      decision.decision === "confirmed-no-static-references" &&
      !refinedKeys.has(key)
    ) {
      fail(`${projectId} is missing the semantic-only promotion for ${key}`);
    }
  }
  return { candidates, promotions };
};

const DECISION_COUNTERS = {
  "confirmed-used": "confirmed_used_count",
  "contract-preserved": "contract_preserved_count",
  "confirmed-no-static-references": "no_static_references_count",
  "retained-unresolved": "unresolved_count",
  "retained-abstained": "abstained_count",
};
const REASON_FIELDS = [
  "no_project",
  "ambiguous_project",
  "blocking_diagnostics",
  "svelte_virtual_module_exports",
  "unknown_symbol",
  "unsupported_syntax",
  "capacity",
];

const decisionCounts = (decisions) => {
  const counts = {
    ...Object.fromEntries(Object.values(DECISION_COUNTERS).map((field) => [field, 0])),
    fix_eligible_count: 0,
    abstention_reasons: Object.fromEntries(REASON_FIELDS.map((field) => [field, 0])),
  };
  for (const decision of decisions) {
    const counter = DECISION_COUNTERS[decision.decision];
    if (!Object.hasOwn(DECISION_COUNTERS, decision.decision))
      fail("unknown semantic decision in metadata");
    counts[counter] += 1;
    if (decision.decision === "confirmed-no-static-references" && decision.closed_world_eligible) {
      counts.fix_eligible_count += 1;
    }
    if (!["retained-unresolved", "retained-abstained"].includes(decision.decision)) continue;
    const reason = decision.reason_code?.replaceAll("-", "_");
    if (reason !== "ambiguous_project" && REASON_FIELDS.includes(reason)) {
      counts.abstention_reasons[reason] += 1;
    }
  }
  return counts;
};

/** Validate global counters, including unrequested capacity omissions, against emitted decisions. */
export const validatePopulationMetadata = (projectId, metadata, decisions, baseline) => {
  const omitted = metadata.candidate_count - decisions.size;
  if (!Number.isSafeInteger(metadata.candidate_count) || omitted < 0) {
    fail(`${projectId} type-aware candidate metadata is inconsistent`);
  }
  const counts = decisionCounts(decisions.values());
  counts.abstained_count += omitted;
  counts.abstention_reasons.capacity += omitted;
  for (const [field, count] of Object.entries(counts)) {
    const matches =
      field === "abstention_reasons"
        ? Object.entries(count).every(([reason, value]) => metadata[field]?.[reason] === value)
        : metadata[field] === count;
    if (!matches) fail(`${projectId} type-aware metadata ${field} is inconsistent`);
  }
  return decisionCounts(
    baseline.map(({ key }) => {
      const decision = decisions.get(key);
      if (!decision) fail(`${projectId} emitted no semantic decision for ${key}`);
      return decision;
    }),
  );
};
