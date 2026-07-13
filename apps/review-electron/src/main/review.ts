import { existsSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { toWalkthroughDocument } from "../model/adapter";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { AgentWalkthrough, Guide } from "../model/agent";
import { describeExecError } from "./errors";
import {
  FALLOW_DEADLINE_MS,
  STDERR_LIMIT_BYTES,
  STDOUT_LIMIT_BYTES,
  runProcess,
} from "./processRun";
import {
  parseGuideContract,
  parseReviewContract,
  parseWalkthroughValidationContract,
  type WalkthroughValidationContract,
} from "./fallowContract";

/**
 * Resolve the fallow binary. Precedence:
 *   1. The current JSONC config's `fallowBin` application state.
 *   2. The ambient `FALLOW_BIN` environment value.
 *   3. The workspace build, when running from source inside the fallow monorepo:
 *      this app lives at `apps/review-electron`, so the repo root (with
 *      `target/{release,debug}/fallow`) is two levels up from the launch cwd.
 *      This lets `pnpm dev` dogfood the repo's own build with no manual env.
 *   4. `fallow` on PATH (a packaged app or an external install).
 */
export interface FallowBinaryEnvironment {
  ambient: string | undefined;
  cwd: string;
  exists: (path: string) => boolean;
}

let configuredFallowBin: string | null = null;

/** Applies the latest config value without changing the ambient environment. */
export const setConfiguredFallowBin = (value: string | null): void => {
  configuredFallowBin = value?.trim() || null;
};

/** Resolves one binary snapshot from explicit config, ambient state, workspace, then PATH. */
export const resolveFallowBin = (
  configured: string | null,
  environment: FallowBinaryEnvironment,
): string => {
  const fromConfig = configured?.trim();
  if (fromConfig) return fromConfig;
  const fromEnvironment = environment.ambient?.trim();
  if (fromEnvironment) return fromEnvironment;
  const repoRoot = join(environment.cwd, "..", "..");
  for (const variant of ["release", "debug"]) {
    const candidate = join(repoRoot, "target", variant, "fallow");
    if (environment.exists(candidate)) return candidate;
  }
  return "fallow";
};

/** Returns the current binary for a newly starting invocation. */
export const currentFallowBin = (
  environment: FallowBinaryEnvironment = {
    ambient: process.env["FALLOW_BIN"],
    cwd: process.cwd(),
    exists: existsSync,
  },
): string => resolveFallowBin(configuredFallowBin, environment);

const at = (root?: string): string => root ?? process.cwd();
/** Run the fallow CLI, translating spawn/exit failures into clean messages. */
const runFallow = async (args: string[], root?: string): Promise<string> => {
  const bin = currentFallowBin();
  try {
    const { stdout } = await runProcess({
      command: bin,
      args,
      cwd: at(root),
      input: "",
      deadlineMs: FALLOW_DEADLINE_MS,
      stdoutLimitBytes: STDOUT_LIMIT_BYTES,
      stderrLimitBytes: STDERR_LIMIT_BYTES,
    });
    return stdout;
  } catch (e) {
    throw describeExecError(e, bin);
  }
};

/** `fallow review --format json` -> normalized W1 document. */
export const runReview = async (root?: string): Promise<WalkthroughDocument> => {
  const stdout = await runFallow(["review", "--format", "json"], root);
  return toWalkthroughDocument(parseReviewContract(stdout));
};

/** `fallow review --walkthrough-guide --format json` -> the E5 agent-contract guide. */
export const runGuide = async (root?: string): Promise<Guide> => {
  const stdout = await runFallow(["review", "--walkthrough-guide", "--format", "json"], root);
  const g = parseGuideContract(stdout);
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
): Promise<WalkthroughValidationContract> =>
  withValidationPayloadFile(payload, async (file) => {
    const stdout = await runFallow(
      ["review", "--walkthrough-file", file, "--format", "json"],
      root,
    );
    return parseWalkthroughValidationContract(stdout);
  });

/** Uses a private random temp directory and removes the validation payload on every exit path. */
export const withValidationPayloadFile = async <T>(
  payload: AgentWalkthrough,
  validate: (file: string) => Promise<T> | T,
): Promise<T> => {
  const directory = await mkdtemp(join(tmpdir(), "fallow-agent-wt-"));
  const file = join(directory, "payload.json");
  try {
    await writeFile(file, JSON.stringify(payload), "utf8");
    return await validate(file);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
};
