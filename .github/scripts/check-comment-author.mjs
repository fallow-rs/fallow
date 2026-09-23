#!/usr/bin/env node
// Check the author of the pull-request comment that the action smoke test in
// test-action.yml posts. The Fallow token broker can fall back to the default
// token, for example on a timeout. The comment author then is
// github-actions[bot], and the check names the fallback cause.
//
// Env: COMMENT_AUTHOR, and FALLOW_TOKEN_BRANDED plus
// FALLOW_TOKEN_FALLBACK_REASON from action/scripts/broker-token.sh.
//
// Run: node .github/scripts/check-comment-author.mjs

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BRANDED_AUTHOR = "fallow-cloud[bot]";
const FALLBACK_AUTHOR = "github-actions[bot]";

/**
 * Compare the comment author with the author that the broker outcome implies.
 *
 * @param {{ author: string, branded: string | undefined, reason?: string }} input
 * @returns {{ ok: boolean, message: string }}
 */
export const checkCommentAuthor = ({ author, branded, reason }) => {
  if (branded !== "true" && branded !== "false") {
    return {
      ok: false,
      message:
        `FALLOW_TOKEN_BRANDED is '${branded ?? ""}', not true or false. ` +
        "The branded token step did not record its outcome.",
    };
  }
  const expected = branded === "true" ? BRANDED_AUTHOR : FALLBACK_AUTHOR;
  if (author !== expected) {
    return { ok: false, message: `Expected comment author ${expected}, found ${author || "none"}` };
  }
  if (branded === "true") {
    return { ok: true, message: `The comment author is ${author}, from the branded token.` };
  }
  return {
    ok: true,
    message:
      `The comment author is ${author}, because the branded token fell back to the default ` +
      `token: ${reason || "no cause recorded"}.`,
  };
};

/** @returns {number} */
export const main = (env = process.env) => {
  const result = checkCommentAuthor({
    author: env.COMMENT_AUTHOR ?? "",
    branded: env.FALLOW_TOKEN_BRANDED,
    reason: env.FALLOW_TOKEN_FALLBACK_REASON,
  });
  if (!result.ok) {
    console.log(`::error::${result.message}`);
    return 1;
  }
  console.log(env.FALLOW_TOKEN_BRANDED === "true" ? result.message : `::notice::${result.message}`);
  return 0;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
