import { describe, it, expect } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { readReviewContext, reviewContextPath } from "./reviewContext";

/**
 * Behaviour of the author-agent review-brief reader (the CAPTURE artifact). The
 * load-bearing invariants are the honesty ones: an ABSENT file is `null` (the
 * render-nothing "no author context" state), a malformed file is defensively
 * coerced rather than thrown, and a genuinely contentless file collapses to `null`
 * rather than a hollow empty card.
 */
const seed = (content: string): string => {
  const root = mkdtempSync(join(tmpdir(), "review-context-"));
  const path = reviewContextPath(root);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, content, "utf8");
  return root;
};

describe("readReviewContext", () => {
  it("returns null when the file is absent (the no-author-context state)", async () => {
    const root = mkdtempSync(join(tmpdir(), "review-context-missing-"));
    expect(await readReviewContext(root)).toBeNull();
  });

  it("reads a present brief and preserves summary, author, and anchored items", async () => {
    const root = seed(
      JSON.stringify({
        summary: "refactored getUser",
        author: "claude-code",
        items: [
          { anchor: "src/users.ts:1", note: "unbounded cache, no invalidation" },
          { anchor: "", note: "a general note with no anchor" },
        ],
      }),
    );
    const ctx = await readReviewContext(root);
    expect(ctx).not.toBeNull();
    expect(ctx?.summary).toBe("refactored getUser");
    expect(ctx?.author).toBe("claude-code");
    expect(ctx?.items).toHaveLength(2);
    expect(ctx?.items[0]).toEqual({
      anchor: "src/users.ts:1",
      note: "unbounded cache, no invalidation",
    });
    expect(ctx?.items[1]?.anchor).toBe("");
  });

  it("returns null on a present-but-unparseable file (not a throw)", async () => {
    const root = seed("{ not json");
    expect(await readReviewContext(root)).toBeNull();
  });

  it("coerces malformed fields: drops items without a string note, defaults anchor/strings", async () => {
    const root = seed(
      JSON.stringify({
        summary: 42,
        author: null,
        items: [
          { anchor: "src/a.ts:2", note: "valid" },
          { anchor: "src/b.ts:3" },
          { note: 99 },
          { anchor: 5, note: "anchor coerces to empty string" },
          "not an object",
        ],
      }),
    );
    const ctx = await readReviewContext(root);
    expect(ctx).not.toBeNull();
    expect(ctx?.summary).toBe("");
    expect(ctx?.author).toBe("");
    expect(ctx?.items).toHaveLength(2);
    expect(ctx?.items[0]).toEqual({ anchor: "src/a.ts:2", note: "valid" });
    expect(ctx?.items[1]).toEqual({ anchor: "", note: "anchor coerces to empty string" });
  });

  it("collapses a genuinely contentless brief (no summary, no items) to null", async () => {
    const root = seed(JSON.stringify({ summary: "", author: "claude-code", items: [] }));
    expect(await readReviewContext(root)).toBeNull();
  });
});
