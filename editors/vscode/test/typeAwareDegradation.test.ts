import { afterEach, describe, expect, it, vi } from "vitest";

const { showWarningMessage } = vi.hoisted(() => ({ showWarningMessage: vi.fn() }));

vi.mock("vscode", () => ({
  window: { showWarningMessage },
}));

import {
  noteTypeAwareDegradation,
  resetTypeAwareDegradationNotice,
  typeAwareDegradationWarnings,
} from "../src/typeAwareDegradation.js";

const DEGRADED = "type-aware refinement unavailable for /repo: sidecar timed out";

const combinedEnvelope = (
  executed: boolean,
  warnings: readonly string[],
): Record<string, unknown> => ({
  _meta: { check: { type_aware: { executed, warnings } } },
});

describe("typeAwareDegradationWarnings", () => {
  it("reads the dead-code section warnings of a combined run", () => {
    expect(typeAwareDegradationWarnings(combinedEnvelope(false, [DEGRADED]))).toEqual([DEGRADED]);
  });

  it("reads a single-section envelope", () => {
    const envelope = { _meta: { type_aware: { executed: false, warnings: [DEGRADED] } } };
    expect(typeAwareDegradationWarnings(envelope)).toEqual([DEGRADED]);
  });

  it("collapses the same reason recorded by the refinement and the coupling pass", () => {
    expect(typeAwareDegradationWarnings(combinedEnvelope(false, [DEGRADED, DEGRADED]))).toEqual([
      DEGRADED,
    ]);
  });

  it("stays silent for a pass that executed", () => {
    expect(typeAwareDegradationWarnings(combinedEnvelope(true, [DEGRADED]))).toEqual([]);
  });

  it("stays silent when type-aware was never requested", () => {
    expect(typeAwareDegradationWarnings({ _meta: { check: {} } })).toEqual([]);
    expect(typeAwareDegradationWarnings(null)).toEqual([]);
  });
});

describe("noteTypeAwareDegradation", () => {
  afterEach(() => {
    resetTypeAwareDegradationNotice();
    showWarningMessage.mockClear();
  });

  it("logs the reason and warns that the results are the wider syntactic set", () => {
    const appendLine = vi.fn();

    noteTypeAwareDegradation([DEGRADED], { appendLine } as never);

    expect(appendLine).toHaveBeenCalledWith(`Fallow: ${DEGRADED}`);
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
    expect(showWarningMessage.mock.calls[0]?.[0]).toContain(DEGRADED);
    expect(showWarningMessage.mock.calls[0]?.[0]).toContain("syntactic");
  });

  it("does not re-notify while the same failure repeats on every re-analysis", () => {
    const appendLine = vi.fn();

    noteTypeAwareDegradation([DEGRADED], { appendLine } as never);
    noteTypeAwareDegradation([DEGRADED], { appendLine } as never);

    expect(showWarningMessage).toHaveBeenCalledTimes(1);
    expect(appendLine).toHaveBeenCalledTimes(2);
  });

  it("notifies again for a different reason", () => {
    noteTypeAwareDegradation([DEGRADED]);
    noteTypeAwareDegradation(["type-aware refinement unavailable for /repo: companion missing"]);

    expect(showWarningMessage).toHaveBeenCalledTimes(2);
  });

  it("notifies again after a run recovers and then degrades once more", () => {
    noteTypeAwareDegradation([DEGRADED]);
    noteTypeAwareDegradation([]);
    noteTypeAwareDegradation([DEGRADED]);

    expect(showWarningMessage).toHaveBeenCalledTimes(2);
  });
});
