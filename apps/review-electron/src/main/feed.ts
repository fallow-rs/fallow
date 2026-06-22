import { appendFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { FeedItem } from "../model/agent";

/** JSONL feed of human annotations the coding agent reads. */
export const feedPath = (root: string): string => join(root, ".fallow-review", "feed.jsonl");

export const appendFeedItem = async (root: string, item: FeedItem): Promise<void> => {
  const path = feedPath(root);
  await mkdir(dirname(path), { recursive: true });
  await appendFile(path, `${JSON.stringify(item)}\n`, "utf8");
};

/**
 * Read every human annotation from the feed (oldest first). The agent loop
 * consumes these as UNVERIFIED reviewer context, closing the write-only dead-end
 * (notes used to land here and never reach the agent). A missing file or a
 * corrupt line yields no item rather than throwing; the feed is best-effort.
 */
export const readFeedItems = async (root: string): Promise<FeedItem[]> => {
  let raw: string;
  try {
    raw = await readFile(feedPath(root), "utf8");
  } catch {
    return [];
  }
  return raw
    .split("\n")
    .filter((line) => line.trim().length > 0)
    .flatMap((line) => {
      try {
        return [JSON.parse(line) as FeedItem];
      } catch {
        return [];
      }
    });
};
