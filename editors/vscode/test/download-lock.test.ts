import * as path from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";

// In-memory filesystem shared by the mock below. Tracks contents + mtimes and
// implements O_EXCL ("wx") semantics so the cross-process lock can be exercised.
let files: Record<string, string | Buffer> = {};
let mtimes: Record<string, number> = {};
let fdPaths: Record<number, string> = {};
let nextFd = 3;
// Paths under this prefix make openSync throw a non-EEXIST error, to exercise
// the lock-free degrade path.
let denyLockDir = "";

const enoent = (): NodeJS.ErrnoException => {
  const err = new Error("ENOENT") as NodeJS.ErrnoException;
  err.code = "ENOENT";
  return err;
};

vi.mock("node:fs", () => ({
  existsSync: (p: string) => p in files,
  readFileSync: (p: string) => {
    if (p in files) return files[p];
    throw enoent();
  },
  writeFileSync: (p: string, content: string | Buffer) => {
    files[p] = content;
    mtimes[p] = Date.now();
  },
  unlinkSync: (p: string) => {
    if (!(p in files)) throw enoent();
    delete files[p];
    delete mtimes[p];
  },
  mkdirSync: () => {},
  chmodSync: () => {},
  openSync: (p: string, flag: string) => {
    if (denyLockDir && p.startsWith(denyLockDir)) {
      const err = new Error("EACCES") as NodeJS.ErrnoException;
      err.code = "EACCES";
      throw err;
    }
    if (flag === "wx" && p in files) {
      const err = new Error("EEXIST") as NodeJS.ErrnoException;
      err.code = "EEXIST";
      throw err;
    }
    files[p] = "";
    mtimes[p] = Date.now();
    const fd = nextFd++;
    fdPaths[fd] = p;
    return fd;
  },
  writeSync: (fd: number, data: string) => {
    files[fdPaths[fd]] = data;
  },
  closeSync: (fd: number) => {
    delete fdPaths[fd];
  },
  statSync: (p: string) => {
    if (!(p in files)) throw enoent();
    return { mtimeMs: mtimes[p] ?? Date.now() };
  },
  renameSync: (from: string, to: string) => {
    if (!(from in files)) throw enoent();
    // Model Windows: a rename cannot overwrite an existing (potentially locked)
    // target. Tests that want the success path leave the target absent.
    if (to in files) {
      const err = new Error("EPERM") as NodeJS.ErrnoException;
      err.code = "EPERM";
      throw err;
    }
    files[to] = files[from];
    mtimes[to] = Date.now();
    delete files[from];
    delete mtimes[from];
  },
  createWriteStream: () => {
    throw new Error("createWriteStream should not be called in these tests");
  },
}));

// getBinaryVersion uses promisify(execFile); the mock resolves with the
// `{ stdout, stderr }` shape promisify hands back via the callback.
vi.mock("node:child_process", () => ({
  execFile: (...args: unknown[]) => {
    const cb = args[args.length - 1] as (
      err: Error | null,
      result?: { stdout: string; stderr: string },
    ) => void;
    cb(null, { stdout: "fallow 2.26.0\n", stderr: "" });
  },
}));

vi.mock("node:crypto", () => ({
  createPublicKey: () => ({ type: "mock-public-key" }),
  createHash: () => {
    let input = "";
    return {
      update(data: string | Buffer) {
        input = Buffer.isBuffer(data) ? data.toString("utf8") : data;
        return this;
      },
      digest() {
        return Buffer.from(input, "utf8").toString("hex").padEnd(64, "0").slice(0, 64);
      },
    };
  },
  verify: () => true,
}));

const httpsGet = vi.fn();

vi.mock("node:https", () => ({
  get: (...args: unknown[]) => httpsGet(...args),
}));

const showInformationMessage = vi.fn();
const showErrorMessage = vi.fn();
const showWarningMessage = vi.fn();

vi.mock("vscode", () => ({
  extensions: {
    getExtension: () => ({ packageJSON: { version: "2.26.0" } }),
  },
  window: {
    withProgress: (_opts: unknown, task: () => Promise<unknown>) => task(),
    showInformationMessage: (...args: unknown[]) => showInformationMessage(...args),
    showErrorMessage: (...args: unknown[]) => showErrorMessage(...args),
    showWarningMessage: (...args: unknown[]) => showWarningMessage(...args),
  },
  commands: { executeCommand: vi.fn() },
  ProgressLocation: { Notification: 15 },
}));

import {
  downloadBinary,
  downloadCliBinary,
  renameIntoPlace,
  tryAcquireInstallLock,
  withInstallLock,
} from "../src/download.js";

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

const binDir = path.join("/storage", "bin");
const lockPath = path.join(binDir, ".install.lock");
const lspPath = path.join(binDir, "fallow-lsp");
const lspSigPath = `${lspPath}.sig`;
const cliPath = path.join(binDir, "fallow");
const cliSigPath = `${cliPath}.sig`;
const versionPath = path.join(binDir, ".fallow-version");

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as never;

