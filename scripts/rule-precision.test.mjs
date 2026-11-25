import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  buildRow,
  combineReviews,
  findingAnchor,
  findingsByRule,
  ledgerErrors,
  renderSummary,
  ruleIdFromKey,
  seededSample,
  clopperPearson,
  stratifiedPrecision,
  upsertRow,
} from "./rule-precision/ledger.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SHA = "a".repeat(40);
const finding = (id, extra = {}) => ({ finding_id: id, path: "src/a.ts", line: 3, ...extra });
const corpus = { app: { repo: "owner/app", commit: SHA } };
const row = (projects) => ({
  rule: "unused-export",
  command: "dead-code",
  fallow_version: "3.48.0",
  reviewed_on: "2026-11-25",
  sample_size: 30,
  reviews: 1,
  agreement_percent: null,
  projects,
  noise_classes: [],
});
const counts = (c) => ({
  project: "app",
  commit: SHA,
  findings: 40,
  sampled: 30,
  real: 20,
  noise: 8,
  unsure: 2,
  ...c,
});

test("report keys become rule ids", () => {
  assert.equal(ruleIdFromKey("unused_dependencies"), "unused-dependency");
  assert.equal(ruleIdFromKey("unused_exports"), "unused-export");
  assert.equal(ruleIdFromKey("re_export_cycles"), "re-export-cycle");
  assert.equal(ruleIdFromKey("unlisted_binaries"), "unlisted-binary");
  assert.equal(ruleIdFromKey("duplicate_export_aliases"), "duplicate-export-alias");
});

test("only findings count, and each finding ID names its rule", () => {
  const rules = findingsByRule({
    version: "3.48.0",
    unused_exports: [finding("dc1:unused-export:1")],
    unused_files: [],
    entry_points: [{ path: "src/main.ts" }],
    next_steps: ["run fix"],
    deprecated_exports_in_use: [finding("dc1:deprecated-export-in-use:2")],
    stale_suppressions: [
      finding("dc1:stale-suppression:3"),
      finding("dc1:missing-suppression-reason:4", { missing_reason: true }),
    ],
  });
  assert.deepEqual(
    [...rules.keys()],
    [
      "unused-export",
      "deprecated-export-in-use",
      "stale-suppression",
      "missing-suppression-reason",
    ],
  );
  assert.deepEqual(
    rules.get("stale-suppression").map((f) => f.finding_id),
    ["dc1:stale-suppression:3"],
  );
});

test("the anchor falls back to the first nested location", () => {
  assert.deepEqual(findingAnchor(finding("x")), { path: "src/a.ts", line: 3 });
  assert.deepEqual(
    findingAnchor({ finding_id: "d", imported_from: [{ path: "src/b.ts", line: 9 }] }),
    { path: "src/b.ts", line: 9 },
  );
  assert.deepEqual(findingAnchor({ finding_id: "c", files: ["src/c.ts"] }), {
    path: "src/c.ts",
    line: null,
  });
  assert.deepEqual(
    findingAnchor({ finding_id: "b", used_in: [{ path: "package.json", line: 12 }] }),
    { path: "package.json", line: 12 },
  );
  assert.deepEqual(
    findingAnchor({ finding_id: "p", edges: [{ path: "packages/a/package.json", line: 4 }] }),
    { path: "packages/a/package.json", line: 4 },
  );
  assert.deepEqual(findingAnchor({ finding_id: "v", from_path: "src/ui/a.ts", line: 2 }), {
    path: "src/ui/a.ts",
    line: 2,
  });
});

test("the sample is deterministic and does not depend on input order", () => {
  const all = Array.from({ length: 50 }, (_, i) => finding(`f${i}`));
  const first = seededSample(all, 10, "unused-export:app:sha");
  const again = seededSample(all.toReversed(), 10, "unused-export:app:sha");
  assert.deepEqual(first, again);
  assert.equal(new Set(first.map((f) => f.finding_id)).size, 10);
  assert.notDeepEqual(first, seededSample(all, 10, "unused-file:app:sha"));
  assert.equal(seededSample(all.slice(0, 4), 10, "s").length, 4);
});

