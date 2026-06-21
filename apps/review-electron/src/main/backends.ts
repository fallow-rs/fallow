import type { AgentWalkthrough } from "../model/agent";

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
 * Build the agent prompt from the deterministic digest. The agent narrates facts
 * we computed and must echo the graph hash + cite only emitted signal_ids; the
 * graph (not a second model) validates the result afterward.
 */
export const buildAgentPrompt = (
  digest: unknown,
  graphSnapshotHash: string,
  schemaShape: string,
): string =>
  [
    "You are a code reviewer. Here is a deterministic review digest (facts only):",
    "",
    JSON.stringify(digest, null, 2),
    "",
    "Return ONLY a JSON object, no prose, matching:",
    schemaShape,
    `Echo graph_snapshot_hash exactly as "${graphSnapshotHash}".`,
    "Cite only signal_id values present in the digest.",
  ].join("\n");

const parseWalkthrough = (text: string): AgentWalkthrough | null => {
  try {
    const value = JSON.parse(text) as Partial<AgentWalkthrough>;
    if (typeof value.graph_snapshot_hash === "string" && Array.isArray(value.judgments)) {
      return value as AgentWalkthrough;
    }
  } catch {
    /* not JSON */
  }
  return null;
};

/** Extract the agent's judgment JSON from stdout (raw, ```json fenced, or embedded). */
export const extractAgentJson = (stdout: string): AgentWalkthrough | null => {
  const whole = parseWalkthrough(stdout.trim());
  if (whole) return whole;

  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(stdout)?.[1];
  if (fenced) {
    const parsed = parseWalkthrough(fenced.trim());
    if (parsed) return parsed;
  }

  const start = stdout.indexOf("{");
  const end = stdout.lastIndexOf("}");
  if (start >= 0 && end > start) {
    const parsed = parseWalkthrough(stdout.slice(start, end + 1));
    if (parsed) return parsed;
  }
  return null;
};
