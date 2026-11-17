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
const WORKFLOW_REF = /^[^/]+\/[^/]+\/\.github\/workflows\/([^/@]+\.ya?ml)@/u;
const API_TIMEOUT_MS = 15_000;

/**
 * Return false only for a proven ordinary-main modification of maintainer
 * prose on top of a commit whose own run of this workflow succeeded.
 */
export const mainChecksRequired = ({ event_name, ref, subject, diff, baseVerified }) => {
  if (event_name !== "push" || ref !== "refs/heads/main") return true;
  if (typeof subject !== "string" || subject.startsWith("chore: release v")) return true;
  // A newer push can replace a queued run, so the previous commit may have no
  // result of its own. Its changes are then not in this diff.
  if (baseVerified !== true) return true;
  if (typeof diff !== "string" || !diff.endsWith("\0")) return true;
  const fields = diff.slice(0, -1).split("\0");
  if (fields.length === 0 || fields.length % 2 !== 0) return true;
  for (let index = 0; index < fields.length; index += 2) {
    if (fields[index] !== "M" || !MAINTAINER_PROSE.has(fields[index + 1])) return true;
  }
  return false;
};

/** Return true when a push run of this workflow on `sha` succeeded. */
const baseRunSucceeded = async (sha) => {
  const workflow = WORKFLOW_REF.exec(process.env.GITHUB_WORKFLOW_REF ?? "")?.[1];
  const repository = process.env.GITHUB_REPOSITORY;
  const token = process.env.GITHUB_TOKEN;
  if (!workflow || !repository || !token) return false;
  const api = process.env.GITHUB_API_URL ?? "https://api.github.com";
  const url = `${api}/repos/${repository}/actions/workflows/${workflow}/runs?head_sha=${sha}&event=push&per_page=20`;
  const response = await fetch(url, {
    headers: { accept: "application/vnd.github+json", authorization: `Bearer ${token}` },
    signal: AbortSignal.timeout(API_TIMEOUT_MS),
  });
  if (!response.ok) return false;
  const body = await response.json();
  const runs = Array.isArray(body?.workflow_runs) ? body.workflow_runs : [];
  return runs.some((run) => run.head_sha === sha && run.conclusion === "success");
};

const detect = async () => {
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
    const decision = {
      event_name: process.env.GITHUB_EVENT_NAME,
      ref: process.env.GITHUB_REF,
      subject: event.head_commit?.message,
      diff,
    };
    // Ask the API only when the diff alone allows a skip.
    if (mainChecksRequired({ ...decision, baseVerified: true })) return true;
    return mainChecksRequired({ ...decision, baseVerified: await baseRunSucceeded(before) });
  } catch {
    console.error("Change detection unavailable; running full main checks.");
    return true;
  }
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = `main-full=${await detect()}\n`;
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
  process.stdout.write(output);
}
