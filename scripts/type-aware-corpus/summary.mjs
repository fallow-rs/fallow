const display = (value) => (value === null || value === undefined ? "n/a" : value);

export const percentile = (values, ratio) => {
  if (values.length === 0) return null;
  const sorted = values.toSorted((left, right) => left - right);
  const index = (sorted.length - 1) * ratio;
  const lower = Math.floor(index);
  const upper = Math.ceil(index);
  if (lower === upper) return sorted[lower];
  const value = sorted[lower] + (sorted[upper] - sorted[lower]) * (index - lower);
  return Math.round(value * 1_000) / 1_000;
};

const addReasonCount = (counts, [reason, count]) => {
  if (!Number.isFinite(count)) return counts;
  counts[reason] = (counts[reason] ?? 0) + count;
  return counts;
};

export const aggregateReasonCounts = (runs) => {
  const entries = runs.flatMap((run) => Object.entries(run.reason_counts ?? {}));
  const counts = entries.reduce(addReasonCount, {});
  return Object.fromEntries(
    Object.entries(counts).toSorted(([left], [right]) => left.localeCompare(right)),
  );
};

const collectPhase = (byPhase, [phase, duration]) => {
  if (!Number.isFinite(duration)) return byPhase;
  const values = byPhase.get(phase) ?? [];
  values.push(duration);
  byPhase.set(phase, values);
  return byPhase;
};

export const aggregatePhaseTimings = (runs) => {
  const entries = runs.flatMap((run) => Object.entries(run.phase_timings_ms ?? {}));
  const byPhase = entries.reduce(collectPhase, new Map());
  return Object.fromEntries(
    [...byPhase.entries()]
      .toSorted(([left], [right]) => left.localeCompare(right))
      .map(([phase, values]) => [
        phase,
        { median: percentile(values, 0.5), p95: percentile(values, 0.95) },
      ]),
  );
};

export const safeRatio = (numerator, denominator) =>
  denominator === 0 ? null : Math.round((numerator / denominator) * 1_000_000) / 1_000_000;

const confirmedUsed = ({ semantic_status: semanticStatus, truth }) =>
  semanticStatus === "confirmed-used" && truth === "used";

const candidateBuckets = (entry) => ({
  key: entry.key,
  buckets: new Set(entry.adjudicated_feature_buckets ?? []),
});

const hasDistinctBuckets = (left, right) =>
  left.key !== right.key &&
  [...left.buckets].some((bucket) => [...right.buckets].some((other) => other !== bucket));

export const summarizeAdjudicatedFeatureBuckets = (entries) => {
  const candidates = entries.filter(confirmedUsed).map(candidateBuckets);
  const buckets = new Set(candidates.flatMap(({ buckets: values }) => [...values]));
  const multiple = candidates.some((left, index) =>
    candidates.slice(index + 1).some((right) => hasDistinctBuckets(left, right)),
  );
  return {
    confirmed_feature_buckets: [...buckets].toSorted(),
    multiple_feature_buckets: multiple,
  };
};

export const renderSummaryMarkdown = (summary) =>
  [
    "# Type-aware corpus summary",
    "",
    `Gate: ${summary.gate.go ? "GO" : "NO-GO"}`,
    "",
    `- Candidates: ${summary.accuracy.candidate_count}`,
    `- Confirmation precision: ${display(summary.accuracy.confirmation_precision)}`,
    `- Safe confirmation yield: ${display(summary.accuracy.confirmation_yield)}`,
    `- Contract-preserved false positives: ${summary.accuracy.contract_preserved_count}`,
    `- Combined safe-resolution yield: ${display(summary.accuracy.safe_resolution_yield)}`,
    `- Independently adjudicated truth coverage: ${display(summary.accuracy.adjudicated_truth_coverage)}`,
    `- Correct-unused retention: ${display(summary.accuracy.correct_unused_retention)}`,
    `- Abstention: ${display(summary.accuracy.abstention)}`,
    `- Median marginal overhead: ${display(summary.performance.marginal_overhead_ms.median)} ms`,
    `- P95 marginal overhead: ${display(summary.performance.marginal_overhead_ms.p95)} ms`,
    `- Independent repositories with confirmed value: ${summary.value.confirmed_repository_count}`,
    `- Semantic buckets with confirmed value: ${summary.value.confirmed_feature_buckets.join(", ") || "none"}`,
    "",
    "No corpus-wide recall is claimed. Indeterminate candidates are excluded from adjudicated accuracy metrics.",
    "",
    "Raw machine output and source evidence remain under target/type-aware-corpus and are not tracked.",
    "",
  ].join("\n");

export const requirePublicationGo = (summary, publicationMode, fail) => {
  if (publicationMode !== "verify") return;
  if (summary.gate.go === true) return;
  const failedChecks = Object.entries(summary.gate.checks)
    .filter(([, passed]) => passed !== true)
    .map(([name]) => name)
    .toSorted();
  const detail = ["unknown", failedChecks.join(", ")][Number(failedChecks.length > 0)];
  fail(`publication gate is NO-GO; failed checks: ${detail}`);
};

const measuredPairs = (runs) => {
  const pairs = new Map();
  runs
    .filter(({ warmup }) => !warmup)
    .forEach((run) => {
      const pair = pairs.get(run.iteration) ?? {};
      pair[run.mode] = run.wall_ms;
      pairs.set(run.iteration, pair);
    });
  return [...pairs.values()];
};

export const pairedOverheads = (projects) =>
  projects
    .flatMap(({ runs }) => measuredPairs(runs))
    .filter(({ baseline, refined }) =>
      [Number.isFinite(baseline), Number.isFinite(refined)].every(Boolean),
    )
    .map(({ baseline, refined }) => refined - baseline);

const metricAtLeast = (value, threshold) => [value !== null, value >= threshold].every(Boolean);

const metricAtMost = (value, threshold) => [value !== null, value <= threshold].every(Boolean);

export const summaryChecks = (values) => ({
  zero_incorrect_removals: values.incorrectConfirmedCount === 0,
  zero_controls_clean: values.zeroControlsClean,
  multiple_repositories: values.confirmedProjectCount >= 2,
  multiple_feature_buckets: values.featureBucketValue.multiple_feature_buckets,
  deterministic_output: values.determinismRuns >= 2,
  focused_cases: values.focusedCasesPassed,
  confirmation_precision: metricAtLeast(
    values.confirmationPrecision,
    values.thresholds.minimum_confirmation_precision,
  ),
  confirmation_yield: metricAtLeast(
    values.confirmationYield,
    values.thresholds.minimum_confirmation_yield,
  ),
  correct_unused_retention: metricAtLeast(
    values.correctUnusedRetention,
    values.thresholds.minimum_correct_unused_retention,
  ),
  abstention: metricAtMost(values.abstention, values.thresholds.maximum_abstention),
  marginal_overhead: metricAtMost(
    values.marginalOverheadP95,
    values.thresholds.maximum_p95_marginal_overhead_ms,
  ),
  refined_rss:
    values.platform === "win32"
      ? true
      : metricAtMost(values.refinedRssP95, values.thresholds.maximum_p95_refined_rss_kb),
  independent_signoff: [
    values.isObject(values.independentSignoff),
    Object(values.independentSignoff).verdict === "approved",
  ].every(Boolean),
});
