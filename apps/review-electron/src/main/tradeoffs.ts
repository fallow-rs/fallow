import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { toTradeOffEnvelope } from "../model/adapter";
import type { TradeOffEnvelope } from "../model/tradeoff";

/**
 * Persisted store for the MODEL-INFERRED trade-off surface: a single JSON envelope
 * written by the trade-off elicitation run and read back at cold-start, beside
 * `feed.ts`'s JSONL feed. Distinct from the deterministic decision surface
 * (`fallow review`); these anchors fallow cannot post-validate, so nothing here is
 * graph-checked.
 */

/** Path to the persisted trade-off envelope. */
export const tradeoffsPath = (root: string): string =>
  join(root, ".fallow-review", "tradeoffs.json");

/**
 * Read + adapt the persisted trade-off envelope. Returns `null` when the file does
 * NOT exist (the renderer's "not run" state). A present-but-corrupt file is
 * best-effort: it yields the empty NON-abstained envelope via the adapter (a parse
 * failure must never masquerade as `abstained: true`), not a thrown error.
 */
export const readPersistedTradeoffs = async (root: string): Promise<TradeOffEnvelope | null> => {
  let raw: string;
  try {
    raw = await readFile(tradeoffsPath(root), "utf8");
  } catch {
    // Absent file: the elicitation was never run. Distinct from an empty envelope.
    return null;
  }
  try {
    return toTradeOffEnvelope(JSON.parse(raw));
  } catch {
    // Present but unparseable: best-effort empty envelope (NOT a fake abstain).
    return toTradeOffEnvelope(null);
  }
};

/** Persist a raw trade-off envelope (the producer's wire shape) for later reads. */
export const writePersistedTradeoffs = async (root: string, envelope: unknown): Promise<void> => {
  const path = tradeoffsPath(root);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, JSON.stringify(envelope), "utf8");
};
