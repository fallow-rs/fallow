import { describe, it, expect } from "vitest";
import {
  resolveBackend,
  buildAgentPrompt,
  buildTradeOffPrompt,
  extractAgentJson,
  extractTradeOffJson,
} from "./backends";

describe("resolveBackend", () => {
  it("resolves known backends and rejects unknown", () => {
    expect(resolveBackend("codex")?.command).toBe("codex");
    expect(resolveBackend("nope")).toBeNull();
  });
});

describe("buildAgentPrompt", () => {
  it("embeds the digest, the hash to echo, and the schema shape", () => {
    const p = buildAgentPrompt({ a: 1 }, "graph:abc", "{ judgments: [...] }");
    expect(p).toContain('"a": 1');
    expect(p).toContain("graph:abc");
    expect(p).toContain("{ judgments: [...] }");
  });
});

describe("extractAgentJson", () => {
  const wt = '{"graph_snapshot_hash":"h","judgments":[{"signal_id":"s","framing":"f"}]}';

  it("parses raw JSON", () => {
    expect(extractAgentJson(wt)?.graph_snapshot_hash).toBe("h");
  });

  it("parses ```json fenced output", () => {
    expect(extractAgentJson("sure!\n```json\n" + wt + "\n```\n")?.judgments).toHaveLength(1);
  });

  it("parses JSON embedded in prose", () => {
    expect(extractAgentJson("here you go: " + wt + " done")?.graph_snapshot_hash).toBe("h");
  });

  it("returns null for non-judgment output", () => {
    expect(extractAgentJson("no json here")).toBeNull();
    expect(extractAgentJson('{"other":1}')).toBeNull();
  });
});

describe("buildTradeOffPrompt", () => {
  it("embeds the digest, the hash to echo, and the envelope shape", () => {
    const p = buildTradeOffPrompt({ a: 1 }, "graph:abc");
    expect(p).toContain('"a": 1');
    expect(p).toContain("graph:abc");
    expect(p).toContain('"tradeoffs"');
    expect(p).toContain("abstained:true");
  });
});

describe("extractTradeOffJson", () => {
  const env =
    '{"graph_snapshot_hash":"h","abstained":false,"tradeoffs":[{"anchor":"src/x.ts:1","lens":"l"}]}';

  it("parses raw, fenced, and embedded envelopes", () => {
    expect(extractTradeOffJson(env)?.["graph_snapshot_hash"]).toBe("h");
    expect(extractTradeOffJson("ok\n```json\n" + env + "\n```")?.["abstained"]).toBe(false);
    expect(extractTradeOffJson("here: " + env + " done")?.["graph_snapshot_hash"]).toBe("h");
  });

  it("accepts an abstained envelope with an empty tradeoffs array", () => {
    const abstained = '{"graph_snapshot_hash":"h","abstained":true,"tradeoffs":[]}';
    expect(extractTradeOffJson(abstained)?.["abstained"]).toBe(true);
  });

  it("returns null when the envelope guard fails", () => {
    expect(extractTradeOffJson("no json here")).toBeNull();
    expect(extractTradeOffJson('{"graph_snapshot_hash":"h"}')).toBeNull();
    expect(extractTradeOffJson('{"tradeoffs":[]}')).toBeNull();
  });
});
