import { runGuide, validateWalkthrough } from "./review";
import { readPersistedTradeoffs } from "./tradeoffs";
import type { ChangeAnchor, Judgment, ValidationEnvelope } from "../model/agent";

const ANCHOR_CROSS_CUTTING = "cross-cutting";

/**
 * Per-trade-off fallow-validation status:
 *  - `anchored`: fallow accepted the trade-off's `change_anchor` , it cites a real
 *    changed region the graph emitted (fallow-grade, not just agent-self-checked).
 *  - `unanchored`: the anchor maps to no changed region in the current diff, or
 *    fallow rejected it (`unknown-change-anchor`).
 *  - `not-anchorable`: a cross-cutting trade-off (no single changed line; it cannot
 *    be graph-validated and stays a model inference).
 */
export type TradeOffAnchorStatus = "anchored" | "unanchored" | "not-anchorable";

export type TradeOffValidation = {
  /** True when the trade-off envelope's snapshot hash no longer matches the live
   * graph: fallow refuses the whole payload as stale (the tree moved). */
  stale: boolean;
  /** Per-trade-off status, keyed by the trade-off `id`. */
  statusById: Record<string, TradeOffAnchorStatus>;
};

/** Map a trade-off's `file:line` anchor to the change_anchor whose changed region
 * contains that line, or `null` when no changed region covers it. */
const resolveChangeAnchor = (anchor: string, anchors: ChangeAnchor[]): string | null => {
  const idx = anchor.lastIndexOf(":");
  if (idx < 0) return null;
  const file = anchor.slice(0, idx);
  const line = Number(anchor.slice(idx + 1));
  if (!Number.isFinite(line)) return null;
  const hit = anchors.find(
    (a) => a.file === file && line >= a.startLine && line < a.startLine + a.lineCount,
  );
  return hit ? hit.changeAnchor : null;
};

/**
 * Close the loop: validate the persisted trade-off surface against the LIVE graph
 * through the SAME `fallow review --walkthrough-file` machinery that validates
 * signal_ids. Each trade-off's `file:line` anchor is mapped to a guide-emitted
 * change_anchor and cited as a judgment; fallow refuses the whole payload as stale
 * on snapshot drift and rejects any anchor it never emitted. Cross-cutting
 * trade-offs are not anchorable and stay model-inferred. A null envelope (no run)
 * or empty/abstained surface yields `null` (nothing to validate).
 */
export const validateTradeoffs = async (root: string): Promise<TradeOffValidation | null> => {
  const envelope = await readPersistedTradeoffs(root);
  if (envelope === null || envelope.abstained || envelope.tradeoffs.length === 0) return null;

  const guide = await runGuide(root);

  const statusById: Record<string, TradeOffAnchorStatus> = {};
  const cited = new Map<string, string>(); // trade-off id -> cited change_anchor id
  for (const t of envelope.tradeoffs) {
    if (t.anchor === ANCHOR_CROSS_CUTTING) {
      statusById[t.id] = "not-anchorable";
      continue;
    }
    const chg = resolveChangeAnchor(t.anchor, guide.changeAnchors);
    if (chg) {
      cited.set(t.id, chg);
    } else {
      statusById[t.id] = "unanchored";
    }
  }

  // Nothing citable (all cross-cutting / unanchored): detect staleness directly
  // from the snapshot hash, since there is no judgment to round-trip.
  if (cited.size === 0) {
    return { stale: envelope.graphSnapshotHash !== guide.graphSnapshotHash, statusById };
  }

  const judgments: Judgment[] = [...cited.entries()].map(([id, chg]) => ({
    signal_id: "",
    change_anchor: chg,
    framing: id,
  }));
  const result = (await validateWalkthrough(
    { graph_snapshot_hash: envelope.graphSnapshotHash, judgments },
    root,
  )) as ValidationEnvelope;

  if (result.stale) {
    return { stale: true, statusById };
  }
  const acceptedChg = new Set((result.accepted ?? []).map((a) => a.change_anchor ?? ""));
  for (const [id, chg] of cited.entries()) {
    statusById[id] = acceptedChg.has(chg) ? "anchored" : "unanchored";
  }
  return { stale: false, statusById };
};
