import { EventEmitter } from "node:events";
import * as path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

// A rename onto a locked path fails, as on Windows when a running process
// holds the binary open. An upgrade that cannot replace the binary must leave
// the installed binary and its verification data intact.

let files: Record<string, string | Buffer> = {};
let lockedPaths = new Set<string>();
// Runs when a rename onto a locked path fails, to model a sibling window.
let onLockedRename: () => void = () => {};
let servedSignature = "";

const errno = (code: string): NodeJS.ErrnoException =>
  Object.assign(new Error(code), { code }) as NodeJS.ErrnoException;

vi.mock("node:os", async (importOriginal) => ({
  ...(await importOriginal<typeof import("node:os")>()),
  platform: () => "linux",
  arch: () => "x64",
}));

vi.mock("../src/libc.js", () => ({ hostLinuxLibc: () => "gnu" }));

vi.mock("node:fs", () => ({
  existsSync: (p: string) => p in files,
  readFileSync: (p: string) => {
    if (p in files) return files[p];
    throw errno("ENOENT");
  },
  writeFileSync: (p: string, content: string | Buffer) => {
    files[p] = content;
  },
  unlinkSync: (p: string) => {
    if (!(p in files)) throw errno("ENOENT");
    delete files[p];
  },
  copyFileSync: (from: string, to: string) => {
    files[to] = files[from];
  },
  renameSync: (from: string, to: string) => {
    if (!(from in files)) throw errno("ENOENT");
    if (lockedPaths.has(to)) {
      onLockedRename();
      throw errno("EPERM");
    }
    files[to] = files[from];
    delete files[from];
  },
  chmodSync: () => {},
  mkdirSync: () => {},
  openSync: () => 3,
  writeSync: () => {},
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
      files[p] = Buffer.concat(chunks);
      queueMicrotask(() => stream.emit("finish"));
    };
    stream.close = (cb?: () => void) => cb?.();
    stream.destroy = () => {};
    return stream;
  },
}));

// The binary bytes carry their version, so the version probe reads them.
vi.mock("node:child_process", () => ({
  execFile: (...args: unknown[]) => {
    const cb = args[args.length - 1] as (
      err: Error | null,
      result?: { stdout: string; stderr: string },
    ) => void;
    const bytes = files[String(args[0])]?.toString() ?? "";
    cb(null, { stdout: `fallow ${bytes.split("@")[1] ?? "0.0.0"}\n`, stderr: "" });
  },
}));

// A signature is valid only for the binary whose bytes it names.
vi.mock("node:crypto", () => ({
  createPublicKey: () => ({ type: "mock-public-key" }),
  createHash: () => {
    let input = "";
    return {
      update(data: string | Buffer) {
        input = data.toString();
        return this;
      },
      digest: () => Buffer.from(input).toString("hex").padEnd(64, "0").slice(0, 64),
    };
  },
  verify: (_algorithm: null, data: Buffer, _key: unknown, signature: Buffer) =>
    signature.toString() === `sig:${data.toString()}`,
}));

const respond = (body: Buffer) => {
  const response = new EventEmitter() as EventEmitter & {
    statusCode: number;
    headers: Record<string, string>;
    complete: boolean;
    resume: () => void;
    destroy: () => void;
    pipe: (dest: { write: (c: Buffer) => boolean; end: () => void }) => void;
  };
  response.statusCode = 200;
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

const NEW_BINARY = "fallow@2.26.0";
const ASSET = "fallow-linux-x64-gnu";

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
        const release = {
          tag_name: "v2.26.0",
          assets: [ASSET, `${ASSET}.sig`].map((name) => ({
            name,
            browser_download_url: `https://example.invalid/${name}`,
          })),
        };
        cb(respond(Buffer.from(JSON.stringify(release))));
        return;
      }
      cb(respond(Buffer.from(url.endsWith(".sig") ? servedSignature : NEW_BINARY)));
    });
    return request;
  },
}));

const showErrorMessage = vi.fn(async () => undefined);

