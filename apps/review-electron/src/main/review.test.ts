import { describe, expect, it } from "vitest";
import { dirname } from "node:path";
import { existsSync, readFileSync } from "node:fs";
import { withValidationPayloadFile } from "./review";
import type { AgentWalkthrough } from "../model/agent";

const payload = { graph_snapshot_hash: "hash", judgments: [] } as unknown as AgentWalkthrough;

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
