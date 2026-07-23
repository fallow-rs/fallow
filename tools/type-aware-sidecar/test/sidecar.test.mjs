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

import { createResponse, createSemanticResponse, parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries } from "../src/semantic.mjs";
import { readAll } from "../src/cli.mjs";
import { canonicalFileIdentity } from "../src/typescript-go.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const makeProject = () => mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));

test("status reports protocol and backend without a project request", () => {
  const result = spawnSync(executable, ["--status"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    package_version: "3.8.0",
    protocol_version: 3,
    backend_family: "typescript-go",
    backend_version: "7.0.2",
  });
});

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

const semanticRequest = (root, queries, options = {}) => ({
  protocol_version: 3,
  operation: "semantic-queries",
  root,
  projects: options.projects ?? ["tsconfig.json"],
  evidence_limit: options.evidenceLimit ?? 40,
  queries,
});

const symbolIdentity = ({
  source,
  marker,
  file,
  namespace,
  declarationKind,
  exportedName,
  localName = exportedName,
  owner,
}) => ({
  path: file,
  namespace,
  declaration_kind: declarationKind,
  exported_name: exportedName,
  local_name: localName,
  ...utf8Position(source, marker),
  ...(owner === undefined ? {} : { owner }),
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
      "Pkg/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["Service.ts"] }),
    );
    write(
      root,
      "pkg/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["service.ts"] }),
    );
    write(
      root,
      "Pkg/Service.ts",
      `export class Service {
  run(): void {}
}
new Service().run();
`,
    );
    write(
      root,
      "pkg/service.ts",
      `export class Service {
  run(): void {}
}
`,
    );

    const upper = path.join(root, "Pkg", "Service.ts");
    const lower = path.join(root, "pkg", "service.ts");
    const upperStat = statSync(upper);
    const lowerStat = statSync(lower);
    if (upperStat.dev === lowerStat.dev && upperStat.ino === lowerStat.ino) {
      t.skip("filesystem is case-insensitive");
      return;
    }

    const body = request(root, [
      candidate({ id: 0, file: "Pkg/Service.ts", owner: "Service", member: "run" }),
      candidate({ id: 1, file: "pkg/service.ts", owner: "Service", member: "run" }),
    ]);
    body.projects = ["Pkg/tsconfig.json", "pkg/tsconfig.json"];
    const response = runSidecar(body);

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
    assert.equal(response.sidecar_version, "3.8.0");
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

test("treats getter and setter declarations as one logical property", () => {
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

test("confirms a generic class member through string-literal element access", () => {
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
      `export class Client {
  execute(): void {}
}
export const call = <T extends Client>(client: T): void => client["execute"]();
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/client.ts", owner: "Client", member: "execute" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("confirms a generic class member through mapped type and indexed access", () => {
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
      `export class Client {
  execute(): void {}
}
type ClientView<T> = { [K in keyof T]: T[K] };
const view: ClientView<Client> = new Client();
view["execute"]();
`,
    );

    const response = runSidecar(
      request(root, [
        candidate({ id: 0, file: "src/client.ts", owner: "Client", member: "execute" }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("confirms a class member in allowJs and checkJs projects", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { allowJs: true, checkJs: true, noEmit: true },
        include: ["src"],
      }),
    );
    const source = `export class JavaScriptClient {
  execute() {}
}
/** @template {JavaScriptClient} T @param {T} client */
export const call = (client) => client.execute();
`;
    write(root, "src/client.js", source);
    const declaration = utf8Position(source, "execute() {}");

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/client.js",
          owner: "JavaScriptClient",
          member: "execute",
          ...declaration,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retains decorator and string-registered members without an exact source use", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { experimentalDecorators: true, strict: true },
        include: ["src"],
      }),
    );
    const source = `declare const Injectable: MethodDecorator;
export class RegisteredService {
  @Injectable
  execute(): void {}
}
export const registry = new Map([["execute", RegisteredService]]);
`;
    write(root, "src/service.ts", source);
    const declaration = utf8Position(source, "execute(): void {}");

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/service.ts",
          owner: "RegisteredService",
          member: "execute",
          ...declaration,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, [0]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retains Angular template-only members without claiming a checker use", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { experimentalDecorators: true, strict: true },
        include: ["src"],
      }),
    );
    const source = `declare const Component: ClassDecorator;
@Component
export class GreetingComponent {
  label = "hello";
}
export const template = "{{ label }}";
`;
    write(root, "src/component.ts", source);
    const declaration = utf8Position(source, 'label = "hello"');

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/component.ts",
          owner: "GreetingComponent",
          member: "label",
          kind: "class_property",
          ...declaration,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, [0]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retains Vue template-only members without claiming a checker use", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `export class ViewModel {
  title = "hello";
}
`;
    write(root, "src/model.ts", source);
    write(root, "src/component.vue", "<template>{{ model.title }}</template>\n");
    const declaration = utf8Position(source, 'title = "hello"');

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/model.ts",
          owner: "ViewModel",
          member: "title",
          kind: "class_property",
          ...declaration,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, [0]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("retains Astro template-only members without claiming a checker use", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src"] }),
    );
    const source = `export class ViewModel {
  title = "hello";
}
`;
    write(root, "src/model.ts", source);
    write(root, "src/component.astro", "<h1>{model.title}</h1>\n");
    const declaration = utf8Position(source, 'title = "hello"');

    const response = runSidecar(
      request(root, [
        candidate({
          id: 0,
          file: "src/model.ts",
          owner: "ViewModel",
          member: "title",
          kind: "class_property",
          ...declaration,
        }),
      ]),
    );

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, [0]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("finds a class member used only from an explicitly opened consumer project", () => {
  const root = makeProject();
  try {
    write(
      root,
      "packages/lib/tsconfig.json",
      JSON.stringify({
        compilerOptions: { composite: true, strict: true },
        include: ["src"],
      }),
    );
    write(
      root,
      "packages/lib/src/client.ts",
      `export class Client {
  execute(): void {}
}
`,
    );
    write(
      root,
      "packages/app/tsconfig.json",
      JSON.stringify({
        compilerOptions: { strict: true },
        include: ["src"],
        references: [{ path: "../lib" }],
      }),
    );
    write(
      root,
      "packages/app/src/main.ts",
      `import { Client } from "../../lib/src/client.js";
new Client().execute();
`,
    );

    const body = request(root, [
      candidate({
        id: 0,
        file: "packages/lib/src/client.ts",
        owner: "Client",
        member: "execute",
      }),
    ]);
    body.projects = ["packages/lib/tsconfig.json", "packages/app/tsconfig.json"];
    const response = runSidecar(body);

    assert.deepEqual(response.confirmed_used_candidate_ids, [0]);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.abstentions, []);
    assert.deepEqual(response.selected_tsconfigs, [
      "packages/app/tsconfig.json",
      "packages/lib/tsconfig.json",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("safely abstains when an explicit solution project has no source files", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ files: [], references: [{ path: "packages/lib" }] }),
    );
    write(
      root,
      "packages/lib/tsconfig.json",
      JSON.stringify({ compilerOptions: { composite: true, strict: true }, include: ["src"] }),
    );
    write(
      root,
      "packages/lib/src/client.ts",
      `export class Client {
  execute(): void {}
}
`,
    );

    const body = request(root, [
      candidate({
        id: 0,
        file: "packages/lib/src/client.ts",
        owner: "Client",
        member: "execute",
      }),
    ]);
    body.projects = ["tsconfig.json"];
    const response = runSidecar(body);

    assert.deepEqual(response.confirmed_used_candidate_ids, []);
    assert.deepEqual(response.unresolved_candidate_ids, []);
    assert.deepEqual(response.abstentions, [{ candidate_id: 0, reason: "no-project" }]);
    assert.deepEqual(response.selected_tsconfigs, []);
    assert.deepEqual(response.projects, []);
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

test("protocol v3 batches exact export and type-use queries through one Program", () => {
  const root = makeProject();
  try {
    const apiSource = [
      "export interface User { id: string }",
      "export const make = (user: User): User => user;",
      "",
    ].join("\n");
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/api.ts", apiSource);
    write(
      root,
      "src/consumer.ts",
      [
        'import { make, type User } from "./api";',
        'const user: User = { id: "1" };',
        "export const result = make(user);",
        "",
      ].join("\n"),
    );

    const user = symbolIdentity({
      source: apiSource,
      marker: "User",
      file: "src/api.ts",
      namespace: "type",
      declarationKind: "interface",
      exportedName: "User",
    });
    const make = symbolIdentity({
      source: apiSource,
      marker: "make",
      file: "src/api.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "make",
    });
    const response = runSidecar(
      semanticRequest(root, [
        { id: 0, operation: "symbol-use", symbol: user },
        { id: 1, operation: "symbol-use", symbol: make },
        { id: 2, operation: "symbol-trace", symbol: make },
      ]),
    );

    assert.equal(response.protocol_version, 3);
    assert.equal(response.operation, "semantic-queries");
    assert.equal(response.projects.length, 1);
    assert.equal(response.projects[0].program_reused, true);
    assert.equal(Object.hasOwn(response, "diagnostics"), false);
    assert.ok(
      response.results.every(
        (result) =>
          !Object.hasOwn(result, "diagnostics") && !Object.hasOwn(result, "compiler_diagnostics"),
      ),
    );
    assert.deepEqual(
      response.results.map((result) => result.assertion),
      ["confirmed-used", "confirmed-used", "references-found"],
    );
    assert.ok(response.results[0].evidence.length <= 1);
    assert.ok(response.results[0].total_evidence_count >= response.results[0].evidence.length);
    assert.equal(response.results[0].data.symbol.namespace, "type");
    assert.ok(response.results[2].evidence.some((entry) => entry.role === "declaration"));
    assert.ok(response.results[2].evidence.some((entry) => entry.source === "checker"));
    const directAnalysis = analyzeSemanticQueries(
      parseRequest(
        semanticRequest(root, [
          { id: 0, operation: "symbol-use", symbol: user },
          { id: 1, operation: "symbol-use", symbol: make },
        ]),
      ),
    );
    assert.equal(directAnalysis.sourceScanCount, 2);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 does not treat paired accessors as uses of each other", () => {
  const root = makeProject();
  try {
    const source = `export class Accessor {
  get value(): string { return ""; }
  set value(next: string) { console.log(next); }
}
`;
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/accessor.ts", source);
    const identities = ["get value", "set value"].map((marker) =>
      symbolIdentity({
        source,
        marker,
        file: "src/accessor.ts",
        namespace: "value",
        declarationKind: "class_method",
        exportedName: "value",
        owner: "Accessor",
      }),
    );

    const response = runSidecar(
      semanticRequest(
        root,
        identities.map((symbol, id) => ({ id, operation: "symbol-use", symbol })),
      ),
    );

    assert.deepEqual(
      response.results.map(({ assertion }) => assertion),
      ["no-confirmed-use", "no-confirmed-use"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 maps public API leaks and project-local public-signature coupling", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "package.json", JSON.stringify({ exports: { ".": "./src/index.ts" } }));
    write(root, "src/model.ts", "export interface Hidden { token: string }\n");
    write(
      root,
      "src/api.ts",
      [
        'import type { Hidden } from "./model";',
        "export interface PublicResult { hidden: Hidden }",
        "",
      ].join("\n"),
    );
    write(root, "src/index.ts", 'export type { PublicResult } from "./api";\n');

    const response = runSidecar(
      semanticRequest(root, [
        { id: 10, operation: "api-surface", entry_points: ["src/index.ts"] },
        {
          id: 11,
          operation: "type-coupling",
          entry_points: ["src/index.ts"],
          include_cycles: true,
        },
      ]),
    );

    const api = response.results[0];
    assert.equal(api.assertion, "leak-confirmed");
    assert.equal(api.status, "complete");
    assert.ok(
      api.data.leaks.some(
        (leak) =>
          leak.exposed_symbol.exported_name === "PublicResult" &&
          leak.private_declaration.local_name === "Hidden",
      ),
    );
    assert.match(api.data.entries[0].signature_fingerprint, /^sha256:[0-9a-f]{64}$/u);
    assert.ok(
      api.data.entries[0].referenced_types.some(
        (reference) => reference.declaration.local_name === "Hidden",
      ),
    );
    const coupling = response.results[1];
    assert.equal(coupling.assertion, "coupling-found");
    assert.equal(coupling.data.scope, "project-local-public-signatures");
    assert.equal(coupling.data.direction, "directed");
    assert.equal(coupling.data.project_size, 3);
    assert.equal(coupling.data.distinct_coupled_files, 2);
    assert.equal(coupling.data.edge_count, 1);
    assert.equal(coupling.data.coupled_file_percentage, (2 / 3) * 100);
    assert.equal(coupling.data.p50_distinct_connections, 1);
    assert.equal(coupling.data.p90_distinct_connections, 1);
    assert.equal(coupling.data.concentration, 1);
    assert.ok(
      coupling.data.files.some(
        (entry) => entry.path === "src/api.ts" && entry.outgoing_files.includes("src/model.ts"),
      ),
    );
    assert.equal(coupling.data.top_contributors[0].path, "src/api.ts");
    assert.deepEqual(coupling.data.cycles, []);
    assert.equal(coupling.data.files[0].outgoing_label, "public API depends on");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 discovers public entry points from a nested package project", () => {
  const root = makeProject();
  try {
    write(
      root,
      "packages/lib/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(
      root,
      "packages/lib/package.json",
      JSON.stringify({
        name: "@fixture/lib",
        exports: { ".": { types: "./dist/index.d.ts", default: "./dist/index.js" } },
      }),
    );
    write(root, "packages/lib/src/model.ts", "export interface Hidden { token: string }\n");
    write(
      root,
      "packages/lib/src/api.ts",
      [
        'import type { Hidden } from "./model";',
        "export interface PublicResult { hidden: Hidden }",
        "",
      ].join("\n"),
    );
    write(root, "packages/lib/src/index.ts", 'export type { PublicResult } from "./api";\n');

    const response = runSidecar(
      semanticRequest(
        root,
        [
          { id: 12, operation: "api-surface", entry_points: [] },
          {
            id: 13,
            operation: "type-coupling",
            entry_points: [],
            include_cycles: true,
          },
        ],
        { projects: ["packages/lib/tsconfig.json"] },
      ),
    );

    assert.equal(response.results[0].assertion, "leak-confirmed");
    assert.equal(response.results[1].assertion, "coupling-found");
    assert.equal(response.results[1].data.edge_count, 1);
    assert.equal(response.results[1].data.files_analyzed, 3);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 finds semantic consumers and tests across selected projects", () => {
  const root = makeProject();
  try {
    const source = "export const execute = (): string => 'ok';\n";
    write(
      root,
      "packages/lib/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(
      root,
      "packages/app/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "packages/lib/src/service.ts", source);
    write(
      root,
      "packages/app/src/consumer.ts",
      'import { execute } from "../../lib/src/service";\nexport const result = execute();\n',
    );
    write(
      root,
      "packages/app/src/consumer.test.ts",
      'import { result } from "./consumer";\nvoid result;\n',
    );
    const symbol = symbolIdentity({
      source,
      marker: "execute",
      file: "packages/lib/src/service.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "execute",
      localName: "execute",
    });
    const response = runSidecar(
      semanticRequest(
        root,
        [
          { id: 5, operation: "symbol-use", symbol },
          { id: 6, operation: "symbol-trace", symbol },
          { id: 7, operation: "symbol-impact", symbol },
        ],
        { projects: ["packages/lib/tsconfig.json", "packages/app/tsconfig.json"] },
      ),
    );
    assert.equal(response.results[0].assertion, "confirmed-used");
    assert.ok(
      response.results[1].evidence.some((entry) => entry.path === "packages/app/src/consumer.ts"),
    );
    assert.deepEqual(response.results[2].data.direct_consumers, [
      { path: "packages/app/src/consumer.ts", namespace: "value" },
    ]);
    assert.deepEqual(response.results[2].data.targeted_tests, [
      {
        path: "packages/app/src/consumer.test.ts",
        provenance: ["packages/app/src/consumer.test.ts", "packages/app/src/consumer.ts"],
      },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 reports exact-symbol impact and shortest targeted-test provenance", () => {
  const root = makeProject();
  try {
    const librarySource = "export const run = (value: string): string => value;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/library.ts", librarySource);
    write(
      root,
      "src/consumer.ts",
      'import { run } from "./library";\nexport const execute = (): string => run("ok");\n',
    );
    write(root, "src/consumer.test.ts", 'import { execute } from "./consumer";\nexecute();\n');
    write(root, "src/unrelated.test.ts", 'import "./library";\n');
    const run = symbolIdentity({
      source: librarySource,
      marker: "run",
      file: "src/library.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "run",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 20, operation: "symbol-impact", symbol: run }]),
    );
    const impact = response.results[0];
    assert.equal(impact.assertion, "consumers-found");
    assert.equal(impact.data.selected_project, "tsconfig.json");
    assert.deepEqual(impact.data.direct_consumers, [
      { path: "src/consumer.ts", namespace: "value" },
    ]);
    assert.deepEqual(impact.data.targeted_tests, [
      {
        path: "src/consumer.test.ts",
        provenance: ["src/consumer.test.ts", "src/consumer.ts"],
      },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 includes a direct test consumer in targeted tests", () => {
  const root = makeProject();
  try {
    const librarySource = "export const run = (value: string): string => value;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/library.ts", librarySource);
    write(root, "src/library.test.ts", 'import { run } from "./library";\nrun("ok");\n');
    const run = symbolIdentity({
      source: librarySource,
      marker: "run",
      file: "src/library.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "run",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 21, operation: "symbol-impact", symbol: run }]),
    );
    const impact = response.results[0];
    assert.deepEqual(impact.data.direct_consumers, [
      { path: "src/library.test.ts", namespace: "value" },
    ]);
    assert.deepEqual(impact.data.targeted_tests, [
      {
        path: "src/library.test.ts",
        provenance: ["src/library.test.ts"],
      },
    ]);
    assert.equal(impact.data.total_targeted_test_count, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 bounds evidence and exposes omissions, reasons, and actions", () => {
  const root = makeProject();
  try {
    const source = "export const used = 1;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/value.ts", source);
    write(
      root,
      "src/consumer.ts",
      'import { used } from "./value";\nexport const total = used + used;\n',
    );
    const used = symbolIdentity({
      source,
      marker: "used",
      file: "src/value.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "used",
    });
    const response = runSidecar(
      semanticRequest(root, [{ id: 30, operation: "symbol-trace", symbol: used }], {
        evidenceLimit: 1,
      }),
    );
    const result = response.results[0];
    assert.equal(result.status, "partial");
    assert.equal(result.reason_code, "evidence-limit");
    assert.equal(result.evidence.length, 1);
    assert.ok(result.total_evidence_count > result.evidence.length);
    assert.equal(result.truncated, true);
    assert.deepEqual(result.omissions, [
      { reason_code: "evidence-limit", count: result.total_evidence_count - 1 },
    ]);
    assert.equal(result.actions.length, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 keeps merged value and type namespaces distinct", () => {
  const root = makeProject();
  try {
    const source = [
      "export interface Token { value: string }",
      'export const Token = "runtime";',
      "",
    ].join("\n");
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/token.ts", source);
    write(
      root,
      "src/consumer.ts",
      [
        'import { Token as RuntimeToken, type Token } from "./token";',
        'const typed: Token = { value: "typed" };',
        "export const runtime = RuntimeToken;",
        "export { typed };",
        "",
      ].join("\n"),
    );
    const typeSymbol = symbolIdentity({
      source,
      marker: "Token",
      file: "src/token.ts",
      namespace: "type",
      declarationKind: "interface",
      exportedName: "Token",
    });
    const valueSymbol = symbolIdentity({
      source,
      marker: "Token",
      file: "src/token.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "Token",
      localName: "Token",
    });
    valueSymbol.line = 2;
    valueSymbol.col = Buffer.byteLength("export const ", "utf8");

    const response = runSidecar(
      semanticRequest(root, [
        { id: 40, operation: "symbol-use", symbol: typeSymbol },
        { id: 41, operation: "symbol-use", symbol: valueSymbol },
      ]),
    );
    assert.deepEqual(
      response.results.map((result) => result.assertion),
      ["confirmed-used", "confirmed-used"],
    );
    assert.ok(
      response.results[0].evidence.every((entry) =>
        ["type-import", "type-reference"].includes(entry.role),
      ),
    );
    assert.ok(
      response.results[1].evidence.every(
        (entry) => !["type-import", "type-reference"].includes(entry.role),
      ),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 fails closed for an unknown exact symbol identity", () => {
  const root = makeProject();
  try {
    const source = "export const known = 1;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/value.ts", source);
    const identity = symbolIdentity({
      source,
      marker: "known",
      file: "src/value.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "wrong-export-name",
      localName: "known",
    });
    const response = runSidecar(
      semanticRequest(root, [{ id: 50, operation: "symbol-use", symbol: identity }]),
    );
    assert.deepEqual(response.results[0], {
      query_id: 50,
      operation: "symbol-use",
      assertion: "no-confirmed-use",
      status: "unavailable",
      reason_code: "unknown-symbol",
      actions: ["Refresh the syntactic result and retry with its exact declaration identity."],
      evidence: [],
      total_evidence_count: 0,
      truncated: false,
      omissions: [{ reason_code: "unknown-symbol", count: 1 }],
      data: {},
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 resolves a generic export identity through an alias", () => {
  const root = makeProject();
  try {
    const source = "const original = 1;\nexport { original as publicName };\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/value.ts", source);
    write(
      root,
      "src/consumer.ts",
      'import { publicName } from "./value";\nexport const used = publicName;\n',
    );
    const identity = symbolIdentity({
      source,
      marker: "publicName",
      file: "src/value.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "publicName",
      localName: "publicName",
    });
    const response = runSidecar(
      semanticRequest(root, [{ id: 60, operation: "symbol-use", symbol: identity }]),
    );
    const result = response.results[0];
    assert.equal(result.assertion, "confirmed-used");
    assert.equal(result.data.symbol.declaration_kind, "variable");
    assert.equal(result.data.symbol.exported_name, "publicName");
    assert.equal(result.data.symbol.local_name, "original");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 resolves a named default export to its actual declaration", () => {
  const root = makeProject();
  try {
    const source = "export default function execute(): string { return 'ok'; }\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/default.ts", source);
    write(root, "src/consumer.ts", 'import run from "./default";\nexport const result = run();\n');
    const identity = symbolIdentity({
      source,
      marker: "execute",
      file: "src/default.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "default",
      localName: "default",
    });
    const response = runSidecar(
      semanticRequest(root, [{ id: 70, operation: "symbol-use", symbol: identity }]),
    );
    assert.equal(response.results[0].assertion, "confirmed-used");
    assert.equal(response.results[0].data.symbol.declaration_kind, "function");
    assert.equal(response.results[0].data.symbol.exported_name, "default");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v3 preserves bulk symbol-use capacity and separately bounds graph queries", () => {
  const baseSymbol = {
    path: "src/value.ts",
    namespace: "value",
    declaration_kind: "export",
    exported_name: "value",
    local_name: "value",
    line: 1,
    col: 0,
  };
  const bulk = semanticRequest(
    sidecarRoot,
    Array.from({ length: 25_000 }, (_, id) => ({
      id,
      operation: "symbol-use",
      symbol: baseSymbol,
    })),
    { projects: [] },
  );
  assert.equal(parseRequest(bulk).queries.length, 25_000);
  assert.ok(Buffer.byteLength(JSON.stringify(bulk)) < 8 * 1024 * 1024);
  assert.throws(
    () =>
      parseRequest({
        ...bulk,
        queries: [...bulk.queries, { id: 25_000, operation: "symbol-use", symbol: baseSymbol }],
      }),
    /queries exceeds the 25000 item limit/,
  );
  assert.throws(
    () =>
      parseRequest(
        semanticRequest(
          sidecarRoot,
          Array.from({ length: 257 }, (_, id) => ({
            id,
            operation: "api-surface",
            entry_points: [],
          })),
          { projects: [] },
        ),
      ),
    /graph queries exceed the 256 item limit/,
  );
  const maximalBulkResponse = createSemanticResponse({
    selectedTsconfigs: ["tsconfig.json"],
    projectResults: [],
    results: bulk.queries.map((query) => ({
      queryId: query.id,
      operation: query.operation,
      assertion: "confirmed-used",
      status: "complete",
      evidence: [{ path: "src/consumer.ts", line: 1, col: 0, role: "value-reference" }],
      totalEvidenceCount: 1,
      data: { symbol: baseSymbol, selected_project: "tsconfig.json" },
    })),
    phaseTimings: { project_setup: 0, diagnostics: 0, semantic_queries: 0 },
    warnings: [],
    elapsedMs: 0,
  });
  assert.ok(Buffer.byteLength(JSON.stringify(maximalBulkResponse)) < 32 * 1024 * 1024);
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
  assert.equal(response.sidecar_version, "3.8.0");
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
