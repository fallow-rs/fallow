import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { createResponse } from "../src/protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = path.join(sidecarRoot, "fallow-type-aware.mjs");

const makeProject = () => mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const request = (root, candidates) => ({
  protocol_version: 1,
  operation: "class-member-uses",
  root,
  candidates,
});

const candidate = ({ id, file, owner, member, kind = "class_method", line = 2 }) => ({
  id,
  path: file,
  parent_name: owner,
  member_name: member,
  kind,
  line,
  col: 2,
});

const runSidecar = (body) => {
  const stdout = execFileSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(body),
    encoding: "utf8",
  });
  return JSON.parse(stdout);
};

test("confirms a nested generic class-member use", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    write(
      root,
      "src/client.ts",
      `export class GenericClient<T> {
  used(value: T): void {}
  dead(value: T): void {}
  usedProperty!: T;
  deadProperty!: T;
}
`,
    );
    write(
      root,
      "src/service.ts",
      `import { GenericClient } from "./client.js";
class BaseService<TClient> {
  constructor(protected readonly client: TClient) {}
}
export class StringService extends BaseService<GenericClient<string>> {
  run(): void {
    this.client.used("ok");
    console.log(this.client.usedProperty);
  }
}
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/client.ts", owner: "GenericClient", member: "used" }),
        candidate({
          id: 1,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "dead",
          line: 3,
        }),
        candidate({
          id: 2,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "usedProperty",
          kind: "class_property",
          line: 4,
        }),
        candidate({
          id: 3,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "deadProperty",
          kind: "class_property",
          line: 5,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0, 2]);
    assert.deepEqual(response.unresolved_candidate_ids, [1, 3]);
    assert.deepEqual(response.selected_tsconfigs, ["tsconfig.json"]);
    assert.equal(response.backend, "typescript-go");
    assert.equal(response.backend_version, "7.0.2");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uses the default project for candidates from multiple configs", () => {
  const root = makeProject();
  try {
    for (const packageName of ["a", "b"]) {
      write(
        root,
        `packages/${packageName}/tsconfig.json`,
        JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
      );
    }
    write(
      root,
      "packages/a/src/alpha.ts",
      `export class Alpha {
  run(): void {}
}
new Alpha().run();
`,
    );
    write(
      root,
      "packages/b/src/beta.ts",
      `export class Beta {
  stop(): void {}
}
new Beta().stop();
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "packages/a/src/alpha.ts", owner: "Alpha", member: "run" }),
        candidate({ id: 1, file: "packages/b/src/beta.ts", owner: "Beta", member: "stop" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0, 1]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.selected_tsconfigs, [
      "packages/a/tsconfig.json",
      "packages/b/tsconfig.json",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("keeps unresolved candidates when source diagnostics exist", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    write(
      root,
      "src/service.ts",
      `export class Service {
  unused(): void {}
}
const broken: string = 1;
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/service.ts", owner: "Service", member: "unused" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, [0]);
    assert.match(response.warnings[0], /TypeScript diagnostics?/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uses an explicit marker for an inferred project", () => {
  const root = makeProject();
  try {
    write(
      root,
      "service.ts",
      `class Service {
  run(): void {}
}
new Service().run();
`,
    );

    const response = runSidecar(
      request(root, [candidate({ id: 0, file: "service.ts", owner: "Service", member: "run" })]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.selected_tsconfigs, ["<inferred>"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("rejects an unsupported protocol version without JSON stdout", () => {
  const result = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify({
      protocol_version: 99,
      operation: "class-member-uses",
      root: sidecarRoot,
      candidates: [],
    }),
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /unsupported protocol_version 99/);
});

test("rejects negative candidate IDs", () => {
  const result = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(
      request(sidecarRoot, [
        candidate({ id: -1, file: "missing.ts", owner: "Missing", member: "run" }),
      ]),
    ),
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /candidates\[0\]\.id must be an integer/);
});

test("rejects unknown request fields and absolute candidate paths", () => {
  const unknown = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify({ ...request(sidecarRoot, []), unexpected: true }),
    encoding: "utf8",
  });
  assert.equal(unknown.status, 2);
  assert.match(unknown.stderr, /request contains unknown field unexpected/);

  const absolute = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(
      request(sidecarRoot, [
        candidate({
          id: 0,
          file: path.join(sidecarRoot, "missing.ts"),
          owner: "Missing",
          member: "run",
        }),
      ]),
    ),
    encoding: "utf8",
  });
  assert.equal(absolute.status, 2);
  assert.match(absolute.stderr, /path must be project-relative/);
});

test("bounds and normalizes warning text", () => {
  const response = createResponse({
    selectedTsconfigs: [],
    confirmedIds: [],
    unresolvedIds: [],
    warnings: [`first\nwarning`, "x".repeat(600)],
    elapsedMs: 1,
  });

  assert.equal(response.warnings[0], "first warning");
  assert.equal(response.warnings[1].length, 512);
});

test("returns provenance for an empty candidate request", () => {
  const response = runSidecar(request(sidecarRoot, []));

  assert.equal(response.protocol_version, 1);
  assert.equal(response.backend, "typescript-go");
  assert.deepEqual(response.selected_tsconfigs, []);
  assert.deepEqual(response.confirmed_used_candidate_ids, []);
  assert.deepEqual(response.unresolved_candidate_ids, []);
});

test("fails when TypeScript cannot construct a project", () => {
  const root = makeProject();
  try {
    const result = spawnSync(executable, {
      cwd: sidecarRoot,
      input: JSON.stringify(
        request(root, [candidate({ id: 0, file: "missing.ts", owner: "Missing", member: "run" })]),
      ),
      encoding: "utf8",
    });

    assert.equal(result.status, 2);
    assert.equal(result.stdout, "");
    assert.match(result.stderr, /could not construct a project/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
