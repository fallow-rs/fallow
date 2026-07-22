import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { Readable } from "node:stream";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { createResponse } from "../src/protocol.mjs";
import { readAll } from "../src/cli.mjs";
import { canonicalFileIdentity } from "../src/typescript-go.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = path.join(sidecarRoot, "fallow-type-aware.mjs");

const makeProject = () => mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const request = (root, candidates) => ({
  protocol_version: 2,
  operation: "class-member-uses",
  root,
  projects: [],
  candidates,
});

const candidate = ({ id, file, owner, member, kind = "class_method", line = 2, col = 2 }) => ({
  id,
  path: file,
  parent_name: owner,
  member_name: member,
  kind,
  line,
  col,
});

const utf8Position = (source, marker, occurrence = 1) => {
  let markerOffset = -1;
  for (let found = 0; found < occurrence; found += 1) {
    markerOffset = source.indexOf(marker, markerOffset + 1);
    assert.notEqual(markerOffset, -1, `marker ${marker} occurrence ${occurrence} exists`);
  }
  const lineStart = source.lastIndexOf("\n", markerOffset - 1) + 1;
  return {
    line: source.slice(0, markerOffset).split("\n").length,
    col: Buffer.byteLength(source.slice(lineStart, markerOffset), "utf8"),
  };
};

const runSidecar = (body) => {
  const stdout = execFileSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(body),
    encoding: "utf8",
  });
  return JSON.parse(stdout);
};

