// Tests for check-comment-author.mjs, the author check of the pull-request
// comment smoke test in test-action.yml.
//
// Run: node --test .github/scripts/check-comment-author.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { checkCommentAuthor } from "./check-comment-author.mjs";

const SCRIPT_PATH = fileURLToPath(new URL("./check-comment-author.mjs", import.meta.url));

test("a branded token must post as the Fallow app", () => {
  assert.deepEqual(checkCommentAuthor({ author: "fallow-cloud[bot]", branded: "true" }), {
    ok: true,
    message: "The comment author is fallow-cloud[bot], from the branded token.",
  });
  const wrong = checkCommentAuthor({ author: "github-actions[bot]", branded: "true" });
  assert.equal(wrong.ok, false);
  assert.match(
    wrong.message,
    /Expected comment author fallow-cloud\[bot\], found github-actions\[bot\]/u,
  );
});

test("a fallback token posts as github-actions and names the cause", () => {
  const result = checkCommentAuthor({
    author: "github-actions[bot]",
    branded: "false",
    reason: "broker unavailable or declined",
  });
  assert.equal(result.ok, true);
  assert.match(result.message, /github-actions\[bot\]/u);
  assert.match(result.message, /broker unavailable or declined/u);
});

test("a fallback token with another author fails", () => {
  const result = checkCommentAuthor({ author: "someone", branded: "false", reason: "x" });
  assert.equal(result.ok, false);
  assert.match(result.message, /Expected comment author github-actions\[bot\], found someone/u);
});

test("an unknown broker outcome fails", () => {
  const result = checkCommentAuthor({ author: "fallow-cloud[bot]", branded: "" });
  assert.equal(result.ok, false);
  assert.match(result.message, /FALLOW_TOKEN_BRANDED/u);
});

test("the command line prints the fallback cause as a notice", () => {
  const result = spawnSync(process.execPath, [SCRIPT_PATH], {
    encoding: "utf8",
    env: {
      ...process.env,
      COMMENT_AUTHOR: "github-actions[bot]",
      FALLOW_TOKEN_BRANDED: "false",
      FALLOW_TOKEN_FALLBACK_REASON: "broker unavailable or declined",
    },
  });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^::notice::.*broker unavailable or declined/mu);
});

test("the command line fails with an error annotation", () => {
  const result = spawnSync(process.execPath, [SCRIPT_PATH], {
    encoding: "utf8",
    env: { ...process.env, COMMENT_AUTHOR: "github-actions[bot]", FALLOW_TOKEN_BRANDED: "true" },
  });
  assert.equal(result.status, 1);
  assert.match(result.stdout, /^::error::Expected comment author fallow-cloud\[bot\]/mu);
});