const reset = (): void => {
  files = {};
  mtimes = {};
  fdPaths = {};
  nextFd = 3;
  denyLockDir = "";
  httpsGet.mockReset();
  showInformationMessage.mockReset();
  showErrorMessage.mockReset();
  showWarningMessage.mockReset();
};

describe("tryAcquireInstallLock", () => {
  beforeEach(reset);

  it("grants the lock once and denies a concurrent holder", () => {
    expect(tryAcquireInstallLock(lockPath)).toBe(true);
    expect(tryAcquireInstallLock(lockPath)).toBe(false);
  });
});

describe("withInstallLock", () => {
  beforeEach(reset);

  it("runs the body and releases the lock", async () => {
    const result = await withInstallLock(binDir, async () => {
      expect(lockPath in files).toBe(true);
      return "done";
    });

    expect(result).toBe("done");
    expect(lockPath in files).toBe(false);
  });

  it("serializes two concurrent holders", async () => {
    const order: string[] = [];
    let releaseA = (): void => {};
    const aBlocker = new Promise<void>((resolve) => {
      releaseA = resolve;
    });

    const a = withInstallLock(binDir, async () => {
      order.push("a-start");
      await aBlocker;
      order.push("a-end");
      return "a";
    });

    // Let A acquire the lock and reach its await.
    await Promise.resolve();
    const b = withInstallLock(binDir, async () => {
      order.push("b-start");
      return "b";
    });

    // B must still be waiting on the lock A holds.
    await sleep(50);
    expect(order).toEqual(["a-start"]);

    releaseA();
    await expect(Promise.all([a, b])).resolves.toEqual(["a", "b"]);
    expect(order).toEqual(["a-start", "a-end", "b-start"]);
  });

  it("steals a stale lock instead of deadlocking", async () => {
    // A crashed window left an ancient lock behind.
    files[lockPath] = "999";
    mtimes[lockPath] = 0;

    const result = await withInstallLock(binDir, async () => "after-steal");

    expect(result).toBe("after-steal");
    expect(lockPath in files).toBe(false);
  });

  it("degrades to lock-free when the lock cannot be created", async () => {
    denyLockDir = binDir;

    const result = await withInstallLock(binDir, async () => "lock-free");

    expect(result).toBe("lock-free");
    // No lock file was ever created.
    expect(lockPath in files).toBe(false);
  });
});

describe("renameIntoPlace", () => {
  beforeEach(reset);

  it("renames the temp binary onto an absent destination", () => {
    const temp = path.join(binDir, ".fallow.tmp");
    files[temp] = Buffer.from("new-binary");

    renameIntoPlace(temp, cliPath);

    expect(files[cliPath]).toEqual(Buffer.from("new-binary"));
    expect(temp in files).toBe(false);
  });

  it("treats a locked but byte-identical destination as success", () => {
    const temp = path.join(binDir, ".fallow.tmp");
    files[temp] = Buffer.from("same-binary");
    files[cliPath] = Buffer.from("same-binary"); // makes renameSync throw EPERM

    expect(() => renameIntoPlace(temp, cliPath)).not.toThrow();
    expect(files[cliPath]).toEqual(Buffer.from("same-binary"));
    expect(temp in files).toBe(false);
  });

  it("rethrows when the locked destination differs from the temp binary", () => {
    const temp = path.join(binDir, ".fallow.tmp");
    files[temp] = Buffer.from("new-binary");
    files[cliPath] = Buffer.from("old-binary"); // locked AND different

    expect(() => renameIntoPlace(temp, cliPath)).toThrow();
    expect(files[cliPath]).toEqual(Buffer.from("old-binary"));
  });
});

describe("downloadCliBinary double-check skip", () => {
  beforeEach(reset);

  it("reuses a sibling-installed current binary without downloading", async () => {
    // Simulate a sibling window that already installed a valid, current CLI.
    files[cliPath] = Buffer.from("installed");
    files[cliSigPath] = Buffer.alloc(64, 1);
    files[versionPath] = "2.26.0";

    const result = await downloadCliBinary(fakeContext);

    expect(result).toBe(cliPath);
    expect(httpsGet).not.toHaveBeenCalled();
    // The lock is released after the skip.
    expect(lockPath in files).toBe(false);
  });

  it("reuses both sibling-installed binaries without downloading or toasting", async () => {
    files[lspPath] = Buffer.from("installed-lsp");
    files[lspSigPath] = Buffer.alloc(64, 1);
    files[cliPath] = Buffer.from("installed-cli");
    files[cliSigPath] = Buffer.alloc(64, 1);
    files[versionPath] = "2.26.0";

    const result = await downloadBinary(fakeContext);

    expect(result).toBe(lspPath);
    expect(httpsGet).not.toHaveBeenCalled();
    // Nothing was downloaded, so no "installed" toast fires.
    expect(showInformationMessage).not.toHaveBeenCalled();
    expect(showWarningMessage).not.toHaveBeenCalled();
    expect(lockPath in files).toBe(false);
  });
});