vi.mock("vscode", () => ({
  extensions: { getExtension: () => ({ packageJSON: { version: "2.26.0" } }) },
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
    showErrorMessage: (...args: unknown[]) => showErrorMessage(...(args as [])),
    showWarningMessage: async () => undefined,
  },
  commands: { executeCommand: async () => undefined },
  workspace: {
    getConfiguration: () => ({ get: (_key: string, fallback: unknown) => fallback }),
  },
}));

import { downloadCliBinary, verifyBinarySignature } from "../src/download.js";

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as never;
const binDir = path.join("/storage", "bin");
const cliPath = path.join(binDir, "fallow");
const OLD_BINARY = "fallow@2.25.0";

const installOldCli = (): void => {
  files[cliPath] = Buffer.from(OLD_BINARY);
  files[`${cliPath}.sig`] = Buffer.from(`sig:${OLD_BINARY}`);
  files[`${cliPath}.target`] = "linux-x64-gnu";
  files[path.join(binDir, ".fallow-version")] = "2.25.0";
};

const tempFiles = (): string[] =>
  Object.keys(files).filter((p) => path.basename(p).includes(".tmp"));

beforeEach(() => {
  files = {};
  lockedPaths = new Set();
  onLockedRename = () => {};
  servedSignature = `sig:${NEW_BINARY}`;
  showErrorMessage.mockClear();
});

describe("managed upgrade", () => {
  it("keeps the installed binary verified when the new binary cannot replace it", async () => {
    installOldCli();
    lockedPaths.add(cliPath);

    expect(await downloadCliBinary(fakeContext)).toBeNull();

    expect(showErrorMessage).toHaveBeenCalledOnce();
    expect(files[cliPath]).toEqual(Buffer.from(OLD_BINARY));
    expect(files[`${cliPath}.sig`]).toEqual(Buffer.from(`sig:${OLD_BINARY}`));
    expect(files[`${cliPath}.target`]).toBe("linux-x64-gnu");
    expect(verifyBinarySignature(cliPath)).toBe(true);
    expect(tempFiles()).toEqual([]);
  });

  it("removes the new sidecars when a first install cannot place the binary", async () => {
    lockedPaths.add(cliPath);

    expect(await downloadCliBinary(fakeContext)).toBeNull();

    expect(cliPath in files).toBe(false);
    expect(`${cliPath}.sig` in files).toBe(false);
    expect(`${cliPath}.target` in files).toBe(false);
    expect(tempFiles()).toEqual([]);
  });

  it("keeps a signature that a sibling window published during the failed upgrade", async () => {
    installOldCli();
    lockedPaths.add(cliPath);
    const siblingSignature = Buffer.from("sig:fallow@2.26.0-sibling");
    onLockedRename = () => {
      files[`${cliPath}.sig`] = siblingSignature;
    };

    expect(await downloadCliBinary(fakeContext)).toBeNull();

    expect(files[`${cliPath}.sig`]).toEqual(siblingSignature);
    expect(files[`${cliPath}.target`]).toBe("linux-x64-gnu");
  });

  it("leaves every installed file alone when the new signature does not verify", async () => {
    installOldCli();
    files[`${cliPath}.sha256`] = "legacy-digest";
    servedSignature = "sig:something-else";

    expect(await downloadCliBinary(fakeContext)).toBeNull();

    expect(files[cliPath]).toEqual(Buffer.from(OLD_BINARY));
    expect(files[`${cliPath}.sig`]).toEqual(Buffer.from(`sig:${OLD_BINARY}`));
    expect(files[`${cliPath}.target`]).toBe("linux-x64-gnu");
    expect(files[`${cliPath}.sha256`]).toBe("legacy-digest");
    expect(tempFiles()).toEqual([]);
  });

  it("replaces the binary and its sidecars on a successful upgrade", async () => {
    installOldCli();

    expect(await downloadCliBinary(fakeContext)).toBe(cliPath);

    expect(files[cliPath]).toEqual(Buffer.from(NEW_BINARY));
    expect(files[`${cliPath}.sig`]).toEqual(Buffer.from(`sig:${NEW_BINARY}`));
    expect(files[`${cliPath}.target`]).toBe("linux-x64-gnu");
    expect(verifyBinarySignature(cliPath)).toBe(true);
    expect(tempFiles()).toEqual([]);
  });
});
