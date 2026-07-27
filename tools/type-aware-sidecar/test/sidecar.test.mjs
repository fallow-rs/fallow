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
import { API } from "typescript/unstable/sync";

import { createSemanticResponse, createStatusResponse, parseRequest } from "../src/protocol.mjs";
import { analyzeSemanticQueries, createSemanticSession } from "../src/semantic.mjs";
import { readAll } from "../src/cli.mjs";
import { canonicalFileIdentity } from "../src/file-identity.mjs";
import {
  ANALYSIS_OPERATION,
  BACKEND_FAMILY,
  BACKEND_VERSION,
  WIRE_PROTOCOL_VERSION,
} from "../src/generated-protocol.mjs";

const sidecarRoot = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const executable = process.env.FALLOW_TYPE_AWARE_BIN
  ? path.resolve(process.env.FALLOW_TYPE_AWARE_BIN)
  : path.join(sidecarRoot, "fallow-type-aware.mjs");

const makeProject = () => mkdtempSync(path.join(tmpdir(), "fallow-type-aware-"));

test("status reports protocol and backend without a project request", () => {
  const result = spawnSync(executable, ["--status"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(result.stdout), {
    package_version: "3.9.1",
    protocol_version: WIRE_PROTOCOL_VERSION,
    backend_family: BACKEND_FAMILY,
    backend_version: BACKEND_VERSION,
  });
});

const write = (root, relativePath, contents) => {
  const fileName = path.join(root, relativePath);
  mkdirSync(path.dirname(fileName), { recursive: true });
  writeFileSync(fileName, contents);
};

const semanticRequest = (root, queries, options = {}) => ({
  protocol_version: WIRE_PROTOCOL_VERSION,
  operation: ANALYSIS_OPERATION,
  root,
  projects: options.projects ?? ["tsconfig.json"],
  evidence_limit: options.evidenceLimit ?? 40,
  queries,
});

test("unexpected semantic backend failures propagate instead of becoming abstentions", () => {
  const root = makeProject();
  const backendError = new Error("injected backend failure");
  let closed = false;
  try {
    const parsed = parseRequest(
      semanticRequest(root, [
        {
          id: 0,
          operation: "api-surface",
          entry_points: [],
          private_leak_candidates: [],
        },
      ]),
    );
    assert.throws(
      () =>
        analyzeSemanticQueries(parsed, {
          createApi: () => ({
            updateSnapshot: () => {
              throw backendError;
            },
            close: () => {
              closed = true;
            },
          }),
        }),
      backendError,
    );
    assert.equal(closed, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 validates request-scoped private leak candidates", () => {
  const root = makeProject();
  try {
    const base = {
      id: 0,
      operation: "api-surface",
      entry_points: [],
      private_leak_candidates: [
        {
          id: 4,
          path: "src/api.ts",
          export_name: "run",
          type_name: "PrivateOptions",
        },
      ],
    };
    assert.equal(
      parseRequest(semanticRequest(root, [base])).queries[0].privateLeakCandidates[0].id,
      4,
    );
    assert.throws(
      () =>
        parseRequest(
          semanticRequest(root, [
            {
              ...base,
              private_leak_candidates: [
                ...base.private_leak_candidates,
                { ...base.private_leak_candidates[0] },
              ],
            },
          ]),
        ),
      /duplicate candidate id 4/,
    );
    assert.throws(
      () =>
        parseRequest(
          semanticRequest(root, [
            {
              ...base,
              private_leak_candidates: [
                { ...base.private_leak_candidates[0], path: path.resolve(root, "src/api.ts") },
              ],
            },
          ]),
        ),
      /must be project-relative/,
    );
    assert.throws(
      () =>
        parseRequest(
          semanticRequest(root, [
            {
              ...base,
              private_leak_candidates: [{ ...base.private_leak_candidates[0], unexpected: true }],
            },
          ]),
        ),
      /unknown field unexpected/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
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

const classMemberQuery = ({
  id,
  file,
  owner,
  member,
  kind = "class_method",
  line = 2,
  col = 2,
}) => ({
  id,
  operation: "symbol-use",
  symbol: {
    path: file,
    namespace: "value",
    declaration_kind: kind,
    exported_name: member,
    local_name: member,
    owner,
    line,
    col,
  },
  framework_contracts: [],
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
      semanticRequest(root, [
        classMemberQuery({
          id: 0,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "used",
        }),
        classMemberQuery({
          id: 1,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "dead",
          line: 3,
        }),
        classMemberQuery({
          id: 2,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "usedProperty",
          kind: "class_property",
          line: 4,
        }),
        classMemberQuery({
          id: 3,
          file: "src/client.ts",
          owner: "GenericClient",
          member: "deadProperty",
          kind: "class_property",
          line: 5,
        }),
      ]),
    );

    assert.deepEqual(
      response.results
        .filter((result) => result.assertion === "confirmed-used")
        .map((result) => result.query_id),
      [0, 2],
    );
    assert.deepEqual(
      response.results
        .filter((result) => result.assertion !== "confirmed-used")
        .map((result) => result.query_id),
      [1, 3],
    );
    assert.deepEqual(response.selected_tsconfigs, ["tsconfig.json"]);
    assert.equal(response.projects[0].candidate_count, 4);
    assert.equal(response.projects[0].confirmed_used_count, 2);
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
      semanticRequest(root, [
        classMemberQuery({
          id: 0,
          file: "src/client.ts",
          owner: "Client",
          member: "execute",
        }),
      ]),
    );

    assert.equal(response.results[0].assertion, "confirmed-used");
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
      semanticRequest(root, [
        classMemberQuery({
          id: 0,
          file: "src/component.ts",
          owner: "GreetingComponent",
          member: "label",
          kind: "class_property",
          ...declaration,
        }),
      ]),
    );

    assert.notEqual(response.results[0].assertion, "confirmed-used");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

for (const [testName, extension, template] of [
  [
    "retains Vue template-only members without claiming a checker use",
    "vue",
    "<template>{{ model.title }}</template>\n",
  ],
  [
    "retains Astro template-only members without claiming a checker use",
    "astro",
    "<h1>{model.title}</h1>\n",
  ],
]) {
  test(testName, () => {
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
      write(root, `src/component.${extension}`, template);
      const declaration = utf8Position(source, 'title = "hello"');

      const response = runSidecar(
        semanticRequest(root, [
          classMemberQuery({
            id: 0,
            file: "src/model.ts",
            owner: "ViewModel",
            member: "title",
            kind: "class_property",
            ...declaration,
          }),
        ]),
      );

      assert.notEqual(response.results[0].assertion, "confirmed-used");
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

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

    const response = runSidecar(
      semanticRequest(
        root,
        [
          classMemberQuery({
            id: 0,
            file: "packages/lib/src/client.ts",
            owner: "Client",
            member: "execute",
          }),
        ],
        { projects: ["packages/lib/tsconfig.json", "packages/app/tsconfig.json"] },
      ),
    );

    assert.equal(response.results[0].assertion, "confirmed-used");
    assert.deepEqual(response.selected_tsconfigs, [
      "packages/app/tsconfig.json",
      "packages/lib/tsconfig.json",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 batches exact export and type-use queries through one Program", () => {
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

    assert.equal(response.protocol_version, 6);
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

test("protocol v6 does not treat paired accessors as uses of each other", () => {
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
    assert.ok(response.results.every(({ reason_code }) => reason_code === "accessor-pair"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 maps public API leaks and project-local public-signature coupling", () => {
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

    const apiRequest = semanticRequest(root, [
      {
        id: 10,
        operation: "api-surface",
        entry_points: ["src/index.ts"],
        private_leak_candidates: [
          {
            id: 7,
            path: "src/api.ts",
            export_name: "PublicResult",
            type_name: "Hidden",
          },
        ],
      },
      {
        id: 11,
        operation: "type-coupling",
        entry_points: ["src/index.ts"],
        include_cycles: true,
      },
    ]);
    const response = runSidecar(apiRequest);
    const directAnalysis = analyzeSemanticQueries(parseRequest(apiRequest));
    assert.equal(directAnalysis.sourceScanCount, 0);

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
    assert.deepEqual(api.data.private_leak_confirmation, {
      requested_candidate_count: 1,
      confirmation_complete: true,
      confirmed_candidate_ids: [7],
    });
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

test("protocol v6 resolves checker-inferred public return types", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/model.ts", "export class Hidden { token = 'secret' }\n");
    write(
      root,
      "src/index.ts",
      'import { Hidden } from "./model";\nexport const create = () => new Hidden();\n',
    );

    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 12,
          operation: "api-surface",
          entry_points: ["src/index.ts"],
          private_leak_candidates: [],
        },
      ]),
    );
    const api = response.results[0];
    assert.equal(api.status, "complete");
    assert.ok(
      api.data.entries
        .find((entry) => entry.exposed.exported_name === "create")
        .referenced_types.some((reference) => reference.declaration.local_name === "Hidden"),
    );
    assert.ok(
      api.data.leaks.some(
        (leak) =>
          leak.exposed_symbol.exported_name === "create" &&
          leak.private_declaration.local_name === "Hidden",
      ),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 reports public-signature cycles spanning three files", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/a.ts", 'import type { B } from "./b";\nexport interface A { b: B }\n');
    write(root, "src/b.ts", 'import type { C } from "./c";\nexport interface B { c: C }\n');
    write(root, "src/c.ts", 'import type { A } from "./a";\nexport interface C { a: A }\n');
    write(
      root,
      "src/index.ts",
      [
        'export type { A } from "./a";',
        'export type { B } from "./b";',
        'export type { C } from "./c";',
        "",
      ].join("\n"),
    );

    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 13,
          operation: "type-coupling",
          entry_points: ["src/index.ts"],
          include_cycles: true,
        },
      ]),
    );
    const coupling = response.results[0];
    assert.equal(coupling.status, "complete");
    assert.deepEqual(coupling.data.cycles, [["src/a.ts", "src/b.ts", "src/c.ts", "src/a.ts"]]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 confirms a requested private leak beyond bounded evidence", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    const declarations = Array.from({ length: 45 }, (_, index) => {
      const suffix = String(index).padStart(2, "0");
      return [
        `interface Hidden${suffix} { value: string }`,
        `export interface Public${suffix} { hidden: Hidden${suffix} }`,
      ].join("\n");
    }).join("\n");
    write(root, "src/index.ts", `${declarations}\n`);

    const response = runSidecar(
      semanticRequest(
        root,
        [
          {
            id: 20,
            operation: "api-surface",
            entry_points: ["src/index.ts"],
            private_leak_candidates: [
              {
                id: 900,
                path: "src/index.ts",
                export_name: "Public44",
                type_name: "Hidden44",
              },
            ],
          },
        ],
        { evidenceLimit: 1 },
      ),
    );
    const api = response.results[0];
    assert.equal(api.data.total_leak_count, 45);
    assert.equal(api.data.leaks.length, 1);
    assert.deepEqual(api.data.private_leak_confirmation, {
      requested_candidate_count: 1,
      confirmation_complete: true,
      confirmed_candidate_ids: [900],
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 reports every missing requested entry point", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/index.ts", "export interface PublicResult { value: string }\n");

    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 12,
          operation: "api-surface",
          entry_points: ["src/index.ts", "src/missing.ts"],
          private_leak_candidates: [
            {
              id: 0,
              path: "src/index.ts",
              export_name: "PublicResult",
              type_name: "Missing",
            },
          ],
        },
        {
          id: 13,
          operation: "type-coupling",
          entry_points: ["src/index.ts", "src/missing.ts"],
          include_cycles: false,
        },
      ]),
    );

    for (const result of response.results) {
      assert.equal(result.status, "partial");
      assert.equal(result.reason_code, "unknown-entry-point");
      assert.deepEqual(result.omissions, [{ reason_code: "unknown-entry-point", count: 1 }]);
    }
    assert.equal(response.results[0].data.private_leak_confirmation.confirmation_complete, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 ignores parameter and generic names in public signatures", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "package.json", JSON.stringify({ exports: { ".": "./src/index.ts" } }));
    write(
      root,
      "src/model.ts",
      "export interface CustomTester { test(value: unknown): boolean }\n",
    );
    write(
      root,
      "src/api.ts",
      [
        'import type { CustomTester } from "./model";',
        "export type PublicMatchers = {",
        "  match<T>(actual: T, expected: T): boolean;",
        "  customTesters: readonly CustomTester[];",
        "}",
        "",
      ].join("\n"),
    );
    write(root, "src/index.ts", 'export type { PublicMatchers } from "./api";\n');

    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 14,
          operation: "api-surface",
          entry_points: ["src/index.ts"],
          private_leak_candidates: [
            {
              id: 3,
              path: "src/api.ts",
              export_name: "PublicMatchers",
              type_name: "CustomTester",
            },
          ],
        },
        {
          id: 15,
          operation: "type-coupling",
          entry_points: ["src/index.ts"],
          include_cycles: true,
        },
      ]),
    );

    const api = response.results[0];
    assert.deepEqual(
      api.data.leaks.map((leak) => leak.private_declaration.local_name),
      ["CustomTester"],
    );
    assert.deepEqual(api.data.private_leak_confirmation, {
      requested_candidate_count: 1,
      confirmation_complete: true,
      confirmed_candidate_ids: [3],
    });
    assert.deepEqual(
      api.data.entries[0].referenced_types.map((reference) => reference.declaration.local_name),
      ["CustomTester"],
    );
    const coupling = response.results[1];
    assert.equal(coupling.data.edge_count, 1);
    assert.deepEqual(
      coupling.data.edges.map((edge) => edge.target.local_name),
      ["CustomTester"],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 deduplicates API data from overlapping projects", () => {
  const root = makeProject();
  try {
    const config = JSON.stringify({
      compilerOptions: { strict: true },
      include: ["src/**/*.ts"],
    });
    write(root, "tsconfig.build.json", config);
    write(root, "tsconfig.test.json", config);
    write(root, "package.json", JSON.stringify({ exports: { ".": "./src/index.ts" } }));
    write(root, "src/model.ts", "interface Hidden { token: string }\nexport { Hidden };\n");
    write(
      root,
      "src/index.ts",
      [
        'import type { Hidden } from "./model";',
        "export interface PublicResult { hidden: Hidden }",
        "",
      ].join("\n"),
    );

    const response = runSidecar(
      semanticRequest(
        root,
        [{ id: 16, operation: "api-surface", entry_points: ["src/index.ts"] }],
        { projects: ["tsconfig.build.json", "tsconfig.test.json"] },
      ),
    );

    const api = response.results[0];
    assert.equal(api.data.total_export_count, 1);
    assert.equal(api.data.total_entry_count, 1);
    assert.equal(api.data.total_public_signature_edge_count, 1);
    assert.equal(api.data.total_leak_count, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 reports symbol-use outcomes per selected project", () => {
  const root = makeProject();
  try {
    const usedSource = "export const used = 1;\n";
    const unusedSource = "export const unused = 2;\n";
    write(
      root,
      "packages/used/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(
      root,
      "packages/unused/tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "packages/used/src/value.ts", usedSource);
    write(
      root,
      "packages/used/src/consumer.ts",
      'import { used } from "./value";\nexport const result = used;\n',
    );
    write(root, "packages/unused/src/value.ts", unusedSource);
    const response = runSidecar(
      semanticRequest(
        root,
        [
          {
            id: 17,
            operation: "symbol-use",
            symbol: symbolIdentity({
              source: usedSource,
              marker: "used",
              file: "packages/used/src/value.ts",
              namespace: "value",
              declarationKind: "export",
              exportedName: "used",
            }),
          },
          {
            id: 18,
            operation: "symbol-use",
            symbol: symbolIdentity({
              source: unusedSource,
              marker: "unused",
              file: "packages/unused/src/value.ts",
              namespace: "value",
              declarationKind: "export",
              exportedName: "unused",
            }),
          },
        ],
        {
          projects: ["packages/used/tsconfig.json", "packages/unused/tsconfig.json"],
        },
      ),
    );

    assert.deepEqual(
      response.projects.map(
        ({
          config,
          candidate_count,
          confirmed_used_count,
          no_static_references_count,
          unresolved_count,
          abstained_count,
        }) => ({
          config,
          candidate_count,
          confirmed_used_count,
          no_static_references_count,
          unresolved_count,
          abstained_count,
        }),
      ),
      [
        {
          config: "packages/unused/tsconfig.json",
          candidate_count: 1,
          confirmed_used_count: 0,
          no_static_references_count: 1,
          unresolved_count: 0,
          abstained_count: 0,
        },
        {
          config: "packages/used/tsconfig.json",
          candidate_count: 1,
          confirmed_used_count: 1,
          no_static_references_count: 0,
          unresolved_count: 0,
          abstained_count: 0,
        },
      ],
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 marks a project unavailable when an assigned symbol cannot be resolved", () => {
  const root = makeProject();
  try {
    const source = "export const value = 1;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/value.ts", source);
    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 19,
          operation: "symbol-use",
          symbol: symbolIdentity({
            source,
            marker: "value",
            file: "src/value.ts",
            namespace: "value",
            declarationKind: "export",
            exportedName: "missing",
          }),
        },
      ]),
    );

    assert.equal(response.results[0].reason_code, "unknown-symbol");
    assert.equal(response.projects[0].status, "unavailable");
    assert.equal(response.projects[0].reason_code, "unknown-symbol");
    assert.equal(response.projects[0].candidate_count, 1);
    assert.equal(response.projects[0].abstained_count, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 discovers public entry points from a nested package project", () => {
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

test("protocol v6 finds semantic consumers and tests across selected projects", () => {
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
    const crossProjectRequest = semanticRequest(
      root,
      [
        { id: 5, operation: "symbol-use", symbol },
        { id: 6, operation: "symbol-trace", symbol },
        { id: 7, operation: "symbol-impact", symbol },
      ],
      { projects: ["packages/lib/tsconfig.json", "packages/app/tsconfig.json"] },
    );
    const response = runSidecar(crossProjectRequest);
    const directAnalysis = analyzeSemanticQueries(parseRequest(crossProjectRequest));
    assert.equal(directAnalysis.referenceScanCount, 0);
    assert.equal(directAnalysis.sourceScanCount, 3);
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

test("protocol v6 reports exact-symbol impact and shortest targeted-test provenance", () => {
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

test("protocol v6 includes a direct test consumer in targeted tests", () => {
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

test("protocol v6 bounds evidence and exposes omissions, reasons, and actions", () => {
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

test("protocol v6 keeps merged value and type namespaces distinct", () => {
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

test("protocol v6 confirms complete closed-world absence of static class-member references", () => {
  const root = makeProject();
  try {
    const source = [
      "class Worker {",
      "  execute(): void {}",
      "}",
      "export const worker = new Worker();",
      "",
    ].join("\n");
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/worker.ts", source);
    const symbol = symbolIdentity({
      source,
      marker: "execute",
      file: "src/worker.ts",
      namespace: "value",
      declarationKind: "class_method",
      exportedName: "execute",
      owner: "Worker",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 42, operation: "symbol-use", symbol }]),
    );
    const result = response.results[0];
    assert.equal(result.assertion, "confirmed-no-static-references");
    assert.equal(result.status, "complete");
    assert.equal(result.data.closed_world_eligible, true);
    assert.deepEqual(result.data.owning_projects, ["tsconfig.json"]);
    assert.deepEqual(result.data.contract_relations, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 preserves required interface, abstract, and inherited contracts", () => {
  const cases = [
    {
      relation: "interface-implementation",
      source: [
        "interface Runnable { execute(): void }",
        "class Worker implements Runnable {",
        "  execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
    },
    {
      relation: "abstract-implementation",
      source: [
        "abstract class Runnable { abstract execute(): void }",
        "class Worker extends Runnable {",
        "  execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
    },
    {
      relation: "override",
      source: [
        "class Runnable { execute(): void {} }",
        "class Worker extends Runnable {",
        "  override execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
    },
    {
      relation: "override",
      source: [
        "abstract class Runnable { execute(): void {} }",
        "class Worker extends Runnable {",
        "  override execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
    },
  ];

  for (const expected of cases) {
    const root = makeProject();
    try {
      write(
        root,
        "tsconfig.json",
        JSON.stringify({
          compilerOptions: { strict: true, noImplicitOverride: true },
          include: ["src/**/*.ts"],
        }),
      );
      write(root, "src/worker.ts", expected.source);
      const symbol = symbolIdentity({
        source: expected.source,
        marker: "execute",
        occurrence: 2,
        file: "src/worker.ts",
        namespace: "value",
        declarationKind: "class_method",
        exportedName: "execute",
        owner: "Worker",
      });
      Object.assign(symbol, utf8Position(expected.source, "execute", 2));

      const response = runSidecar(
        semanticRequest(root, [
          { id: 43, operation: "symbol-use", symbol, framework_contracts: [] },
          { id: 44, operation: "symbol-impact", symbol },
        ]),
      );
      const result = response.results[0];
      assert.equal(result.assertion, "contract-preserved");
      assert.equal(result.data.closed_world_eligible, false);
      assert.equal(result.data.contract_relations[0].relation, expected.relation);
      assert.equal(result.data.contract_relations[0].declaration.local_name, "execute");
      const impact = response.results[1];
      assert.equal(impact.data.confidence, "bounded");
      assert.deepEqual(impact.omissions, [{ reason_code: "virtual-dispatch", count: 1 }]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("protocol v6 abstains for optional contracts, decorators, and dynamic member access", () => {
  const cases = [
    {
      reason: "optional-contract",
      source: [
        "interface Runnable { execute?(): void }",
        "class Worker implements Runnable {",
        "  execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
      markerOccurrence: 2,
    },
    {
      reason: "decorated-declaration",
      source: [
        "declare const Register: MethodDecorator;",
        "class Worker {",
        "  @Register",
        "  execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
      markerOccurrence: 1,
      experimentalDecorators: true,
    },
    {
      reason: "dynamic-member-access",
      source: [
        "class Worker {",
        "  execute(): void {}",
        "}",
        "const worker = new Worker();",
        'const key: keyof Worker = Math.random() ? "execute" : "execute";',
        "worker[key]();",
        "export { worker };",
        "",
      ].join("\n"),
      markerOccurrence: 1,
    },
    {
      reason: "abstract-declaration",
      source: ["export abstract class Worker {", "  abstract execute(): void;", "}", ""].join("\n"),
      markerOccurrence: 1,
    },
    {
      reason: "overload-set",
      source: [
        "class Worker {",
        "  execute(value: string): void;",
        "  execute(value: number): void;",
        "  execute(value: string | number): void { console.log(value); }",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
      markerOccurrence: 1,
    },
    {
      reason: "attached-comment",
      source: [
        "class Worker {",
        "  /** Registered by the host application. */",
        "  execute(): void {}",
        "}",
        "export const worker = new Worker();",
        "",
      ].join("\n"),
      markerOccurrence: 1,
    },
  ];

  for (const expected of cases) {
    const root = makeProject();
    try {
      write(
        root,
        "tsconfig.json",
        JSON.stringify({
          compilerOptions: {
            strict: true,
            experimentalDecorators: expected.experimentalDecorators ?? false,
          },
          include: ["src/**/*.ts"],
        }),
      );
      write(root, "src/worker.ts", expected.source);
      const symbol = symbolIdentity({
        source: expected.source,
        marker: "execute",
        file: "src/worker.ts",
        namespace: "value",
        declarationKind: "class_method",
        exportedName: "execute",
        owner: "Worker",
      });
      Object.assign(symbol, utf8Position(expected.source, "execute", expected.markerOccurrence));

      const response = runSidecar(
        semanticRequest(root, [{ id: 44, operation: "symbol-use", symbol }]),
      );
      const result = response.results[0];
      assert.equal(result.assertion, "no-confirmed-use");
      assert.equal(result.status, "partial");
      assert.equal(result.reason_code, expected.reason);
      assert.equal(result.data.closed_world_eligible, false);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("protocol v6 fails closed for an unknown exact symbol identity", () => {
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

test("protocol v6 resolves a generic export identity through an alias", () => {
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
    assert.equal(result.data.symbol.declaration_kind, "export");
    assert.equal(result.data.symbol.exported_name, "publicName");
    assert.equal(result.data.symbol.local_name, "publicName");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 resolves an import type through a renamed non-type barrel export", () => {
  const root = makeProject();
  try {
    const barrelSource = 'export { Api as PublicApi } from "./source";\n';
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/source.ts", "export interface Api<T> { value: T }\n");
    write(root, "src/barrel.ts", barrelSource);
    write(
      root,
      "src/consumer.ts",
      'type Used = import("./barrel").PublicApi<string>;\nexport const used: Used = { value: "ok" };\n',
    );
    const identity = symbolIdentity({
      source: barrelSource,
      marker: "PublicApi",
      file: "src/barrel.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "PublicApi",
      localName: "PublicApi",
    });

    const requestValue = semanticRequest(root, [
      { id: 66, operation: "symbol-use", symbol: identity },
      { id: 67, operation: "symbol-trace", symbol: identity },
    ]);
    const response = runSidecar(requestValue);
    const directAnalysis = analyzeSemanticQueries(parseRequest(requestValue));

    assert.equal(response.results[0].assertion, "confirmed-used");
    assert.equal(response.results[0].data.symbol.namespace, "value");
    assert.equal(response.results[0].evidence[0].namespace, "type");
    assert.equal(response.results[0].evidence[0].role, "type-reference");
    assert.ok(response.results[1].evidence.some((entry) => entry.path === "src/consumer.ts"));
    assert.equal(directAnalysis.sourceScanCount, 3);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 resolves typeof import for both halves of a declaration merge", () => {
  const root = makeProject();
  try {
    const source = [
      "export interface Merged { id: string }",
      "export namespace Merged { export const kind = 'merged' }",
      "",
    ].join("\n");
    const barrelSource = 'export { Merged as PublicMerged } from "./source";\n';
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/source.ts", source);
    write(root, "src/barrel.ts", barrelSource);
    write(
      root,
      "src/consumer.ts",
      'type Used = typeof import("./barrel").PublicMerged;\nexport const kind: Used["kind"] = "merged";\n',
    );
    const barrelIdentity = symbolIdentity({
      source: barrelSource,
      marker: "PublicMerged",
      file: "src/barrel.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "PublicMerged",
      localName: "PublicMerged",
    });
    const sourceIdentity = symbolIdentity({
      source,
      marker: "Merged",
      file: "src/source.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "Merged",
      localName: "Merged",
    });
    Object.assign(sourceIdentity, utf8Position(source, "Merged", 2));

    const response = runSidecar(
      semanticRequest(root, [
        { id: 68, operation: "symbol-use", symbol: barrelIdentity },
        { id: 69, operation: "symbol-use", symbol: sourceIdentity },
      ]),
    );

    assert.equal(response.results[0].assertion, "confirmed-used");
    assert.equal(response.results[0].evidence[0].namespace, "value");
    assert.equal(response.results[1].assertion, "confirmed-used");
    assert.ok(response.results[1].evidence.some((entry) => entry.path === "src/barrel.ts"));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 abstains when a dynamic import can consume an unresolved export", () => {
  const root = makeProject();
  try {
    const source = "export const runtimeValue = 1;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/source.ts", source);
    write(
      root,
      "src/consumer.ts",
      'void import("./source").then(({ runtimeValue }) => runtimeValue);\n',
    );
    const identity = symbolIdentity({
      source,
      marker: "runtimeValue",
      file: "src/source.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "runtimeValue",
      localName: "runtimeValue",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 70, operation: "symbol-use", symbol: identity }]),
    );
    const result = response.results[0];

    assert.equal(result.assertion, "no-confirmed-use");
    assert.equal(result.status, "partial");
    assert.equal(result.reason_code, "dynamic-behavior");
    assert.equal(result.data.closed_world_eligible, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 abstains when a non-literal dynamic import may consume an export", () => {
  const root = makeProject();
  try {
    const source = "export const runtimeValue = 1;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/source.ts", source);
    write(root, "src/consumer.ts", 'const modulePath = "./source";\nvoid import(modulePath);\n');
    const identity = symbolIdentity({
      source,
      marker: "runtimeValue",
      file: "src/source.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "runtimeValue",
      localName: "runtimeValue",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 71, operation: "symbol-use", symbol: identity }]),
    );
    const result = response.results[0];

    assert.equal(result.assertion, "no-confirmed-use");
    assert.equal(result.status, "partial");
    assert.equal(result.reason_code, "dynamic-behavior");
    assert.equal(result.data.closed_world_eligible, false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 scopes literal dynamic import uncertainty to its module", () => {
  const root = makeProject();
  try {
    const importedSource = "export const importedValue = 1;\n";
    const staticSource = "export const staticValue = 2;\n";
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/imported.ts", importedSource);
    write(root, "src/static.ts", staticSource);
    write(root, "src/consumer.ts", 'void import("./imported");\n');

    const response = runSidecar(
      semanticRequest(root, [
        {
          id: 72,
          operation: "symbol-use",
          symbol: symbolIdentity({
            source: importedSource,
            marker: "importedValue",
            file: "src/imported.ts",
            namespace: "value",
            declarationKind: "export",
            exportedName: "importedValue",
            localName: "importedValue",
          }),
        },
        {
          id: 73,
          operation: "symbol-use",
          symbol: symbolIdentity({
            source: staticSource,
            marker: "staticValue",
            file: "src/static.ts",
            namespace: "value",
            declarationKind: "export",
            exportedName: "staticValue",
            localName: "staticValue",
          }),
        },
      ]),
    );

    assert.equal(response.results[0].status, "partial");
    assert.equal(response.results[0].reason_code, "dynamic-behavior");
    assert.equal(response.results[1].status, "complete");
    assert.equal(response.results[1].assertion, "confirmed-no-static-references");
    assert.equal(response.results[1].data.closed_world_eligible, true);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 does not count same-module references as export use", () => {
  const root = makeProject();
  try {
    const source = [
      "export const locallyUsed = 1;",
      "export const localResult = locallyUsed + 1;",
      "",
    ].join("\n");
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/value.ts", source);
    const identity = symbolIdentity({
      source,
      marker: "locallyUsed",
      file: "src/value.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "locallyUsed",
      localName: "locallyUsed",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 61, operation: "symbol-use", symbol: identity }]),
    );

    assert.equal(response.results[0].assertion, "confirmed-no-static-references");
    assert.equal(response.results[0].total_evidence_count, 0);
    assert.equal(response.projects[0].no_static_references_count, 1);
    assert.equal(response.projects[0].fix_eligible_count, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 keeps an unused barrel alias distinct from its used source export", () => {
  const root = makeProject();
  try {
    const barrelSource = 'export { shared } from "./source";\n';
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    write(root, "src/source.ts", "export const shared = 1;\n");
    write(root, "src/barrel.ts", barrelSource);
    write(
      root,
      "src/consumer.ts",
      'import { shared } from "./source";\nexport const result = shared;\n',
    );
    const identity = symbolIdentity({
      source: barrelSource,
      marker: "shared",
      file: "src/barrel.ts",
      namespace: "value",
      declarationKind: "export",
      exportedName: "shared",
      localName: "shared",
    });

    const response = runSidecar(
      semanticRequest(root, [{ id: 62, operation: "symbol-use", symbol: identity }]),
    );

    assert.equal(response.results[0].assertion, "confirmed-no-static-references");
    assert.equal(response.results[0].total_evidence_count, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 counts namespace property and element access through the exact source module", () => {
  for (const access of ["source.shared", 'source["shared"]']) {
    const root = makeProject();
    try {
      const source = "export const shared = 1;\n";
      write(
        root,
        "tsconfig.json",
        JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
      );
      write(root, "src/source.ts", source);
      write(
        root,
        "src/consumer.ts",
        `import * as source from "./source";\nexport const result = ${access};\n`,
      );
      const identity = symbolIdentity({
        source,
        marker: "shared",
        file: "src/source.ts",
        namespace: "value",
        declarationKind: "export",
        exportedName: "shared",
        localName: "shared",
      });

      const response = runSidecar(
        semanticRequest(root, [
          { id: 63, operation: "symbol-use", symbol: identity },
          { id: 64, operation: "symbol-trace", symbol: identity },
          { id: 65, operation: "symbol-impact", symbol: identity },
        ]),
      );

      assert.equal(response.results[0].assertion, "confirmed-used");
      assert.equal(response.results[0].total_evidence_count, 1);
      assert.equal(response.results[0].evidence[0].path, "src/consumer.ts");
      assert.equal(response.results[1].assertion, "references-found");
      assert.ok(response.results[1].evidence.some((entry) => entry.path === "src/consumer.ts"));
      assert.equal(response.results[2].assertion, "consumers-found");
      assert.deepEqual(response.results[2].data.direct_consumers, [
        { path: "src/consumer.ts", namespace: "value" },
      ]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test("protocol v6 tracks a named default export by its module export identity", () => {
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
    assert.equal(response.results[0].data.symbol.declaration_kind, "export");
    assert.equal(response.results[0].data.symbol.exported_name, "default");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("protocol v6 preserves bulk symbol-use capacity and separately bounds graph queries", () => {
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

test("rejects the removed protocol v2 without JSON stdout", () => {
  const result = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify({
      protocol_version: 2,
      operation: "class-member-uses",
      root: sidecarRoot,
      projects: [],
      candidates: [],
    }),
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /unsupported protocol_version 2/);
});

test("rejects negative semantic query IDs", () => {
  const result = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(
      semanticRequest(
        sidecarRoot,
        [
          {
            id: -1,
            operation: "api-surface",
            entry_points: [],
            private_leak_candidates: [],
          },
        ],
        { projects: [] },
      ),
    ),
    encoding: "utf8",
  });

  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /queries\[0\]\.id must be an integer/);
});

test("rejects unknown request fields and absolute symbol paths", () => {
  const unknown = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify({
      ...semanticRequest(sidecarRoot, [], { projects: [] }),
      unexpected: true,
    }),
    encoding: "utf8",
  });
  assert.equal(unknown.status, 2);
  assert.match(unknown.stderr, /request contains unknown field unexpected/);

  const absolute = spawnSync(executable, {
    cwd: sidecarRoot,
    input: JSON.stringify(
      semanticRequest(
        sidecarRoot,
        [
          {
            id: 0,
            operation: "symbol-use",
            symbol: {
              path: path.join(sidecarRoot, "missing.ts"),
              namespace: "value",
              declaration_kind: "function",
              exported_name: "run",
              local_name: "run",
              line: 1,
              col: 0,
            },
            framework_contracts: [],
          },
        ],
        { projects: [] },
      ),
    ),
    encoding: "utf8",
  });
  assert.equal(absolute.status, 2);
  assert.match(absolute.stderr, /path must be project-relative/);
});

test("bounds and normalizes warning text", () => {
  const response = createSemanticResponse({
    selectedTsconfigs: ["\u{10000}.json", "\uE000.json"],
    projectResults: [],
    results: [],
    phaseTimings: { project_setup: 0, diagnostics: 0, semantic_queries: 0 },
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
  const response = createStatusResponse();

  assert.equal(response.package_version, packageJson.version);
});

test("rejects oversized stdin while reading", async () => {
  await assert.rejects(readAll(Readable.from(["12345"]), 4), /4 byte request limit/);
});

test("returns provenance for an empty semantic request", () => {
  const response = runSidecar(semanticRequest(sidecarRoot, [], { projects: [] }));

  assert.equal(response.protocol_version, 6);
  assert.equal(response.operation, "semantic-queries");
  assert.equal(response.sidecar_version, "3.9.1");
  assert.equal(response.backend, "typescript-go");
  assert.deepEqual(response.selected_tsconfigs, []);
  assert.deepEqual(response.projects, []);
  assert.deepEqual(response.results, []);
});

test("persistent semantic session invalidates changed source without stale references", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    const repositorySource = ["export class UserRepository {", "  save(): void {}", "}", ""].join(
      "\n",
    );
    write(root, "src/repository.ts", repositorySource);
    write(
      root,
      "src/consumer.ts",
      'import { UserRepository } from "./repository"; new UserRepository().save();\n',
    );
    const save = symbolIdentity({
      source: repositorySource,
      marker: "save",
      file: "src/repository.ts",
      namespace: "value",
      declarationKind: "class_method",
      exportedName: "save",
      localName: "save",
      owner: "UserRepository",
    });
    const parsed = parseRequest(
      semanticRequest(root, [
        { id: 0, operation: "symbol-use", symbol: save, framework_contracts: [] },
      ]),
    );
    const snapshots = [];
    const session = createSemanticSession(root, {
      createApi: (cwd) => {
        const api = new API({ cwd });
        return {
          updateSnapshot: (params) => {
            const snapshot = api.updateSnapshot(params);
            snapshots.push(snapshot);
            return snapshot;
          },
          close: () => api.close(),
        };
      },
    });
    try {
      const first = session.analyze(parsed, { revision: 1 });
      assert.equal(first.results[0].assertion, "confirmed-used");
      assert.equal(first.projectResults[0].program_reused_from_previous_snapshot, false);
      assert.equal(snapshots[0].isDisposed(), false);

      write(root, "src/consumer.ts", "export const unrelated = true;\n");
      const second = session.analyze(parsed, {
        revision: 2,
        fileChanges: { changed: [path.join(root, "src/consumer.ts")] },
      });
      assert.equal(second.results[0].assertion, "confirmed-no-static-references");
      assert.equal(second.projectResults[0].program_reused_from_previous_snapshot, true);
      assert.equal(second.projectResults[0].snapshot_revision, 2);
      assert.equal(second.projectResults[0].invalidation_kind, "incremental");
      assert.equal(snapshots[0].isDisposed(), true);
      assert.equal(snapshots[1].isDisposed(), false);

      const third = session.analyze(parsed, {
        revision: 3,
        fileChanges: { invalidateAll: true },
      });
      assert.equal(third.results[0].assertion, "confirmed-no-static-references");
      assert.equal(third.projectResults[0].program_reused_from_previous_snapshot, false);
      assert.equal(third.projectResults[0].snapshot_revision, 3);
      assert.equal(third.projectResults[0].invalidation_kind, "full");

      const addedConsumer = path.join(root, "src/new-consumer.ts");
      write(
        root,
        "src/new-consumer.ts",
        'import { UserRepository } from "./repository"; new UserRepository().save();\n',
      );
      const fourth = session.analyze(parsed, {
        revision: 4,
        fileChanges: { created: [addedConsumer] },
      });
      assert.equal(fourth.results[0].assertion, "confirmed-used");
      assert.equal(fourth.projectResults[0].invalidation_kind, "incremental");

      rmSync(addedConsumer);
      const fifth = session.analyze(parsed, {
        revision: 5,
        fileChanges: { deleted: [addedConsumer] },
      });
      assert.equal(fifth.results[0].assertion, "confirmed-no-static-references");
      assert.equal(fifth.projectResults[0].snapshot_revision, 5);
      session.close();
      assert.equal(
        snapshots.every((snapshot) => snapshot.isDisposed()),
        true,
      );
    } finally {
      session.close();
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("session mode frames revisions and reuses a compatible snapshot", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({ compilerOptions: { strict: true }, include: ["src/**/*.ts"] }),
    );
    const source = "export const value = 1;\n";
    write(root, "src/value.ts", source);
    const value = symbolIdentity({
      source,
      marker: "value",
      file: "src/value.ts",
      namespace: "value",
      declarationKind: "variable",
      exportedName: "value",
    });
    const body = semanticRequest(root, [
      { id: 0, operation: "symbol-use", symbol: value, framework_contracts: [] },
    ]);
    const input = [
      JSON.stringify({ type: "analyze", request_id: 11, revision: 1, request: body }),
      JSON.stringify({ type: "analyze", request_id: 12, revision: 2, request: body }),
      JSON.stringify({ type: "shutdown" }),
      "",
    ].join("\n");
    const result = spawnSync(executable, ["--session"], {
      cwd: sidecarRoot,
      input,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    const responses = result.stdout
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));
    assert.deepEqual(
      responses.map((response) => [response.request_id, response.revision]),
      [
        [11, 1],
        [12, 2],
      ],
    );
    assert.equal(responses[1].response.projects[0].program_reused_from_previous_snapshot, true);
    assert.equal(responses[1].response.projects[0].invalidation_kind, "none");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("framework contract requires the exact package declaration", () => {
  const root = makeProject();
  try {
    write(
      root,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { strict: true, module: "nodenext", moduleResolution: "nodenext" },
        include: ["src/**/*.ts"],
      }),
    );
    write(
      root,
      "node_modules/lit/package.json",
      JSON.stringify({ name: "lit", version: "1.0.0", types: "index.d.ts" }),
    );
    write(
      root,
      "node_modules/lit/index.d.ts",
      "export declare class LitElement { protected render(): unknown; }\n",
    );
    const source = [
      'import { LitElement } from "lit";',
      "export class UserCard extends LitElement {",
      "  protected render(): unknown { return null; }",
      "}",
      "",
    ].join("\n");
    write(root, "src/card.ts", source);
    const render = symbolIdentity({
      source,
      marker: "render",
      file: "src/card.ts",
      namespace: "value",
      declarationKind: "class_method",
      exportedName: "render",
      localName: "render",
      owner: "UserCard",
    });
    const contract = {
      framework: "lit",
      package: "lit",
      heritage_symbol: "LitElement",
      heritage_names: ["LitElement"],
      relation: "extends",
      members: ["render"],
    };
    const response = analyzeSemanticQueries(
      parseRequest(
        semanticRequest(root, [
          {
            id: 0,
            operation: "symbol-use",
            symbol: render,
            framework_contracts: [contract],
          },
        ]),
      ),
    );
    assert.equal(
      response.results[0].assertion,
      "contract-preserved",
      JSON.stringify(response.results[0]),
    );
    assert.equal(response.results[0].data.framework_contract_relations.length, 1);
    assert.equal(response.results[0].data.framework_contract_relations[0].package, "lit");

    write(
      root,
      "src/local.ts",
      [
        "class LitElement { protected render(): unknown { return null; } }",
        "export class LocalCard extends LitElement {",
        "  protected render(): unknown { return null; }",
        "}",
        "",
      ].join("\n"),
    );
    const localSource = readFileSync(path.join(root, "src/local.ts"), "utf8");
    const localRender = symbolIdentity({
      source: localSource,
      marker: "render",
      file: "src/local.ts",
      namespace: "value",
      declarationKind: "class_method",
      exportedName: "render",
      localName: "render",
      owner: "LocalCard",
    });
    Object.assign(localRender, utf8Position(localSource, "render", 2));
    const localResponse = analyzeSemanticQueries(
      parseRequest(
        semanticRequest(root, [
          {
            id: 0,
            operation: "symbol-use",
            symbol: localRender,
            framework_contracts: [contract],
          },
        ]),
      ),
    );
    assert.deepEqual(localResponse.results[0].data.framework_contract_relations, []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("unknown external framework package provenance abstains", () => {
  const monorepo = makeProject();
  const root = path.join(monorepo, "packages", "app");
  try {
    write(
      monorepo,
      "tsconfig.json",
      JSON.stringify({
        compilerOptions: { strict: true, module: "nodenext", moduleResolution: "nodenext" },
        include: ["packages/**/*.ts"],
      }),
    );
    write(monorepo, "packages/framework/lit.d.ts", "export declare class LitElement {}\n");
    const source = [
      'import { LitElement } from "../../framework/lit.js";',
      "export class UserCard extends LitElement {",
      "  protected render(): unknown { return null; }",
      "}",
      "",
    ].join("\n");
    write(root, "src/card.ts", source);
    const render = symbolIdentity({
      source,
      marker: "render",
      file: "src/card.ts",
      namespace: "value",
      declarationKind: "class_method",
      exportedName: "render",
      localName: "render",
      owner: "UserCard",
    });
    const response = analyzeSemanticQueries(
      parseRequest(
        semanticRequest(
          root,
          [
            {
              id: 0,
              operation: "symbol-use",
              symbol: render,
              framework_contracts: [
                {
                  framework: "lit",
                  package: "lit",
                  heritage_symbol: "LitElement",
                  heritage_names: ["LitElement"],
                  relation: "extends",
                  members: ["render"],
                },
              ],
            },
          ],
          { projects: ["../../tsconfig.json"] },
        ),
      ),
    );

    assert.equal(response.results[0].assertion, "no-confirmed-use");
    assert.equal(response.results[0].reasonCode, "framework-contract-provenance");
    assert.equal(response.results[0].data.closed_world_eligible, false);
  } finally {
    rmSync(monorepo, { recursive: true, force: true });
  }
});
