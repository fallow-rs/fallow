import { afterEach, describe, expect, it } from "vitest";
import { dirname, join } from "node:path";
import { existsSync, readFileSync } from "node:fs";
import {
  currentFallowBin,
  resolveFallowBin,
  setConfiguredFallowBin,
  withValidationPayloadFile,
  type FallowBinaryEnvironment,
} from "./review";
import type { AgentWalkthrough } from "../model/agent";

const payload = { graph_snapshot_hash: "hash", judgments: [] } as unknown as AgentWalkthrough;

const environment = (
  ambient: string | undefined,
  existing: string[] = [],
): FallowBinaryEnvironment => ({
  ambient,
  cwd: "/repo/apps/review-electron",
  exists: (path) => existing.includes(path),
});

afterEach(() => setConfiguredFallowBin(null));

describe("fallow binary selection", () => {
  it("applies initial configuration ahead of the ambient environment", () => {
    setConfiguredFallowBin("/configured/initial");

    expect(currentFallowBin(environment("/ambient/fallow"))).toBe("/configured/initial");
  });

  it("uses a reloaded replacement for the next selection", () => {
    setConfiguredFallowBin("/configured/initial");
    expect(currentFallowBin(environment("/ambient/fallow"))).toBe("/configured/initial");

    setConfiguredFallowBin("/configured/reloaded");

    expect(currentFallowBin(environment("/ambient/fallow"))).toBe("/configured/reloaded");
  });

  it("restores the independent ambient fallback when configuration is removed", () => {
    setConfiguredFallowBin("/configured/initial");
    setConfiguredFallowBin(null);

    expect(currentFallowBin(environment("/ambient/fallow"))).toBe("/ambient/fallow");
  });

  it("falls back through release build, debug build, then PATH", () => {
    const release = join("/repo", "target", "release", "fallow");
    const debug = join("/repo", "target", "debug", "fallow");

    expect(resolveFallowBin(null, environment(undefined, [release, debug]))).toBe(release);
    expect(resolveFallowBin(null, environment(undefined, [debug]))).toBe(debug);
    expect(resolveFallowBin(null, environment(undefined))).toBe("fallow");
  });

  it("does not switch an invocation that already selected its binary", () => {
    setConfiguredFallowBin("/configured/initial");
    const selectedForInFlightRun = currentFallowBin(environment("/ambient/fallow"));

    setConfiguredFallowBin("/configured/reloaded");

    expect(selectedForInFlightRun).toBe("/configured/initial");
    expect(currentFallowBin(environment("/ambient/fallow"))).toBe("/configured/reloaded");
  });

  it("never persists configured state into the ambient environment", () => {
    const before = process.env["FALLOW_BIN"];
    process.env["FALLOW_BIN"] = "/ambient/fallow";
    try {
      setConfiguredFallowBin("/configured/initial");
      expect(process.env["FALLOW_BIN"]).toBe("/ambient/fallow");
    } finally {
      if (before === undefined) delete process.env["FALLOW_BIN"];
      else process.env["FALLOW_BIN"] = before;
    }
  });
});

describe("withValidationPayloadFile", () => {
  it("removes the temporary payload after success", async () => {
    let file = "";
    const result = await withValidationPayloadFile(payload, async (path) => {
      file = path;
      expect(JSON.parse(readFileSync(path, "utf8"))).toEqual(payload);
      return "validated";
    });

    expect(result).toBe("validated");
    expect(existsSync(file)).toBe(false);
    expect(existsSync(dirname(file))).toBe(false);
  });

  it("removes the temporary payload after child failure", async () => {
    let file = "";
    await expect(
      withValidationPayloadFile(payload, (path) => {
        file = path;
        throw new Error("child failed");
      }),
    ).rejects.toThrow("child failed");

    expect(existsSync(file)).toBe(false);
    expect(existsSync(dirname(file))).toBe(false);
  });

  it("removes the temporary payload after parse failure", async () => {
    let file = "";
    await expect(
      withValidationPayloadFile(payload, (path) => {
        file = path;
        return JSON.parse("{");
      }),
    ).rejects.toThrow();

    expect(existsSync(file)).toBe(false);
    expect(existsSync(dirname(file))).toBe(false);
  });
});
