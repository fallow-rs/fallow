// Tests for check-pr-deferrals.mjs, the pull request description check in
// commitlint.yml.
//
// Run: node --test .github/scripts/check-pr-deferrals.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { checkPrDeferrals } from "./check-pr-deferrals.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./check-pr-deferrals.mjs", import.meta.url));

test("a description without deferrals passes", () => {
  assert.deepEqual(checkPrDeferrals("## What\n\nFix the parser.\n\n## Test plan\n\n- cargo test"), {
    ok: true,
  });
  assert.deepEqual(checkPrDeferrals(""), { ok: true });
  assert.deepEqual(checkPrDeferrals(undefined), { ok: true });
});

test("an inline 'separate change' without an issue fails", () => {
  const body =
    "Keeps every source map.\n\nThe cloud side (Node V8 absolute paths) is a separate change.";
  const result = checkPrDeferrals(body);
  assert.equal(result.ok, false);
  assert.deepEqual(result.untracked, [
    "The cloud side (Node V8 absolute paths) is a separate change.",
  ]);
});

test("an inline deferral with an issue link passes", () => {
  for (const ref of [
    "#3320",
    "fallow-rs/fallow-cloud#41",
    "https://github.com/fallow-rs/fallow/issues/3320",
  ]) {
    const body = `A regular expression alias find is a separate change (${ref}).`;
    assert.deepEqual(checkPrDeferrals(body), { ok: true }, ref);
  }
});

test("each item under a 'Not in this change' heading needs its own issue", () => {
  const body = [
    "## Not in this change",
    "",
    "- JSONC `package.json` for Bun (#3321).",
    "- `.entries()` and `.forEach` calls.",
    "",
    "## Test plan",
    "",
    "- A line without a link here is fine.",
  ].join("\n");
  assert.deepEqual(checkPrDeferrals(body), {
    ok: false,
    untracked: ["- `.entries()` and `.forEach` calls."],
  });
});

test("a wrapped list item counts as one deferral", () => {
  const body = "## Follow-ups\n\n- A function config that returns an array\n  stays open (#3322).";
  assert.deepEqual(checkPrDeferrals(body), { ok: true });
});

test("'No follow-up planned.' marks an intentional non-goal", () => {
  const body = "## Out of scope\n\n- Program sharing between processes. No follow-up planned.";
  assert.deepEqual(checkPrDeferrals(body), { ok: true });
});

test("a back-reference to an earlier pull request is not a deferral", () => {
  assert.deepEqual(checkPrDeferrals("A follow-up to #3114 for gitignored targets."), { ok: true });
  assert.deepEqual(checkPrDeferrals("Follow-up fixes after the review."), { ok: true });
});

test("a section that records changes made after review is not a deferral", () => {
  const body = "## Follow-up after review\n\n- Renamed the helper.\n- Added a test.";
  assert.deepEqual(checkPrDeferrals(body), { ok: true });
});

test("template comments and code blocks are ignored", () => {
  const body = [
    "<!-- Out of scope items need an issue link. -->",
    "```",
    "// this is out of scope",
    "```",
  ].join("\n");
  assert.deepEqual(checkPrDeferrals(body), { ok: true });
});

test("the script exits 1 and names the untracked deferral", () => {
  const run = spawnSync(process.execPath, [SCRIPT_PATH], {
    env: { PR_BODY: "That is a separate change." },
    encoding: "utf8",
  });
  assert.equal(run.status, 1);
  assert.match(run.stderr, /- That is a separate change\./u);
  assert.match(run.stderr, /No follow-up planned/u);
});

test("the script exits 0 for a clean description", () => {
  const run = spawnSync(process.execPath, [SCRIPT_PATH], {
    env: { PR_BODY: "Fix the parser." },
    encoding: "utf8",
  });
  assert.equal(run.status, 0, run.stderr);
});
