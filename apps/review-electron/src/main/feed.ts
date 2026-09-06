import { appendFile, mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { FeedItem } from "../model/agent";

/** JSONL feed of human annotations the coding agent reads. */
export const feedPath = (root: string): string => join(root, ".fallow-review", "feed.jsonl");

export const appendFeedItem = async (root: string, item: FeedItem): Promise<void> => {
  const path = feedPath(root);
  await mkdir(dirname(path), { recursive: true });
  await appendFile(path, `${JSON.stringify(item)}\n`, "utf8");
};
