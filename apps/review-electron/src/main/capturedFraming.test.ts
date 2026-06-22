import { describe, it, expect } from "vitest";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { capturedFramingPath, readCapturedLines } from "./capturedFraming";

/** A root with a seeded `.fallow-review/captured.jsonl`. */
const seedRoot = (label: string, contents: string): string => {
  const root = mkdtempSync(join(tmpdir(), label));
  mkdirSync(join(root, ".fallow-review"), { recursive: true });
  writeFileSync(capturedFramingPath(root), contents, "utf8");
  return root;
};

describe("readCapturedLines", () => {
  it("returns [] when the captured file does not exist (best-effort)", async () => {
    const root = mkdtempSync(join(tmpdir(), "captured-missing-"));
    expect(await readCapturedLines(root)).toEqual([]);
  });

  it("round-trips valid lines and skips corrupt JSON without dropping valid ones", async () => {
    const root = seedRoot(
      "captured-roundtrip-",
      [
        JSON.stringify({ signal_id: "sig:a", framing: "is this coupling intended?" }),
        "not json at all",
        JSON.stringify({
          signal_id: "sig:b",
          framing: "new public export",
          concern: "leaks infra",
        }),
        "",
      ].join("\n"),
    );
    const lines = await readCapturedLines(root);
    expect(lines).toHaveLength(2);
    expect(lines[0]?.signal_id).toBe("sig:a");
    expect(lines[1]?.concern).toBe("leaks infra");
  });

  it("skips malformed-shape lines (missing/empty signal_id, wrong types)", async () => {
    const root = seedRoot(
      "captured-shape-",
      [
        JSON.stringify({ framing: "no signal id" }),
        JSON.stringify({ signal_id: "", framing: "empty signal id" }),
        JSON.stringify({ signal_id: "sig:ok", framing: 42 }),
        JSON.stringify({ signal_id: "sig:bad-concern", framing: "f", concern: 7 }),
        JSON.stringify({ signal_id: "sig:keep", framing: "valid" }),
      ].join("\n"),
    );
    const lines = await readCapturedLines(root);
    expect(lines).toHaveLength(1);
    expect(lines[0]?.signal_id).toBe("sig:keep");
  });
});
