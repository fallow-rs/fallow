import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Keep this list limited to maintainer prose with no runtime or build consumers.
const MAINTAINER_PROSE = new Set([
  "docs/development/ai-tooling.md",
  "docs/development/review-routing.md",
]);
const SHA = /^[0-9a-f]{40}$/u;
const ZERO_SHA = "0".repeat(40);

/** Return false only for a proven ordinary-main modification of maintainer prose. */
export const mainChecksRequired = ({ event_name, ref, subject, diff }) => {
  if (event_name !== "push" || ref !== "refs/heads/main") return true;
  if (typeof subject !== "string" || subject.startsWith("chore: release v")) return true;
  if (typeof diff !== "string" || !diff.endsWith("\0")) return true;
  const fields = diff.slice(0, -1).split("\0");
  if (fields.length === 0 || fields.length % 2 !== 0) return true;
  for (let index = 0; index < fields.length; index += 2) {
    if (fields[index] !== "M" || !MAINTAINER_PROSE.has(fields[index + 1])) return true;
  }
  return false;
};

const detect = () => {
  if (process.env.GITHUB_EVENT_NAME !== "push") return true;
  try {
    const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
    const { before, after } = event;
    if (!SHA.test(before) || !SHA.test(after) || before === ZERO_SHA || after === ZERO_SHA)
      return true;
    if (process.env.GITHUB_SHA && process.env.GITHUB_SHA !== after) return true;
    const subject = execFileSync("git", ["log", "-1", "--format=%s", after], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    if (subject.startsWith("chore: release v")) return true;
    const diff = execFileSync(
      "git",
      ["diff", "--name-status", "-z", "--no-renames", before, after, "--"],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
    return mainChecksRequired({
      event_name: process.env.GITHUB_EVENT_NAME,
      ref: process.env.GITHUB_REF,
      subject: event.head_commit?.message,
      diff,
    });
  } catch {
    console.error("Change detection unavailable; running full main checks.");
    return true;
  }
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = `main-full=${detect()}\n`;
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
  process.stdout.write(output);
}
