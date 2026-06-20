import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { toWalkthroughDocument, type AuditBrief } from "../model/adapter";
import type { WalkthroughDocument } from "../model/walkthrough";

const run = promisify(execFile);

/** Resolved fallow binary: FALLOW_BIN override, else `fallow` on PATH. */
const fallowBin = (): string => process.env["FALLOW_BIN"] ?? "fallow";

/**
 * Run `fallow review --format json` in `root` (default cwd) and normalize the
 * audit-brief into a {@link WalkthroughDocument}.
 */
export const runReview = async (root?: string): Promise<WalkthroughDocument> => {
  const { stdout } = await run(fallowBin(), ["review", "--format", "json"], {
    cwd: root ?? process.cwd(),
    maxBuffer: 64 * 1024 * 1024,
  });
  return toWalkthroughDocument(JSON.parse(stdout) as AuditBrief);
};
