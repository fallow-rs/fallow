import { execFile } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { toWalkthroughDocument, type AuditBrief } from "../model/adapter";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { AgentWalkthrough, Guide } from "../model/agent";
import { describeExecError } from "./errors";

const run = promisify(execFile);
const fallowBin = (): string => process.env["FALLOW_BIN"] ?? "fallow";
const at = (root?: string): string => root ?? process.cwd();
const MAX_BUFFER = 64 * 1024 * 1024;

/** Run the fallow CLI, translating spawn/exit failures into clean messages. */
const runFallow = async (args: string[], root?: string): Promise<string> => {
  const bin = fallowBin();
  try {
    const { stdout } = await run(bin, args, { cwd: at(root), maxBuffer: MAX_BUFFER });
    return stdout;
  } catch (e) {
    throw describeExecError(e, bin);
  }
};

/** Parse fallow JSON output, mapping malformed payloads to a clean message. */
const parseFallowJson = <T>(stdout: string): T => {
  try {
    return JSON.parse(stdout) as T;
  } catch {
    throw new Error("fallow returned output that couldn't be read as JSON.");
  }
};

/** `fallow review --format json` -> normalized W1 document. */
export const runReview = async (root?: string): Promise<WalkthroughDocument> => {
  const stdout = await runFallow(["review", "--format", "json"], root);
  return toWalkthroughDocument(parseFallowJson<AuditBrief>(stdout));
};

/** `fallow review --walkthrough-guide --format json` -> the E5 agent-contract guide. */
export const runGuide = async (root?: string): Promise<Guide> => {
  const stdout = await runFallow(["review", "--walkthrough-guide", "--format", "json"], root);
  const g = parseFallowJson<{
    graph_snapshot_hash?: string;
    digest?: { decisions?: { emitted_signal_ids?: string[] } };
    direction?: { order?: string[] };
    agent_schema?: { judgment_shape?: string };
  }>(stdout);
  return {
    graphSnapshotHash: g.graph_snapshot_hash ?? "",
    emittedSignalIds: g.digest?.decisions?.emitted_signal_ids ?? [],
    order: g.direction?.order ?? [],
    digest: g.digest ?? null,
    schemaShape: g.agent_schema?.judgment_shape ?? "",
  };
};

/**
 * Post-validate an agent-walkthrough against the live graph via
 * `fallow review --walkthrough-file`. Returns the raw validation envelope
 * (accepted/rejected per judgment; whole-payload stale rejection on hash drift).
 */
export const validateWalkthrough = async (
  payload: AgentWalkthrough,
  root?: string,
): Promise<unknown> => {
  const file = join(tmpdir(), `fallow-agent-wt-${process.pid}-${Date.now()}.json`);
  await writeFile(file, JSON.stringify(payload), "utf8");
  const stdout = await runFallow(["review", "--walkthrough-file", file, "--format", "json"], root);
  return parseFallowJson<unknown>(stdout);
};
