import { spawn } from "node:child_process";
import { once } from "node:events";
import { resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { describe, expect, it } from "vitest";

const fixturePath = resolve(__dirname, "integration/fixtures/fallow-lsp.cjs");

const frame = (message: unknown): Buffer => {
  const payload = Buffer.from(JSON.stringify(message), "utf8");
  return Buffer.concat([
    Buffer.from(`Content-Length: ${payload.length}\r\n\r\n`, "ascii"),
    payload,
  ]);
};

const runFixture = async (
  chunks: readonly Buffer[],
): Promise<{ code: number | null; output: Buffer }> => {
  const child = spawn(process.execPath, [fixturePath], { stdio: ["pipe", "pipe", "pipe"] });
  const output: Buffer[] = [];
  let stderr = "";
  child.stdout.on("data", (chunk: Buffer) => output.push(chunk));
  child.stderr.on("data", (chunk: Buffer) => {
    stderr += chunk.toString("utf8");
  });
  const closed = once(child, "close");
  const deadline = setTimeout(() => child.kill("SIGKILL"), 3000);
  try {
    for (const chunk of chunks) {
      child.stdin.write(chunk);
      // Separate writes exercise buffering before a header or payload is complete.
      await delay(20);
    }
    child.stdin.end();
    const [code, signal] = (await closed) as [number | null, NodeJS.Signals | null];
    expect(signal, stderr).toBeNull();
    return { code, output: Buffer.concat(output) };
  } finally {
    clearTimeout(deadline);
    child.kill();
  }
};

const initialize = (rootPath: string): unknown => ({
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: { rootPath },
});
const initialized = frame({ jsonrpc: "2.0", id: 1, result: { capabilities: {} } });
const exit = frame({ jsonrpc: "2.0", method: "exit" });

describe("integration fake LSP transport", () => {
  it("accepts a header split across writes", async () => {
    const request = frame(initialize("/tmp/ascii"));
    const result = await runFixture([request.subarray(0, 8), request.subarray(8), exit]);
    expect(result.code).toBe(0);
    expect(result.output).toEqual(initialized);
  });

  it("buffers UTF-8 payload bytes split inside a Unicode character", async () => {
    const request = frame(initialize("/tmp/project ruimte 漢字"));
    const split = request.indexOf(Buffer.from("漢", "utf8")) + 1;
    const result = await runFixture([request.subarray(0, split), request.subarray(split), exit]);
    expect(result.code).toBe(0);
    expect(result.output).toEqual(initialized);
  });

  it("processes consecutive frames and preserves request and shutdown responses", async () => {
    const result = await runFixture([
      Buffer.concat([
        frame(initialize("/tmp/project 漢字")),
        frame({ jsonrpc: "2.0", method: "initialized", params: {} }),
        frame({ jsonrpc: "2.0", id: 2, method: "unknown" }),
        frame({ jsonrpc: "2.0", id: 3, method: "shutdown" }),
        exit,
      ]),
    ]);
    expect(result.code).toBe(0);
    expect(result.output).toEqual(
      Buffer.concat([
        initialized,
        frame({ jsonrpc: "2.0", id: 2, error: { code: -32601, message: "Method not found" } }),
        frame({ jsonrpc: "2.0", id: 3, result: null }),
      ]),
    );
  });

  it("rejects a header without Content-Length", async () => {
    const result = await runFixture([Buffer.from("Invalid: 0\r\n\r\n", "ascii")]);
    expect(result.code).toBe(1);
    expect(result.output.length).toBe(0);
  });
});
