#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { extname, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const SOURCE_EXTENSIONS = new Set([
  ".astro",
  ".bash",
  ".c",
  ".cc",
  ".cjs",
  ".cpp",
  ".css",
  ".cs",
  ".go",
  ".h",
  ".hpp",
  ".htm",
  ".html",
  ".js",
  ".jsx",
  ".java",
  ".kt",
  ".kts",
  ".mjs",
  ".php",
  ".py",
  ".rb",
  ".rs",
  ".scss",
  ".sh",
  ".svelte",
  ".swift",
  ".toml",
  ".ts",
  ".tsx",
  ".vue",
  ".yaml",
  ".yml",
  ".zig",
]);

const NARRATOR_START = /^(?:here we|now we|let's|next we|finally we|first we)\b/iu;
const STEP_START = /^step\s+\d+\b/iu;
const KEEPER_KEYWORD = /\b(?:TODO|FIXME|HACK|NOTE|SAFETY|WARN(?:ING)?|BUG|XXX|PERF)\b/u;
const TOOL_DIRECTIVE =
  /\b(?:fallow-ignore|eslint|oxlint|prettier|rustfmt|clippy|ts-expect-error|ts-ignore|istanbul ignore|c8 ignore)\b/iu;
const EXPLANATION_SIGNAL =
  /\b(?:because|since|so that|otherwise|avoid|prevent|workaround|compatib\w*|invariant|safety|performance|protocol)\b/iu;

const isSourcePath = (path) => SOURCE_EXTENSIONS.has(extname(path).toLowerCase());

const hasClosingQuote = (line, start, quote) => {
  for (let index = start + 1; index < line.length; index += 1) {
    if (line[index] === "\\") {
      index += 1;
    } else if (line[index] === quote) {
      return true;
    }
  }
  return false;
};

const isRustLifetime = (line, index) => {
  if (line[index] !== "'") {
    return false;
  }

  const name = line.slice(index + 1).match(/^[A-Za-z_][A-Za-z0-9_]*/u)?.[0];
  if (name === undefined || line[index + name.length + 1] === "'") {
    return false;
  }

  const previous = line[index - 1];
  return previous === "&" || previous === "<" || previous === ",";
};

const lineCommentBodies = (line) => {
  const bodies = [];
  let quote = null;

  for (let index = 0; index < line.length - 1; index += 1) {
    const character = line[index];
    if (quote !== null) {
      if (character === "\\") {
        index += 1;
      } else if (character === quote) {
        quote = null;
      }
      continue;
    }

    if (
      (character === '"' || character === "'" || character === "`") &&
      !isRustLifetime(line, index) &&
      hasClosingQuote(line, index, character)
    ) {
      quote = character;
      continue;
    }

    if (character === "/" && line[index + 1] === "/") {
      const directComment = line.slice(0, index).trim().length === 0;
      if (directComment && (line.startsWith("///", index) || line.startsWith("//!", index))) {
        return [];
      }
      bodies.push(line.slice(index + 2).trim());
      index += 1;
    }
  }

  return bodies;
};

const commentBodies = (line) => {
  const trimmed = line.trimStart();
  if (trimmed.startsWith("/**") || trimmed.startsWith("/*!") || trimmed.startsWith("*")) {
    return [];
  }

  const direct = trimmed.match(/^(?:#|\/\*+|<!--)\s*(.*?)(?:\s*\*\/|\s*-->)?$/u);
  return [...(direct === null ? [] : [direct[1].trim()]), ...lineCommentBodies(line)];
};

const isNarratorBody = (body) => {
  if (!NARRATOR_START.test(body) && !STEP_START.test(body)) {
    return false;
  }

  return !KEEPER_KEYWORD.test(body) && !TOOL_DIRECTIVE.test(body) && !EXPLANATION_SIGNAL.test(body);
};

const isNarratorComment = (line) => commentBodies(line).some(isNarratorBody);

export const scanSourceText = (path, source) => {
  if (!isSourcePath(path)) {
    return [];
  }

  const findings = [];
  for (const [index, line] of source.split(/\r?\n/u).entries()) {
    if (isNarratorComment(line)) {
      findings.push({ path, line: index + 1, text: line.trim() });
    }
  }
  return findings;
};

export const scanUnifiedDiff = (diff) => {
  const findings = [];
  let path = null;
  let newLine = 0;

  for (const line of diff.split(/\r?\n/u)) {
    if (line.startsWith("+++ ")) {
      const candidate = line.slice(4);
      path = candidate === "/dev/null" ? null : candidate.replace(/^b\//u, "");
      continue;
    }

    const hunk = line.match(/^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@/u);
    if (hunk !== null) {
      newLine = Number.parseInt(hunk[1], 10);
      continue;
    }

    if (line.startsWith("+") && !line.startsWith("+++")) {
      if (path !== null && isNarratorComment(line.slice(1)) && isSourcePath(path)) {
        findings.push({ path, line: newLine, text: line.slice(1).trim() });
      }
      newLine += 1;
      continue;
    }

    if (!line.startsWith("-") && !line.startsWith("\\")) {
      newLine += 1;
    }
  }

  return findings;
};

const git = (args) =>
  execFileSync("git", args, {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });

const scanTrackedFiles = () => {
  const paths = git(["ls-files", "-z"]).split("\0").filter(Boolean);
  return paths.flatMap((path) =>
    isSourcePath(path) ? scanSourceText(path, readFileSync(path, "utf8")) : [],
  );
};

const scanUntrackedFiles = () => {
  const paths = git(["ls-files", "--others", "--exclude-standard", "-z"])
    .split("\0")
    .filter(Boolean);
  return paths.flatMap((path) =>
    isSourcePath(path) ? scanSourceText(path, readFileSync(path, "utf8")) : [],
  );
};

const scanDiff = (args) => scanUnifiedDiff(git(["diff", "--no-ext-diff", "--unified=0", ...args]));

const usage = () => {
  console.error(
    "Usage: node scripts/check-comment-quality.mjs (--all | --staged | --working-tree) [--claude-hook]",
  );
};

const repeatedClaudeStop = () => {
  try {
    const input = readFileSync(0, "utf8").trim();
    return input.length > 0 && JSON.parse(input).stop_hook_active === true;
  } catch {
    return false;
  }
};

const run = () => {
  const args = new Set(process.argv.slice(2));
  const modes = ["--all", "--staged", "--working-tree"].filter((mode) => args.has(mode));
  const allowed = new Set([...modes, "--claude-hook"]);
  const unknown = [...args].filter((arg) => !allowed.has(arg));

  if (modes.length !== 1 || unknown.length > 0) {
    usage();
    process.exitCode = 2;
    return;
  }

  if (args.has("--claude-hook") && repeatedClaudeStop()) {
    return;
  }

  let findings;
  if (modes[0] === "--all") {
    findings = scanTrackedFiles();
  } else if (modes[0] === "--staged") {
    findings = scanDiff(["--cached", "--diff-filter=ACMR"]);
  } else {
    findings = [...scanDiff(["HEAD", "--diff-filter=ACMR"]), ...scanUntrackedFiles()];
  }

  if (findings.length === 0) {
    return;
  }

  console.error("Narrator-style comments found:");
  for (const finding of findings) {
    console.error(`  ${finding.path}:${finding.line}: ${finding.text}`);
  }
  console.error("Remove routine narration or replace it with non-obvious rationale.");
  process.exitCode = args.has("--claude-hook") ? 2 : 1;
};

const invokedPath =
  process.argv[1] === undefined ? null : pathToFileURL(resolve(process.argv[1])).href;
if (invokedPath === import.meta.url) {
  run();
}