test("uses physical file identity across alternate spellings and symlinks", () => {
  const root = makeProject();
  try {
    write(root, "src/Service.ts", "export class Service {}\n");
    symlinkSync(path.join("src", "Service.ts"), path.join(root, "service-link.ts"));

    const direct = path.join(root, "src", "Service.ts");
    const alternate = path.join(root, "src", "..", "src", ".", "Service.ts");
    const symlink = path.join(root, "service-link.ts");

    assert.equal(canonicalFileIdentity(alternate), canonicalFileIdentity(direct));
    assert.equal(canonicalFileIdentity(symlink), canonicalFileIdentity(direct));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("keeps case-different physical files distinct when the filesystem supports them", (t) => {
  const root = makeProject();
  try {
    write(root, "src/Service.ts", "export class UpperService {}\n");
    write(root, "src/service.ts", "export class LowerService {}\n");

    const upper = path.join(root, "src", "Service.ts");
    const lower = path.join(root, "src", "service.ts");
    const upperStat = statSync(upper);
    const lowerStat = statSync(lower);
    if (upperStat.dev === lowerStat.dev && upperStat.ino === lowerStat.ino) {
      t.skip("filesystem is case-insensitive");
      return;
    }

    assert.notEqual(canonicalFileIdentity(upper), canonicalFileIdentity(lower));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("keeps unresolved case-different paths distinct", () => {
  const root = makeProject();
  try {
    const upper = path.join(root, "Missing.ts");
    const lower = path.join(root, "missing.ts");

    assert.notEqual(canonicalFileIdentity(upper), canonicalFileIdentity(lower));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("confirms only the used case-different candidate on case-sensitive filesystems", (t) => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    write(
      root,
      "src/Service.ts",
      `export class Service {
  run(): void {}
}
new Service().run();
`,
    );
    write(
      root,
      "src/service.ts",
      `export class Service {
  run(): void {}
}
`,
    );

    const upper = path.join(root, "src", "Service.ts");
    const lower = path.join(root, "src", "service.ts");
    const upperStat = statSync(upper);
    const lowerStat = statSync(lower);
    if (upperStat.dev === lowerStat.dev && upperStat.ino === lowerStat.ino) {
      t.skip("filesystem is case-insensitive");
      return;
    }

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/Service.ts", owner: "Service", member: "run" }),
        candidate({ id: 1, file: "src/service.ts", owner: "Service", member: "run" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, [1]);
    assert.deepEqual(response.abstentions, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

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
    assert.equal(response.sidecar_version, "0.1.0");
    assert.deepEqual(response.abstentions, []);
    assert.ok(response.projects[0].source_file_count >= 2);
    const { source_file_count: _sourceFileCount, ...projectResult } = response.projects[0];
    assert.deepEqual(projectResult, {
      config: "tsconfig.json",
      source: "auto",
      status: "refined",
      candidate_count: 4,
      confirmed_used_count: 2,
      unresolved_count: 2,
      abstained_count: 0,
      blocking_diagnostic_count: 0,
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("matches same-named declarations by their exact source coordinates", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `namespace First {
  export class Service {
    run(): void {}
  }
}
namespace Second {
  export class Service {
    run(): void {}
  }
}
new First.Service().run();
`;
    write(root, "src/services.ts", source);
    const first = utf8Position(source, "run(): void {}", 1);
    const second = utf8Position(source, "run(): void {}", 2);

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/services.ts",
          owner: "Service",
          member: "run",
          ...first,
        }),
        candidate({
          id: 1,
          file: "src/services.ts",
          owner: "Service",
          member: "run",
          ...second,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, [1]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("interprets protocol columns as zero-based UTF-8 byte offsets", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `const label = "🙂"; export class UnicodeService { run(): void {} }
new UnicodeService().run();
`;
    write(root, "src/unicode.ts", source);
    const declaration = utf8Position(source, "run(): void {}");
    const utf16Col = source.slice(0, source.indexOf("run(): void {}")).length;
    assert.notEqual(declaration.col, utf16Col);

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/unicode.ts",
          owner: "UnicodeService",
          member: "run",
          ...declaration,
        }),
        candidate({
          id: 1,
          file: "src/unicode.ts",
          owner: "UnicodeService",
          member: "run",
          line: declaration.line,
          col: utf16Col,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, [1]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("matches getter and setter declarations at their keyword positions", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `export class Accessor {
  get value(): string { return ""; }
  set value(next: string) { console.log(next); }
}
const accessor = new Accessor();
console.log(accessor.value);
accessor.value = "next";
`;
    write(root, "src/accessor.ts", source);
    const getter = utf8Position(source, "get value");
    const setter = utf8Position(source, "set value");

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/accessor.ts",
          owner: "Accessor",
          member: "value",
          ...getter,
        }),
        candidate({
          id: 1,
          file: "src/accessor.ts",
          owner: "Accessor",
          member: "value",
          ...setter,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0, 1]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("distinguishes a nested class declaration from a same-named outer class", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `export class Container {
  make(): void {
    class Service {
      run(): void {}
    }
    new Service().run();
  }
}
class Service {
  run(): void {}
}
`;
    write(root, "src/nested.ts", source);
    const nested = utf8Position(source, "run(): void {}", 1);
    const outer = utf8Position(source, "run(): void {}", 2);

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/nested.ts",
          owner: "Service",
          member: "run",
          ...nested,
        }),
        candidate({
          id: 1,
          file: "src/nested.ts",
          owner: "Service",
          member: "run",
          ...outer,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, [1]);
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

test("uses an explicitly selected ancestor project outside the analysis root", () => {
  const monorepo = makeProject();
  const root = path.join(monorepo, "packages", "app");
  try {
    write(
      monorepo,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["packages/app/src"] }),
    );
    write(
      monorepo,
      "packages/app/src/service.ts",
      `export class Service {
  run(): void {}
}
new Service().run();
`,
    );
    const body = request(root, [
      candidate({ id: 0, file: "src/service.ts", owner: "Service", member: "run" }),
    ]);
    body.projects = ["../../tsconfig.json"];

    const response = runSidecar(body);

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.selected_tsconfigs, ["../../tsconfig.json"]);
  } finally {
    rmSync(monorepo, { recursive: true, force: true });
  }
});

test("does not run a full semantic diagnostic pass before exact refinement", () => {
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
    assert.deepEqual(response.abstentions, []);
    assert.equal(response.projects[0].status, "refined");
    assert.equal(response.projects[0].blocking_diagnostic_count, 0);
    assert.deepEqual(response.warnings, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("abstains for every candidate when a project has blocking diagnostics", () => {
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
  run(): void {}
}
new Service().run();
const broken = ;
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/service.ts", owner: "Service", member: "run" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.abstentions, [{ candidate_id: 0, reason: "blocking-diagnostics" }]);
    assert.equal(response.projects[0].status, "abstained");
    assert.equal(response.projects[0].abstain_reason, "blocking-diagnostics");
    assert.equal(response.projects[0].abstained_count, 1);
    assert.ok(response.projects[0].blocking_diagnostic_count > 0);
    assert.match(response.warnings[0], /blocking TypeScript diagnostics?/);
    assert.match(response.warnings[0], /tsc -p tsconfig\.json --noEmit/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("uses an explicit marker when an inferred project safely abstains", () => {
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

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.abstentions, [{ candidate_id: 0, reason: "blocking-diagnostics" }]);
    assert.deepEqual(response.selected_tsconfigs, ["<inferred>"]);
    assert.match(response.warnings[0], /pass an explicit tsconfig with --type-aware-project/);
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
    selectedTsconfigs: ["\u{10000}.json", "\uE000.json"],
    confirmedIds: [],
    unresolvedIds: [],
    warnings: [`first\nwarning`, "first warning", "\u{10000}", "\uE000", "x".repeat(600)],
    elapsedMs: 1,
  });

  assert.deepEqual(response.selected_tsconfigs, ["\uE000.json", "\u{10000}.json"]);
  assert.equal(response.warnings.length, 4);
  assert.equal(response.warnings[0], "first warning");
  assert.equal(response.warnings[1].length, 512);
  assert.deepEqual(response.warnings.slice(2), ["\uE000", "\u{10000}"]);
});

test("sidecar version matches the package version", () => {
  const packageJson = JSON.parse(readFileSync(path.join(sidecarRoot, "package.json"), "utf8"));
  const response = createResponse({
    selectedTsconfigs: [],
    confirmedIds: [],
    unresolvedIds: [],
    warnings: [],
    elapsedMs: 0,
  });

  assert.equal(response.sidecar_version, packageJson.version);
});

test("rejects oversized stdin while reading", async () => {
  await assert.rejects(readAll(Readable.from(["12345"]), 4), /4 byte request limit/);
});

test("returns provenance for an empty candidate request", () => {
  const response = runSidecar(request(sidecarRoot, []));

  assert.equal(response.protocol_version, 2);
  assert.equal(response.sidecar_version, "0.1.0");
  assert.equal(response.backend, "typescript-go");
  assert.deepEqual(response.selected_tsconfigs, []);
  assert.deepEqual(response.confirmed_used_candidate_ids, []);
  assert.deepEqual(response.unresolved_candidate_ids, []);
  assert.deepEqual(response.abstentions, []);
  assert.deepEqual(response.projects, []);
});

test("abstains when TypeScript cannot construct a project", () => {
  const root = makeProject();
  try {
    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "missing.ts", owner: "Missing", member: "run" }),
        candidate({ id: 1, file: "missing.ts", owner: "Missing", member: "stop" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.abstentions, [
      { candidate_id: 0, reason: "no-project" },
      { candidate_id: 1, reason: "no-project" },
    ]);
    assert.deepEqual(response.projects, []);
    assert.equal(response.warnings.length, 1);
    assert.match(response.warnings[0], /No TypeScript project/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
