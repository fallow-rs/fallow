import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import {
  configureBundledTypeAwareCompanion,
  typeAwareCommand,
  typeAwareEnvironment,
} from "../src/typeAwareCompanion.js";

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

const writeJson = (file: string, value: unknown): void => {
  writeFileSync(file, JSON.stringify(value));
};

interface Fixture {
  readonly root: string;
  readonly nodeModules: string;
  readonly nativeBinary: string;
  readonly projectCompanion: string;
}

const createProject = (
  base: string,
  fallowVersion: string,
  companionVersion: string | null,
): Fixture => {
  const root = join(base, "project");
  const nodeModules = join(root, "node_modules");
  const platformDir = join(nodeModules, "@fallow-cli", "linux-x64-gnu");
  mkdirSync(platformDir, { recursive: true });
  const nativeBinary = join(platformDir, "fallow");
  writeFileSync(nativeBinary, "");
  writeJson(join(platformDir, "package.json"), { version: fallowVersion });

  const wrapperDir = join(nodeModules, "fallow");
  mkdirSync(wrapperDir, { recursive: true });
  writeJson(join(wrapperDir, "package.json"), { version: fallowVersion });

  const companionDir = join(nodeModules, "fallow-type-aware");
  const projectCompanion = join(companionDir, "fallow-type-aware.mjs");
  if (companionVersion !== null) {
    mkdirSync(companionDir, { recursive: true });
    writeJson(join(companionDir, "package.json"), { version: companionVersion });
    writeFileSync(projectCompanion, "");
  }
  return { root, nodeModules, nativeBinary, projectCompanion };
};

describe("typeAwareEnvironment", () => {
  let base: string;
  const bundled = "/extension/dist/type-aware/fallow-type-aware.mjs";
  const unix = { bundled, env: {}, platform: "linux" as const, execPath: "/usr/bin/node" };

  beforeEach(() => {
    base = mkdtempSync(join(tmpdir(), "fallow-type-aware-"));
  });

  afterEach(() => {
    rmSync(base, { recursive: true, force: true });
  });

  it("uses the project companion when its version equals the local fallow version", () => {
    const project = createProject(base, "3.38.1", "3.38.1");

    expect(typeAwareEnvironment(project.nativeBinary, project.root, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: project.projectCompanion,
      FALLOW_TYPE_AWARE_SCRIPT: "",
      FALLOW_TYPE_AWARE_BIN_SOURCE: "npm-wrapper",
    });
  });

  it("runs the project companion through the extension host executable on Windows", () => {
    const project = createProject(base, "3.38.1", "3.38.1");

    expect(
      typeAwareEnvironment(project.nativeBinary, project.root, {
        bundled,
        env: {},
        platform: "win32",
        execPath: "C:\\Code.exe",
      }),
    ).toEqual({
      FALLOW_TYPE_AWARE_BIN: "C:\\Code.exe",
      FALLOW_TYPE_AWARE_SCRIPT: project.projectCompanion,
      FALLOW_TYPE_AWARE_BIN_SOURCE: "npm-wrapper",
    });
  });

  it("reads the wrapper version when the local binary is the .bin launcher", () => {
    const project = createProject(base, "3.38.1", "3.38.1");
    const binDir = join(project.nodeModules, ".bin");
    mkdirSync(binDir, { recursive: true });
    const shim = join(binDir, "fallow-lsp");
    writeFileSync(shim, "");

    expect(typeAwareEnvironment(shim, project.root, unix).FALLOW_TYPE_AWARE_BIN).toBe(
      project.projectCompanion,
    );
  });

  it("finds the companion next to the wrapper in a pnpm layout", () => {
    const root = join(base, "project");
    const nodeModules = join(root, "node_modules");
    const store = join(nodeModules, ".pnpm", "fallow@3.38.1", "node_modules");
    mkdirSync(join(store, "fallow"), { recursive: true });
    writeJson(join(store, "fallow", "package.json"), { version: "3.38.1" });
    mkdirSync(join(store, "fallow-type-aware"), { recursive: true });
    writeJson(join(store, "fallow-type-aware", "package.json"), { version: "3.38.1" });
    writeFileSync(join(store, "fallow-type-aware", "fallow-type-aware.mjs"), "");
    symlinkSync(join(store, "fallow"), join(nodeModules, "fallow"), "junction");
    mkdirSync(join(nodeModules, ".bin"), { recursive: true });
    const shim = join(nodeModules, ".bin", "fallow-lsp");
    writeFileSync(shim, "");

    expect(typeAwareEnvironment(shim, root, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: realpathSync(join(store, "fallow-type-aware", "fallow-type-aware.mjs")),
      FALLOW_TYPE_AWARE_SCRIPT: "",
      FALLOW_TYPE_AWARE_BIN_SOURCE: "npm-wrapper",
    });
  });

  it("keeps the bundled companion when the project companion version differs", () => {
    const project = createProject(base, "3.38.1", "3.38.0");

    expect(typeAwareEnvironment(project.nativeBinary, project.root, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: bundled,
      FALLOW_TYPE_AWARE_SCRIPT: "",
    });
  });

  it("keeps the bundled companion when the project has no companion", () => {
    const project = createProject(base, "3.38.1", null);

    expect(typeAwareEnvironment(project.nativeBinary, project.root, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: bundled,
      FALLOW_TYPE_AWARE_SCRIPT: "",
    });
  });

  it("keeps the bundled companion when the binary is not in the project node_modules", () => {
    const project = createProject(base, "3.38.1", "3.38.1");
    const managed = join(base, "storage", "fallow");

    expect(typeAwareEnvironment(managed, project.root, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: bundled,
      FALLOW_TYPE_AWARE_SCRIPT: "",
    });
    expect(typeAwareEnvironment(project.nativeBinary, null, unix)).toEqual({
      FALLOW_TYPE_AWARE_BIN: bundled,
      FALLOW_TYPE_AWARE_SCRIPT: "",
    });
  });

  it("keeps a user-set FALLOW_TYPE_AWARE_BIN", () => {
    const project = createProject(base, "3.38.1", "3.38.1");

    expect(
      typeAwareEnvironment(project.nativeBinary, project.root, {
        ...unix,
        env: { FALLOW_TYPE_AWARE_BIN: "/custom/fallow-type-aware" },
      }),
    ).toEqual({});
  });

  it("sets nothing when no companion is available", () => {
    const project = createProject(base, "3.38.1", null);

    expect(
      typeAwareEnvironment(project.nativeBinary, project.root, { ...unix, bundled: null }),
    ).toEqual({});
  });
});

describe("configureBundledTypeAwareCompanion", () => {
  let extensionPath: string;
  const saved = process.env.FALLOW_TYPE_AWARE_BIN;

  beforeEach(() => {
    extensionPath = mkdtempSync(join(tmpdir(), "fallow-extension-"));
    const dir = join(extensionPath, "dist", "type-aware");
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "fallow-type-aware.mjs"), "");
    delete process.env.FALLOW_TYPE_AWARE_BIN;
  });

  afterEach(() => {
    rmSync(extensionPath, { recursive: true, force: true });
    if (saved === undefined) {
      delete process.env.FALLOW_TYPE_AWARE_BIN;
    } else {
      process.env.FALLOW_TYPE_AWARE_BIN = saved;
    }
  });

  it("does not write the bundled companion into the extension host environment", () => {
    configureBundledTypeAwareCompanion(extensionPath);

    expect(process.env.FALLOW_TYPE_AWARE_BIN).toBeUndefined();
  });
});
