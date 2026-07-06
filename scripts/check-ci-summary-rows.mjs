#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const rowId = ({ anchor, key, label }) => `${label}\t${key}\t${anchor}`;

const SUMMARY_PROBLEM_KEYS = [
  "githubMissing",
  "gitlabMissing",
  "githubExtra",
  "gitlabExtra",
  "parityMissingFromGithub",
  "parityMissingFromGitlab",
];

const sortRows = (rows) => rows.toSorted((a, b) => rowId(a).localeCompare(rowId(b)));

const diffRows = (left, right) => {
  const rightIds = new Set(right.map(rowId));
  return sortRows(left.filter((row) => !rightIds.has(rowId(row))));
};

export const expectedSummaryRows = (registry) =>
  sortRows(
    (registry.issue_types ?? [])
      .filter((issue) => issue.command === "dead-code")
      .filter((issue) => issue.counts_in_total === true)
      .filter((issue) => issue.result_key && issue.summary_label && issue.summary_docs_anchor)
      .map((issue) => ({
        anchor: issue.summary_docs_anchor,
        key: issue.result_key,
        label: issue.summary_label,
      })),
  );

export const parseSummaryRows = (source) => {
  const rows = [];
  const pattern = /table_row\("([^"]+)";\s*"([^"]+)";\s*"([^"]+)"\)/g;
  let match;

  while ((match = pattern.exec(source)) !== null) {
    rows.push({
      anchor: match[3],
      key: match[2],
      label: match[1],
    });
  }

  return sortRows(rows);
};

export const checkSummaryRows = ({ github, gitlab, registry }) => {
  const expected = expectedSummaryRows(registry);
  const githubRows = parseSummaryRows(github);
  const gitlabRows = parseSummaryRows(gitlab);

  return {
    expected,
    githubRows,
    gitlabRows,
    githubMissing: diffRows(expected, githubRows),
    gitlabMissing: diffRows(expected, gitlabRows),
    githubExtra: diffRows(githubRows, expected),
    gitlabExtra: diffRows(gitlabRows, expected),
    parityMissingFromGithub: diffRows(gitlabRows, githubRows),
    parityMissingFromGitlab: diffRows(githubRows, gitlabRows),
  };
};

const formatRows = (rows) =>
  rows.map((row) => `  - ${row.label} (${row.key}, ${row.anchor})`).join("\n");

const reportSection = (title, rows) => (rows.length > 0 ? `${title}:\n${formatRows(rows)}` : "");

export const formatSummaryRowProblems = (result) =>
  [
    reportSection("GitHub summary rows missing registry rows", result.githubMissing),
    reportSection("GitLab summary rows missing registry rows", result.gitlabMissing),
    reportSection("GitHub summary rows not in registry", result.githubExtra),
    reportSection("GitLab summary rows not in registry", result.gitlabExtra),
    reportSection("Rows present in GitLab but not GitHub", result.parityMissingFromGithub),
    reportSection("Rows present in GitHub but not GitLab", result.parityMissingFromGitlab),
  ]
    .filter(Boolean)
    .join("\n\n");

export const hasSummaryRowProblems = (result) =>
  SUMMARY_PROBLEM_KEYS.some((key) => result[key].length > 0);

export const checkSummaryRowFiles = ({
  githubPath = "action/jq/summary-check.jq",
  gitlabPath = "ci/jq/summary-check.jq",
  registryPath = "npm/fallow/issue-registry.json",
} = {}) =>
  checkSummaryRows({
    github: readFileSync(githubPath, "utf8"),
    gitlab: readFileSync(gitlabPath, "utf8"),
    registry: JSON.parse(readFileSync(registryPath, "utf8")),
  });

const main = () => {
  const result = checkSummaryRowFiles();
  if (hasSummaryRowProblems(result)) {
    console.error(formatSummaryRowProblems(result));
    process.exitCode = 1;
    return;
  }

  console.log("ci summary rows: GitHub and GitLab match the issue registry");
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
