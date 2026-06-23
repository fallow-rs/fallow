/** A pluggable coding-agent CLI backend (codiff-style: spawn an external agent). */
export type AgentBackend = {
  id: string;
  label: string;
  command: string;
  args: string[];
};

export const BACKENDS: AgentBackend[] = [
  { id: "claude-code", label: "claude code", command: "claude", args: ["-p"] },
  { id: "codex", label: "codex", command: "codex", args: ["exec"] },
  { id: "opencode", label: "opencode", command: "opencode", args: ["run"] },
];

export const resolveBackend = (id: string): AgentBackend | null =>
  BACKENDS.find((b) => b.id === id) ?? null;

/**
 * Build the trade-off elicitation prompt from the deterministic digest. The agent
 * reads the diff through the non-deterministic lenses fallow cannot prove
 * (abstraction, error handling, data-model shape, ...) and emits the trade-off
 * envelope. Governing principle is TASTE OWNERSHIP: name the open question, never
 * prescribe the answer. The full honesty contract lives in the `fallow-review`
 * skill's `tradeoff-elicitation.md`; this is the wire-level instruction.
 */
export const buildTradeOffPrompt = (digest: unknown, graphSnapshotHash: string): string =>
  [
    "You are a code reviewer surfacing the NON-DETERMINISTIC trade-offs in a diff:",
    "the architectural choices fallow cannot prove from the module graph (abstraction,",
    "error handling, data-model shape, coupling, naming, testability, trust boundaries).",
    "",
    "Here is the deterministic review digest (facts fallow already owns):",
    "",
    JSON.stringify(digest, null, 2),
    "",
    "Do NOT re-raise anything already framed in digest.decisions.decisions[].",
    "For each trade-off, keep three layers separate and neutral: `observed` (a fact",
    "read straight from the diff), `tradeoff` (your inference: name both gain and cost),",
    "and `question` (the GENUINELY OPEN call the human owns; name no fix, never use the",
    "leading '..., or should you X?' form). Anchor every item to a real changed file:line",
    "present in the diff; use the literal `cross-cutting` only when no single line is the",
    "locus. Keep at most the top FIVE by `consequence` then `confidence`; abstain freely.",
    "",
    "Return ONLY a JSON object, no prose, matching:",
    '{ "graph_snapshot_hash": string, "abstained": boolean, "tradeoffs": [ {',
    '  "id": "to:<anchor>:<lens>", "anchor": "<file:line>|cross-cutting", "lens": string,',
    '  "observed": string, "tradeoff": string, "question": string,',
    '  "consequence": "low"|"medium"|"high", "confidence": "low"|"medium"|"high",',
    '  "captured": boolean, "deterministic": false } ] }',
    "If nothing rises to a real decision, return abstained:true with tradeoffs:[].",
    `Echo graph_snapshot_hash exactly as "${graphSnapshotHash}".`,
  ].join("\n");

/**
 * Parse a raw trade-off envelope, guarding ONLY on the wire shape
 * (`graph_snapshot_hash` string + `tradeoffs` array). Content normalization
 * (severity coercion, anchor drop, sort, `deterministic` pin) is the adapter's
 * job, not this parser's; here we just confirm the model returned the envelope.
 * Returns the raw object for the adapter, or null when no envelope is present.
 */
const parseTradeOffEnvelope = (text: string): Record<string, unknown> | null => {
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value !== "object" || value === null) return null;
    const v = value as Record<string, unknown>;
    if (typeof v["graph_snapshot_hash"] === "string" && Array.isArray(v["tradeoffs"])) {
      return v;
    }
  } catch {
    /* not JSON */
  }
  return null;
};

/** Extract the trade-off envelope JSON from stdout (raw, ```json fenced, or embedded). */
export const extractTradeOffJson = (stdout: string): Record<string, unknown> | null => {
  const whole = parseTradeOffEnvelope(stdout.trim());
  if (whole) return whole;

  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(stdout)?.[1];
  if (fenced) {
    const parsed = parseTradeOffEnvelope(fenced.trim());
    if (parsed) return parsed;
  }

  const start = stdout.indexOf("{");
  const end = stdout.lastIndexOf("}");
  if (start >= 0 && end > start) {
    const parsed = parseTradeOffEnvelope(stdout.slice(start, end + 1));
    if (parsed) return parsed;
  }
  return null;
};
