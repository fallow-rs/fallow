import { describe, it, expect } from "vitest";
import { describeExecError, describeLoadError, firstLine } from "./errors";

describe("describeExecError", () => {
  it("gives actionable copy when the fallow binary is missing", () => {
    const e = Object.assign(new Error("spawn fallow ENOENT"), { code: "ENOENT" });
    const msg = describeExecError(e, "fallow").message;
    expect(msg).toContain("Couldn't find");
    expect(msg).toContain("fallow");
    expect(msg).toContain("FALLOW_BIN");
    expect(msg).not.toContain("ENOENT");
    expect(msg).not.toContain("spawn");
  });

  it("uses the basename and PATH hint for other binaries", () => {
    const e = Object.assign(new Error("spawn x ENOENT"), { code: "ENOENT" });
    expect(describeExecError(e, "/opt/bin/claude").message).toContain('"claude"');
    expect(describeExecError(e, "/opt/bin/claude").message).toContain("PATH");
  });

  it("surfaces the first stderr line on a non-zero exit", () => {
    const e = Object.assign(new Error("Command failed"), {
      code: 1,
      stderr: "error: bad flag\n  more detail\n",
    });
    const msg = describeExecError(e, "fallow").message;
    expect(msg).toContain("error: bad flag");
    expect(msg).not.toContain("more detail");
  });

  it("reports permission denied", () => {
    const e = Object.assign(new Error("spawn EACCES"), { code: "EACCES" });
    expect(describeExecError(e, "fallow").message).toContain("permission denied");
  });
});

describe("describeLoadError", () => {
  it("explains a refused connection", () => {
    const e = new Error("ERR_CONNECTION_REFUSED (-102) loading 'http://localhost:5273/'");
    const msg = describeLoadError(e, "http://localhost:5273").message;
    expect(msg).toContain("Couldn't reach");
    expect(msg).toContain("dev server");
    expect(msg).not.toContain("ERR_CONNECTION_REFUSED");
  });

  it("humanizes an arbitrary ERR_ code", () => {
    const e = new Error("ERR_TIMED_OUT (-7) loading 'http://x'");
    expect(describeLoadError(e, "http://x").message).toContain("timed out");
  });
});

describe("firstLine", () => {
  it("returns the first non-empty trimmed line", () => {
    expect(firstLine("\n  hi there \nsecond")).toBe("hi there");
  });
});
