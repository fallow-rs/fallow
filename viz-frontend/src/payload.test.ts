import { describe, expect, it } from "vitest";
import { decodeTables, hydratePayload } from "./payload";
import type { LazySection } from "./payload";

// The same literal is pinned in `crates/cli/src/viz/payload.rs`
// (`encoded_table_matches_the_frontend_fixture`), so the Rust encoder and
// this decoder cannot drift apart.
const PINNED_TABLE = '{"$k":["line","name","props"],"$r":[[1,"a",2],[2,"b"],[3,null,4]]}';

const counted = (path: string, value: unknown): LazySection & { reads: () => number } => {
  let reads = 0;
  return {
    path,
    text: () => {
      reads += 1;
      return JSON.stringify(value);
    },
    reads: () => reads,
  };
};

describe("decodeTables", () => {
  it("restores rows and leaves absent keys absent", () => {
    expect(decodeTables(JSON.parse(PINNED_TABLE))).toEqual([
      { line: 1, name: "a", props: 2 },
      { line: 2, name: "b" },
      { line: 3, props: 4 },
    ]);
    const third = (decodeTables(JSON.parse(PINNED_TABLE)) as Array<Record<string, unknown>>)[2];
    expect("name" in third).toBe(false);
  });

  it("decodes nested tables and keeps plain values", () => {
    const nested = {
      files: { $k: ["facts", "path"], $r: [[{ $k: ["label"], $r: [["x"], ["y"]] }, "a.ts"]] },
      edges: [[0, 1, 0]],
      root: "demo",
    };
    expect(decodeTables(nested)).toEqual({
      files: [{ facts: [{ label: "x" }, { label: "y" }], path: "a.ts" }],
      edges: [[0, 1, 0]],
      root: "demo",
    });
  });
});

describe("hydratePayload", () => {
  const core = JSON.stringify({
    root: "demo",
    files: { $k: ["path"], $r: [["a.ts"], ["b.ts"]] },
    health: { score: 90 },
  });

  it("parses a lazy section on first access only", () => {
    const findings = counted("health.findings", {
      $k: ["title"],
      $r: [["one"], ["two"]],
    });
    const data = hydratePayload(core, [findings]) as unknown as {
      health: { score: number; findings: Array<{ title: string }> };
    };

    expect(findings.reads()).toBe(0);
    expect(data.health.score).toBe(90);
    expect(findings.reads()).toBe(0);
    expect(data.health.findings).toEqual([{ title: "one" }, { title: "two" }]);
    expect(data.health.findings).toHaveLength(2);
    expect(findings.reads()).toBe(1);
  });

  it("spreads a per-file column over the file list with one parse", () => {
    const functions = counted("files.*.functions", [[{ name: "f", line: 1 }], []]);
    const data = hydratePayload(core, [functions]) as unknown as {
      files: Array<{ path: string; functions?: Array<{ name: string }> }>;
    };

    expect(functions.reads()).toBe(0);
    expect(data.files[0].path).toBe("a.ts");
    expect(functions.reads()).toBe(0);
    expect(data.files[1].functions).toBeUndefined();
    expect(data.files[0].functions).toEqual([{ name: "f", line: 1 }]);
    expect(functions.reads()).toBe(1);
  });

  it("keeps table-shaped objects when the page turns tables off", () => {
    const text = JSON.stringify({ root: "demo", summary: { $k: ["x"], $r: [[1]] } });
    const data = hydratePayload(text, [], false) as unknown as Record<string, unknown>;
    expect(data.summary).toEqual({ $k: ["x"], $r: [[1]] });
  });

  it("ignores a section whose parent is missing", () => {
    const orphan = counted("missing.findings", []);
    const data = hydratePayload(core, [orphan]) as unknown as Record<string, unknown>;
    expect(data.missing).toBeUndefined();
    expect(orphan.reads()).toBe(0);
  });
});
