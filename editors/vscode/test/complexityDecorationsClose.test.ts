import { describe, expect, it, vi } from "vitest";

// A richer vscode mock than the pure-function suite needs: the controller's
// constructor creates a decoration type and an EventEmitter, and the
// stale-document lifecycle reads `window.visibleTextEditors`.
vi.mock("vscode", () => {
  class FakeEventEmitter<T> {
    public readonly event = (): { dispose: () => void } => ({ dispose: () => undefined });
    public fire(_value?: T): void {
      // no-op: tests assert on staleness, not on emitter wiring.
    }
    public dispose(): void {
      // no-op
    }
  }
  return {
    EventEmitter: FakeEventEmitter,
    window: {
      visibleTextEditors: [] as unknown[],
      createTextEditorDecorationType: () => ({ dispose: () => undefined }),
    },
  };
});

import { ComplexityDecorationController } from "../src/complexityDecorations.js";

const makeDocument = (uri: string): { uri: { toString: () => string; scheme: string } } => ({
  uri: { toString: () => uri, scheme: "file" },
});

const makeController = (): ComplexityDecorationController =>
  new ComplexityDecorationController(
    () => true,
    () => false,
    () => "/project",
  );

describe("ComplexityDecorationController staleDocuments eviction", () => {
  it("evicts a closed document so a re-open is no longer stale", () => {
    const controller = makeController();
    const doc = makeDocument("file:///project/src/index.ts") as never;

    controller.handleDocumentChange(doc);
    expect(controller.isStale(doc)).toBe(true);

    controller.handleDocumentClose(doc);
    expect(controller.isStale(doc)).toBe(false);
  });

  it("only evicts the closed document, leaving other stale documents marked", () => {
    const controller = makeController();
    const closed = makeDocument("file:///project/src/a.ts") as never;
    const other = makeDocument("file:///project/src/b.ts") as never;

    controller.handleDocumentChange(closed);
    controller.handleDocumentChange(other);

    controller.handleDocumentClose(closed);
    expect(controller.isStale(closed)).toBe(false);
    expect(controller.isStale(other)).toBe(true);
  });

  it("is a no-op for a document that was never stale", () => {
    const controller = makeController();
    const doc = makeDocument("file:///project/src/clean.ts") as never;

    controller.handleDocumentClose(doc);
    expect(controller.isStale(doc)).toBe(false);
  });
});
