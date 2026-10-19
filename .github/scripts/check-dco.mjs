// Checks that every commit in a pull request carries a Developer Certificate
// of Origin sign-off: a `Signed-off-by: Name <email>` trailer whose email
// matches the commit author. Merge commits are skipped.
//
// Run: BASE_SHA=<sha> node .github/scripts/check-dco.mjs

import { execFileSync } from "node:child_process";
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

const main = () => {
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
