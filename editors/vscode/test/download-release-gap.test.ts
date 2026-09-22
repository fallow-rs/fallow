import { EventEmitter } from "node:events";
import * as path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The window between a VSIX publication and its GitHub Release: the release
// endpoint answers 404 while an older, verified binary is already installed.

let mockFiles: Record<string, string | Buffer> = {};
let mockReleaseStatus = 404;
let mockReleaseBody = "";
let mockAssetBytes = Buffer.from("new-binary");
const httpsCalls: string[] = [];
const infoMessages: string[] = [];
const errorMessages: string[] = [];
const executedCommands: string[] = [];
let mockInfoChoice: string | undefined;
let mockErrorChoice: string | undefined;
let mockAutoDownload = true;

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
  readdirSync: (dir: string) => {
    const prefix = `${dir}${path.sep}`;
    return Object.keys(mockFiles)
      .filter((p) => p.startsWith(prefix))
      .map((p) => p.slice(prefix.length))
      .filter((name) => !name.includes(path.sep));
  },
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
      // Real streams emit "finish" asynchronously, after the listener is attached.
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
    const binary = String(args[0]);
    const bytes = mockFiles[binary];
    const version = bytes && bytes.toString() === "new-binary" ? "2.26.0" : "2.25.0";
    cb(null, { stdout: `fallow ${version}\n`, stderr: "" });
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
    if (statusCode < 400) {
      response.emit("data", body);
    }
    response.complete = true;
    response.emit("end");
  });
  return response;
};

