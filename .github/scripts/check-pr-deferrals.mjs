// Checks that every piece of work a pull request description defers has a
// tracking issue. A deferral without a link is easy to lose after the merge.
//
// A deferral is one of these:
// - a paragraph or list item under a heading that names work this change does
//   not do, such as "Not in this change", "Follow-ups", "Out of scope", "Not
//   covered", "Not in scope", "Future work" or "Known limit outside this
//   change",
// - a paragraph or list item that says the work is "a separate change", "left
//   for a follow-up" or "out of scope".
//
// Each deferral must link an issue (`#123`, `owner/repo#123` or an issue URL).
// The CLI verifies references to this repository through the GitHub API. It
// accepts a reference to another repository on its syntax.
//
// A "Possible follow-ups" section records improvement ideas that have no known
// need. Its items need no issue: the maintainer reads them later with
// scripts/list-possible-follow-ups.mjs and picks the ideas that become issues.
// A deferral heading inside or around that section still needs an issue for
// each item.
//
// Run: PR_BODY="..." node .github/scripts/check-pr-deferrals.mjs

import process from "node:process";
import { fileURLToPath } from "node:url";

const DEFERRAL_HEADING =
  /^#{1,6}\s*(?:not in this (?:change|pr)\b|(?:what (?:is|was) )?not (?:yet )?(?:covered|in scope|handled|addressed|done|fixed|included|implemented)\b|follow-?ups?\b(?! after review)(?!.*\bin this (?:change|pr)\b)|out of scope\b|deferred\b|(?:remaining|future) work\b|left out\b)/iu;
// A heading that puts its subject outside this change anywhere in its text,
// for example "Follow-up (not in this PR)" or "Known limit outside this change".
const OUTSIDE_CHANGE_HEADING = /^#{1,6}\s.*\b(?:not in|outside(?: of)?) this (?:change|pr)\b/iu;
const ANY_HEADING = /^#{1,6}\s/u;
const BOLD_HEADING = /^(?:\*\*(.+?)\*\*|__(.+?)__):?$/u;
const DEFERRAL_PHRASE =
  /\b(?:(?:a|its own) (?:separate|later|future) (?:change|pr)\b|separate change\b|left for a follow-?up\b|(?:is|as|for) a (?:possible |reasonable )?follow-?up\b(?! (?:to|of|from|for|after)\b)|(?:is|are|remains?) deferred (?:to|until|for)\b|out of scope\b)/iu;
