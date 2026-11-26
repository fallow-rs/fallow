import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { createApi, withReadScope } from "../src/backend.mjs";
import { createReadScope, isWithin, resolvesInside } from "../src/read-scope.mjs";
import { parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries } from "../src/semantic.mjs";
import { ANALYSIS_OPERATION, WIRE_PROTOCOL_VERSION } from "../src/generated-protocol.mjs";

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
    const scope = createReadScope(root);
    assert.equal(scope.readable(path.join(root, "src", "own.ts")), true);
    assert.equal(scope.readable(path.join(root, "src", "linked.ts")), false);
    assert.equal(scope.readable(path.join(outside, "other.ts")), false);
    assert.equal(scope.readable(path.join(root, "src")), false, "a directory is not a file");
    assert.equal(scope.inScope(path.join(root, "src")), true);
    assert.equal(scope.readable("bundled:///libs/lib.d.ts"), true);
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

test("a Git work tree above the root widens the scope only when fallow names it", () => {
  withLayout(({ base, root }) => {
    rmSync(path.join(root, ".git"), { recursive: true, force: true });
    mkdirSync(path.join(base, ".git"), { recursive: true });
    writeFileSync(path.join(base, "parent.ts"), OUTSIDE_TEXT);
    assert.equal(createReadScope(root, [], "").readable(path.join(base, "parent.ts")), false);
    assert.equal(createReadScope(root, [], base).readable(path.join(base, "parent.ts")), true);
    const elsewhere = path.join(root, "src");
    assert.equal(
      createReadScope(root, [], elsewhere).readable(path.join(base, "parent.ts")),
      false,
      "a scope root below the root is ignored",
    );
  });
});

test("a node_modules link does not open its target", () => {
  withLayout(({ root, outside }) => {
    symlinkSync(outside, path.join(root, "node_modules"));
    const scope = createReadScope(root, [], "");
    assert.equal(scope.readable(path.join(root, "node_modules", "other.ts")), false);
    assert.equal(scope.readable(path.join(outside, "other.ts")), false);
  });
});

test("only the content callbacks of the checker go through the read scope", () => {
  withLayout(({ root }) => {
    const scoped = withReadScope(createReadScope(root, [], ""));
    assert.deepEqual(Object.keys(scoped).toSorted(), ["getAccessibleEntries", "readFile"]);
  });
});

test("the scope check follows links", () => {
  withLayout(({ root, outside }) => {
    const scope = createReadScope(root, [], "");
    assert.equal(scope.inScope(outside), false);
    assert.equal(scope.inScope(path.join(root, "src", "linked.ts")), false);
    assert.equal(scope.inScope(path.join(root, "src", "own.ts")), true);
  });
});

test("a subpackage reads the shared tsconfig and a workspace package of its work tree", () => {
  withLayout(({ base }) => {
    const top = path.join(base, "repo");
    const app = path.join(top, "packages", "app");
    const lib = path.join(top, "packages", "lib");
    mkdirSync(path.join(top, ".git"), { recursive: true });
    mkdirSync(path.join(app, "src"), { recursive: true });
    mkdirSync(lib, { recursive: true });
    mkdirSync(path.join(top, "node_modules", "@scope"), { recursive: true });
    symlinkSync(lib, path.join(top, "node_modules", "@scope", "lib"));
    writeFileSync(
      path.join(top, "tsconfig.base.json"),
      JSON.stringify({
        compilerOptions: { strict: true, moduleResolution: "bundler", module: "esnext" },
      }),
    );
    writeFileSync(
      path.join(app, "tsconfig.json"),
      JSON.stringify({ extends: "../../tsconfig.base.json", include: ["src/**/*.ts"] }),
    );
    writeFileSync(
      path.join(lib, "package.json"),
      JSON.stringify({ name: "@scope/lib", types: "index.d.ts" }),
    );
    writeFileSync(path.join(lib, "index.d.ts"), "export declare const shared: number;\n");
    writeFileSync(
      path.join(app, "src", "main.ts"),
      'import { shared } from "@scope/lib";\nexport const value: number = shared;\n',
    );
    const api = createApi({ cwd: app, scopeRoot: top });
    const config = path.join(app, "tsconfig.json");
    const snapshot = api.updateSnapshot({ openProjects: [config] });
    try {
      const program = snapshot.getProject(config).program;
      assert.equal(program.getCompilerOptions().strict, true, "the shared tsconfig applies");
      const files = program.getSourceFileNames();
      assert.ok(
        files.some((file) => file.endsWith("lib/index.d.ts")),
        `the workspace package loads: ${files.filter((f) => !f.includes("lib.")).join(", ")}`,
      );
    } finally {
      snapshot.dispose();
      api.close();
    }
  });
});

