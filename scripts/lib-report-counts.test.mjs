import assert from "node:assert/strict";
import { test } from "node:test";

import { fallowIssueCounts, fallowIssueTotal, knipIssueTotal } from "./lib/report-counts.mjs";

test("fallow counts come from total_issues and summary, not from root arrays", () => {
  const report = {
    total_issues: 1,
    summary: { total_issues: 1, unused_files: 1, unused_exports: 0 },
    unused_files: [{}],
    next_steps: [{}, {}],
    workspace_diagnostics: [{}],
  };
  assert.equal(fallowIssueTotal(report), 1);
  assert.deepEqual(fallowIssueCounts(report), { unused_files: 1 });
});

test("fallowIssueTotal falls back to the summary and else gives null", () => {
  assert.equal(fallowIssueTotal({ unused_files: [{}] }), null);
  assert.equal(fallowIssueTotal({ summary: { total_issues: 4 } }), 4);
});

test("knipIssueTotal counts each issue of each file row", () => {
  const report = {
    files: ["a.ts"],
    issues: [
      {
        file: "b.ts",
        owners: ["@x"],
        exports: [{}, {}],
        types: [{}],
        enumMembers: { E: [{}, {}] },
      },
    ],
  };
  assert.equal(knipIssueTotal(report), 6);
});