const stratum = (project, findings, sampled, real, noise) => ({
  project,
  findings,
  sampled,
  real,
  noise,
  unsure: sampled - real - noise,
});

test("the estimate weights each project by its count of findings", () => {
  const { result } = stratifiedPrecision([
    stratum("small", 30, 30, 30, 0),
    stratum("large", 3000, 30, 0, 30),
  ]);
  assert.deepEqual(result, { estimate: 1, low: 1, high: 14 });
});

test("the exact interval matches its reference values", () => {
  const seven = clopperPearson(7, 10, 0.05);
  assert.equal(seven.low.toFixed(4), "0.3475");
  assert.equal(seven.high.toFixed(4), "0.9333");
  assert.deepEqual(clopperPearson(0, 10, 0.05).low, 0);
  assert.deepEqual(clopperPearson(10, 10, 0.05).high, 1);
});

test("a small or fully reviewed sample still has a range", () => {
  assert.deepEqual(stratifiedPrecision([stratum("a", 10, 10, 7, 3)]).result, {
    estimate: 70,
    low: 35,
    high: 93,
  });
  assert.deepEqual(stratifiedPrecision([stratum("a", 40, 30, 20, 8)]).result, {
    estimate: 71,
    low: 51,
    high: 87,
  });
  const allReal = stratifiedPrecision([stratum("a", 600, 30, 30, 0), stratum("b", 70, 30, 30, 0)]);
  assert.equal(allReal.result.estimate, 100);
  assert.equal(allReal.result.high, 100);
});

// Sums the probability of every outcome whose range contains the true
// precision, for `projects` equal projects with `n` verdicts each.
const exactCoverage = (n, projects, truth) => {
  const pmf = (x) => {
    let ways = 1;
    for (let i = 0; i < x; i += 1) ways = (ways * (n - i)) / (i + 1);
    return ways * truth ** x * (1 - truth) ** (n - x);
  };
  const walk = (reals, weight) => {
    if (reals.length === projects) {
      const { result } = stratifiedPrecision(
        reals.map((x, i) => stratum(`p${i}`, 100, n, x, n - x)),
      );
      return result.low <= truth * 100 && result.high >= truth * 100 ? weight : 0;
    }
    let sum = 0;
    for (let x = 0; x <= n; x += 1) sum += walk([...reals, x], weight * pmf(x));
    return sum;
  };
  return walk([], 1);
};

test("the range covers the true precision in at least 95 percent of samples", () => {
  for (const truth of [0.5, 0.7, 0.95]) {
    const one = exactCoverage(30, 1, truth);
    assert.ok(one >= 0.95, `one project at ${truth}: ${one}`);
  }
  const four = exactCoverage(3, 4, 0.95);
  assert.ok(four >= 0.95, `four projects: ${four}`);
});

test("a project with findings but no verdict makes the estimate unavailable", () => {
  const value = stratifiedPrecision([
    stratum("small", 30, 30, 30, 0),
    stratum("large", 3000, 30, 0, 0),
  ]);
  assert.deepEqual(value, { result: null, unjudged: ["large"] });
  const text = renderSummary({
    schema_version: 1,
    corpus,
    rows: [row([counts({ real: 0, noise: 0, unsure: 30 })])],
  });
  assert.match(text, /\| n\/a \(no verdict in app\) \|/);
});

const sample = {
  rule: "unused-export",
  command: "dead-code",
  fallow_version: "3.48.0",
  sample_size: 30,
  projects: [{ project: "app", commit: SHA, findings: 5, sampled_ids: ["a", "b", "c"] }],
};

test("verdicts become a row with counts and noise classes", () => {
  const result = buildRow({
    sample,
    verdicts: {
      reviewed_on: "2026-11-25",
      verdicts: [
        { project: "app", id: "a", verdict: "real" },
        { project: "app", id: "b", verdict: "noise", class: "framework convention" },
        { project: "app", id: "c", verdict: "unsure" },
      ],
    },
  });
  assert.equal(result.ok, true);
  assert.deepEqual(result.row.projects, [
    { project: "app", commit: SHA, findings: 5, sampled: 3, real: 1, noise: 1, unsure: 1 },
  ]);
  assert.deepEqual(result.row.noise_classes, [{ class: "framework convention", count: 1 }]);
});

