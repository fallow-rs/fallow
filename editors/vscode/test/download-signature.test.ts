import { EventEmitter } from "node:events";
import * as path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

// A release asset without an Ed25519 `.sig` sibling must never install, even
// when its GitHub API `digest` matches the bytes: anyone with upload rights to
// the release controls that digest.

let mockFiles: Record<string, string | Buffer> = {};
const showErrorMessage = vi.fn(async (..._args: unknown[]) => undefined);

vi.mock("node:os", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:os")>()),
  platform: () => "darwin",
  arch: () => "arm64",
}));

vi.mock("node:fs", () => ({
  existsSync: (p: string) => p in mockFiles,
  readFileSync: (p: string) => {
    if (p in mockFiles) return mockFiles[p];
    throw new Error("ENOENT");
  },
  writeFileSync: (p: string, content: string | Buffer) => {
    mockFiles[p] = content;
  },
  unlinkSync: (p: string) => {
    delete mockFiles[p];
  },
  renameSync: (from: string, to: string) => {
    mockFiles[to] = mockFiles[from];
    delete mockFiles[from];
  },
  chmodSync: () => {},
  mkdirSync: () => {},
  openSync: () => 3,
  closeSync: () => {},
  statSync: () => ({ mtimeMs: Date.now() }),
  readdirSync: () => [],
  createWriteStream: (p: string) => {
    const chunks: Buffer[] = [];
    const stream = new EventEmitter() as EventEmitter & {
      write: (chunk: Buffer) => boolean;
      end: () => void;
      close: (cb?: () => void) => void;
      destroy: () => void;
    };
    stream.write = (chunk: Buffer) => {
      chunks.push(chunk);
      return true;
    };
    stream.end = () => {
      mockFiles[p] = Buffer.concat(chunks);
      queueMicrotask(() => stream.emit("finish"));
    };
    stream.close = (cb?: () => void) => cb?.();
    stream.destroy = () => {};
    return stream;
  },
}));

vi.mock("node:child_process", () => ({
  execFile: (...args: unknown[]) => {
    const cb = args[args.length - 1] as (
      err: Error | null,
      result?: { stdout: string; stderr: string },
    ) => void;
    cb(null, { stdout: "fallow 2.26.0\n", stderr: "" });
  },
}));

const MATCHING_DIGEST = "a".repeat(64);

vi.mock("node:crypto", () => ({
  createPublicKey: () => ({ type: "mock-public-key" }),
  createHash: () => ({
    update() {
      return this;
    },
    digest() {
      return MATCHING_DIGEST;
    },
  }),
  verify: () => true,
}));

const respond = (statusCode: number, body: Buffer) => {
  const response = new EventEmitter() as EventEmitter & {
    statusCode: number;
    headers: Record<string, string>;
    complete: boolean;
    resume: () => void;
    destroy: () => void;
    pipe: (dest: { write: (c: Buffer) => boolean; end: () => void }) => void;
  };
  response.statusCode = statusCode;
  response.headers = {};
  response.complete = false;
  response.resume = () => {};
  response.destroy = () => {};
  response.pipe = (dest) => {
    dest.write(body);
    dest.end();
  };
  queueMicrotask(() => {
    response.emit("data", body);
    response.complete = true;
    response.emit("end");
  });
  return response;
};

const unsignedReleaseBody = (): string =>
  JSON.stringify({
    tag_name: "v2.26.0",
    assets: ["fallow-lsp", "fallow"].map((name) => ({
      name: `${name}-darwin-arm64`,
      browser_download_url: `https://example.invalid/${name}-darwin-arm64`,
      digest: `sha256:${MATCHING_DIGEST}`,
    })),
  });

vi.mock("node:https", () => ({
  get: (url: string, _options: unknown, cb: (response: unknown) => void) => {
    const request = new EventEmitter() as EventEmitter & {
      setTimeout: () => void;
      destroy: () => void;
    };
    request.setTimeout = () => {};
    request.destroy = () => {};
    queueMicrotask(() => {
      if (url.includes("/releases/")) {
        cb(respond(200, Buffer.from(unsignedReleaseBody())));
        return;
      }
      cb(respond(200, Buffer.from("unsigned-binary")));
    });
    return request;
  },
}));

vi.mock("vscode", () => ({
  extensions: {
    getExtension: () => ({ packageJSON: { version: "2.26.0" } }),
  },
  ProgressLocation: { Notification: 15 },
  window: {
    withProgress: async (
      _options: unknown,
      task: (p: unknown, token: unknown) => Promise<unknown>,
    ) =>
      task(
        {},
        { isCancellationRequested: false, onCancellationRequested: () => ({ dispose() {} }) },
      ),
    showInformationMessage: async () => undefined,
    showErrorMessage: (...args: unknown[]) => showErrorMessage(...args),
    showWarningMessage: async () => undefined,
  },
  commands: { executeCommand: async () => undefined },
  workspace: {
    getConfiguration: () => ({ get: (_key: string, fallback: unknown) => fallback }),
  },
}));

import { downloadBinary, downloadCliBinary } from "../src/download.js";

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as never;
const binDir = path.join("/storage", "bin");
const lspPath = path.join(binDir, "fallow-lsp");
const cliPath = path.join(binDir, "fallow");

beforeEach(() => {
  mockFiles = {};
  showErrorMessage.mockClear();
});

describe("release asset without a signature", () => {
  it("does not install the LSP binary when only the GitHub digest matches", async () => {
    expect(await downloadBinary(fakeContext)).toBeNull();

    expect(lspPath in mockFiles).toBe(false);
    expect(`${lspPath}.sha256` in mockFiles).toBe(false);
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    const message = String(showErrorMessage.mock.calls[0][0]);
    expect(message).toContain("fallow-lsp-darwin-arm64 has no Ed25519 signature");
    expect(message).toContain("fallow.lspPath");
  });

  it("does not install the CLI binary when only the GitHub digest matches", async () => {
    expect(await downloadCliBinary(fakeContext)).toBeNull();

    expect(cliPath in mockFiles).toBe(false);
    expect(`${cliPath}.sha256` in mockFiles).toBe(false);
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(String(showErrorMessage.mock.calls[0][0])).toContain("fallow.lspPath");
  });
});
