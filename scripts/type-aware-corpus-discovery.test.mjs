import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { summaryChecks, zeroControlProjectsClean } from "./type-aware-corpus/summary.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const source = (member) => ({
  path: "src/service.ts",
  parent_name: "Service",
  member_name: member,
  kind: "class_method",
  line: member === "used" ? 2 : 3,
  col: 3,
});
const decision = (member, outcome) => ({
  subject: {
    path: "src/service.ts",
    owner: "Service",
    local_name: member,
    declaration_kind: "class_method",
    line: member === "used" ? 2 : 3,
    col: 3,
  },
  decision: outcome,
  status: "complete",
  owning_projects: ["tsconfig.json"],
  reason_code: null,
  closed_world_eligible: false,
});
const metadata = (confirmed, promoted) => ({
  candidate_count: Number(confirmed) + Number(promoted),
  confirmed_used_count: Number(confirmed),
  contract_preserved_count: 0,
  no_static_references_count: Number(promoted),
  fix_eligible_count: 0,
  unresolved_count: 0,
  abstained_count: 0,
  abstention_reasons: {
    no_project: 0,
    ambiguous_project: 0,
    blocking_diagnostics: 0,
    svelte_virtual_module_exports: 0,
    unknown_symbol: 0,
    unsupported_syntax: 0,
    capacity: 0,
  },
  candidate_decisions: [
    ...(confirmed ? [decision("used", "confirmed-used")] : []),
    ...(promoted ? [decision("unused", "confirmed-no-static-references")] : []),
  ],
});
const run = (binary, args, cwd) =>
  spawnSync(binary, args, { cwd, encoding: "utf8", timeout: 30_000 });
const requireSuccess = (result) =>
  assert.equal(result.status, 0, `${result.stdout}\n${result.stderr}`);

const sandbox = (t) => {
  const root = realpathSync(mkdtempSync(resolve(tmpdir(), "fallow-corpus-discovery-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  for (const path of [
    "scripts/type-aware-corpus.mjs",
    "scripts/type-aware-corpus",
    "scripts/cargo-target.mjs",
    "tools/type-aware-sidecar/src/generated-protocol.mjs",
  ]) {
    const target = resolve(root, path);
    mkdirSync(dirname(target), { recursive: true });
    cpSync(resolve(REPO_ROOT, path), target, { recursive: true });
  }
  mkdirSync(resolve(root, "node_modules"));
  writeFileSync(resolve(root, "pnpm-lock.yaml"), "lockfileVersion: '9.0'\n");
  const manifest = JSON.parse(
    readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-corpus.json")),
  );
  for (const project of manifest.projects.filter(({ id }) => ["svelte", "vue-core"].includes(id))) {
    const fixture = resolve(root, project.fixture);
    mkdirSync(resolve(fixture, "src"), { recursive: true });
    writeFileSync(
      resolve(fixture, "src/service.ts"),
      "export class Service {\n  used() {}\n  unused() {}\n}\n",
    );
    requireSuccess(run("git", ["init", "--quiet", fixture], root));
    requireSuccess(run("git", ["add", "src/service.ts"], fixture));
    requireSuccess(
      run(
        "git",
        [
          "-c",
          "user.name=Corpus Test",
          "-c",
          "user.email=corpus@example.invalid",
          "-c",
          "commit.gpgsign=false",
          "-c",
          "core.hooksPath=/dev/null",
          "commit",
          "--quiet",
          "-m",
          "test: fixture",
        ],
        fixture,
      ),
    );
    project.ref = "HEAD";
  }
  const manifestPath = resolve(root, "manifest.json");
  writeFileSync(manifestPath, JSON.stringify(manifest));
  const binary = resolve(root, "fallow.mjs");
  writeFileSync(
    binary,
    `#!${process.execPath}\nimport { readFileSync } from "node:fs";\nif (process.argv.includes("--version")) console.log("fallow test boundary");\nelse {\n const payload = JSON.parse(readFileSync(new URL("./payload.json", import.meta.url)));\n console.log(JSON.stringify(payload[process.argv.includes("--type-aware") ? "refined" : "baseline"]));\n}\n`,
    { mode: 0o755 },
  );
  const sidecar = resolve(root, "sidecar.mjs");
  writeFileSync(sidecar, "process.exitCode = 99;\n");
  return { root, binary, sidecar, manifestPath };
};

const discover = (context, project, name, confirmed, promoted) => {
  const { root, binary, sidecar, manifestPath } = context;
  const out = resolve(root, name);
  const shared = ["--manifest", manifestPath, "--out-dir", out, "--project", project];
  const script = resolve(root, "scripts/type-aware-corpus.mjs");
  requireSuccess(run(process.execPath, [script, "prepare", ...shared], root));
  writeFileSync(
    resolve(root, "payload.json"),
    JSON.stringify({
      baseline: { unused_class_members: confirmed ? [source("used")] : [] },
      refined: {
        unused_class_members: promoted ? [source("unused")] : [],
        _meta: { type_aware: metadata(confirmed, promoted) },
      },
    }),
  );
  const result = run(
    process.execPath,
    [script, "discover", ...shared, "--fallow-bin", binary, "--sidecar-bin", sidecar],
    root,
  );
  return { result, artifact: resolve(out, "discovery.json") };
};

test(
  "discovery rejects promotions on zero-controls and persists accuracy-core cohorts",
  { skip: process.platform === "win32" },
  (t) => {
    const context = sandbox(t);
    const clean = discover(context, "vue-core", "clean", false, false);
    requireSuccess(clean.result);
    const cleanProject = JSON.parse(readFileSync(clean.artifact)).projects[0];
    assert.equal(cleanProject.baseline_candidate_count, 0);
    assert.equal(cleanProject.refined_candidate_count, 0);
    assert.deepEqual(cleanProject.semantic_only_promotions, []);

    const accuracy = discover(context, "svelte", "accuracy", true, true);
    requireSuccess(accuracy.result);
    const project = JSON.parse(readFileSync(accuracy.artifact)).projects[0];
    assert.equal(project.baseline_candidate_count, 1);
    assert.equal(project.type_aware.candidate_count, 2);
    assert.equal(project.baseline_type_aware.confirmed_used_count, 1);
    assert.equal(project.baseline_type_aware.no_static_references_count, 0);
    assert.deepEqual(
      project.candidates.map(({ member_name }) => member_name),
      ["used"],
    );
    assert.deepEqual(
      project.semantic_only_promotions.map(({ member_name }) => member_name),
      ["unused"],
    );

    const invalid = discover(context, "vue-core", "promoted-control", false, true);
    assert.notEqual(
      invalid.result.status,
      0,
      "a zero-control with a complete promotion must fail discovery",
    );
    assert.match(invalid.result.stderr, /zero-candidate control.*refined/);
    assert.equal(existsSync(invalid.artifact), false);
  },
);

test("summary gate rejects a promoted zero-control while allowing accuracy-core promotions", () => {
  const gate = (role, baseline, refined) =>
    summaryChecks({
      zeroControlsClean: zeroControlProjectsClean([
        { role, baseline_candidate_count: baseline, refined_candidate_count: refined },
      ]),
      coverage: {},
      featureBucketValue: {},
      thresholds: {},
      isObject: (value) => value !== null && typeof value === "object",
    }).zero_controls_clean;
  assert.equal(gate("zero-control", 0, 0), true);
  assert.equal(gate("accuracy-core", 1, 1), true);
  assert.equal(gate("zero-control", 0, 1), false);
  assert.equal(gate("zero-control", 1, 0), false);
});
