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

  it("matches configuration changes by exact key list", () => {
    const event = {
      affectsConfiguration: (key: string): boolean => key === "fallow.duplication",
    };

    expect(affectsAnyConfiguration(event, RESTART_CONFIG_KEYS)).toBe(true);
    expect(affectsAnyConfiguration(event, ["fallow.production"])).toBe(false);
  });
});
