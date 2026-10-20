import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createApi } from "../src/backend.mjs";

const withProject = (callback) => {
  const root = mkdtempSync(path.join(tmpdir(), "fallow-backend-"));
  writeFileSync(path.join(root, "tsconfig.json"), JSON.stringify({ files: ["a.ts"] }));
  writeFileSync(path.join(root, "a.ts"), "export const a = 1;\n");
  const api = createApi({ cwd: root });
  try {
    callback(api, path.join(root, "tsconfig.json"));
  } finally {
    api.close();
    rmSync(root, { recursive: true, force: true });
  }
};

test("a disposed snapshot handle acts as disposed", () => {
  withProject((api, config) => {
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    snapshot.dispose();
    assert.equal(snapshot.isDisposed(), true);
    assert.throws(() => snapshot.getProject(config), /disposed/u);
  });
});

test("a snapshot disposed through Symbol.dispose does not stop later updates", () => {
  withProject((api, config) => {
    const first = api.updateSnapshot({ openProjects: [config] });
    first[Symbol.dispose]();
    const next = api.updateSnapshot({});
    try {
      assert.ok(next.getProject(config));
    } finally {
      next.dispose();
    }
  });
});
