// Aggregates the answer records of one tracker run into counts per agent
// version and per prompt, and renders them as a Markdown table. An agent
// version is the agent, its CLI version and the model that answered.

const NO_PICK = "(none)";
const UNKNOWN = "-";
const KEY_SEPARATOR = "\u0000";

/** The fields that identify one agent version. */
const versionFields = (record) => ({
  agent: record.agent,
  cliVersion: record.cliVersion ?? null,
  model: record.model ?? null,
});

const versionKey = (record) =>
  [record.agent, record.cliVersion ?? "", record.model ?? ""].join(KEY_SEPARATOR);

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
  firstPicks: {},
  mentions: {},
});

const addRecord = (group, record) => {
  if (record.status !== "ok") {
    group.errors += 1;
    return;
  }
  group.runs += 1;
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
    .map(([, group]) => group);
};

/**
 * Aggregate answer records. A record with an error has no model, so its error
 * counts in the row of the agent and CLI version without a model.
 * @param {Array<{ agent: string, promptId: string, status: string, model?: string | null,
 *   cliVersion?: string | null, firstPick: string | null, picks: string[] }>} records
 * @param {Array<{ agent: string, reason: string }>} [skipped]
 */
export const summarize = (records, skipped = []) => ({
  agents: groupBy(records, versionKey, versionFields),
  prompts: groupBy(
    records,
    (record) => `${record.promptId}${KEY_SEPARATOR}${versionKey(record)}`,
    (record) => ({ promptId: record.promptId, ...versionFields(record) }),
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
    "## By agent version",
    "",
    "| Agent | CLI version | Model | Runs | Errors | fallow first | fallow named | First picks |",
    "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ...summary.agents.map(
      (group) =>
        `| ${group.agent} | ${group.cliVersion ?? UNKNOWN} | ${group.model ?? UNKNOWN} | ${group.runs} | ${group.errors} | ${share(group.firstPicks.fallow ?? 0, group.runs)} | ${share(group.mentions.fallow ?? 0, group.runs)} | ${formatCounts(group.firstPicks)} |`,
    ),
    "",
    "## By prompt",
    "",
    "| Prompt | Agent | CLI version | Model | Runs | Errors | First picks | Named at all |",
    "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ...summary.prompts.map(
      (group) =>
        `| ${group.promptId} | ${group.agent} | ${group.cliVersion ?? UNKNOWN} | ${group.model ?? UNKNOWN} | ${group.runs} | ${group.errors} | ${formatCounts(group.firstPicks)} | ${formatMentions(group.mentions, group.runs)} |`,
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
