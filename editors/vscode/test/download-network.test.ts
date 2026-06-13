import { EventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Isolated mocks (own file) so the node:https response faker does not perturb
// download.test.ts / download-lock.test.ts. This file exercises the network
// fetch path: fetchReleaseForExtension -> httpsGet -> withRedirects, and the
// downloadBinary / downloadCliBinary error handling above it.
type FakeResponse = EventEmitter & {
  statusCode?: number;
  headers?: Record<string, string>;
  resume?: () => void;
};

// The body the mock https.get hands back, plus the response shape. A test sets
// `body` for the 200 path or `statusCode` for the non-200 reject path.
const httpsState = vi.hoisted(() => ({
  body: "" as string,
  statusCode: 200 as number,
}));

vi.mock("node:https", () => ({
  get: (_url: string, _opts: unknown, cb: (res: FakeResponse) => void) => {
    const response = Object.assign(new EventEmitter(), {
      statusCode: httpsState.statusCode,
      headers: {} as Record<string, string>,
      resume: vi.fn(),
    });
    // Deliver the response on a microtask so listeners attached synchronously
    // inside withRedirects / httpsGet are registered before data flows.
    queueMicrotask(() => {
      cb(response);
      if (httpsState.statusCode >= 200 && httpsState.statusCode < 300) {
        response.emit("data", Buffer.from(httpsState.body));
        response.emit("end");
      }
    });
    return new EventEmitter();
  },
}));

// Empty fs: no installed binary, so getManagedBinaryPath returns null and the
// download path is always taken. createWriteStream must never run because no
// asset is ever resolved in these tests.
vi.mock("node:fs", () => ({
  existsSync: () => false,
  readFileSync: () => {
    throw new Error("ENOENT");
  },
  writeFileSync: () => {},
  unlinkSync: () => {},
  mkdirSync: () => {},
  chmodSync: () => {},
  openSync: () => 3,
  writeSync: () => {},
  closeSync: () => {},
  statSync: () => {
    throw new Error("ENOENT");
  },
  createWriteStream: () => {
    throw new Error("createWriteStream should not be called in these tests");
  },
}));

vi.mock("node:os", () => ({
  platform: () => "darwin",
  arch: () => "arm64",
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
    // The default undefined return means promptAfterDownloadFailure sees no
    // "Retry" choice, so the loop bails after a single attempt.
    showErrorMessage: (...args: unknown[]) => showErrorMessage(...args),
    showWarningMessage: (...args: unknown[]) => showWarningMessage(...args),
  },
  commands: { executeCommand: vi.fn() },
  ProgressLocation: { Notification: 15 },
}));

import { downloadBinary, downloadCliBinary } from "../src/download.js";

const fakeContext = { globalStorageUri: { fsPath: "/storage" } } as never;

const reset = (): void => {
  httpsState.body = "";
  httpsState.statusCode = 200;
  showInformationMessage.mockReset();
  showErrorMessage.mockReset();
  showWarningMessage.mockReset();
};

const releaseBody = (assets: ReadonlyArray<{ name: string }>): string =>
  JSON.stringify({
    tag_name: "v2.26.0",
    assets: assets.map((a) => ({
      name: a.name,
      browser_download_url: `https://example.test/${a.name}`,
      digest: "sha256:" + "0".repeat(64),
    })),
  });

describe("downloadCliBinary network error paths", () => {
  beforeEach(reset);

  it("returns null and prompts when the release contains no matching CLI asset", async () => {
    // Valid 200 release JSON, but no fallow-darwin-arm64 asset in it.
    httpsState.body = releaseBody([{ name: "fallow-linux-x64-gnu" }]);

    const result = await downloadCliBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("no CLI binary found for");
  });

  it("returns null and prompts on a non-200 release response", async () => {
    // withRedirects rejects with `HTTP <code>` on any 4xx; the rejection
    // propagates through fetchReleaseForExtension to the outer catch.
    httpsState.statusCode = 404;

    const result = await downloadCliBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("failed to download CLI binary");
    expect(showErrorMessage.mock.calls[0][0]).toContain("HTTP 404");
  });

  it("returns null and prompts when the release body is malformed JSON", async () => {
    // A moved endpoint / proxy HTML body: 200 with a non-JSON payload. The
    // JSON.parse SyntaxError propagates to the outer catch.
    httpsState.body = "not json";

    const result = await downloadCliBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("failed to download CLI binary");
  });
});

describe("downloadBinary network error paths", () => {
  beforeEach(reset);

  it("returns null and prompts when the release contains no matching LSP asset", async () => {
    httpsState.body = releaseBody([{ name: "fallow-lsp-linux-x64-gnu" }]);

    const result = await downloadBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("no LSP binary found for");
  });

  it("returns null and prompts on a non-200 release response", async () => {
    httpsState.statusCode = 500;

    const result = await downloadBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("failed to download binaries");
    expect(showErrorMessage.mock.calls[0][0]).toContain("HTTP 500");
  });

  it("returns null and prompts when the release body is malformed JSON", async () => {
    httpsState.body = "<html>proxy error</html>";

    const result = await downloadBinary(fakeContext);

    expect(result).toBeNull();
    expect(showErrorMessage).toHaveBeenCalledTimes(1);
    expect(showErrorMessage.mock.calls[0][0]).toContain("failed to download binaries");
  });
});
