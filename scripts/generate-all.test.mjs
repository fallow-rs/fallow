import assert from "node:assert/strict";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  MUTATION_COMPANION_FILES,
  MUTATION_COMPANION_SOURCE_ROOT,
  MUTATION_COMPANION_TARGET_ROOT,
} from "./contract-surfaces.mjs";
import {
  assertMutationRunnerManifest,
  ensureTrailingNewline,
  generateMutationCompanion,
  generateMutationRunnerProtocol,
  generateTypeAwareProtocol,
  issueRegistry,
  jsLiteral,
  main,
  parseArgs,
} from "./generate-all.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const installForeignCodegenDependency = (root) => {
  const directory = join(root, "node_modules", "json-schema-to-typescript");
  mkdirSync(directory, { recursive: true });
  writeFileSync(
    join(directory, "package.json"),
    `${JSON.stringify({ name: "json-schema-to-typescript", version: "0.0.0" })}\n`,
  );
};

const installCargoMarker = (root, marker) => {
  const directory = join(root, "fake-bin");
  mkdirSync(directory, { recursive: true });
  if (process.platform === "win32") {
    writeFileSync(join(directory, "cargo.cmd"), `@echo called>"${marker}"\r\n@exit /b 97\r\n`);
  } else {
    const binary = join(directory, "cargo");
    writeFileSync(binary, `#!/bin/sh\nprintf called > "${marker}"\nexit 97\n`);
    chmodSync(binary, 0o755);
  }
  return directory;
};

test("generation validates local codegen resolution before Cargo", () => {
  const outer = mkdtempSync(join(tmpdir(), "fallow-generate-preflight-"));
  const checkout = join(outer, ".git-worktrees", "nested");
  const marker = join(outer, "cargo-called");
  mkdirSync(checkout, { recursive: true });
  cpSync(join(REPO_ROOT, "scripts"), join(checkout, "scripts"), { recursive: true });
  writeFileSync(
    join(checkout, "package.json"),
    `${JSON.stringify({ private: true, type: "module" })}\n`,
  );
  installForeignCodegenDependency(outer);
  const fakeBin = installCargoMarker(outer, marker);
  const env = { ...process.env, PATH: `${fakeBin}${delimiter}${process.env.PATH ?? ""}` };

  try {
    const help = spawnSync(process.execPath, ["scripts/generate-all.mjs", "--help"], {
      cwd: checkout,
      encoding: "utf8",
      env,
    });
    assert.equal(help.status, 0, `${help.stdout}\n${help.stderr}`);

    const generated = spawnSync(process.execPath, ["scripts/generate-all.mjs", "--check"], {
      cwd: checkout,
      encoding: "utf8",
      env,
    });
    assert.equal(generated.status, 1, `${generated.stdout}\n${generated.stderr}`);
    assert.match(`${generated.stdout}\n${generated.stderr}`, /outside this checkout/u);
    assert.equal(existsSync(marker), false, "Cargo must not run before the codegen preflight");
  } finally {
    rmSync(outer, { recursive: true, force: true });
  }
});

