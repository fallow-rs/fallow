import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  symlinkSync,
  rmSync,
  chmodSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const scripts = resolve(dirname(fileURLToPath(import.meta.url)), "../scripts");
const TEST_TIMEOUT_MS = 15000;
const cleanResult = readFileSync(resolve(scripts, "../tests/fixtures/check-clean.json"), "utf8");
const fixture = (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-artifacts-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const work = join(root, "work");
  const bin = join(root, "bin");
  mkdirSync(work);
  mkdirSync(bin);
  writeFileSync(join(root, "clean.json"), cleanResult);
  writeFileSync(join(bin, "fallow"), '#!/bin/sh\ncat "$MOCK_CLEAN_RESULT"\n', { mode: 0o755 });
  writeFileSync(join(bin, "gh"), '#!/bin/sh\nprintf "diff --git a/src/a.ts b/src/a.ts\\n"\n', {
    mode: 0o755,
  });
  const run = (script, extra = {}) =>
    spawnSync("bash", [join(scripts, script)], {
      cwd: work,
      encoding: "utf8",
      timeout: TEST_TIMEOUT_MS,
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        MOCK_CLEAN_RESULT: join(root, "clean.json"),
        GITHUB_OUTPUT: join(root, "output"),
        GITHUB_ENV: join(root, "env"),
        GITHUB_STEP_SUMMARY: join(root, "summary"),
        INPUT_ROOT: ".",
        INPUT_COMMAND: "dead-code",
        INPUT_FORMAT: "json",
        ...extra,
      },
    });
  return { root, work, run };
};

test("analysis refuses a result symlink before overwriting its target", (t) => {
  const { root, work, run } = fixture(t);
  const sentinel = join(root, "sentinel");
  writeFileSync(sentinel, "keep these bytes\n");
  symlinkSync(sentinel, join(work, "fallow-results.json"));
  const result = run("analyze.sh");
  assert.equal(readFileSync(sentinel, "utf8"), "keep these bytes\n");
  assert.equal(result.status, 2, result.stdout + result.stderr);
});

test("rejected dependency spec remains one escaped warning and falls back to latest", (t) => {
  const { work, run } = fixture(t);
  writeFileSync(
    join(work, "package.json"),
    JSON.stringify({ devDependencies: { fallow: "bad%\r\n::error::injected" } }),
  );
  const result = run("install.sh", { FALLOW_INSTALL_DRY_RUN: "true", FALLOW_VERSION: "" });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.match(
    result.stdout,
    /::warning::Ignoring unsupported fallow package.json spec 'bad%25%0D%0A::error::injected'/,
  );
  assert.doesNotMatch(result.stdout, /^::error::injected/m);
  assert.match(result.stdout, /^DRY RUN: npm install -g --ignore-scripts fallow$/m);
});

const outputs = {
  "analyze.sh": [
    "fallow-results.json",
    "fallow-results-raw.json",
    "fallow-results-scoped.json",
    "fallow-results.sarif",
    "fallow-stderr.log",
    "fallow-analysis-args.sh",
    "fallow-changed-files.json",
    "fallow-pr.diff",
    "fallow-stale-baseline-gate-raw.json",
    "fallow-stale-baseline-gate.json",
    "fallow-stale-baseline-gate-stderr.log",
  ],
  "comment.sh": [
    "fallow-pr-comment.md",
    "fallow-pr-comment-envelope.json",
    "fallow-pr-decision.json",
    "fallow-pr-details.json",
    "fallow-comment-stderr.log",
    "fallow-pr-comment-plan.json",
    "fallow-check-run.json",
    "fallow-check-run-stderr.log",
  ],
  "review.sh": [
    "fallow-review.json",
    "fallow-review-stderr.log",
    "fallow-review-post.json",
    "fallow-review-post-stderr.log",
  ],
  "mutation.sh": [
    "fallow-mutation.json",
    "fallow-mutation-tasks.json",
    "fallow-mutation-status.json",
    "fallow-mutation-section.md",
    "fallow-mutation-stderr.log",
  ],
};
const postingEnv = { GH_TOKEN: "test-token", GH_REPO: "test/repo", PR_NUMBER: "1" };

for (const [script, names] of Object.entries(outputs)) {
  test(`${script} preflights every destination before writing artifacts`, (t) => {
    const { root, work, run } = fixture(t);
    const sentinel = join(root, "sentinel");
    if (script === "comment.sh" || script === "review.sh") {
      writeFileSync(join(work, "fallow-results.json"), cleanResult);
    }
    writeFileSync(sentinel, "untouched\n");
    for (const name of names) {
      symlinkSync(sentinel, join(work, name));
      const result = run(script, postingEnv);
      assert.equal(readFileSync(sentinel, "utf8"), "untouched\n", name);
      assert.equal(result.status, 2, `${name}: ${result.stdout}${result.stderr}`);
      assert.match(
        result.stderr + result.stdout,
        /::error::Artifact paths must not contain symlinks/,
      );
      rmSync(join(work, name));
    }
  });

  test(`${script} refuses a symlinked artifact parent`, (t) => {
    const { root, work, run } = fixture(t);
    const outside = join(root, "outside");
    mkdirSync(outside);
    writeFileSync(join(outside, names[0]), "keep this artifact\n");
    symlinkSync(outside, join(work, "artifacts"));
    const result = run(script, {
      ...postingEnv,
      INPUT_ARTIFACTS_DIR: "artifacts/nested",
      FALLOW_ARTIFACTS_DIR: "artifacts",
    });
    assert.equal(readFileSync(join(outside, names[0]), "utf8"), "keep this artifact\n");
    assert.equal(result.status, 2, result.stdout + result.stderr);
  });
}

