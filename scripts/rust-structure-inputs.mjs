import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

// Only known prose and frontend source can skip analysis. Keep build inputs,
// workflow/analyzer configuration and unfamiliar paths outside this list.
const ROOT_PROSE = new Set([
  "README.md",
  "CHANGELOG.md",
  "CONTRIBUTING.md",
  "CODE_OF_CONDUCT.md",
  "SECURITY.md",
]);
const PROSE_PATH = /^(?:docs|changelog\.d)\/.+\.md$/u;
const FRONTEND_SOURCE =
  /^(?:npm|editors|viz-frontend|tools|examples|benchmarks)\/.+\.(?:[cm]?[jt]s|[jt]sx|css|html)$/u;
const SHA = /^(?!0{40}$)[0-9a-f]{40}$/u;
const CHANGE_STATUS = new Set(["A", "M", "D"]);

const unrelated = (path) => {
  if (path.split("/").some((part) => part === "" || part === "." || part === "..")) return false;
  return ROOT_PROSE.has(path) || PROSE_PATH.test(path) || FRONTEND_SOURCE.test(path);
};

const git = (...args) =>
  execFileSync("git", args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

const detect = () => {
  if (process.env.GITHUB_EVENT_NAME !== "pull_request") return true;
  try {
    const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
    const base = event.pull_request?.base?.sha;
    const head = event.pull_request?.head?.sha;
    const checkout = process.env.GITHUB_SHA;
    if (!SHA.test(base) || !SHA.test(head) || !SHA.test(checkout)) return true;
    if (git("rev-parse", "HEAD").trim() !== checkout) return true;
    git("merge-base", "--is-ancestor", head, checkout);
    const mergeBase = git("merge-base", base, checkout).trim();
    const diff = git("diff", "--name-status", "-z", "--no-renames", mergeBase, checkout, "--");
    if (!diff.endsWith("\0")) return true;
    const fields = diff.slice(0, -1).split("\0");
    if (fields.length % 2 !== 0) return true;
    for (let index = 0; index < fields.length; index += 2) {
      if (!CHANGE_STATUS.has(fields[index]) || !unrelated(fields[index + 1])) return true;
    }
    return false;
  } catch {
    console.error("Rust Structure input detection unavailable; running analysis.");
    return true;
  }
};

const output = `scan=${detect()}\n`;
if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, output);
process.stdout.write(output);
