import { runGuide } from "./review";
import { buildTradeOffPrompt, extractTradeOffJson, resolveBackend } from "./backends";
import { describeExecError } from "./errors";
import { writePersistedTradeoffs } from "./tradeoffs";
import { toTradeOffEnvelope } from "../model/adapter";
import type { TradeOffEnvelope } from "../model/tradeoff";
import {
  AGENT_DEADLINE_MS,
  STDERR_LIMIT_BYTES,
  STDOUT_LIMIT_BYTES,
  ProcessRunError,
  runProcess,
} from "./processRun";

export type TradeOffRunResult =
  | { ok: true; tradeoffs: TradeOffEnvelope }
  | { ok: false; error: string };

const spawnAgent = (cmd: string, args: string[], input: string, cwd: string): Promise<string> =>
  runProcess({
    command: cmd,
    args,
    cwd,
    input,
    deadlineMs: AGENT_DEADLINE_MS,
    stdoutLimitBytes: STDOUT_LIMIT_BYTES,
    stderrLimitBytes: STDERR_LIMIT_BYTES,
  })
    .then(({ stdout }) => stdout)
    .catch((error: unknown) => {
      if (error instanceof ProcessRunError && error.kind !== "spawn") throw error;
      throw describeExecError(error, cmd);
    });

/**
 * The trade-off elicitation run: spawn the chosen agent CLI on the deterministic
 * guide digest, with NO post-validation step. fallow cannot validate these broader anchors the
 * way it validates a structural `signal_id`, so the discipline is the prompt's
 * (anchor-to-diff, fence everything `deterministic:false`), not graph-grade. The
 * adapter still drops anchorless items and pins `deterministic:false` defensively.
 * On success the raw envelope is persisted so the renderer can read it at
 * cold-start via `getTradeoffs()`.
 */
export const runTradeoffElicitation = async (
  root: string,
  backendId: string,
): Promise<TradeOffRunResult> => {
  const backend = resolveBackend(backendId);
  if (!backend) return { ok: false, error: `unknown backend: ${backendId}` };
  try {
    const guide = await runGuide(root);
    const prompt = buildTradeOffPrompt(guide.digest, guide.graphSnapshotHash);
    const stdout = await spawnAgent(backend.command, backend.args, prompt, root);
    const raw = extractTradeOffJson(stdout);
    if (!raw) return { ok: false, error: "agent did not return a valid trade-off envelope" };
    await writePersistedTradeoffs(root, raw);
    return { ok: true, tradeoffs: toTradeOffEnvelope(raw) };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
};
