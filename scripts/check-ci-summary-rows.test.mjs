import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  checkSummaryRows,
  expectedSummaryRows,
  parseSummaryRows,
} from "./check-ci-summary-rows.mjs";

test("expected summary rows come from counted dead-code registry rows", () => {
  const registry = {
    issue_types: [
      {
        command: "dead-code",
        counts_in_total: true,
        result_key: "unused_files",
        summary_label: "Unused files",
        summary_docs_anchor: "unused-files",
      },
      {
        command: "dead-code",
        counts_in_total: false,
        result_key: "prop_drilling_chains",
        summary_label: "Prop drilling",
        summary_docs_anchor: "prop-drilling",
      },
      {
        command: "health",
        counts_in_total: true,
        result_key: "complexity",
        summary_label: "Complexity",
        summary_docs_anchor: "complexity",
      },
    ],
  };

  assert.deepEqual(expectedSummaryRows(registry), [
    {
      anchor: "unused-files",
      key: "unused_files",
      label: "Unused files",
    },
  ]);
});

test("summary row parser extracts table_row contracts", () => {
  const rows = parseSummaryRows(`
    table_row("Unused files"; "unused_files"; "unused-files"),
    table_row("Unused exports"; "unused_exports"; "unused-exports")
  `);

  assert.deepEqual(rows, [
    {
      anchor: "unused-exports",
      key: "unused_exports",
      label: "Unused exports",
    },
    {
      anchor: "unused-files",
      key: "unused_files",
      label: "Unused files",
    },
  ]);
});

test("current GitHub and GitLab summary rows match the generated registry", () => {
  const registry = JSON.parse(readFileSync("npm/fallow/issue-registry.json", "utf8"));
  const github = readFileSync("action/jq/summary-check.jq", "utf8");
  const gitlab = readFileSync("ci/jq/summary-check.jq", "utf8");
  const result = checkSummaryRows({ github, gitlab, registry });

  assert.deepEqual(result.githubMissing, []);
  assert.deepEqual(result.gitlabMissing, []);
  assert.deepEqual(result.githubExtra, []);
  assert.deepEqual(result.gitlabExtra, []);
  assert.deepEqual(result.parityMissingFromGithub, []);
  assert.deepEqual(result.parityMissingFromGitlab, []);
});
