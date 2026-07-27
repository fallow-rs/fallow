import { describe, expect, it } from "vitest";

import { typeAwareCommand } from "../src/typeAwareCompanion.js";

describe("typeAwareCommand", () => {
  it("runs the bundled module through the VS Code Electron executable on Windows", () => {
    expect(
      typeAwareCommand("C:\\extension\\fallow-type-aware.mjs", "win32", "C:\\Code.exe"),
    ).toEqual({
      binary: "C:\\Code.exe",
      script: "C:\\extension\\fallow-type-aware.mjs",
    });
  });

  it("runs the bundled executable directly on Unix", () => {
    expect(typeAwareCommand("/extension/fallow-type-aware.mjs", "linux", "/usr/bin/node")).toEqual({
      binary: "/extension/fallow-type-aware.mjs",
      script: null,
    });
  });
});
