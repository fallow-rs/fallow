import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// The pull request path filter of pgo-validate.yml lists these two files, but
// most of their edits cannot change a PGO build: a version bump, a dependency
// entry, a lint, or a publish job. The weekly run covers dependency changes.
const CARGO_MANIFEST = "Cargo.toml";
/** The `pull_request.paths` filter of pgo-validate.yml, in the same order. */
export const PGO_PATHS = Object.freeze([
  "scripts/pgo-train.sh",
  ".github/scripts/pgo-compare.mjs",
  ".github/scripts/pgo-profile-match.mjs",
  "benchmarks/download-fixtures.mjs",
  ".github/actions/setup-rust/**",
  ".cargo/config.toml",
  ".github/workflows/pgo-validate.yml",
  ".github/workflows/release.yml",
  "Cargo.toml",
  "rust-toolchain.toml",
]);
const RELEASE_WORKFLOW = ".github/workflows/release.yml";
// The release jobs that train the profile and build with it.
const PGO_RELEASE_JOBS = ["pgo-profile", "build"];
const SHA = /^(?!0{40}$)[0-9a-f]{40}$/u;
const PROFILE_RELEASE_HEADER = /^\[profile\.release(?:[.\]])/u;

// A profile in a `[profile]` table, a dotted `profile.` key or an indented
// table header is a form that the section reader does not follow.
const UNREAD_PROFILE_FORM = /^[ \t]*\[profile\]|^[ \t]*profile[ \t]*\.|^[ \t]+\[[A-Za-z]/mu;

/**
 * Return the `[profile.release]` and `[profile.release.package.*]` sections,
 * or null when the manifest writes a profile in a form that this reader
 * does not follow.
 */
export const releaseProfileSections = (manifest) => {
  if (UNREAD_PROFILE_FORM.test(manifest)) return null;
  const sections = [];
  let current = null;
  for (const line of manifest.split("\n")) {
    if (line.startsWith("[")) {
      current = PROFILE_RELEASE_HEADER.test(line) ? [] : null;
      if (current) sections.push(current);
    }
    if (current) current.push(line.trimEnd());
  }
  return sections.map((section) => section.join("\n").trim()).join("\n\n");
};

/** Return the top-level `env` block of a workflow, or an empty string. */
export const workflowEnv = (workflow) => {
  const lines = workflow.split("\n");
  const start = lines.indexOf("env:");
  if (start === -1) return "";
  let end = start + 1;
  while (end < lines.length && !/^\S/u.test(lines[end])) end += 1;
  return lines.slice(start, end).join("\n").trimEnd();
};

/** Return the text of one top-level job of a workflow, or null when it is absent. */
export const workflowJob = (workflow, job) => {
  const lines = workflow.split("\n");
  const start = lines.indexOf(`  ${job}:`);
  if (start === -1) return null;
  let end = start + 1;
  while (end < lines.length && !/^ {2}\S/u.test(lines[end]) && !/^\S/u.test(lines[end])) end += 1;
  return lines.slice(start, end).join("\n").trimEnd();
};

const matchesFilter = (path) =>
  PGO_PATHS.some((pattern) =>
    pattern.endsWith("/**") ? path.startsWith(pattern.slice(0, -2)) : path === pattern,
  );

// For each file whose edits are only partly PGO inputs: the text that a PGO
// build reads. Two equal texts mean that the edit cannot change the build.
const PGO_VIEWS = new Map([
  [CARGO_MANIFEST, (text) => [releaseProfileSections(text)]],
  // The top-level env reaches every job, so it is part of the build.
  [
    RELEASE_WORKFLOW,
    (text) => [workflowEnv(text), ...PGO_RELEASE_JOBS.map((job) => workflowJob(text, job))],
  ],
]);

const pathNeedsValidation = (path, read) => {
  const view = PGO_VIEWS.get(path);
  if (!view) return true;
  const base = read("base", path);
  const head = read("head", path);
  if (base === null || head === null) return true;
  const before = view(base);
  const after = view(head);
  // A missing job cannot prove that the build stays the same.
  if (before.includes(null) || after.includes(null)) return true;
  return before.some((text, index) => text !== after[index]);
};

/**
 * Decide whether a pull request diff needs the PGO comparison.
 * `read(side, path)` returns the file text at `base` or `head`, or null.
 * A path outside the filter does not start the workflow on its own.
 */
export const pgoValidationRequired = (paths, read) =>
  paths.some((path) => matchesFilter(path) && pathNeedsValidation(path, read));

const readMergeDiff = () => {
  const git = (...args) =>
    execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
  const checkout = process.env.GITHUB_SHA;
  const base = event.pull_request?.base?.sha;
  if (!SHA.test(base) || !SHA.test(checkout)) return null;
  if (git("rev-parse", "HEAD").trim() !== checkout) return null;
  const mergeBase = git("merge-base", base, checkout).trim();
  const diff = git("diff", "--name-only", "-z", "--no-renames", mergeBase, checkout, "--");
  if (!diff.endsWith("\0")) return null;
  const read = (side, path) => {
    try {
      return git("show", `${side === "base" ? mergeBase : checkout}:${path}`);
    } catch {
      return null;
    }
  };
  return { paths: diff.slice(0, -1).split("\0"), read };
};

const detect = () => {
  if (process.env.GITHUB_EVENT_NAME !== "pull_request") return true;
  try {
    const diff = readMergeDiff();
    return diff === null || pgoValidationRequired(diff.paths, diff.read);
  } catch {
    console.error("PGO input detection unavailable; running the comparison.");
    return true;
  }
};

const started = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (started) {
  const line = `run=${detect()}\n`;
  process.stdout.write(line);
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, line);
}
