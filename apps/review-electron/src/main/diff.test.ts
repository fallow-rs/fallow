import { afterEach, describe, it, expect } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { getAllDiffs, getFileDiff, sanitizeRef } from "./diff";

describe("sanitizeRef", () => {
  it("accepts well-formed refs unchanged", () => {
    expect(sanitizeRef("HEAD")).toBe("HEAD");
    expect(sanitizeRef("HEAD~2")).toBe("HEAD~2");
    expect(sanitizeRef("HEAD^")).toBe("HEAD^");
    expect(sanitizeRef("main")).toBe("main");
    expect(sanitizeRef("origin/main")).toBe("origin/main");
    expect(sanitizeRef("feature/foo-bar")).toBe("feature/foo-bar");
    expect(sanitizeRef("v1.2.3")).toBe("v1.2.3");
    expect(sanitizeRef("a1b2c3d")).toBe("a1b2c3d");
    expect(sanitizeRef("a1b2c3d4e5f6789012345678901234567890abcd")).toBe(
      "a1b2c3d4e5f6789012345678901234567890abcd",
    );
  });

  it("falls back to HEAD for option-injection attempts (a ref beginning with '-')", () => {
    expect(sanitizeRef("--output=/tmp/x")).toBe("HEAD");
    expect(sanitizeRef("-rf")).toBe("HEAD");
    expect(sanitizeRef("--upload-pack=evil")).toBe("HEAD");
  });

  it("falls back to HEAD for empty input", () => {
    expect(sanitizeRef("")).toBe("HEAD");
  });
});

describe("real Git diffs", () => {
  let root: string | undefined;
  afterEach(() => {
    if (root) rmSync(root, { recursive: true, force: true });
    root = undefined;
  });

  const repository = (): string => {
    root = mkdtempSync(join(tmpdir(), "fallow-diff-test-"));
    execFileSync("git", ["init", "-q"], { cwd: root });
    writeFileSync(join(root, "file.ts"), "export const value = 1;\n");
    execFileSync("git", ["add", "."], { cwd: root });
    execFileSync(
      "git",
      [
        "-c",
        "user.name=Fallow Test",
        "-c",
        "user.email=test@example.com",
        "-c",
        "commit.gpgsign=false",
        "-c",
        `core.hooksPath=${join(root, "disabled-hooks")}`,
        "commit",
        "-q",
        "-m",
        "baseline",
      ],
      { cwd: root },
    );
    return root;
  };

  it("keeps genuine empty diffs separate from modified content", async () => {
    const cwd = repository();
    expect(await getAllDiffs(cwd, "HEAD")).toEqual({ patch: "" });
    expect(await getFileDiff(cwd, "HEAD", "file.ts")).toEqual({ patch: "", binary: false });
    writeFileSync(join(cwd, "file.ts"), "export const value = 2;\n");
    expect((await getAllDiffs(cwd, "HEAD")).patch).toContain("+export const value = 2;");
    expect(await getFileDiff(cwd, "HEAD", "file.ts")).toMatchObject({ binary: false });
    expect((await getFileDiff(cwd, "HEAD", "file.ts")).patch).toContain("+export const value = 2;");
  });

  it("rejects an unavailable base instead of reporting no changes", async () => {
    const cwd = repository();
    await expect(getAllDiffs(cwd, "nonexistent-review-base")).rejects.toThrow();
    await expect(getFileDiff(cwd, "nonexistent-review-base", "file.ts")).rejects.toThrow();
  });

  it("rejects a disappeared checkout instead of reporting no changes", async () => {
    const cwd = repository();
    rmSync(cwd, { recursive: true, force: true });
    await expect(getAllDiffs(cwd, "HEAD")).rejects.toThrow();
    await expect(getFileDiff(cwd, "HEAD", "file.ts")).rejects.toThrow();
  });
});
