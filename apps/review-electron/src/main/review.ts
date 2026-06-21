import { execFile } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { toWalkthroughDocument, type AuditBrief } from "../model/adapter";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { AgentWalkthrough, Guide } from "../model/agent";

const run = promisify(execFile);
const fallowBin = (): string => process.env["FALLOW_BIN"] ?? "fallow";
const at = (root?: string): string => root ?? process.cwd();
const MAX_BUFFER = 64 * 1024 * 1024;

/** `fallow review --format json` -> normalized W1 document. */
export const runReview = async (root?: string): Promise<WalkthroughDocument> => {
  const { stdout } = await run(fallowBin(), ["review", "--format", "json"], {
    cwd: at(root),
    maxBuffer: MAX_BUFFER,
  });
  return toWalkthroughDocument(JSON.parse(stdout) as AuditBrief);
};

/** `fallow review --walkthrough-guide --format json` -> the E5 agent-contract guide. */
export const runGuide = async (root?: string): Promise<Guide> => {
  const { stdout } = await run(fallowBin(), ["review", "--walkthrough-guide", "--format", "json"], {
    cwd: at(root),
    maxBuffer: MAX_BUFFER,
  });
  const g = JSON.parse(stdout) as {
    graph_snapshot_hash?: string;
    digest?: { decisions?: { emitted_signal_ids?: string[] } };
    direction?: { order?: string[] };
    agent_schema?: { judgment_shape?: string };
  };
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
  const { stdout } = await run(
    fallowBin(),
    ["review", "--walkthrough-file", file, "--format", "json"],
    {
      cwd: at(root),
      maxBuffer: MAX_BUFFER,
    },
  );
  return JSON.parse(stdout);
};
