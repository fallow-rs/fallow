import { EventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Isolated mocks (own file) so the node:https + write-stream fakes do not
// perturb download.test.ts. Only httpsDownload is exercised here, so the fs
// mock needs just createWriteStream + unlink.
type FakeStream = EventEmitter & {
  statusCode?: number;
  headers?: Record<string, string>;
  resume?: () => void;
  pipe?: () => void;
  destroy?: () => void;
  close?: () => void;
};

const httpsState = vi.hoisted(() => ({
  response: null as unknown as FakeStream,
  request: null as unknown as EventEmitter,
}));
const fsState = vi.hoisted(() => ({
  writeStream: null as unknown as FakeStream,
  unlinked: [] as string[],
}));

vi.mock("node:https", () => ({
  get: (_url: string, _opts: unknown, cb: (res: unknown) => void) => {
    cb(httpsState.response);
    return httpsState.request;
  },
}));

vi.mock("node:fs", () => ({
  createWriteStream: () => fsState.writeStream,
  unlink: (p: string, done: () => void) => {
    fsState.unlinked.push(p);
    done();
  },
}));

vi.mock("vscode", () => ({}));

import { httpsDownload } from "../src/download.js";

describe("httpsDownload stream-error handling", () => {
  beforeEach(() => {
    const response = Object.assign(new EventEmitter(), {
      statusCode: 200,
      headers: {} as Record<string, string>,
      resume: vi.fn(),
      pipe: vi.fn(),
    });
    httpsState.response = response;
    httpsState.request = new EventEmitter();
    const ws = Object.assign(new EventEmitter(), {
      destroy: vi.fn(),
      close: vi.fn(),
    });
    fsState.writeStream = ws;
    fsState.unlinked = [];
  });

  it("rejects, destroys the write stream, and unlinks the partial on a response error", async () => {
    const pending = httpsDownload("https://example.test/bin", "/tmp/partial");
    // The response (readable) errors mid-download. pipe() does not forward this
    // to the write stream, so without the guard it would be an unhandled crash.
    const err = new Error("socket hang up");
    httpsState.response.emit("error", err);

    await expect(pending).rejects.toThrow("socket hang up");
    expect((fsState.writeStream.destroy as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
    expect(fsState.unlinked).toContain("/tmp/partial");
  });

  it("still resolves on the normal finish path", async () => {
    const pending = httpsDownload("https://example.test/bin", "/tmp/ok");
    fsState.writeStream.emit("finish");
    await expect(pending).resolves.toBeUndefined();
    expect((fsState.writeStream.close as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
    expect(fsState.unlinked).not.toContain("/tmp/ok");
  });
});
