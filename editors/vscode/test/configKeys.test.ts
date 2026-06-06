import { describe, expect, it } from "vitest";

import {
  REANALYSIS_CONFIG_KEYS,
  RESTART_CONFIG_KEYS,
  affectsAnyConfiguration,
} from "../src/configKeys.js";

describe("config keys", () => {
  it("restarts the LSP when duplication settings change", () => {
    expect(RESTART_CONFIG_KEYS).toContain("fallow.duplication");
    expect(REANALYSIS_CONFIG_KEYS).toContain("fallow.duplication");
  });

  it("restarts the LSP when inline complexity changes", () => {
    expect(RESTART_CONFIG_KEYS).toContain("fallow.health.inlineComplexity");
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
});
