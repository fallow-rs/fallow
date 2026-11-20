/** The min, max, mean and median of a list of times that is not empty. */
export const stats = (times) => {
  const sorted = [...times].toSorted((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  const median = sorted.length % 2 === 0 ? (sorted[mid - 1] + sorted[mid]) / 2 : sorted[mid];
  return {
    min: sorted[0],
    max: sorted.at(-1),
    mean: sorted.reduce((a, b) => a + b, 0) / sorted.length,
    median,
  };
};

/**
 * The outcome of one measured sample. `run` is a `classifyRun` result.
 * `report` is `{ ok: true, value }` or `{ ok: false, error }` for the report
 * of the tool. Only a valid sample can enter the times.
 */
export const sampleOutcome = (run, report) => {
  if (!run.ok) return { valid: false, reason: run.reason };
  if (!report.ok) return { valid: false, reason: report.error };
  return { valid: true, reason: null };
};

/**
 * Compares the successful samples with the requested count. `stats` is null
 * when no sample was successful. A speedup needs `complete` to be true.
 */
export const sampleSummary = (times, requested) => ({
  requested,
  successful: times.length,
  complete: requested > 0 && times.length === requested,
  stats: times.length > 0 ? stats(times) : null,
});
