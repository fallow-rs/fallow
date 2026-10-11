// Checks that every piece of work a pull request description defers has a
// tracking issue. A deferral without a link is easy to lose after the merge.
//
// A deferral is one of these:
// - a paragraph or list item under a "Not in this change", "Follow-ups" or
//   "Out of scope" heading,
// - a paragraph or list item that says the work is "a separate change", "left
//   for a follow-up" or "out of scope".
//
// Each deferral must link an issue (`#123`, `owner/repo#123` or an issue URL),
// or say "No follow-up planned." to record an intentional non-goal.
//
// Run: PR_BODY="..." node .github/scripts/check-pr-deferrals.mjs

import process from "node:process";
import { fileURLToPath } from "node:url";

const DEFERRAL_HEADING =
  /^#{1,6}\s*(?:not in this (?:change|pr)\b|follow-?ups?\b(?! after review)(?!.*\bin this (?:change|pr)\b)|out of scope\b|deferred\b)/iu;
const ANY_HEADING = /^#{1,6}\s/u;
const BOLD_HEADING = /^(?:\*\*(.+?)\*\*|__(.+?)__):?$/u;
const DEFERRAL_PHRASE =
  /\b(?:(?:a|its own) (?:separate|later|future) (?:change|pr)\b|separate change\b|left for a follow-?up\b|(?:is|as|for) a (?:possible |reasonable )?follow-?up\b(?! (?:to|of|from|for|after)\b)|(?:is|are|remains?) deferred (?:to|until|for)\b|out of scope\b)/iu;
const ISSUE_REF =
  /(?:^|[\s(])(?:(?<repo>[\w.-]+\/[\w.-]+))?#(?<number>\d+)\b|https:\/\/github\.com\/(?<urlRepo>[\w.-]+\/[\w.-]+)\/issues\/(?<urlNumber>\d+)\b/u;
const OPT_OUT = /\bno follow-?up planned\b/iu;
const TABLE_ROW = /^\|/u;
const LIST_ITEM = /^[ \t]*(?:[-*+]|\d+[.)])[ \t]/u;
const ISSUE_LOOKUP_TIMEOUT_MS = 10_000;

/**
 * Removes HTML comments and fenced code blocks, so template hints and code
 * samples do not count as deferrals.
 * @param {string} body
 * @returns {string}
 */
const stripNonProse = (body) =>
  body.replace(/<!--[\s\S]*?-->/gu, "").replace(/^(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\1[^\n]*$/gmu, "");

/**
 * Splits markdown into blocks: headings, list items (with their wrapped
 * lines) and paragraphs.
 * @param {string} body
 * @returns {{ text: string, heading: boolean }[]}
 */
const splitBlocks = (body) => {
  const blocks = [];
  let current = null;
  const flush = () => {
    if (current) blocks.push(current);
    current = null;
  };
  for (const line of body.split(/\r?\n/u)) {
    const boldHeading = BOLD_HEADING.exec(line.trim());
    const headingLine = boldHeading ? `## ${boldHeading[1] ?? boldHeading[2]}` : line;
    if (line.trim() === "") {
      flush();
    } else if (ANY_HEADING.test(headingLine)) {
      flush();
      blocks.push({ text: headingLine.trim(), heading: true });
    } else if (LIST_ITEM.test(line) || !current) {
      flush();
      current = { text: line.trim(), heading: false };
    } else {
      current.text += ` ${line.trim()}`;
    }
  }
  flush();
  return blocks;
};

const findDeferrals = (body) => {
  const deferrals = [];
  let deferralLevel = null;
  for (const block of splitBlocks(stripNonProse(body ?? ""))) {
    if (block.heading) {
      const level = /^#+/u.exec(block.text)[0].length;
      if (deferralLevel !== null && level <= deferralLevel) deferralLevel = null;
      if (deferralLevel === null && DEFERRAL_HEADING.test(block.text)) deferralLevel = level;
      continue;
    }
    if (TABLE_ROW.test(block.text)) continue;
    const isDeferral = deferralLevel !== null || DEFERRAL_PHRASE.test(block.text);
    if (isDeferral && !OPT_OUT.test(block.text)) deferrals.push(block.text);
  }
  return deferrals;
};

/**
 * Checks reference syntax without contacting GitHub. The CLI also verifies
 * that each reference identifies an issue.
 * @param {string} body
 * @returns {{ ok: true } | { ok: false, untracked: string[] }}
 */
export const checkPrDeferrals = (body) => {
  const untracked = findDeferrals(body).filter((text) => !ISSUE_REF.test(text));
  return untracked.length === 0 ? { ok: true } : { ok: false, untracked };
};

const lookupIssue = async (repository, number) => {
  if (!repository || !/^[\w.-]+\/[\w.-]+$/u.test(repository)) {
    throw new Error("Set GITHUB_REPOSITORY to verify local issue references.");
  }
  const headers = { Accept: "application/vnd.github+json" };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const reference = `${repository}#${number}`;
  let response;
  try {
    response = await fetch(`https://api.github.com/repos/${repository}/issues/${number}`, {
      headers,
      signal: AbortSignal.timeout(ISSUE_LOOKUP_TIMEOUT_MS),
    });
  } catch {
    throw new Error(`Could not verify ${reference}: GitHub request failed.`);
  }
  if (response.status === 404) return false;
  if (!response.ok)
    throw new Error(`Could not verify ${reference}: GitHub returned ${response.status}.`);
  const issue = await response.json();
  if (!issue || issue.number !== Number(number) || typeof issue.html_url !== "string") {
    throw new Error(`Could not verify ${reference}: invalid GitHub issue response.`);
  }
  return !Object.hasOwn(issue, "pull_request");
};

const verifyTrackingIssues = async (body) => {
  const verified = new Map();
  const untracked = [];
  for (const text of findDeferrals(body)) {
    let tracked = false;
    for (const match of text.matchAll(new RegExp(ISSUE_REF, "gu"))) {
      const repository = match.groups.urlRepo ?? match.groups.repo ?? process.env.GITHUB_REPOSITORY;
      const number = match.groups.urlNumber ?? match.groups.number;
      const key = `${repository}#${number}`;
      if (!verified.has(key)) verified.set(key, await lookupIssue(repository, number));
      if (verified.get(key)) {
        tracked = true;
        break;
      }
    }
    if (!tracked) untracked.push(text);
  }
  return untracked.length === 0 ? { ok: true } : { ok: false, untracked };
};

const formatFailure = (untracked) =>
  [
    "The pull request description defers work without a tracking issue.",
    "Open an issue for each item and link it, or add 'No follow-up planned.' to an intentional non-goal.",
    "",
    ...untracked.map((text) => `- ${text.length > 200 ? `${text.slice(0, 197)}...` : text}`),
  ].join("\n");

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const result = await verifyTrackingIssues(process.env.PR_BODY);
    if (result.ok) {
      console.log("Every deferral in the pull request description has a verified issue reference.");
    } else {
      console.error(formatFailure(result.untracked));
      process.exitCode = 1;
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