const ISSUE_REF =
  /(?:^|[\s(])(?:(?<repo>[\w.-]+\/[\w.-]+))?#(?<number>\d+)\b|https:\/\/github\.com\/(?<urlRepo>[\w.-]+\/[\w.-]+)\/issues\/(?<urlNumber>\d+)\b/u;
const POSSIBLE_HEADING = /^#{1,6}\s*possible follow-?ups?\b/iu;
const ISSUE_REFS = new RegExp(ISSUE_REF, "gu");
const REPOSITORY_NAME = /^[\w.-]+\/[\w.-]+$/u;
const OPEN_BLOCK_KINDS = new Set(["start", "continue"]);
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
  (body ?? "")
    .replace(/<!--[\s\S]*?-->/gu, "")
    .replace(/^(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\1[^\n]*$/gmu, "");

/**
 * Converts a line that is bold text only (`**Follow-ups**`) to a heading.
 * @param {string} line
 * @returns {string}
 */
const normalizeHeading = (line) => {
  const bold = BOLD_HEADING.exec(line.trim());
  return bold ? `## ${bold[1] ?? bold[2]}` : line;
};

const startsBlock = (line, open) => LIST_ITEM.test(line) || !open;

/**
 * Classifies a line: "blank", "heading", "start" (a new list item or
 * paragraph) or "continue" (a wrapped line of the open block).
 * @param {string} line
 * @param {string} headingLine
 * @param {boolean} open
 * @returns {string}
 */
const lineKind = (line, headingLine, open) => {
  if (line.trim() === "") return "blank";
  if (ANY_HEADING.test(headingLine)) return "heading";
  if (startsBlock(line, open)) return "start";
  return "continue";
};

const toBlock = (kind, line, headingLine) =>
  kind === "heading"
    ? { text: headingLine.trim(), heading: true }
    : { text: line.trim(), heading: false };

/**
 * Splits markdown into blocks: headings, list items (with their wrapped
 * lines) and paragraphs.
 * @param {string} body
 * @returns {{ text: string, heading: boolean }[]}
 */
const splitBlocks = (body) => {
  const blocks = [];
  let open = false;
  for (const line of body.split(/\r?\n/u)) {
    const headingLine = normalizeHeading(line);
    const kind = lineKind(line, headingLine, open);
    if (kind === "continue") blocks.at(-1).text += ` ${line.trim()}`;
    else if (kind !== "blank") blocks.push(toBlock(kind, line, headingLine));
    open = OPEN_BLOCK_KINDS.has(kind);
  }
  return blocks;
};

const sectionKind = (heading) => {
  if (DEFERRAL_HEADING.test(heading) || OUTSIDE_CHANGE_HEADING.test(heading)) return "deferral";
  if (POSSIBLE_HEADING.test(heading)) return "possible";
  return null;
};

/**
 * Returns the open sections after `heading`. A section includes its nested
 * headings and ends at a heading of the same or a higher level.
 * @param {string} heading
 * @param {{ kind: string, level: number }[]} sections
 * @returns {{ kind: string, level: number }[]}
 */
const nextSections = (heading, sections) => {
  const level = /^#+/u.exec(heading)[0].length;
  const open = sections.filter((section) => section.level < level);
  const kind = sectionKind(heading);
  return kind === null ? open : [...open, { kind, level }];
};

// A deferral section wins over an enclosing or nested exempt section, so an
// exempt heading cannot hide needed work.
const effectiveKind = (sections) => {
  const kinds = new Set(sections.map((section) => section.kind));
  if (kinds.has("deferral")) return "deferral";
  return sections.at(-1)?.kind ?? null;
};

/**
 * Returns the prose blocks of a description, each with the kind of the
 * section that contains it ("deferral", "possible" or null).
 * @param {string} body
 * @returns {{ text: string, kind: string | null }[]}
 */
const classifyBlocks = (body) => {
  const blocks = [];
  let sections = [];
  for (const block of splitBlocks(stripNonProse(body))) {
    if (block.heading) sections = nextSections(block.text, sections);
    else if (!TABLE_ROW.test(block.text))
      blocks.push({ text: block.text, kind: effectiveKind(sections) });
  }
  return blocks;
};

const isDeferral = ({ text, kind }) =>
  kind === "deferral" || (kind === null && DEFERRAL_PHRASE.test(text));

const findDeferrals = (body) =>
  classifyBlocks(body)
    .filter(isDeferral)
    .map((block) => block.text);

/**
 * Returns the items of every "Possible follow-ups" section of a description.
 * @param {string} body
 * @returns {string[]}
 */
export const findPossibleFollowUps = (body) =>
  classifyBlocks(body)
    .filter((block) => block.kind === "possible")
    .map((block) => block.text);

const toResult = (untracked) => (untracked.length === 0 ? { ok: true } : { ok: false, untracked });

/**
 * Checks reference syntax without contacting GitHub. The CLI also verifies
 * that each same-repository reference identifies an issue.
 * @param {string} body
 * @returns {{ ok: true } | { ok: false, untracked: string[] }}
 */
export const checkPrDeferrals = (body) =>
  toResult(findDeferrals(body).filter((text) => !ISSUE_REF.test(text)));

const assertRepository = (repository) => {
  if (!repository || !REPOSITORY_NAME.test(repository)) {
    throw new Error("Set GITHUB_REPOSITORY to verify local issue references.");
  }
};

const requestHeaders = () => {
  const headers = { Accept: "application/vnd.github+json" };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  return headers;
};

const fetchIssue = async (repository, number, reference) => {
  try {
    return await fetch(`https://api.github.com/repos/${repository}/issues/${number}`, {
      headers: requestHeaders(),
      signal: AbortSignal.timeout(ISSUE_LOOKUP_TIMEOUT_MS),
    });
  } catch {
    throw new Error(`Could not verify ${reference}: GitHub request failed.`);
  }
};

const isIssueRecord = (issue, number) =>
  Boolean(issue) && issue.number === Number(number) && typeof issue.html_url === "string";

/**
 * Reads an issue response. Returns false for an unknown number, and false
 * for a pull request, because a pull request does not track work.
 * @param {Response} response
 * @param {string} number
 * @param {string} reference
 * @returns {Promise<boolean>}
 */
const readIssueResponse = async (response, number, reference) => {
  if (response.status === 404) return false;
  if (!response.ok)
    throw new Error(`Could not verify ${reference}: GitHub returned ${response.status}.`);
  const issue = await response.json();
  if (!isIssueRecord(issue, number)) {
    throw new Error(`Could not verify ${reference}: invalid GitHub issue response.`);
  }
  return !Object.hasOwn(issue, "pull_request");
};

const lookupIssue = async (repository, number) => {
  assertRepository(repository);
  const reference = `${repository}#${number}`;
  const response = await fetchIssue(repository, number, reference);
  return readIssueResponse(response, number, reference);
};

/**
 * Returns a lookup that asks GitHub once for each issue reference.
 * @returns {(repository: string, number: string) => Promise<boolean>}
 */
const cachedLookup = () => {
  const verified = new Map();
  return async (repository, number) => {
    const key = `${repository}#${number}`;
    if (!verified.has(key)) verified.set(key, await lookupIssue(repository, number));
    return verified.get(key);
  };
};

const namedRepository = (groups) => groups.urlRepo ?? groups.repo;

const issueNumber = (groups) => groups.urlNumber ?? groups.number;

// The workflow token reads only this repository, and a private repository
// answers 404. Thus a reference to another repository is accepted on its
// syntax.
const isOtherRepository = (named, local) =>
  Boolean(named) && named.toLowerCase() !== String(local).toLowerCase();

const isTrackingReference = async (match, lookup) => {
  const local = process.env.GITHUB_REPOSITORY;
  if (isOtherRepository(namedRepository(match.groups), local)) return true;
  return lookup(local, issueNumber(match.groups));
};

const hasTrackingIssue = async (text, lookup) => {
  for (const match of text.matchAll(ISSUE_REFS)) {
    if (await isTrackingReference(match, lookup)) return true;
  }
  return false;
};

const verifyTrackingIssues = async (body) => {
  const lookup = cachedLookup();
  const untracked = [];
  for (const text of findDeferrals(body)) {
    if (!(await hasTrackingIssue(text, lookup))) untracked.push(text);
  }
  return toResult(untracked);
};

const formatFailure = (untracked) =>
  [
    "The pull request description defers work without a tracking issue.",
    "Each item below is needed work or an idea:",
    "- Needed work (a defect, or a part of the task that is not done): open an issue and link it.",
    "- An improvement idea with no known need: move it under a 'Possible follow-ups' heading.",
    "- No work at all: describe it without a deferral heading or phrase.",
    "Then edit the pull request description. The edit starts a new run of this check.",
    "Do not re-run this job: a re-run keeps the old description and cancels the new run.",
    "",
    ...untracked.map((text) => `- ${text.length > 200 ? `${text.slice(0, 197)}...` : text}`),
  ].join("\n");

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const result = await verifyTrackingIssues(process.env.PR_BODY);
    if (result.ok) {
      console.log("Every deferral in the pull request description has an issue reference.");
    } else {
      console.error(formatFailure(result.untracked));
      process.exitCode = 1;
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
