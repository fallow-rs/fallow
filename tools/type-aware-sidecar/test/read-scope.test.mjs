import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createApi } from "../src/backend.mjs";
import { createReadScope, resolvesInside } from "../src/read-scope.mjs";

const OUTSIDE_TEXT = 'export const value = "text-of-the-other-file";\n';

const withLayout = (callback) => {
  const base = mkdtempSync(path.join(tmpdir(), "fallow-read-scope-"));
  const root = path.join(base, "project");
  const outside = path.join(base, "elsewhere");
  mkdirSync(path.join(root, ".git"), { recursive: true });
  mkdirSync(path.join(root, "src"), { recursive: true });
  mkdirSync(outside, { recursive: true });
  writeFileSync(path.join(outside, "other.ts"), OUTSIDE_TEXT);
  symlinkSync(path.join(outside, "other.ts"), path.join(root, "src", "linked.ts"));
  writeFileSync(path.join(root, "src", "own.ts"), "export const own = 1;\n");
  try {
    callback({ base, root, outside });
  } finally {
    rmSync(base, { recursive: true, force: true });
  }
};

test("the read scope refuses files outside the work tree, also through links", () => {
  withLayout(({ root, outside }) => {
    const inScope = createReadScope(root);
    assert.equal(inScope(path.join(root, "src", "own.ts")), true);
    assert.equal(inScope(path.join(root, "src", "linked.ts")), false);
    assert.equal(inScope(path.join(outside, "other.ts")), false);
    assert.equal(inScope(path.join(root, "src")), false, "a directory is not a regular file");
    assert.equal(inScope("bundled:///libs/lib.d.ts"), true);
  });
});

test("the checker does not read a linked file outside the root", () => {
  withLayout(({ root }) => {
    writeFileSync(
      path.join(root, "tsconfig.json"),
      JSON.stringify({ files: ["src/own.ts", "src/linked.ts"] }),
    );
    const api = createApi({ cwd: root });
    const config = path.join(root, "tsconfig.json");
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    try {
      const program = snapshot.getProject(config).program;
      const linked = program.getSourceFile(path.join(root, "src", "linked.ts"));
      assert.ok(!linked?.text?.includes("text-of-the-other-file"));
      assert.ok(program.getSourceFile(path.join(root, "src", "own.ts")));
    } finally {
      snapshot.dispose();
      api.close();
    }
  });
});

test("a project path must resolve inside the root", () => {
  withLayout(({ root, outside }) => {
    symlinkSync(outside, path.join(root, "linked-dir"));
    assert.equal(resolvesInside(root, path.join(root, "tsconfig.json")), true);
    assert.equal(resolvesInside(root, path.join(root, "..", "tsconfig.json")), false);
    assert.equal(resolvesInside(root, path.join(root, "linked-dir", "tsconfig.json")), false);
  });
});

test("a Git work tree above the root does not widen the read scope", () => {
  withLayout(({ base, root }) => {
    rmSync(path.join(root, ".git"), { recursive: true, force: true });
    mkdirSync(path.join(base, ".git"), { recursive: true });
    writeFileSync(path.join(base, "parent.ts"), OUTSIDE_TEXT);
    const inScope = createReadScope(root);
    assert.equal(inScope(path.join(base, "parent.ts")), false);
    assert.equal(resolvesInside(root, path.join(base, "tsconfig.json")), false);
  });
});
