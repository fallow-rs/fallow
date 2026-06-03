import { afterEach, describe, expect, it, vi } from "vitest";

const { showWarningMessage } = vi.hoisted(() => ({ showWarningMessage: vi.fn() }));

vi.mock("vscode", () => ({
  window: { showWarningMessage },
}));

import { resetBinarySkewToast, showBinarySkewToastOnce } from "../src/binary-skew.js";

describe("showBinarySkewToastOnce", () => {
  afterEach(() => {
    resetBinarySkewToast();
    showWarningMessage.mockClear();
  });

  it("shows the first skew toast", () => {
    showBinarySkewToastOnce("first");
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
    expect(showWarningMessage).toHaveBeenCalledWith("first");
  });

  it("suppresses later toasts in the same session regardless of source", () => {
    showBinarySkewToastOnce("lsp skew");
    showBinarySkewToastOnce("cli skew");
    expect(showWarningMessage).toHaveBeenCalledTimes(1);
    expect(showWarningMessage).toHaveBeenCalledWith("lsp skew");
  });

  it("shows again after a reset (new session)", () => {
    showBinarySkewToastOnce("before");
    resetBinarySkewToast();
    showBinarySkewToastOnce("after");
    expect(showWarningMessage).toHaveBeenCalledTimes(2);
  });
});
