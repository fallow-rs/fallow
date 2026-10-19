// Checks that every commit in a pull request carries a Developer Certificate
// of Origin sign-off: a `Signed-off-by: Name <email>` trailer whose email
// matches the commit author. Merge commits are skipped.
//
// Run: BASE_SHA=<sha> node .github/scripts/check-dco.mjs
//      node .github/scripts/check-dco.mjs --message-file <path>  (commit-msg hook)

import { execFileSync, spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import process from "node:process";
import { fileURLToPath } from "node:url";

const FIELD_SEPARATOR = "\u001f";
const RECORD_SEPARATOR = "\u001e";
const SIGN_OFF = /^Signed-off-by:\s+.+?\s+<([^<>\s]+)>\s*$/gimu;

/**
 * Returns the commits that lack a sign-off by their author.
 * @param {{ sha: string, authorEmail: string, message: string }[]} commits
 * @returns {{ ok: true } | { ok: false, missing: string[] }}
 */
export const checkDco = (commits) => {
  const missing = [];
  for (const commit of commits) {
    const author = commit.authorEmail.toLowerCase();
    const emails = [...commit.message.matchAll(SIGN_OFF)].map((match) => match[1].toLowerCase());
    if (!emails.includes(author)) {
      missing.push(commit.sha);
    }
  }
  return missing.length === 0 ? { ok: true } : { ok: false, missing };
};

/**
 * Parses `git log --format=%H<US>%ae<US>%B<RS>` output.
 * @param {string} output
 * @returns {{ sha: string, authorEmail: string, message: string }[]}
 */
export const parseLog = (output) =>
  output
    .split(RECORD_SEPARATOR)
    .map((record) => record.replace(/^\n+/u, ""))
    .filter((record) => record.length > 0)
    .map((record) => {
      const [sha, authorEmail, ...rest] = record.split(FIELD_SEPARATOR);
      return { sha, authorEmail, message: rest.join(FIELD_SEPARATOR) };
    });

const git = (args) => execFileSync("git", args, { encoding: "utf8" }).trim();

/**
 * Checks one commit message before the commit exists, for the commit-msg hook.
 * A merge commit is skipped, as in the pull request check.
 * @param {string} messageFile
 * @returns {number} the exit code
 */
const checkMessageFile = (messageFile) => {
  const merging = spawnSync("git", ["rev-parse", "-q", "--verify", "MERGE_HEAD"], {
    stdio: "ignore",
  });
  if (merging.status === 0) {
    return 0;
  }
  const ident = git(["var", "GIT_AUTHOR_IDENT"]);
  const authorEmail = ident.match(/<([^<>]+)>/u)?.[1] ?? "";
  const message = readFileSync(messageFile, "utf8");
  if (checkDco([{ sha: "HEAD", authorEmail, message }]).ok) {
    return 0;
  }
  console.error(`commit-msg: no Signed-off-by line for the author <${authorEmail}>.`);
  console.error("Commit with `git commit -S -s`, or add the line with `git commit -s --amend`.");
  console.error("See the Developer Certificate of Origin section in CONTRIBUTING.md.");
  return 1;
};

const main = () => {
  const flag = process.argv.indexOf("--message-file");
  if (flag !== -1) {
    const messageFile = process.argv[flag + 1];
    if (!messageFile) {
      console.error("--message-file needs a path.");
      return 2;
    }
    return checkMessageFile(messageFile);
  }
  const base = process.env.BASE_SHA;
  if (!base) {
    console.error("BASE_SHA is not set.");
    return 2;
  }
  const output = execFileSync(
    "git",
    [
      "log",
      "--no-merges",
      `--format=%H${FIELD_SEPARATOR}%ae${FIELD_SEPARATOR}%B${RECORD_SEPARATOR}`,
      `${base}..HEAD`,
    ],
    { encoding: "utf8" },
  );
  const result = checkDco(parseLog(output));
  if (result.ok) {
    return 0;
  }
  console.error("These commits have no Signed-off-by line that matches the author:");
  for (const sha of result.missing) {
    console.error(`  ${sha}`);
  }
  console.error(
    "Sign off with `git commit -s --amend`, or `git rebase --signoff <base>` for several commits.",
  );
  console.error("See the Developer Certificate of Origin section in CONTRIBUTING.md.");
  return 1;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
