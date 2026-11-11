// Aggregates the answer records of one tracker run into counts per agent and
// per prompt, and renders them as a Markdown table.

const NO_PICK = "(none)";

const increment = (counts, key) => {
  counts[key] = (counts[key] ?? 0) + 1;
};

/** Sort count entries by count, high first, then by name. */
const rankCounts = (counts) =>
  Object.entries(counts).toSorted(
    ([leftName, left], [rightName, right]) => right - left || leftName.localeCompare(rightName),
  );

const emptyGroup = (fields) => ({
  ...fields,
  runs: 0,
  errors: 0,
  models: [],
  firstPicks: {},
  mentions: {},
});

const addRecord = (group, record) => {
  if (record.status !== "ok") {
    group.errors += 1;
    return;
  }
  group.runs += 1;
  if (record.model && !group.models.includes(record.model)) group.models.push(record.model);
  increment(group.firstPicks, record.firstPick ?? NO_PICK);
  for (const pick of record.picks) increment(group.mentions, pick);
};

const groupBy = (records, keyOf, fieldsOf) => {
  const groups = new Map();
  for (const record of records) {
    const key = keyOf(record);
    if (!groups.has(key)) groups.set(key, emptyGroup(fieldsOf(record)));
    addRecord(groups.get(key), record);
  }
  return [...groups.entries()]
    .toSorted(([left], [right]) => left.localeCompare(right))
    .map(([, group]) => ({ ...group, models: group.models.toSorted() }));
};

/**
 * Aggregate answer records.
 * @param {Array<{ agent: string, promptId: string, status: string, model?: string | null,
 *   firstPick: string | null, picks: string[] }>} records
 * @param {Array<{ agent: string, reason: string }>} [skipped]
 */
export const summarize = (records, skipped = []) => ({
  agents: groupBy(
    records,
    (record) => record.agent,
    (record) => ({ agent: record.agent }),
  ),
  prompts: groupBy(
    records,
    (record) => `${record.promptId}\u0000${record.agent}`,
    (record) => ({ promptId: record.promptId, agent: record.agent }),
  ),
  skipped: skipped.toSorted((left, right) => left.agent.localeCompare(right.agent)),
});

const formatCounts = (counts) =>
  rankCounts(counts)
    .map(([name, count]) => `${name} ${count}`)
    .join(", ") || "-";

const formatMentions = (counts, runs) =>
  rankCounts(counts)
    .map(([name, count]) => `${name} ${count}/${runs}`)
    .join(", ") || "-";

const share = (count, runs) => (runs === 0 ? "-" : `${Math.round((100 * count) / runs)}%`);

/**
 * Render a summary as Markdown.
 * @param {ReturnType<typeof summarize>} summary
 * @returns {string}
 */
export const formatSummary = (summary) => {
  const lines = [
    "## By agent",
    "",
    "| Agent | Models | Runs | Errors | fallow first | fallow named | First picks |",
    "| --- | --- | --- | --- | --- | --- | --- |",
    ...summary.agents.map(
      (group) =>
        `| ${group.agent} | ${group.models.join(", ") || "-"} | ${group.runs} | ${group.errors} | ${share(group.firstPicks.fallow ?? 0, group.runs)} | ${share(group.mentions.fallow ?? 0, group.runs)} | ${formatCounts(group.firstPicks)} |`,
    ),
    "",
    "## By prompt",
    "",
    "| Prompt | Agent | Runs | Errors | First picks | Named at all |",
    "| --- | --- | --- | --- | --- | --- |",
    ...summary.prompts.map(
      (group) =>
        `| ${group.promptId} | ${group.agent} | ${group.runs} | ${group.errors} | ${formatCounts(group.firstPicks)} | ${formatMentions(group.mentions, group.runs)} |`,
    ),
  ];
  if (summary.skipped.length > 0) {
    lines.push(
      "",
      "## Skipped agents",
      "",
      ...summary.skipped.map(({ agent, reason }) => `- ${agent}: ${reason}`),
    );
  }
  return `${lines.join("\n")}\n`;
};