test("a missing, extra or duplicate verdict is an error", () => {
  const result = buildRow({
    sample,
    verdicts: {
      reviewed_on: "2026-11-25",
      verdicts: [
        { project: "app", id: "a", verdict: "real" },
        { project: "app", id: "a", verdict: "real" },
        { project: "app", id: "b", verdict: "real" },
        { project: "app", id: "z", verdict: "real" },
      ],
    },
  });
  assert.equal(result.ok, false);
  assert.deepEqual(result.errors.toSorted(), [
    "app a: two verdicts",
    "app c: no verdict",
    "app z: not in the sample",
  ]);
});

test("an unknown verdict and a noise verdict without a class are errors", () => {
  const result = buildRow({
    sample,
    verdicts: {
      reviewed_on: "2026-11-25",
      verdicts: [
        { project: "app", id: "a", verdict: "maybe" },
        { project: "app", id: "b", verdict: "noise" },
        { project: "app", id: "c", verdict: "real" },
      ],
    },
  });
  assert.deepEqual(result.errors, [
    'app a: unknown verdict "maybe"',
    "app b: a noise verdict needs a class",
  ]);
});

test("a sample that lists one project twice is an error", () => {
  const twice = { ...sample, projects: [...sample.projects, ...sample.projects] };
  const result = buildRow({ sample: twice, verdicts: { verdicts: [] } });
  assert.deepEqual(result.errors, ["the sample lists project app twice"]);
});

test("upsert replaces the row of the same rule and command", () => {
  const ledger = { schema_version: 1, corpus, rows: [row([counts()])] };
  const next = upsertRow(ledger, { ...row([counts({ real: 28, noise: 0 })]) });
  assert.equal(next.rows.length, 1);
  assert.equal(next.rows[0].projects[0].real, 28);
});

test("a row measured at another commit than the corpus pin is an error", () => {
  const errors = ledgerErrors({
    schema_version: 1,
    corpus,
    rows: [row([counts({ commit: "b".repeat(40) })])],
  });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /app: measured at b{40}, but the corpus pins a{40}/);
  assert.deepEqual(ledgerErrors({ schema_version: 1 }), [
    "the ledger needs a corpus object and a rows array",
  ]);
  assert.deepEqual(ledgerErrors({ schema_version: 1, corpus: {}, rows: [] }), [
    "the corpus needs at least one project",
  ]);
});

test("ledger validation catches bad counts and unknown projects", () => {
  assert.deepEqual(ledgerErrors({ schema_version: 1, corpus, rows: [row([counts()])] }), []);
  const errors = ledgerErrors({
    schema_version: 1,
    corpus: { ...corpus, lib: { repo: "owner/lib", commit: "main" } },
    rows: [row([counts({ real: 21 }), counts({ project: "ghost", sampled: 50, real: 40 })])],
  });
  assert.deepEqual(errors, [
    "corpus lib: commit must be a full SHA",
    "dead-code unused-export app: real + noise + unsure must equal sampled",
    "dead-code unused-export ghost: project is not in the corpus",
    "dead-code unused-export ghost: sampled exceeds findings",
    "dead-code unused-export ghost: sampled exceeds sample_size",
  ]);
});

test("the summary shows precision with its interval and leaves unsure out", () => {
  const text = renderSummary({ schema_version: 1, corpus, rows: [row([counts()])] });
  assert.match(
    text,
    /\| `dead-code` \| `unused-export` \| 40 \| 30 \| 20 \| 8 \| 2 \| 71% \(51 to 87\) \| 1 \|/,
  );
  const twice = renderSummary({
    schema_version: 1,
    corpus,
    rows: [{ ...row([counts()]), reviews: 3, agreement_percent: 83 }],
  });
  assert.match(twice, /\| 3 \(83% agree\) \|/);
  assert.match(text, /\| app \| owner\/app \| `aaaaaaaaaaaa` \|/);
});

