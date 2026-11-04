// Tests for list-possible-follow-ups.mjs.
//
// Run: node --test scripts/list-possible-follow-ups.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  collectPossibleFollowUps,
  formatText,
  parseOptions,
  searchQuery,
} from "./list-possible-follow-ups.mjs";

const pull = (number, mergedAt, body) => ({
  number,
  title: `Change ${number}`,
  url: `https://github.com/fallow-rs/fallow/pull/${number}`,
  mergedAt,
  body,
});

test("collects the possible follow-ups of each pull request, newest merge first", () => {
  const entries = collectPossibleFollowUps([
    pull(1, "2026-10-01T10:00:00Z", "## Possible follow-ups\n\n- Cache the config."),
    pull(2, "2026-10-02T10:00:00Z", "## What\n\nThe body names Possible follow-ups in prose."),
    pull(
      3,
      "2026-10-03T10:00:00Z",
      "## Possible follow-ups\n\n- Share the resolver.\n- Add a flag.",
    ),
  ]);
  assert.deepEqual(
    entries.map((entry) => [entry.number, entry.items]),
    [
      [3, ["- Share the resolver.", "- Add a flag."]],
      [1, ["- Cache the config."]],
    ],
  );
});

test("the text output names each pull request and its items", () => {
  const entries = collectPossibleFollowUps([
    pull(4, "2026-10-04T10:00:00Z", "## Possible follow-ups\n\n- Cache the config."),
  ]);
  assert.equal(
    formatText(entries),
    "#4 Change 4 (2026-10-04)\nhttps://github.com/fallow-rs/fallow/pull/4\n- Cache the config.",
  );
  assert.equal(formatText([]), "No possible follow-ups found.");
});

test("the search query adds the merge date when --since is set", () => {
  assert.equal(searchQuery(undefined), '"Possible follow-ups" OR "Possible follow-up" in:body');
  assert.equal(
    searchQuery("2026-10-01"),
    '"Possible follow-ups" OR "Possible follow-up" in:body merged:>=2026-10-01',
  );
});

test("invalid options give an error, not a gh call", () => {
  assert.deepEqual(parseOptions(["--since", "last week"]), {
    ok: false,
    error: "--since must be a date in the format YYYY-MM-DD.",
  });
  assert.equal(parseOptions(["--limit", "0"]).ok, false);
  assert.equal(parseOptions(["--format", "csv"]).ok, false);
  assert.deepEqual(parseOptions([]), {
    ok: true,
    data: { repo: "fallow-rs/fallow", since: undefined, limit: 100, format: "text" },
  });
});

test("the CLI exits 2 for an unknown option", () => {
  const script = fileURLToPath(new URL("./list-possible-follow-ups.mjs", import.meta.url));
  const run = spawnSync(process.execPath, [script, "--bogus"], { encoding: "utf8" });
  assert.equal(run.status, 2);
  assert.match(run.stderr, /bogus/u);
});
