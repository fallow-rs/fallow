import { describe, expect, it } from "vitest";

import {
  DIAGNOSTIC_RENDER_CONFIG_KEYS,
  REANALYSIS_CONFIG_KEYS,
  RESTART_CONFIG_KEYS,
  affectsAnyConfiguration,
} from "../src/configKeys.js";

describe("config keys", () => {
  it("restarts the LSP when duplication settings change", () => {
    expect(RESTART_CONFIG_KEYS).toContain("fallow.duplication");
    expect(REANALYSIS_CONFIG_KEYS).toContain("fallow.duplication");
  });

  it("never restarts the LSP for inline complexity (extension renders the lens)", () => {
    // The lens is rendered by the extension's ComplexityLensProvider and is no
    // longer an LSP init option, so toggling it must not restart the server or
    // re-run the sidebar; extension.ts refreshes the lens live instead.
    expect(RESTART_CONFIG_KEYS).not.toContain("fallow.health.inlineComplexity");
    expect(REANALYSIS_CONFIG_KEYS).not.toContain("fallow.health.inlineComplexity");
  });

  it("matches configuration changes by exact key list", () => {
    const event = {
      affectsConfiguration: (key: string): boolean => key === "fallow.duplication",
    };

    expect(affectsAnyConfiguration(event, RESTART_CONFIG_KEYS)).toBe(true);
    expect(affectsAnyConfiguration(event, ["fallow.production"])).toBe(false);
  });

  it("re-analyzes (but never restarts the LSP) on a workspace-scope change", () => {
    // A pinned `fallow.workspace` change must re-run the dead-code/dupes sidebar
    // + status bar, but the LSP is not workspace-scoped so it must not restart.
    expect(REANALYSIS_CONFIG_KEYS).toContain("fallow.workspace");
    expect(RESTART_CONFIG_KEYS).not.toContain("fallow.workspace");
  });

  it("refreshes diagnostics rendering without restarting or re-analyzing", () => {
    expect(DIAGNOSTIC_RENDER_CONFIG_KEYS).toContain("fallow.diagnostics.severity");
    expect(RESTART_CONFIG_KEYS).not.toContain("fallow.diagnostics.severity");
    expect(REANALYSIS_CONFIG_KEYS).not.toContain("fallow.diagnostics.severity");
  });
});