test("the tracked ledger is valid and its summary is current", () => {
  const ledger = JSON.parse(
    readFileSync(resolve(REPO_ROOT, "benchmarks/rule-precision.json"), "utf8"),
  );
  assert.deepEqual(ledgerErrors(ledger), []);
  const summary = readFileSync(resolve(REPO_ROOT, "benchmarks/rule-precision-summary.md"), "utf8");
  assert.equal(summary, renderSummary(ledger));
});

const review = (pairs) => ({
  reviewed_on: "2026-11-25",
  verdicts: pairs.map(([id, verdict, cls]) => ({
    project: "app",
    id,
    verdict,
    ...(cls && { class: cls }),
  })),
});

test("one review passes through without an agreement rate", () => {
  const only = review([["a", "real"]]);
  assert.deepEqual(combineReviews([only]), {
    ok: true,
    verdicts: only,
    agreement: null,
    reviews: 1,
  });
});

test("a later review settles a disagreement by majority", () => {
  const result = combineReviews([
    review([
      ["a", "real"],
      ["b", "noise", "dynamic import"],
      ["c", "real"],
      ["d", "real"],
    ]),
    review([
      ["a", "real"],
      ["b", "real"],
      ["c", "noise", "framework entry"],
      ["d", "real"],
    ]),
    review([
      ["b", "noise", "dynamic import"],
      ["c", "unsure"],
    ]),
  ]);
  assert.equal(result.ok, true);
  assert.equal(result.agreement, 50);
  assert.equal(result.reviews, 3);
  const byId = Object.fromEntries(result.verdicts.verdicts.map((v) => [v.id, v]));
  assert.equal(byId.b.verdict, "noise");
  assert.equal(byId.b.class, "dynamic import");
  assert.equal(byId.c.verdict, "unsure");
  assert.equal(byId.d.verdict, "real");
});

test("a disagreement without a settling review is an error", () => {
  const result = combineReviews([review([["a", "real"]]), review([["a", "noise", "x"]])]);
  assert.equal(result.ok, false);
  assert.match(result.errors[0], /app a: the first two reviews disagree/);
});

test("a finding missing from one of the first two reviews is an error", () => {
  const result = combineReviews([
    review([
      ["a", "real"],
      ["b", "real"],
    ]),
    review([["a", "real"]]),
  ]);
  assert.equal(result.ok, false);
  assert.match(result.errors[0], /app b: missing from one of the first two reviews/);
});

test("a row with two reviews needs an agreement rate", () => {
  const errors = ledgerErrors({
    schema_version: 1,
    corpus,
    rows: [{ ...row([counts()]), reviews: 2 }],
  });
  assert.deepEqual(errors, [
    "dead-code unused-export: agreement_percent must be null for one review, else 0 to 100",
  ]);
});

test("an invalid review is an error, not a crash or a vote", () => {
  const extra = combineReviews([
    review([["x", "real"]]),
    review([["x", "real"]]),
    review([["extra", "noise", "y"]]),
  ]);
  assert.deepEqual(extra.errors, ["app extra: missing from one of the first two reviews"]);
  const typo = combineReviews([
    review([["x", "rael"]]),
    review([["x", "real"]]),
    review([["x", "real"]]),
  ]);
  assert.deepEqual(typo.errors, ['app x: unknown verdict "rael"']);
  assert.deepEqual(combineReviews([review([]), review([])]).errors, [
    "the reviews have no verdicts",
  ]);
});

// The CLI tests run `sample` against a local bare repository and a stand-in
// fallow binary, so they need git but no network.
const CLI = resolve(REPO_ROOT, "scripts/rule-precision.mjs");
const run = (args, env = {}) =>
  spawnSync(process.execPath, [CLI, ...args], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });
const gitIn = (cwd, args) => {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
};

