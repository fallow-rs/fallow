import { describe, it, expect, afterEach } from "vitest";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import {
  startInspectServer,
  isLoopbackHost,
  isAllowedOrigin,
  parseSelection,
} from "./inspectServer";
import { feedPath } from "./feed";

let server: Server | undefined;

afterEach(() => {
  server?.close();
  server = undefined;
});

const bootServer = async (root: string): Promise<{ url: string; port: number }> => {
  server = startInspectServer(
    () => null,
    () => undefined,
    root,
    0,
  );
  const started = server;
  await new Promise<void>((resolvePromise) => started.once("listening", resolvePromise));
  const { port } = started.address() as AddressInfo;
  return { url: `http://127.0.0.1:${port}/fallow-select`, port };
};

const feedLineCount = (root: string): number => {
  if (!existsSync(feedPath(root))) return 0;
  return readFileSync(feedPath(root), "utf8")
    .split("\n")
    .filter((line) => line.trim().length > 0).length;
};

const validBody = JSON.stringify({ file: "src/App.tsx", line: 3, component: "App" });

describe("startInspectServer", () => {
  it("accepts a valid POST with no Origin header (e2e-compatibility case)", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-noorigin-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: validBody,
    });
    expect(res.status).toBe(200);
    expect(feedLineCount(root)).toBe(1);
  });

  it("accepts a valid POST from a loopback Origin and echoes it back", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-origin-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", origin: "http://localhost:5173" },
      body: validBody,
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("access-control-allow-origin")).toBe("http://localhost:5173");
    expect(feedLineCount(root)).toBe(1);
  });

  it("rejects a POST from a non-loopback Origin", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-evil-origin-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", origin: "https://evil.example" },
      body: validBody,
    });
    expect(res.status).toBe(403);
    expect(feedLineCount(root)).toBe(0);
  });

  it("rejects a POST with a non-loopback Host (DNS-rebinding)", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-evil-host-"));
    const { port } = await bootServer(root);
    // fetch() derives Host from the URL; talk raw HTTP so we can force a bogus Host.
    const net = await import("node:net");
    const response = await new Promise<string>((resolvePromise, reject) => {
      const socket = net.connect(port, "127.0.0.1", () => {
        const payload = validBody;
        socket.write(
          `POST /fallow-select HTTP/1.1\r\n` +
            `Host: evil.example\r\n` +
            `Content-Type: application/json\r\n` +
            `Content-Length: ${Buffer.byteLength(payload)}\r\n` +
            `Connection: close\r\n\r\n${payload}`,
        );
      });
      let data = "";
      socket.on("data", (chunk) => {
        data += chunk.toString();
      });
      socket.on("end", () => resolvePromise(data));
      socket.on("error", reject);
    });
    expect(response.startsWith("HTTP/1.1 403")).toBe(true);
    expect(feedLineCount(root)).toBe(0);
  });

  it("rejects a body larger than 64 KiB", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-toolarge-"));
    const { url } = await bootServer(root);
    const bigComponent = "x".repeat(70 * 1024);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ file: "src/App.tsx", line: 1, component: bigComponent }),
    });
    expect(res.status).toBe(413);
    expect(feedLineCount(root)).toBe(0);
  });

  it("rejects malformed JSON with 400", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-badjson-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "not json",
    });
    expect(res.status).toBe(400);
    expect(feedLineCount(root)).toBe(0);
  });

  it("rejects a payload with the wrong shape with 400", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-badshape-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ file: 42 }),
    });
    expect(res.status).toBe(400);
    expect(feedLineCount(root)).toBe(0);
  });

  it("answers OPTIONS from a loopback origin with 204", async () => {
    const root = mkdtempSync(join(tmpdir(), "inspect-options-"));
    const { url } = await bootServer(root);
    const res = await fetch(url, {
      method: "OPTIONS",
      headers: { origin: "http://localhost:5173" },
    });
    expect(res.status).toBe(204);
  });
});

describe("isLoopbackHost", () => {
  it("accepts bare loopback hostnames", () => {
    expect(isLoopbackHost("127.0.0.1")).toBe(true);
    expect(isLoopbackHost("localhost")).toBe(true);
  });

  it("accepts loopback hostnames with a port, including bracketed IPv6", () => {
    expect(isLoopbackHost("[::1]:7787")).toBe(true);
    expect(isLoopbackHost("localhost:0")).toBe(true);
  });

  it("rejects non-loopback hosts, the literal 'null', and empty/undefined input", () => {
    expect(isLoopbackHost("evil.example")).toBe(false);
    expect(isLoopbackHost("null")).toBe(false);
    expect(isLoopbackHost("")).toBe(false);
    expect(isLoopbackHost(undefined)).toBe(false);
  });
});

describe("isAllowedOrigin", () => {
  it("allows an absent Origin header (non-browser clients)", () => {
    expect(isAllowedOrigin(undefined)).toBe(true);
  });

  it("allows loopback http/https origins at any port", () => {
    expect(isAllowedOrigin("http://localhost:5173")).toBe(true);
    expect(isAllowedOrigin("http://127.0.0.1:3000")).toBe(true);
    expect(isAllowedOrigin("http://[::1]:8080")).toBe(true);
  });

  it("rejects non-loopback origins, the literal 'null', and malformed input", () => {
    expect(isAllowedOrigin("https://evil.example")).toBe(false);
    expect(isAllowedOrigin("null")).toBe(false);
    expect(isAllowedOrigin("")).toBe(false);
    expect(isAllowedOrigin("localhost:0")).toBe(false);
  });
});

describe("parseSelection", () => {
  it("parses a valid selection", () => {
    expect(parseSelection(validBody)).toEqual({ file: "src/App.tsx", line: 3, component: "App" });
  });

  it("rejects malformed JSON", () => {
    expect(parseSelection("not json")).toBeNull();
  });

  it("rejects a non-object payload", () => {
    expect(parseSelection("42")).toBeNull();
    expect(parseSelection("null")).toBeNull();
  });

  it("rejects an out-of-range or wrong-typed field", () => {
    expect(parseSelection(JSON.stringify({ file: 42, line: 1 }))).toBeNull();
    expect(parseSelection(JSON.stringify({ file: "", line: 1 }))).toBeNull();
    expect(parseSelection(JSON.stringify({ file: "a.tsx", line: -1 }))).toBeNull();
    expect(parseSelection(JSON.stringify({ file: "a.tsx", line: 1.5 }))).toBeNull();
    expect(parseSelection(JSON.stringify({ file: "a.tsx", line: 1, column: -1 }))).toBeNull();
    expect(
      parseSelection(JSON.stringify({ file: "a.tsx", line: 1, component: "x".repeat(257) })),
    ).toBeNull();
  });
});
