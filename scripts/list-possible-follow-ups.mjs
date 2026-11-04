#!/usr/bin/env node

// Lists the "Possible follow-ups" of merged pull requests. Agents record an
// improvement idea with no known need in that section instead of an issue, so
// the issue tracker holds only needed work. The maintainer reads this list when
// planning, and opens an issue for each idea that is worth the work.
//
// Run: node scripts/list-possible-follow-ups.mjs [--since YYYY-MM-DD] [--limit N] [--format json]

import { spawnSync } from "node:child_process";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { findPossibleFollowUps } from "../.github/scripts/check-pr-deferrals.mjs";

const DEFAULT_REPOSITORY = "fallow-rs/fallow";
const DEFAULT_LIMIT = 100;
const DATE = /^\d{4}-\d{2}-\d{2}$/u;
const POSITIVE_INTEGER = /^[1-9]\d*$/u;
const FORMATS = new Set(["text", "json"]);

/**
 * Returns one entry for each pull request that has possible follow-ups,
 * newest merge first.
 * @param {{ number: number, title: string, url: string, mergedAt: string, body: string }[]} pulls
 * @returns {{ number: number, title: string, url: string, mergedAt: string, items: string[] }[]}
 */
export const collectPossibleFollowUps = (pulls) =>
  pulls
    .map(({ number, title, url, mergedAt, body }) => ({
      number,
      title,
      url,
      mergedAt,
      items: findPossibleFollowUps(body),
    }))
    .filter((pull) => pull.items.length > 0)
    .toSorted((a, b) => b.mergedAt.localeCompare(a.mergedAt));

/**
 * Renders the entries as plain text, one block for each pull request.
 * @param {ReturnType<typeof collectPossibleFollowUps>} entries
 * @returns {string}
 */
export const formatText = (entries) => {
  if (entries.length === 0) return "No possible follow-ups found.";
  return entries
    .map((entry) =>
      [
        `#${entry.number} ${entry.title} (${entry.mergedAt.slice(0, 10)})`,
        entry.url,
        ...entry.items,
      ].join("\n"),
    )
    .join("\n\n");
};

/**
 * Builds the GitHub search query for merged pull requests with the section.
 * @param {string | undefined} since
 * @returns {string}
 */
export const searchQuery = (since) =>
  ['"Possible follow-ups" OR "Possible follow-up" in:body', since ? `merged:>=${since}` : ""]
    .filter(Boolean)
    .join(" ");

/**
 * Validates the command-line options.
 * @param {string[]} argv
 * @returns {{ ok: true, data: { repo: string, since: string | undefined, limit: number, format: string } } | { ok: false, error: string }}
 */
export const parseOptions = (argv) => {
  const { values } = parseArgs({
    args: argv,
    options: {
      repo: { type: "string", default: DEFAULT_REPOSITORY },
      since: { type: "string" },
      limit: { type: "string", default: String(DEFAULT_LIMIT) },
      format: { type: "string", default: "text" },
    },
    strict: true,
  });
  if (values.since !== undefined && !DATE.test(values.since)) {
    return { ok: false, error: "--since must be a date in the format YYYY-MM-DD." };
  }
  if (!POSITIVE_INTEGER.test(values.limit)) {
    return { ok: false, error: "--limit must be a positive integer." };
  }
  if (!FORMATS.has(values.format)) return { ok: false, error: "--format must be text or json." };
  return {
    ok: true,
    data: {
      repo: values.repo,
      since: values.since,
      limit: Number(values.limit),
      format: values.format,
    },
  };
};

const fetchMergedPulls = ({ repo, since, limit }) => {
  const run = spawnSync(
    "gh",
    [
      "pr",
      "list",
      "--repo",
      repo,
      "--state",
      "merged",
      "--search",
      searchQuery(since),
      "--limit",
      String(limit),
      "--json",
      "number,title,url,mergedAt,body",
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (run.error) throw new Error(`Could not run gh: ${run.error.message}`);
  if (run.status !== 0) throw new Error(run.stderr.trim() || `gh exited with ${run.status}.`);
  return JSON.parse(run.stdout);
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const options = parseOptions(process.argv.slice(2));
    if (!options.ok) {
      console.error(options.error);
      process.exitCode = 2;
    } else {
      const entries = collectPossibleFollowUps(fetchMergedPulls(options.data));
      console.log(
        options.data.format === "json" ? JSON.stringify(entries, null, 2) : formatText(entries),
      );
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