const makeFixture = () => {
  const root = mkdtempSync(join(tmpdir(), "rule-precision-"));
  const source = join(root, "source");
  mkdirSync(join(source, "src"), { recursive: true });
  writeFileSync(join(source, "src", "a.ts"), "export const a = 1;\n");
  gitIn(source, ["init", "-q"]);
  gitIn(source, ["-c", "user.email=t@t", "-c", "user.name=t", "add", "."]);
  gitIn(source, ["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-q", "-m", "x"]);
  const commit = gitIn(source, ["rev-parse", "HEAD"]);
  const bare = join(root, "remotes", "owner", "app.git");
  mkdirSync(dirname(bare), { recursive: true });
  gitIn(root, ["clone", "-q", "--bare", source, bare]);
  gitIn(bare, ["config", "uploadpack.allowAnySHA1InWant", "true"]);
  const fake = join(root, "fake-fallow.mjs");
  const report = {
    unused_exports: [{ finding_id: "dc1:unused-export:1", path: "src/a.ts", line: 1 }],
  };
  writeFileSync(
    fake,
    `#!/usr/bin/env node
const args = process.argv.slice(2);
if (args[0] === "--version") console.log("fallow 9.9.9");
else if (args[0] === "explain") console.log("Unused exports");
else if (process.env.FAKE_BAD_JSON) console.log("not json");
else { console.log(${JSON.stringify(JSON.stringify(report))}); process.exit(1); }
`,
  );
  chmodSync(fake, 0o755);
  const ledgerCorpus = { app: { repo: "owner/app", commit } };
  return {
    root,
    commit,
    fake,
    ledgerCorpus,
    env: { RULE_PRECISION_GIT_BASE: `${join(root, "remotes")}/` },
  };
};

// The CLI reads the corpus from the tracked ledger, so these tests use a
// copy of the script with its own ledger next to it.
const withLedger = (fixture) => {
  const scripts = join(fixture.root, "repo", "scripts");
  mkdirSync(join(scripts, "rule-precision"), { recursive: true });
  mkdirSync(join(fixture.root, "repo", "benchmarks"), { recursive: true });
  writeFileSync(join(scripts, "rule-precision.mjs"), readFileSync(CLI));
  writeFileSync(
    join(scripts, "rule-precision", "ledger.mjs"),
    readFileSync(resolve(REPO_ROOT, "scripts/rule-precision/ledger.mjs")),
  );
  writeFileSync(
    join(fixture.root, "repo", "benchmarks", "rule-precision.json"),
    JSON.stringify({ schema_version: 1, corpus: fixture.ledgerCorpus, rows: [] }),
  );
  const cli = join(scripts, "rule-precision.mjs");
  return (args, env = {}) =>
    spawnSync(process.execPath, [cli, ...args], {
      encoding: "utf8",
      env: { ...process.env, ...fixture.env, ...env },
    });
};