vi.mock("node:https", () => ({
  get: (url: string, _options: unknown, cb: (response: unknown) => void) => {
    httpsCalls.push(url);
    const request = new EventEmitter() as EventEmitter & {
      setTimeout: () => void;
      destroy: () => void;
    };
    request.setTimeout = () => {};
    request.destroy = () => {};
    queueMicrotask(() => {
      if (url.includes("/releases/")) {
        cb(respond(mockReleaseStatus, Buffer.from(mockReleaseBody)));
      } else if (url.endsWith(".sig")) {
        cb(respond(200, Buffer.alloc(64, 1)));
      } else {
        cb(respond(200, mockAssetBytes));
      }
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
    showInformationMessage: async (message: string) => {
      infoMessages.push(message);
      return mockInfoChoice;
    },
    showErrorMessage: async (message: string) => {
      errorMessages.push(message);
      return mockErrorChoice;
    },
    showWarningMessage: async () => undefined,
  },
  commands: {
    executeCommand: async (command: string) => {
      executedCommands.push(command);
    },
  },
  workspace: {
    getConfiguration: () => ({
      get: (key: string, fallback: unknown) =>
        key === "autoDownload" ? mockAutoDownload : fallback,
    }),
  },
}));

import {
  cancelReleaseRetry,
  downloadBinary,
  downloadCliBinary,
  isReleaseNotPublished,
  platformTargetFor,
  readVersionMarker,
} from "../src/download.js";

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as any;
const binDir = path.join("/storage", "bin");
const lspPath = path.join(binDir, "fallow-lsp");
const cliPath = path.join(binDir, "fallow");
const oldBytes = Buffer.from("old-binary");
const signatureBytes = Buffer.alloc(64, 1);
const outputLines: string[] = [];
const outputChannel = { appendLine: (line: string) => outputLines.push(line) } as any;

const installOldBinaries = () => {
  mockFiles[lspPath] = oldBytes;
  mockFiles[`${lspPath}.sig`] = signatureBytes;
  mockFiles[cliPath] = oldBytes;
  mockFiles[`${cliPath}.sig`] = signatureBytes;
  mockFiles[path.join(binDir, ".fallow-version")] = "2.25.0";
};

const publishedRelease = () => {
  const target = platformTargetFor(process.platform, process.arch);
  const extension = process.platform === "win32" ? ".exe" : "";
  mockReleaseStatus = 200;
  mockReleaseBody = JSON.stringify({
    tag_name: "v2.26.0",
    assets: ["fallow-lsp", "fallow"].flatMap((name) => [
      {
        name: `${name}-${target}${extension}`,
        browser_download_url: `https://example.invalid/${name}`,
      },
      {
        name: `${name}-${target}${extension}.sig`,
        browser_download_url: `https://example.invalid/${name}.sig`,
      },
    ]),
  });
};

beforeEach(() => {
  mockFiles = {};
  mockReleaseStatus = 404;
  mockReleaseBody = "";
  mockAssetBytes = Buffer.from("new-binary");
  httpsCalls.length = 0;
  infoMessages.length = 0;
  errorMessages.length = 0;
  executedCommands.length = 0;
  outputLines.length = 0;
  mockInfoChoice = undefined;
  mockErrorChoice = undefined;
  mockAutoDownload = true;
  vi.useFakeTimers();
});

afterEach(() => {
  cancelReleaseRetry();
  vi.useRealTimers();
});

describe("isReleaseNotPublished", () => {
  it("recognises only the release endpoint's 404", () => {
    expect(isReleaseNotPublished(new Error("HTTP 404"))).toBe(true);
    expect(isReleaseNotPublished(new Error("HTTP 500"))).toBe(false);
    expect(isReleaseNotPublished(new Error("ENOTFOUND"))).toBe(false);
    expect(isReleaseNotPublished("HTTP 404")).toBe(false);
  });
});

describe("downloadBinary while the release is not published", () => {
  it("keeps serving the installed LSP, shows no modal, and schedules a retry", async () => {
    installOldBinaries();

    const result = await downloadBinary(fakeContext, outputChannel);

    expect(result).toBe(lspPath);
    expect(mockFiles[lspPath]).toBe(oldBytes);
    expect(mockFiles[cliPath]).toBe(oldBytes);
    expect(errorMessages).toEqual([]);
    expect(infoMessages).toHaveLength(1);
    expect(infoMessages[0]).toMatch(/release v2\.26\.0 is not published yet/u);
    expect(infoMessages[0]).toMatch(/retries in the background/u);
    expect(
      outputLines.some((line) => line.includes("retrying the release download in 5 minutes")),
    ).toBe(true);
  });

  it("prompts as before when nothing is installed to fall back to", async () => {
    mockErrorChoice = undefined;

    const result = await downloadBinary(fakeContext, outputChannel);

    expect(result).toBeNull();
    expect(errorMessages).toHaveLength(1);
    expect(errorMessages[0]).toMatch(/not published yet and no earlier binary is installed/u);
    expect(infoMessages).toEqual([]);
  });

  it("downloads the new version once the release exists and offers a restart", async () => {
    installOldBinaries();
    await downloadBinary(fakeContext, outputChannel);
    const releaseCallsBefore = httpsCalls.filter((url) => url.includes("/releases/")).length;

    publishedRelease();
    mockInfoChoice = "Restart";
    await vi.advanceTimersByTimeAsync(5 * 60 * 1000);

    expect(httpsCalls.filter((url) => url.includes("/releases/")).length).toBe(
      releaseCallsBefore + 1,
    );
    expect(mockFiles[lspPath]).toEqual(Buffer.from("new-binary"));
    expect(mockFiles[cliPath]).toEqual(Buffer.from("new-binary"));
    expect(readVersionMarker(binDir)).toBe("2.26.0");
    expect(infoMessages.at(-1)).toMatch(/v2\.26\.0 is now installed\. Restart/u);
    expect(executedCommands).toEqual(["fallow.restart"]);
  });

  it("backs off while the release stays missing and stops after the last delay", async () => {
    installOldBinaries();
    await downloadBinary(fakeContext, outputChannel);
    const releaseCalls = () => httpsCalls.filter((url) => url.includes("/releases/")).length;
    const first = releaseCalls();

    await vi.advanceTimersByTimeAsync(5 * 60 * 1000);
    expect(releaseCalls()).toBe(first + 1);
    await vi.advanceTimersByTimeAsync(15 * 60 * 1000);
    expect(releaseCalls()).toBe(first + 2);
    await vi.advanceTimersByTimeAsync(30 * 60 * 1000);
    expect(releaseCalls()).toBe(first + 3);
    await vi.advanceTimersByTimeAsync(60 * 60 * 1000);
    expect(releaseCalls()).toBe(first + 4);
    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
    expect(releaseCalls()).toBe(first + 4);
    expect(outputLines.at(-1)).toMatch(/next window reload retries the download/u);
    expect(mockFiles[lspPath]).toBe(oldBytes);
  });

  it("announces the gap once per session and schedules a single retry", async () => {
    installOldBinaries();
    await downloadBinary(fakeContext, outputChannel);
    await downloadCliBinary(fakeContext, outputChannel);
    await downloadCliBinary(fakeContext, outputChannel);

    expect(infoMessages).toHaveLength(1);
    expect(
      outputLines.filter((line) => line.includes("is not published yet; using the installed")),
    ).toHaveLength(3);
    expect(
      outputLines.filter((line) => line.includes("retrying the release download")),
    ).toHaveLength(1);
  });

  it("does not arm a background download when autoDownload is off", async () => {
    installOldBinaries();
    mockAutoDownload = false;

    const result = await downloadBinary(fakeContext, outputChannel);

    expect(result).toBe(lspPath);
    expect(infoMessages[0]).not.toMatch(/retries in the background/u);
    expect(outputLines.some((line) => line.includes("retrying the release download"))).toBe(false);
    publishedRelease();
    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);
    expect(mockFiles[lspPath]).toBe(oldBytes);
  });

  it("stops a scheduled retry when autoDownload is turned off before it fires", async () => {
    installOldBinaries();
    await downloadBinary(fakeContext, outputChannel);
    const releaseCallsBefore = httpsCalls.filter((url) => url.includes("/releases/")).length;

    mockAutoDownload = false;
    publishedRelease();
    await vi.advanceTimersByTimeAsync(5 * 60 * 1000);

    expect(httpsCalls.filter((url) => url.includes("/releases/")).length).toBe(releaseCallsBefore);
    expect(mockFiles[lspPath]).toBe(oldBytes);
    expect(outputLines.at(-1)).toMatch(/automatic download is disabled/u);
  });

  it("cancelReleaseRetry stops a pending retry", async () => {
    installOldBinaries();
    await downloadBinary(fakeContext, outputChannel);
    const releaseCallsBefore = httpsCalls.filter((url) => url.includes("/releases/")).length;

    cancelReleaseRetry();
    publishedRelease();
    await vi.advanceTimersByTimeAsync(6 * 60 * 60 * 1000);

    expect(httpsCalls.filter((url) => url.includes("/releases/")).length).toBe(releaseCallsBefore);
    expect(mockFiles[lspPath]).toBe(oldBytes);
  });
});

describe("downloadCliBinary while the release is not published", () => {
  it("keeps serving the installed CLI", async () => {
    installOldBinaries();

    const result = await downloadCliBinary(fakeContext, outputChannel);

    expect(result).toBe(cliPath);
    expect(mockFiles[cliPath]).toBe(oldBytes);
    expect(errorMessages).toEqual([]);
  });
});
