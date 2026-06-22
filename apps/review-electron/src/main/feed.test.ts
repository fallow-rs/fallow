import { describe, it, expect } from "vitest";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { appendFeedItem, feedPath, readFeedItems } from "./feed";

describe("readFeedItems", () => {
  it("returns [] when the feed file does not exist (best-effort)", async () => {
    const root = mkdtempSync(join(tmpdir(), "feed-missing-"));
    expect(await readFeedItems(root)).toEqual([]);
  });

  it("round-trips appended notes and skips corrupt lines without dropping valid ones", async () => {
    const root = mkdtempSync(join(tmpdir(), "feed-roundtrip-"));
    await appendFeedItem(root, {
      target: { kind: "file_line", value: "src/a.ts:10" },
      note: "is this coupling intended?",
      at: "t1",
    });
    // A corrupt line must neither throw nor drop the surrounding valid items.
    writeFileSync(feedPath(root), "not json\n", { flag: "a" });
    await appendFeedItem(root, {
      target: { kind: "signal_id", value: "sig:1" },
      note: "looks fine",
      at: "t2",
    });

    const items = await readFeedItems(root);
    expect(items).toHaveLength(2);
    expect(items[0]?.note).toBe("is this coupling intended?");
    expect(items[1]?.target.value).toBe("sig:1");
  });
});
