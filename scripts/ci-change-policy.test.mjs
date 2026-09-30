import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { test } from "node:test";
import { mainChecksRequired } from "./ci-change-policy.mjs";

const prose = "docs/development/ai-tooling.md";
const event = { event_name: "push", ref: "refs/heads/main", subject: "docs: clarify gates" };
const decide = (diff, overrides = {}) => mainChecksRequired({ ...event, diff, ...overrides });

test("only modified allowlisted maintainer prose can skip ordinary main checks", () => {
  assert.equal(decide(`M\0${prose}\0`), false);
  for (const path of [
    "crates/cli/src/lib.rs",
    "docs/backwards-compatibility.md",
    "docs/output-schema.json",
    "schema.json",
    ".github/workflows/ci.yml",
    "scripts/ci-change-policy.mjs",
    "docs/new-guide.md",
    "docs/development/quality-gates.md",
    "unknown",
  ]) {
    assert.equal(decide(`M\0${prose}\0M\0${path}\0`), true, path);
  }
});

test("release pushes, manual coverage and missing or failed detection run full", () => {
  assert.equal(decide(`M\0${prose}\0`, { subject: "chore: release v1.2.3" }), true);
  assert.equal(decide(`M\0${prose}\0`, { event_name: "workflow_dispatch" }), true);
  for (const diff of [null, "", "M\0", `M\0${prose}`, `?\0${prose}\0`])
    assert.equal(decide(diff), true);
  assert.equal(decide(`M\0${prose}\0`, { subject: null }), true);
  assert.equal(decide(`M\0${prose}\0`, { ref: "refs/heads/other" }), true);
});

test("additions, deletions and renames require full checks", () => {
  for (const diff of [
    `A\0${prose}\0`,
    `D\0${prose}\0`,
    `R100\0${prose}\0docs/development/review-routing.md\0`,
    `D\0crates/cli/src/lib.rs\0A\0${prose}\0`,
  ])
    assert.equal(decide(diff), true);
});

test("the executable reads the entire pushed range and falls back on invalid refs", (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-ci-change-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
  git("init", "-q");
  git("config", "user.email", "test@example.com");
  git("config", "user.name", "Test");
  const commit = (subject) => {
    git("add", ".");
    git("-c", "commit.gpgsign=false", "commit", "-qm", subject);
    return git("rev-parse", "HEAD");
  };
  mkdirSync(join(root, "docs/development"), { recursive: true });
  writeFileSync(join(root, prose), "base prose");
  writeFileSync(join(root, "runtime.rs"), "base");
  const before = commit("base");
  writeFileSync(join(root, prose), "updated prose");
  const proseOnly = commit("docs: first guide change");
  writeFileSync(join(root, "runtime.rs"), "changed");
  const runtime = commit("runtime change");
  writeFileSync(join(root, prose), "more prose");
  const after = commit("docs: last commit");
  const eventPath = join(root, "event.json");
  const outputPath = join(root, "output");
  const script = join(process.cwd(), "scripts/ci-change-policy.mjs");
  const run = (base, head = after) => {
    writeFileSync(
      eventPath,
      JSON.stringify({ before: base, after: head, head_commit: { message: "docs: last commit" } }),
    );
    return execFileSync(process.execPath, [script], {
      cwd: root,
      encoding: "utf8",
      env: {
        ...process.env,
        GITHUB_EVENT_NAME: "push",
        GITHUB_REF: "refs/heads/main",
        GITHUB_SHA: head,
        GITHUB_EVENT_PATH: eventPath,
        GITHUB_OUTPUT: outputPath,
      },
    });
  };
  assert.match(run(runtime), /main-full=false/, "last commit is eligible prose");
  assert.match(run(proseOnly), /main-full=true/, "pushed range includes earlier runtime change");
  assert.match(run(before), /main-full=true/);
  assert.match(run("0000000000000000000000000000000000000000"), /main-full=true/);
  assert.match(run("f".repeat(40)), /main-full=true/);
  writeFileSync(join(root, prose), "release prose");
  const release = commit("chore: release v1.2.3");
  assert.match(run(after, release), /main-full=true/, "actual release subject forces full checks");
});
