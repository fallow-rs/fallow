import { describe, it, expect } from "vitest";
import { resolveBackend, buildAgentPrompt, extractAgentJson } from "./backends";

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
