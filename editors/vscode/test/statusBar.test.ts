import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("vscode", () => ({
  StatusBarAlignment: { Left: 1 },
  MarkdownString: class {
    public isTrusted = false;
    public supportThemeIcons = false;
    public constructor(public readonly value: string) {}
  },
  window: {
    createStatusBarItem: () => ({ text: "", tooltip: undefined, show: vi.fn(), dispose: vi.fn() }),
  },
}));

vi.mock("../src/config.js", () => ({
  getChangedSince: () => "",
  getHealthStatusBar: () => false,
}));

vi.mock("../src/health-utils.js", () => ({ formatHealthStatusPart: () => null }));

import {
  createStatusBar,
  disposeStatusBar,
  setStatusBarAnalyzing,
  setStatusBarError,
  updateStatusBarFromLsp,
  updateStatusBarHealth,
} from "../src/statusBar.js";
import { buildParamsFromCli } from "../src/statusBar-utils.js";

afterEach(disposeStatusBar);

describe("analysis status bar", () => {
  it("keeps pending and error states when health updates after a package-scoped result", () => {
    const item = createStatusBar();
    updateStatusBarFromLsp(
      buildParamsFromCli(null, null, {
        package_baselines: [{ workspace_root: "packages/web", reference: "main" }],
      }),
    );
    expect(item.text).toContain("package baselines");

    setStatusBarAnalyzing();
    updateStatusBarHealth(null);
    expect(item.text).toBe("$(loading~spin) Fallow: Analyzing...");

    setStatusBarError();
    updateStatusBarHealth(null);
    expect(item.text).toBe("$(error) Fallow: Error");
    expect(item.tooltip).toBeUndefined();
  });
});
