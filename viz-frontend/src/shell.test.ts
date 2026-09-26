/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";
import shell from "./shell.html?raw";
import { ROOT_PLACEHOLDER, ROW_IDS } from "./shell";

describe("static shell", () => {
  it("has the page rows of the hydrated app, in the same order", () => {
    const ids = [...shell.matchAll(/\sid="([^"]+)"/g)].map((match) => match[1]);
    const rows = ids.filter((id) => id !== "crumbs" && id !== "hints");
    expect(rows).toEqual(["app", ...Object.values(ROW_IDS)]);
  });

  it("has one project name slot for the CLI to fill", () => {
    expect(shell.split(ROOT_PLACEHOLDER)).toHaveLength(2);
  });

  it("holds no script", () => {
    expect(shell).not.toMatch(/<script/i);
  });
});
