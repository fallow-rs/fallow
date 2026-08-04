import { EventEmitter } from "node:events";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Isolated mocks (own file) so the node:https + write-stream fakes do not
// perturb download.test.ts. Only httpsDownload is exercised here, so the fs
// mock needs just createWriteStream + unlink.
type FakeStream = EventEmitter & {
  statusCode?: number;
  headers?: Record<string, string>;
  complete?: boolean;
  resume?: () => void;
  pipe?: () => void;
  destroy?: (err?: Error) => void;
  close?: () => void;
};

const httpsState = vi.hoisted(() => ({
  response: null as unknown as FakeStream,
  request: null as unknown as EventEmitter,
  // The inactivity-timeout callback withRedirects registers via
  // request.setTimeout, captured so tests can fire a simulated stall.
  onRequestTimeout: null as (() => void) | null,
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
      complete: false,
      resume: vi.fn(),
      pipe: vi.fn(),
      destroy: vi.fn((err?: Error) => {
        response.emit("error", err);
      }),
    });
    httpsState.response = response;
    httpsState.request = Object.assign(new EventEmitter(), {
      setTimeout: vi.fn((_ms: number, cb: () => void) => {
        httpsState.onRequestTimeout = cb;
      }),
      destroy: vi.fn(),
    });
    httpsState.onRequestTimeout = null;
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

  it("rejects and cleans up when the response stalls mid-body", async () => {
    const pending = httpsDownload("https://example.test/bin", "/tmp/stalled");
    // The socket goes idle with an incomplete body; the inactivity timeout
    // destroys the response, which must route through the same cleanup path
    // as a socket error.
    httpsState.onRequestTimeout?.();

    await expect(pending).rejects.toThrow("Download timed out");
    expect((fsState.writeStream.destroy as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
    expect(fsState.unlinked).toContain("/tmp/stalled");
  });

  it("does not destroy a completed response on a late keep-alive timeout", async () => {
    const pending = httpsDownload("https://example.test/bin", "/tmp/done");
    httpsState.response.complete = true;
    fsState.writeStream.emit("finish");
    await expect(pending).resolves.toBeUndefined();

    // A keep-alive socket can fire the inactivity timeout after the download
    // finished; that must not tear down the published file.
    httpsState.onRequestTimeout?.();
    expect((httpsState.response.destroy as ReturnType<typeof vi.fn>)).not.toHaveBeenCalled();
    expect(fsState.unlinked).not.toContain("/tmp/done");
  });
});
