import { describe, it, expect } from "vitest";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { FeedItem } from "../model/agent";
import { appendFeedItem, feedPath } from "./feed";

describe("appendFeedItem", () => {
  it("creates the agent feed and appends complete ordered JSONL records", async () => {
    const root = mkdtempSync(join(tmpdir(), "feed-append-"));
    try {
      const items: FeedItem[] = [
        {
          target: { kind: "file_line", value: "src/a.ts:10" },
          note: 'is this coupling intended?\nConsider "isolation".',
          at: "t1",
        },
        {
          target: { kind: "signal_id", value: "sig:1" },
          note: "looks fine",
          at: "t2",
        },
      ];
      for (const item of items) await appendFeedItem(root, item);

      const raw = readFileSync(feedPath(root), "utf8");
      expect(raw.endsWith("\n")).toBe(true);
      expect(
        raw
          .trimEnd()
          .split("\n")
          .map((line) => JSON.parse(line)),
      ).toEqual(items);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});
