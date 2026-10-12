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

test("bold deferral headings require tracking and stop at the next heading", () => {
  for (const heading of ["**Not in this change**", "**Follow-ups:**", "__Out of scope__:"]) {
    const body = `${heading}\n\n- Regex aliases remain open.\n\n**Validation**\n\n- Parser tests pass.`;
    assert.deepEqual(checkPrDeferrals(body), {
      ok: false,
      untracked: ["- Regex aliases remain open."],
    });
    assert.deepEqual(checkPrDeferrals(body.replace("remain open.", "remain open (#3320).")), {
      ok: true,
    });
  }
});

test("work deferred to a future iteration needs tracking", () => {
  for (const body of [
    "Regex alias support is deferred to a future iteration.",
    "Array configs are deferred until the next iteration.",
  ]) {
    assert.deepEqual(checkPrDeferrals(body), { ok: false, untracked: [body] });
    assert.deepEqual(checkPrDeferrals(`${body} See #3320.`), { ok: true });
  }
  assert.deepEqual(
    checkPrDeferrals("This was deferred to an earlier iteration and is now fixed."),
    {
      ok: true,
    },
  );
});

const runWithIssueResponses = (body, responses) =>
  spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "--eval",
      `
        import assert from 'node:assert/strict';
        import { pathToFileURL } from 'node:url';
        const responses = JSON.parse(process.env.ISSUE_RESPONSES);
        globalThis.fetch = async (url, options) => {
          assert.equal(new URL(url).origin, 'https://api.github.com');
          assert.equal(options.headers.Authorization, 'Bearer test-token');
          const response = responses[new URL(url).pathname];
          assert.ok(response, 'unexpected issue request');
          return new Response(JSON.stringify(response.body), { status: response.status });
        };
        process.argv[1] = process.env.CHECK_SCRIPT;
        await import(pathToFileURL(process.env.CHECK_SCRIPT).href);
      `,
    ],
    {
      env: {
        PR_BODY: body,
        GITHUB_REPOSITORY: "fallow-rs/fallow",
        GITHUB_TOKEN: "test-token",
        CHECK_SCRIPT: SCRIPT_PATH,
        ISSUE_RESPONSES: JSON.stringify(responses),
      },
      encoding: "utf8",
    },
  );

test("the CLI distinguishes a pull request reference from a tracking issue", () => {
  const responses = {
    "/repos/fallow-rs/fallow/issues/3179": {
      status: 200,
      body: {
        number: 3179,
        html_url: "https://github.com/fallow-rs/fallow/pull/3179",
        state: "closed",
        pull_request: { url: "https://api.github.com/repos/fallow-rs/fallow/pulls/3179" },
      },
    },
    "/repos/fallow-rs/fallow/issues/3317": {
      status: 200,
      body: {
        number: 3317,
        html_url: "https://github.com/fallow-rs/fallow/issues/3317",
        state: "open",
      },
    },
  };
  const rejected = runWithIssueResponses(
    "## Follow-up\n\nRegex aliases remain open. See #3179.",
    responses,
  );
  assert.equal(rejected.status, 1, rejected.stderr);
  assert.match(rejected.stderr, /Regex aliases remain open/u);
  const accepted = runWithIssueResponses(
    "## Follow-up\n\nRegex aliases remain open. See #3317.",
    responses,
  );
  assert.equal(accepted.status, 0, accepted.stderr);
});

test("the CLI verifies same-repository issue URLs per deferred item", () => {
  const responses = {
    "/repos/fallow-rs/fallow/issues/3179": {
      status: 200,
      body: {
        number: 3179,
        html_url: "https://github.com/fallow-rs/fallow/pull/3179",
        pull_request: {},
      },
    },
    "/repos/fallow-rs/fallow/issues/3317": {
      status: 200,
      body: {
        number: 3317,
        html_url: "https://github.com/fallow-rs/fallow/issues/3317",
        state: "open",
      },
    },
  };
  const body =
    "## Follow-ups\n\n- Parser support: #3317.\n- Regex aliases: https://github.com/fallow-rs/fallow/issues/3179.";
  const rejected = runWithIssueResponses(body, responses);
  assert.equal(rejected.status, 1, rejected.stderr);
  assert.match(rejected.stderr, /Regex aliases/u);
  assert.doesNotMatch(rejected.stderr, /Parser support/u);
  const accepted = runWithIssueResponses(
    body.replace(
      "https://github.com/fallow-rs/fallow/issues/3179",
      "https://github.com/FALLOW-RS/FALLOW/issues/3317",
    ),
    responses,
  );
  assert.equal(accepted.status, 0, accepted.stderr);
});

test("the CLI accepts cross-repository references without a lookup", () => {
  // The workflow token cannot read other repositories, and a private
  // repository answers 404. The mock fails on any request.
  const run = runWithIssueResponses(
    "## Follow-ups\n\n- Cloud support: fallow-rs/fallow-cloud#41.\n- Cloud quota: https://github.com/fallow-rs/fallow-cloud/issues/42.",
    {},
  );
  assert.equal(run.status, 0, run.stderr);
});

test("the CLI distinguishes missing issues from failed verification", () => {
  for (const [status, body, expectedExit] of [
    [404, { message: "Not Found" }, 1],
    [403, { message: "API rate limit exceeded" }, 2],
    [200, { unexpected: true }, 2],
  ]) {
    const run = runWithIssueResponses("This is a separate change (#123).", {
      "/repos/fallow-rs/fallow/issues/123": { status, body },
    });
    assert.equal(run.status, expectedExit, run.stderr);
    assert.doesNotMatch(run.stdout, /has an issue reference/u);
    assert.doesNotMatch(run.stderr, /test-token/u);
  }
  const optedOut = runWithIssueResponses("## Follow-up\n\n- No follow-up planned.", {});
  assert.equal(optedOut.status, 0, optedOut.stderr);
});

test("nested list items need their own tracking reference", () => {
  for (const indent of ["    ", "        ", "\t"]) {
    const item = "- Add support for nested resolver inputs.";
    const body = `## Follow-ups\n\n- Parser follow-up (#3320):\n${indent}${item}`;
    assert.deepEqual(checkPrDeferrals(body), { ok: false, untracked: [item] });
    assert.deepEqual(checkPrDeferrals(`${body} See #3321.`), { ok: true });
    assert.deepEqual(checkPrDeferrals(`${body} No follow-up planned.`), { ok: true });
  }
});

test("deferral sections include nested headings and end at the same or higher level", () => {
  const item = "- Add support for regex aliases.";
  for (const nested of ["### Resolver", "### Out of scope\n\n### Resolver"]) {
    const body = `## Follow-ups\n\n${nested}\n\n${item}`;
    assert.deepEqual(checkPrDeferrals(body), { ok: false, untracked: [item] });
    assert.deepEqual(checkPrDeferrals(`${body} See #3320.`), { ok: true });
    for (const next of ["## Validation", "# Validation"]) {
      assert.deepEqual(checkPrDeferrals(`${body} See #3320.\n\n${next}\n\n- The checks pass.`), {
        ok: true,
      });
    }
  }
});
