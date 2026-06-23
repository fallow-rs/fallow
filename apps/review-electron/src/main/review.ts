import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";
import { toWalkthroughDocument, type AuditBrief } from "../model/adapter";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { AgentWalkthrough, Guide } from "../model/agent";
import { describeExecError } from "./errors";

const run = promisify(execFile);

/**
 * Resolve the fallow binary. Precedence:
 *   1. `FALLOW_BIN` (also carries the JSONC config's `fallowBin`, set in main).
 *   2. The workspace build, when running from source inside the fallow monorepo:
 *      this app lives at `apps/review-electron`, so the repo root (with
 *      `target/{release,debug}/fallow`) is two levels up from the launch cwd.
 *      This lets `pnpm dev` dogfood the repo's own build with no manual env.
 *   3. `fallow` on PATH (a packaged app or an external install).
 */
const fallowBin = (): string => {
  const fromEnv = process.env["FALLOW_BIN"]?.trim();
  if (fromEnv) return fromEnv;
  const repoRoot = join(process.cwd(), "..", "..");
  for (const variant of ["release", "debug"]) {
    const candidate = join(repoRoot, "target", variant, "fallow");
    if (existsSync(candidate)) return candidate;
  }
  return "fallow";
};
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
    change_anchors?: Array<{
      change_anchor?: string;
      file?: string;
      start_line?: number;
      line_count?: number;
      previous_change_anchor?: string;
    }>;
    direction?: { order?: string[] };
    agent_schema?: { judgment_shape?: string };
  }>(stdout);
  return {
    graphSnapshotHash: g.graph_snapshot_hash ?? "",
    emittedSignalIds: g.digest?.decisions?.emitted_signal_ids ?? [],
    changeAnchors: (g.change_anchors ?? []).flatMap((a) =>
      typeof a.change_anchor === "string" &&
      typeof a.file === "string" &&
      typeof a.start_line === "number" &&
      typeof a.line_count === "number"
        ? [
            {
              changeAnchor: a.change_anchor,
              file: a.file,
              startLine: a.start_line,
              lineCount: a.line_count,
              previousChangeAnchor: a.previous_change_anchor,
            },
          ]
        : [],
    ),
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