const withStagingRoot = async (body) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-generate-all-staging-"));
  try {
    await body(root);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

const readRepoJson = (path) => JSON.parse(readFileSync(join(REPO_ROOT, path), "utf8"));

const MUTATION_RUNNER_MANIFEST = "crates/api/mutation-runner-protocol.json";
const RUNNER_PROTOCOL_FILE = "generated-protocol.mjs";

test("parseArgs reads the check and help flags", () => {
  assert.deepEqual(parseArgs([]), { check: false, help: false });
  assert.deepEqual(parseArgs(["--check"]), { check: true, help: false });
  assert.deepEqual(parseArgs(["--help"]), { check: false, help: true });
  assert.deepEqual(parseArgs(["-h", "--check"]), { check: true, help: true });
});

test("parseArgs rejects the first unknown argument", () => {
  assert.throws(() => parseArgs(["--check", "--write", "extra"]), /unknown argument: --write/u);
});

test("main prints the usage for help and does no generation", (t) => {
  const log = t.mock.method(console, "log", () => {});
  assert.equal(main(["--help"]), 0);
  assert.deepEqual(
    log.mock.calls.map((call) => call.arguments),
    [["Usage: node scripts/generate-all.mjs [--check]"]],
  );
});

test("main fails on an unknown argument", () => {
  assert.throws(() => main(["--bogus"]), /unknown argument: --bogus/u);
});

test("ensureTrailingNewline adds one newline only when it is missing", () => {
  assert.equal(ensureTrailingNewline("a"), "a\n");
  assert.equal(ensureTrailingNewline("a\n"), "a\n");
  assert.equal(ensureTrailingNewline(""), "\n");
});

test("issueRegistry orders issue types by registry index, then by id", () => {
  const schema = JSON.stringify({
    issue_types: [
      { id: "zeta" },
      { id: "beta", registry_index: 2 },
      { id: "alpha" },
      { id: "gamma", registry_index: 1 },
      { id: "delta", registry_index: 2 },
    ],
  });
  const text = issueRegistry(schema);
  assert.ok(text.endsWith("}\n"));
  const parsed = JSON.parse(text);
  assert.equal(parsed.schema_version, 1);
  assert.equal(parsed.source, "fallow schema issue_types");
  assert.deepEqual(
    parsed.issue_types.map(({ id }) => id),
    ["gamma", "beta", "delta", "alpha", "zeta"],
  );
});

test("issueRegistry gives an empty list when the schema has no issue types", () => {
  assert.deepEqual(JSON.parse(issueRegistry("{}")).issue_types, []);
});

test("jsLiteral prints scalars as JSON", () => {
  assert.equal(jsLiteral(1, 0, ""), "1");
  assert.equal(jsLiteral("a", 0, ""), '"a"');
  assert.equal(jsLiteral(null, 0, ""), "null");
});

test("jsLiteral puts objects on several lines and short scalar lists on one line", () => {
  assert.equal(jsLiteral({ a: [1, 2], b: true }, 0, ""), "{\n  a: [1, 2],\n  b: true,\n}");
  assert.equal(jsLiteral([], 0, ""), "[]");
  assert.equal(jsLiteral([{ a: 1 }], 0, ""), "[\n  {\n    a: 1,\n  },\n]");
});

test("jsLiteral breaks a scalar list that does not fit the line width", () => {
  const fits = "x".repeat(95);
  const tooLong = "x".repeat(96);
  assert.equal(jsLiteral([fits], 0, ""), `["${fits}"]`);
  assert.equal(jsLiteral([tooLong], 0, ""), `[\n  "${tooLong}",\n]`);
});

test("assertMutationRunnerManifest accepts the committed manifest", () => {
  assert.doesNotThrow(() => assertMutationRunnerManifest(readRepoJson(MUTATION_RUNNER_MANIFEST)));
});

test("assertMutationRunnerManifest rejects each invalid part", () => {
  const invalidParts = [
    { schema_version: 2 },
    { wire_protocol_version: "1" },
    { runtime_semantics_version: 1.5 },
    { session_token_env: "lower_case" },
    { session_token_env: undefined },
    { request_types: [] },
    { error_codes: ["ok", ""] },
    { runtime: { global_name: "g" } },
    { runners: [] },
    {
      runners: [
        { name: "vitest", package: "vitest", min_version: "1.0", max_version_exclusive: "2.0.0" },
      ],
    },
  ];
  for (const part of invalidParts) {
    const manifest = { ...readRepoJson(MUTATION_RUNNER_MANIFEST), ...part };
    assert.throws(
      () => assertMutationRunnerManifest(manifest),
      /invalid mutation runner protocol manifest/u,
      JSON.stringify(part),
    );
  }
});

test("generateTypeAwareProtocol writes a module that holds the manifest values", async () => {
  await withStagingRoot(async (root) => {
    generateTypeAwareProtocol(root);
    const manifest = readRepoJson("crates/api/type-aware-protocol.json");
    const modulePath = join(root, "tools/type-aware-sidecar/src/generated-protocol.mjs");
    const generated = await import(pathToFileURL(modulePath).href);
    assert.equal(generated.WIRE_PROTOCOL_VERSION, manifest.wire_protocol_version);
    assert.equal(generated.SEMANTIC_SCHEMA_VERSION, manifest.semantic_schema_version);
    assert.deepEqual([...generated.QUERY_OPERATIONS], manifest.query_operations);
    assert.deepEqual([...generated.SESSION_ENVELOPE_TYPES], manifest.session_envelope_types);
    assert.equal(generated.BACKEND_FAMILY, manifest.backend.family);
    assert.equal(generated.SIDECAR_PACKAGE, manifest.sidecar.package);
    assert.ok(Object.isFrozen(generated.TYPE_AWARE_PROTOCOL));
  });
});

test("generateMutationRunnerProtocol writes a frozen module equal to the manifest", async () => {
  await withStagingRoot(async (root) => {
    generateMutationRunnerProtocol(root);
    const manifest = readRepoJson(MUTATION_RUNNER_MANIFEST);
    const modulePath = join(root, MUTATION_COMPANION_SOURCE_ROOT, RUNNER_PROTOCOL_FILE);
    const generated = await import(pathToFileURL(modulePath).href);
    assert.deepEqual(generated.MUTATION_RUNNER_PROTOCOL, manifest);
    assert.ok(Object.isFrozen(generated.MUTATION_RUNNER_PROTOCOL.runners[0]));
    assert.equal(generated.SESSION_TOKEN_ENV, manifest.session_token_env);
    assert.equal(generated.GLOBAL_NAME, manifest.runtime.global_name);
  });
});

test("generateMutationCompanion copies each file with a header and uses the staged protocol", async () => {
  await withStagingRoot(async (root) => {
    generateMutationRunnerProtocol(root);
    generateMutationCompanion(root);
    for (const file of MUTATION_COMPANION_FILES) {
      const sourcePath = `${MUTATION_COMPANION_SOURCE_ROOT}/${file}`;
      const sourceRoot = file === RUNNER_PROTOCOL_FILE ? root : REPO_ROOT;
      const source = readFileSync(join(sourceRoot, sourcePath), "utf8");
      const copy = readFileSync(join(root, MUTATION_COMPANION_TARGET_ROOT, file), "utf8");
      assert.equal(copy, `// Generated from ${sourcePath}. Do not edit.\n${source}`);
    }
  });
});
