import { readFile } from "node:fs/promises";
import { join } from "node:path";
import type { AgentWalkthrough, InlineFraming, Judgment } from "../model/agent";
import { runGuide, validateWalkthrough } from "./review";

/**
 * Reader for AUTHOR-CAPTURED framing: the framing an author agent recorded at
 * write-time, persisted to `.fallow-review/captured.jsonl`. Distinct module name
 * from `capture.ts` (the PNG screenshotter) to avoid a collision.
 *
 * Captured framing is fact-ish about authorial intent, but it is still held to
 * the IDENTICAL anchoring bar as reconstructed (agent-run) framing: it is routed
 * through the SAME `validateWalkthrough` graph path (signal_id membership +
 * snapshot-hash, content-agnostic), so a moved tree or an unanchored signal is
 * rejected exactly as it would be for feed/reconstructed framing.
 *
 * Honesty rule: if no captured source exists (or it is empty/corrupt), the reader
 * yields []. The UI then shows only reconstructed framing (or nothing). A
 * reconstructed item is NEVER relabelled captured, because origin is tagged here
 * at the source, not inferred downstream.
 */

/** One author-captured line: a signal-anchored framing, JSONL like feed.jsonl. */
export type CapturedLine = {
  signal_id: string;
  framing: string;
  concern?: string;
};

export const capturedFramingPath = (root: string): string =>
  join(root, ".fallow-review", "captured.jsonl");

const isCapturedLine = (value: unknown): value is CapturedLine => {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v["signal_id"] === "string" &&
    v["signal_id"].length > 0 &&
    typeof v["framing"] === "string" &&
    (v["concern"] === undefined || typeof v["concern"] === "string")
  );
};

/**
 * Read every captured line (oldest first). A missing file or a corrupt/malformed
 * line yields no item rather than throwing; the source is best-effort, exactly
 * like {@link readFeedItems} for the human feed.
 */
export const readCapturedLines = async (root: string): Promise<CapturedLine[]> => {
  let raw: string;
  try {
    raw = await readFile(capturedFramingPath(root), "utf8");
  } catch {
    return [];
  }
  return raw
    .split("\n")
    .filter((line) => line.trim().length > 0)
    .flatMap((line) => {
      try {
        const parsed: unknown = JSON.parse(line);
        return isCapturedLine(parsed) ? [parsed] : [];
      } catch {
        return [];
      }
    });
};

const toJudgment = (line: CapturedLine): Judgment => {
  const judgment: Judgment = { signal_id: line.signal_id, framing: line.framing };
  if (line.concern !== undefined) judgment.concern = line.concern;
  return judgment;
};

/** Subset of the validation envelope this reader maps to inline framing. */
type ValidatedAccepted = {
  accepted?: { signal_id: string; agent_framing: string; concern?: string }[];
};

const toCapturedFraming = (envelope: ValidatedAccepted): InlineFraming[] =>
  (envelope.accepted ?? []).map((j) => {
    const framing: InlineFraming = {
      signalId: j.signal_id,
      origin: "captured",
      framing: j.agent_framing,
      deterministic: false,
    };
    if (j.concern !== undefined) framing.concern = j.concern;
    return framing;
  });

/**
 * Read captured framing and validate it against the live graph, returning the
 * accepted entries tagged `origin:'captured'`. Empty source -> []. The current
 * guide's `graph_snapshot_hash` is echoed into the payload so stale captures are
 * rejected by the same path that rejects stale reconstructed judgments.
 */
export const readCapturedFraming = async (root: string): Promise<InlineFraming[]> => {
  const lines = await readCapturedLines(root);
  if (lines.length === 0) return [];
  const guide = await runGuide(root);
  const payload: AgentWalkthrough = {
    graph_snapshot_hash: guide.graphSnapshotHash,
    judgments: lines.map(toJudgment),
  };
  const envelope = (await validateWalkthrough(payload, root)) as ValidatedAccepted;
  return toCapturedFraming(envelope);
};
