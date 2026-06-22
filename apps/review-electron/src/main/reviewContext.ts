import { readFile } from "node:fs/promises";
import { join } from "node:path";
import type { ReviewContext, ReviewContextItem } from "../model/reviewContext";

/**
 * Persisted reader for the AUTHOR-AGENT review brief (the CAPTURE artifact): a
 * single JSON object the agent that did the work writes at write-time, read back at
 * cold-start beside `tradeoffs.json`. Modeled on `readPersistedTradeoffs`.
 *
 * Honesty rule: returns `null` when the file is ABSENT or unreadable (the renderer's
 * "no author context" state, which renders nothing). A present-but-malformed file is
 * defensively coerced rather than thrown: `summary`/`author` fall back to `""`,
 * `items` keep only objects with a string `note`, and each `anchor` coerces to a
 * string (default `""`). A genuinely empty/contentless file prefers `null`.
 */

/** Path to the persisted author-agent review brief. */
export const reviewContextPath = (root: string): string =>
  join(root, ".fallow-review", "review-context.json");

const asString = (v: unknown): string => (typeof v === "string" ? v : "");

/** Coerce one raw item: keep only when `note` is a string; default `anchor` to `""`. */
const toItem = (value: unknown): ReviewContextItem | null => {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  if (typeof v["note"] !== "string") return null;
  return { anchor: asString(v["anchor"]), note: v["note"] };
};

/**
 * Read + coerce the persisted review brief. Returns `null` when the file is absent
 * or unparseable, OR when the coerced result has no content at all (no summary and
 * no surviving items) , so the renderer's "no author context" (render-nothing) state
 * is reachable rather than a hollow empty card.
 */
export const readReviewContext = async (root: string): Promise<ReviewContext | null> => {
  let raw: string;
  try {
    raw = await readFile(reviewContextPath(root), "utf8");
  } catch {
    // Absent file: the author agent recorded no brief. The surface is simply absent.
    return null;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    // Present but unparseable: treated as "no author context", same as absent.
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const x = parsed as Record<string, unknown>;
  const summary = asString(x["summary"]);
  const author = asString(x["author"]);
  const items = (Array.isArray(x["items"]) ? x["items"] : [])
    .map(toItem)
    .filter((i): i is ReviewContextItem => i !== null);
  // Genuinely no content: prefer null over a hollow empty-but-present shape.
  if (summary.length === 0 && items.length === 0) return null;
  return { summary, author, items };
};
