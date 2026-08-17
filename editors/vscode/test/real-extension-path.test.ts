import { mkdirSync, mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { resolveExtensionDevelopmentPath } from "./integration/real/extensionPath.js";

const temporaryRoots: string[] = [];

afterEach(() => {
  for (const root of temporaryRoots.splice(0)) {
    rmSync(root, { force: true, recursive: true });
  }
});

const temporaryRoot = (): string => {
  const root = mkdtempSync(join(realpathSync(tmpdir()), "fallow-extension-path-"));
  temporaryRoots.push(root);
  return root;
};

describe("real extension host path", () => {
  it("canonicalizes the configured packaged extension path", () => {
    const root = temporaryRoot();
    const extensionPath = join(root, "extension");
    mkdirSync(extensionPath);

    expect(
      resolveExtensionDevelopmentPath(
        join(root, "repository"),
        join(root, "nested", "..", "extension"),
      ),
    ).toBe(realpathSync(extensionPath));
  });

  it("fails instead of falling back when the configured path is missing", () => {
    const root = temporaryRoot();
    const repositoryPath = join(root, "repository");
    mkdirSync(repositoryPath);

    expect(() =>
      resolveExtensionDevelopmentPath(repositoryPath, join(root, "missing-extension")),
    ).toThrow();
  });
});
