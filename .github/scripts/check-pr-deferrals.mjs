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
const DEFERRAL_PHRASE =
  /\b(?:(?:a|its own) (?:separate|later|future) (?:change|pr)\b|separate change\b|left for a follow-?up\b|(?:is|as|for) a (?:possible |reasonable )?follow-?up\b(?! (?:to|of|from|for|after)\b)|out of scope\b)/iu;
const ISSUE_REF =
  /(?:^|[\s(])(?:[\w.-]+\/[\w.-]+)?#\d+\b|https:\/\/github\.com\/[\w.-]+\/[\w.-]+\/issues\/\d+/u;
const OPT_OUT = /\bno follow-?up planned\b/iu;
const TABLE_ROW = /^\|/u;
const LIST_ITEM = /^\s{0,3}(?:[-*+]|\d+[.)])\s/u;

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
    if (line.trim() === "") {
      flush();
    } else if (ANY_HEADING.test(line)) {
      flush();
      blocks.push({ text: line.trim(), heading: true });
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

const isResolved = (text) => ISSUE_REF.test(text) || OPT_OUT.test(text);

/**
 * Lists the deferrals in a pull request description that have no tracking
 * issue and no explicit opt-out.
 * @param {string} body
 * @returns {{ ok: true } | { ok: false, untracked: string[] }}
 */
export const checkPrDeferrals = (body) => {
  const untracked = [];
  let inDeferralSection = false;
  for (const block of splitBlocks(stripNonProse(body ?? ""))) {
    if (block.heading) {
      inDeferralSection = DEFERRAL_HEADING.test(block.text);
      continue;
    }
    if (TABLE_ROW.test(block.text)) continue;
    const isDeferral = inDeferralSection || DEFERRAL_PHRASE.test(block.text);
    if (isDeferral && !isResolved(block.text)) untracked.push(block.text);
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
  const result = checkPrDeferrals(process.env.PR_BODY);
  if (result.ok) {
    console.log("Every deferral in the pull request description has a tracking issue.");
  } else {
    console.error(formatFailure(result.untracked));
    process.exitCode = 1;
  }
}
