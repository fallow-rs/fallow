import { EventEmitter } from "node:events";
import * as path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

// A Linux x64 host whose C library the test sets. The managed download must
// pick the release asset for that C library, and a binary for the other C
// library must not count as installed.

let mockFiles: Record<string, string | Buffer> = {};
let mockLibc: "gnu" | "musl" = "gnu";
const downloadedUrls: string[] = [];

vi.mock("node:os", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:os")>()),
  platform: () => "linux",
  arch: () => "x64",
}));

vi.mock("../src/libc.js", () => ({
  hostLinuxLibc: () => mockLibc,
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

// A glibc binary does not start on a musl host, so its version probe fails.
vi.mock("node:child_process", () => ({
  execFile: (...args: unknown[]) => {
    const cb = args[args.length - 1] as (
      err: Error | null,
      result?: { stdout: string; stderr: string },
    ) => void;
    const bytes = mockFiles[String(args[0])]?.toString() ?? "";
    if (!bytes.startsWith(`${mockLibc}-`)) {
      cb(new Error("spawn ENOENT"));
      return;
    }
    cb(null, { stdout: "fallow 2.26.0\n", stderr: "" });
  },
}));

vi.mock("node:crypto", () => ({
  createPublicKey: () => ({ type: "mock-public-key" }),
  createHash: () => ({
    update() {
      return this;
    },
    digest() {
      return "0".repeat(64);
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

const ASSET_TARGETS = ["linux-x64-gnu", "linux-x64-musl"];

const releaseBody = (): string =>
  JSON.stringify({
    tag_name: "v2.26.0",
    assets: ["fallow-lsp", "fallow"].flatMap((name) =>
      ASSET_TARGETS.flatMap((target) => [
        {
          name: `${name}-${target}`,
          browser_download_url: `https://example.invalid/${name}-${target}`,
        },
        {
          name: `${name}-${target}.sig`,
          browser_download_url: `https://example.invalid/${name}-${target}.sig`,
        },
      ]),
    ),
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
        cb(respond(200, Buffer.from(releaseBody())));
        return;
      }
      downloadedUrls.push(url);
      if (url.endsWith(".sig")) {
        cb(respond(200, Buffer.alloc(64, 1)));
        return;
      }
      const libc = url.endsWith("-musl") ? "musl" : "gnu";
      cb(respond(200, Buffer.from(`${libc}-binary`)));
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
    showErrorMessage: async () => undefined,
    showWarningMessage: async () => undefined,
  },
  commands: { executeCommand: async () => undefined },
  workspace: {
    getConfiguration: () => ({ get: (_key: string, fallback: unknown) => fallback }),
  },
}));

import { downloadBinary, getInstalledBinaryPath, platformTargetFor } from "../src/download.js";

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as any;
const binDir = path.join("/storage", "bin");
const lspPath = path.join(binDir, "fallow-lsp");
const cliPath = path.join(binDir, "fallow");
const outputLines: string[] = [];
const outputChannel = { appendLine: (line: string) => outputLines.push(line) } as any;

// The install that an earlier extension version left: a glibc binary pair, a
// current version marker, and no target marker.
const installLegacyGlibcBinaries = (): void => {
  for (const binary of [lspPath, cliPath]) {
    mockFiles[binary] = Buffer.from("gnu-binary");
    mockFiles[`${binary}.sig`] = Buffer.alloc(64, 1);
  }
  mockFiles[path.join(binDir, ".fallow-version")] = "2.26.0";
};

const binaryDownloads = (): string[] => downloadedUrls.filter((url) => !url.endsWith(".sig"));

beforeEach(() => {
  mockFiles = {};
  mockLibc = "gnu";
  downloadedUrls.length = 0;
  outputLines.length = 0;
});

describe("platformTargetFor", () => {
  it("maps a musl Linux host to the musl release target", () => {
    expect(platformTargetFor("linux", "x64", "musl")).toBe("linux-x64-musl");
    expect(platformTargetFor("linux", "arm64", "musl")).toBe("linux-arm64-musl");
  });

  it("keeps glibc as the Linux default", () => {
    expect(platformTargetFor("linux", "x64")).toBe("linux-x64-gnu");
    expect(platformTargetFor("linux", "arm64", "gnu")).toBe("linux-arm64-gnu");
  });

  it("ignores the C library outside Linux", () => {
    expect(platformTargetFor("darwin", "arm64", "musl")).toBe("darwin-arm64");
    expect(platformTargetFor("win32", "x64", "musl")).toBe("win32-x64-msvc");
  });
});

describe("managed download on a Linux host", () => {
  it("downloads the musl binaries on a musl host and records their target", async () => {
    mockLibc = "musl";

    expect(await downloadBinary(fakeContext, outputChannel)).toBe(lspPath);

    expect(binaryDownloads()).toEqual([
      "https://example.invalid/fallow-lsp-linux-x64-musl",
      "https://example.invalid/fallow-linux-x64-musl",
    ]);
    expect(mockFiles[lspPath]).toEqual(Buffer.from("musl-binary"));
    expect(mockFiles[`${lspPath}.target`]).toBe("linux-x64-musl");
    expect(mockFiles[`${cliPath}.target`]).toBe("linux-x64-musl");
  });

  it("replaces glibc binaries from an earlier version on a musl host", async () => {
    mockLibc = "musl";
    installLegacyGlibcBinaries();

    expect(await downloadBinary(fakeContext, outputChannel)).toBe(lspPath);

    expect(binaryDownloads()).toEqual([
      "https://example.invalid/fallow-lsp-linux-x64-musl",
      "https://example.invalid/fallow-linux-x64-musl",
    ]);
    expect(mockFiles[lspPath]).toEqual(Buffer.from("musl-binary"));
    expect(mockFiles[cliPath]).toEqual(Buffer.from("musl-binary"));
  });

  it("does not count a glibc binary as installed on a musl host", async () => {
    mockLibc = "musl";
    installLegacyGlibcBinaries();

    expect(await getInstalledBinaryPath(fakeContext, outputChannel)).toBeNull();

    expect(outputLines).toEqual([
      "Fallow: installed LSP binary is for linux-x64-gnu, this host needs linux-x64-musl. Re-downloading.",
    ]);
    expect(mockFiles[lspPath]).toEqual(Buffer.from("gnu-binary"));
  });

  it("keeps glibc binaries from an earlier version on a glibc host", async () => {
    installLegacyGlibcBinaries();

    expect(await downloadBinary(fakeContext, outputChannel)).toBe(lspPath);

    expect(binaryDownloads()).toEqual([]);
    expect(mockFiles[lspPath]).toEqual(Buffer.from("gnu-binary"));
  });

  it("keeps musl binaries with a musl target marker on a musl host", async () => {
    mockLibc = "musl";
    await downloadBinary(fakeContext, outputChannel);
    downloadedUrls.length = 0;

    expect(await downloadBinary(fakeContext, outputChannel)).toBe(lspPath);

    expect(binaryDownloads()).toEqual([]);
  });
});
