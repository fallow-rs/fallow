import { describe, expect, it } from "vitest";
import { detectLinuxLibc, type LibcProbes } from "../src/libc.js";

const probes = (overrides: Partial<LibcProbes>): LibcProbes => ({
  fileExists: () => false,
  glibcVersionRuntime: () => "2.36",
  ...overrides,
});

describe("detectLinuxLibc", () => {
  it("reports glibc when the diagnostic report names a glibc version", () => {
    expect(detectLinuxLibc(probes({}))).toBe("gnu");
  });

  it("reports musl when the diagnostic report has no glibc version", () => {
    expect(detectLinuxLibc(probes({ glibcVersionRuntime: () => null }))).toBe("musl");
  });

  it("reports musl on Alpine without reading the diagnostic report", () => {
    let reportRead = false;
    const result = detectLinuxLibc(
      probes({
        fileExists: (filePath) => filePath === "/etc/alpine-release",
        glibcVersionRuntime: () => {
          reportRead = true;
          return "2.36";
        },
      }),
    );
    expect(result).toBe("musl");
    expect(reportRead).toBe(false);
  });

  it("keeps glibc when the diagnostic report is not available", () => {
    const result = detectLinuxLibc(
      probes({
        glibcVersionRuntime: () => {
          throw new Error("no report");
        },
      }),
    );
    expect(result).toBe("gnu");
  });

  it("reads the diagnostic report of this Node runtime without an error", () => {
    expect(["gnu", "musl"]).toContain(detectLinuxLibc());
  });
});
