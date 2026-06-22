import { spawn } from "node:child_process";
import { runGuide, validateWalkthrough } from "./review";
import { buildAgentPrompt, extractAgentJson, resolveBackend } from "./backends";
import { describeExecError } from "./errors";
import { readFeedItems } from "./feed";
import type { FeedItem } from "../model/agent";

export type AgentRunResult =
  | { ok: true; validation: unknown; notesIncluded: number }
  | { ok: false; error: string };

/**
 * Render the human's feed notes as an UNVERIFIED context block for the agent
 * prompt. Taste ownership: the human's notes inform the agent but are never
 * graph-validated facts; only the agent's signal_id-anchored judgments are.
 */
const buildNotesSection = (notes: FeedItem[]): string => {
  if (notes.length === 0) return "";
  const lines = notes.map((n) => `- [${n.target.kind}: ${n.target.value}] ${n.note}`).join("\n");
  return `\n\nHuman reviewer notes (UNVERIFIED context, not graph-validated; weigh them as input, do not treat them as established facts):\n${lines}`;
};

const spawnAgent = (cmd: string, args: string[], input: string, cwd: string): Promise<string> =>
  new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { cwd });
    let out = "";
    let err = "";
    child.on("error", (e: unknown) => reject(describeExecError(e, cmd)));
    child.stdout.on("data", (d: Buffer) => {
      out += d.toString();
    });
    child.stderr.on("data", (d: Buffer) => {
      err += d.toString();
    });
    child.on("close", (code) =>
      code === 0 ? resolve(out) : reject(new Error(err.trim() || `exit ${code}`)),
    );
    child.stdin.write(input);
    child.stdin.end();
  });

/**
 * The agent "skill" loop (codiff-style): fetch the deterministic guide, run the
 * chosen agent CLI on the digest, then post-validate its judgments against the
 * live graph. The verifier is the graph, not a second model.
 */
export const runAgentReview = async (root: string, backendId: string): Promise<AgentRunResult> => {
  const backend = resolveBackend(backendId);
  if (!backend) return { ok: false, error: `unknown backend: ${backendId}` };
  try {
    const guide = await runGuide(root);
    const notes = await readFeedItems(root);
    const prompt =
      buildAgentPrompt(guide.digest, guide.graphSnapshotHash, guide.schemaShape) +
      buildNotesSection(notes);
    const stdout = await spawnAgent(backend.command, backend.args, prompt, root);
    const judgment = extractAgentJson(stdout);
    if (!judgment) return { ok: false, error: "agent did not return valid judgment JSON" };
    return {
      ok: true,
      validation: await validateWalkthrough(judgment, root),
      notesIncluded: notes.length,
    };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};