test("sample works from an empty corpus folder and again on a second run", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    const corpusDir = join(fixture.root, "corpus");
    const args = (out) => [
      "sample",
      "--fallow-bin",
      fixture.fake,
      "--corpus-dir",
      corpusDir,
      "--out",
      out,
    ];
    const first = cli(args(join(fixture.root, "out1")));
    assert.equal(first.status, 0, first.stderr);
    const manifest = JSON.parse(
      readFileSync(join(fixture.root, "out1", "unused-export", "sample.json"), "utf8"),
    );
    assert.equal(manifest.projects[0].commit, fixture.commit);
    const second = cli(args(join(fixture.root, "out2")));
    assert.equal(second.status, 0, second.stderr);
    const reused = cli(args(join(fixture.root, "out2")));
    assert.equal(reused.status, 2);
    assert.match(reused.stderr, /is not empty/);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("sample refuses a plain folder inside another repository", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    const parent = join(fixture.root, "parent");
    mkdirSync(join(parent, "corpus", "app"), { recursive: true });
    gitIn(parent, ["init", "-q"]);
    const result = cli([
      "sample",
      "--fallow-bin",
      fixture.fake,
      "--corpus-dir",
      join(parent, "corpus"),
      "--out",
      join(fixture.root, "out"),
    ]);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /is not its own git checkout/);
    assert.equal(
      spawnSync("git", ["rev-parse", "--verify", "-q", "HEAD"], { cwd: parent }).status,
      1,
    );
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("sample refuses a checkout with local changes and a bad sample size", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    const corpusDir = join(fixture.root, "corpus");
    const base = ["sample", "--fallow-bin", fixture.fake, "--corpus-dir", corpusDir];
    assert.equal(cli([...base, "--out", join(fixture.root, "o1")]).status, 0);
    writeFileSync(join(corpusDir, "app", "src", "scratch.ts"), "export const s = 1;\n");
    const untracked = cli([...base, "--out", join(fixture.root, "o2a")]);
    assert.equal(untracked.status, 2);
    assert.match(untracked.stderr, /untracked files/);
    rmSync(join(corpusDir, "app", "src", "scratch.ts"));
    writeFileSync(join(corpusDir, "app", "src", "a.ts"), "export const a = 2;\n");
    const dirty = cli([...base, "--out", join(fixture.root, "o2")]);
    assert.equal(dirty.status, 2);
    assert.match(dirty.stderr, /local changes/);
    const size = cli([...base, "--out", join(fixture.root, "o3"), "--sample-size=-1"]);
    assert.equal(size.status, 2);
    assert.match(size.stderr, /--sample-size/);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("analyzer output that is not JSON is an error with exit code 2", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    const result = cli(
      [
        "sample",
        "--fallow-bin",
        fixture.fake,
        "--corpus-dir",
        join(fixture.root, "c"),
        "--out",
        join(fixture.root, "o"),
      ],
      { FAKE_BAD_JSON: "1" },
    );
    assert.equal(result.status, 2);
    assert.match(result.stderr, /the output of fallow dead-code in .* is not valid JSON/);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("an unknown option is an error with exit code 2", () => {
  const result = run(["summary", "--no-such-option"]);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /no-such-option/);
});

test("a failed install removes an earlier install marker", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    const corpusDir = join(fixture.root, "corpus");
    const base = ["sample", "--fallow-bin", fixture.fake, "--corpus-dir", corpusDir];
    assert.equal(cli([...base, "--out", join(fixture.root, "o1")]).status, 0);
    const marker = join(corpusDir, "app", ".git", "rule-precision-installed");
    assert.equal(readFileSync(marker, "utf8").trim(), fixture.commit);
    // A lockfile makes the next run install. The stand-in installer fails.
    writeFileSync(marker, "0".repeat(40));
    const bin = join(fixture.root, "bin");
    mkdirSync(bin);
    writeFileSync(join(bin, "pnpm"), "#!/bin/sh\nexit 9\n");
    chmodSync(join(bin, "pnpm"), 0o755);
    const lockRepo = join(corpusDir, "app");
    writeFileSync(join(lockRepo, ".git", "info", "exclude"), "pnpm-lock.yaml\n");
    writeFileSync(join(lockRepo, "pnpm-lock.yaml"), "lockfileVersion: 9\n");
    const failed = cli([...base, "--out", join(fixture.root, "o2")], {
      PATH: `${bin}:${process.env.PATH}`,
    });
    assert.equal(failed.status, 2);
    assert.match(failed.stderr, /pnpm install .* exited with 9/);
    assert.equal(spawnSync("test", ["-e", marker]).status, 1);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("check refuses a ledger without corpus and rows, with exit code 2", () => {
  const fixture = makeFixture();
  try {
    const cli = withLedger(fixture);
    writeFileSync(
      join(fixture.root, "repo", "benchmarks", "rule-precision.json"),
      '{"schema_version":1}',
    );
    const result = cli(["check"]);
    assert.equal(result.status, 2);
    assert.match(result.stderr, /needs a corpus object and a rows array/);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("record reports a missing verdict file with exit code 2", () => {
  const result = run(["record", "--in", "/nonexistent", "--verdicts", "/nonexistent/v.json"]);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /does not exist/);
});