for (const script of ["summary.sh", "annotate.sh"]) {
  for (const native of ["true", "false"]) {
    test(`${script} rejects a scoped-result symlink with native=${native}`, (t) => {
      const { root, work, run } = fixture(t);
      const sentinel = join(root, "sentinel");
      writeFileSync(sentinel, "keep scoped target\n");
      writeFileSync(join(work, "fallow-results.json"), '{"unused_files":[{"path":"src/a.ts"}]}');
      writeFileSync(join(work, "fallow-changed-files.json"), '["src/a.ts"]');
      symlinkSync(sentinel, join(work, "fallow-results-scoped.json"));
      const result = run(script, {
        HAS_NATIVE_REPORT: native,
        CHANGED_SINCE: "main",
        FALLOW_COMMAND: "dead-code",
        ACTION_JQ_DIR: resolve(scripts, "../jq"),
        GITHUB_STEP_SUMMARY: join(root, "summary"),
      });
      assert.equal(readFileSync(sentinel, "utf8"), "keep scoped target\n");
      assert.equal(result.status, 2, result.stdout + result.stderr);
    });
  }
}

test("analysis rejects dangling links, internal links, directories and FIFOs", (t) => {
  const { work, run } = fixture(t);
  const output = join(work, "fallow-results.json");
  for (const kind of ["dangling", "internal", "directory", "fifo"]) {
    if (kind === "dangling") symlinkSync(join(work, "missing"), output);
    if (kind === "internal") {
      writeFileSync(join(work, "source.ts"), "export const keep = true;\n");
      symlinkSync(join(work, "source.ts"), output);
    }
    if (kind === "directory") mkdirSync(output);
    if (kind === "fifo") assert.equal(spawnSync("mkfifo", [output]).status, 0);
    const result = run("analyze.sh");
    assert.equal(result.status, 2, `${kind}: ${result.stdout}${result.stderr}`);
    rmSync(output, { recursive: true });
  }
  assert.equal(readFileSync(join(work, "source.ts"), "utf8"), "export const keep = true;\n");
});

test("analysis creates and replaces regular artifacts in default and nested directories", (t) => {
  const { work, run } = fixture(t);
  for (const dir of [".", "reports/nested"]) {
    for (const iteration of [1, 2]) {
      const result = run("analyze.sh", { INPUT_ARTIFACTS_DIR: dir });
      assert.equal(result.status, 0, `${iteration}: ${result.stdout}${result.stderr}`);
      assert.equal(
        JSON.parse(readFileSync(join(work, dir, "fallow-results.json"), "utf8")).total_issues,
        0,
      );
    }
  }
});

test("invalid explicit version is escaped in both the notice and error", (t) => {
  const { run } = fixture(t);
  const result = run("install.sh", {
    FALLOW_INSTALL_DRY_RUN: "true",
    FALLOW_VERSION: "bad%\r\n::error::injected",
  });
  assert.equal(result.status, 2);
  assert.match(result.stdout, /::error::Invalid version specifier: bad%25%0D%0A::error::injected/);
  assert.doesNotMatch(result.stdout, /^::error::injected/m);
  assert.doesNotMatch(result.stdout, /DRY RUN: npm install/);
});

test("mutation refuses an unsafe output even when it contains an old successful status", (t) => {
  const { work, run } = fixture(t);
  mkdirSync(join(work, "outside"));
  writeFileSync(
    join(work, "outside/fallow-mutation-status.json"),
    '{"outcome":"ok","completion":"complete","clean":true}',
  );
  symlinkSync(join(work, "outside"), join(work, "artifacts"));
  const result = run("mutation.sh", { FALLOW_ARTIFACTS_DIR: "artifacts" });
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.doesNotMatch(result.stdout, /Mutation testing: ok/);
});

test("analysis refuses unwritable destinations and uncreatable parents", (t) => {
  const { work, run } = fixture(t);
  const output = join(work, "fallow-results.json");
  writeFileSync(output, "keep read-only output\n");
  chmodSync(output, 0o444);
  try {
    const result = run("analyze.sh");
    assert.equal(result.status, 2, result.stdout + result.stderr);
    assert.equal(readFileSync(output, "utf8"), "keep read-only output\n");
  } finally {
    chmodSync(output, 0o644);
  }
  writeFileSync(join(work, "blocked"), "keep parent file\n");
  const result = run("analyze.sh", { INPUT_ARTIFACTS_DIR: "blocked/nested" });
  assert.equal(result.status, 2, result.stdout + result.stderr);
  assert.equal(readFileSync(join(work, "blocked"), "utf8"), "keep parent file\n");
});