test("a link to an outside directory named node_modules stays out of scope", () => {
  withLayout(({ root, outside }) => {
    const store = path.join(outside, "store", "node_modules", "pkg");
    mkdirSync(store, { recursive: true });
    writeFileSync(path.join(store, "index.d.ts"), OUTSIDE_TEXT);
    mkdirSync(path.join(root, "node_modules"), { recursive: true });
    symlinkSync(store, path.join(root, "node_modules", "pkg"));
    symlinkSync(path.join(store, "index.d.ts"), path.join(root, "node_modules", "pkg.d.ts"));
    const scope = createReadScope(root, [], "");
    assert.equal(scope.readable(path.join(root, "node_modules", "pkg", "index.d.ts")), false);
    assert.equal(scope.readable(path.join(root, "node_modules", "pkg.d.ts")), false);
  });
});

test("a directory that appears after a first miss is resolved again", () => {
  withLayout(({ root, outside }) => {
    const scope = createReadScope(root, [], "");
    const later = path.join(root, "later");
    assert.equal(scope.readable(path.join(later, "other.ts")), true, "a missing file");
    symlinkSync(outside, later);
    assert.equal(scope.readable(path.join(later, "other.ts")), false);
  });
});

test("a reset drops the resolved directories of an earlier snapshot", () => {
  withLayout(({ root, outside }) => {
    const scope = createReadScope(root, [], "");
    const swapped = path.join(root, "swapped");
    mkdirSync(swapped);
    writeFileSync(path.join(swapped, "other.ts"), OUTSIDE_TEXT);
    assert.equal(scope.readable(path.join(swapped, "other.ts")), true);
    rmSync(swapped, { recursive: true, force: true });
    symlinkSync(outside, swapped);
    scope.reset();
    assert.equal(scope.readable(path.join(swapped, "other.ts")), false);
  });
});

test("a project config outside the root drops only that project", () => {
  withLayout(({ root, outside }) => {
    writeFileSync(path.join(outside, "tsconfig.json"), "{}");
    symlinkSync(path.join(outside, "tsconfig.json"), path.join(root, "linked.json"));
    const request = parseRequest({
      protocol_version: WIRE_PROTOCOL_VERSION,
      operation: ANALYSIS_OPERATION,
      root,
      projects: ["tsconfig.json", "linked.json", "../elsewhere/tsconfig.json"],
      evidence_limit: 1,
      queries: [],
    });
    assert.deepEqual(
      request.projects.map((project) => project.path),
      ["tsconfig.json"],
    );
  });
});

test("a path check ignores case only on Windows", () => {
  const dir = path.join(path.sep, "Repo", "App");
  const file = path.join(path.sep, "repo", "app", "src", "a.ts");
  assert.equal(isWithin(dir, file, "win32"), true);
  assert.equal(isWithin(dir, file, "linux"), false);
  assert.equal(isWithin(dir, path.join(path.sep, "repo", "apps", "a.ts"), "win32"), false);
});

test("a node_modules directory above the scope base stays out of scope", () => {
  withLayout(({ base, root }) => {
    const above = path.join(base, "node_modules", "pkg");
    mkdirSync(above, { recursive: true });
    writeFileSync(path.join(above, "index.d.ts"), OUTSIDE_TEXT);
    const scope = createReadScope(root, [], "");
    assert.equal(scope.readable(path.join(above, "index.d.ts")), false);
  });
});

test("a request whose named projects all leave the root opens no project", () => {
  withLayout(({ root, outside }) => {
    writeFileSync(path.join(outside, "tsconfig.json"), "{}");
    writeFileSync(path.join(root, "tsconfig.json"), JSON.stringify({ include: ["src"] }));
    writeFileSync(
      path.join(root, "src", "use.ts"),
      'import { own } from "./own";\nexport const v = own;\n',
    );
    const request = parseRequest({
      protocol_version: WIRE_PROTOCOL_VERSION,
      operation: ANALYSIS_OPERATION,
      root,
      projects: ["../elsewhere/tsconfig.json"],
      evidence_limit: 5,
      queries: [
        {
          id: 0,
          operation: "symbol-use",
          symbol: {
            path: "src/own.ts",
            namespace: "value",
            declaration_kind: "export",
            exported_name: "own",
            local_name: "own",
            line: 1,
            col: 13,
          },
          framework_contracts: [],
        },
      ],
    });
    const result = analyzeSemanticQueries(request);
    assert.deepEqual(result.selectedTsconfigs, []);
    assert.equal(result.results[0].status, "unavailable");
    assert.equal(result.results[0].evidence.length, 0);
  });
});
