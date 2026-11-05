#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { basename, dirname, extname, isAbsolute, join, relative, resolve } from "node:path";
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
// Each phrase only makes sense to a reader who saw the edit happen. Fallow
// analyzes diffs, so "the change adds", "this commit fixes" and "the old
// version" are domain prose and stay out of this list. The lookbehinds keep
// "X is used to be Y" and sentence-initial "Used to".
const HISTORY_PHRASES = [
  String.raw`(?<!\b(?:is|are|be|been|data|value|values)\s)(?<![.!?]\s|^)\bused to be\b`,
  String.raw`\b(?:it|this|they|these|we) used to \w+`,
  String.raw`\bpreviously,? (?:this|it|we|the|these|those|a|an)\b`,
  String.raw`\boriginally,? (?:this|it|we)\b`,
  String.raw`\bwe changed\b`,
  String.raw`\bthe (?:old|previous) (?:behavior|code|implementation|logic)\b`,
  String.raw`\b(?:keeps?|preserves?|retains?) the (?:old|previous) behavior\b`,
  String.raw`\bas (?:requested|discussed|suggested|agreed)(?: (?:in|by|during) (?:the )?(?:review|reviewer|user|maintainer|chat|thread|discussion)\b|\s*[.!]?$)`,
  String.raw`\bper (?:the )?(?:review|reviewer|maintainer|user)(?:'s)? (?:feedback|request|comment|suggestion)s?\b`,
  String.raw`\bas per (?:the )?review\b`,
  String.raw`\bthis (?:change|fix|PR|pull request|commit|patch) exists to\b`,
  String.raw`\b(?:before|after|until) this (?:change|fix|PR|pull request|commit|patch)\b`,
];
const HISTORY_PHRASE = new RegExp(HISTORY_PHRASES.join("|"), "iu");
const JAVASCRIPT_EXTENSIONS = new Set([
  ".astro",
  ".cjs",
  ".js",
  ".jsx",
  ".mjs",
  ".svelte",
  ".ts",
  ".tsx",
  ".vue",
]);

const isSourcePath = (path) => SOURCE_EXTENSIONS.has(extname(path).toLowerCase());

// A `/` after one of these characters, or at the start of the line, starts a
// regex literal. After an identifier, a number or `)`, it is a division.
const REGEX_PRECEDER = /[(,=:[!&|?{};+\-*%<>~^]/u;
const REGEX_KEYWORD = /\b(?:return|typeof|case|do|else|in|of|void|yield|await)$/u;

const startsRegexLiteral = (line, index) => {
  const before = line.slice(0, index).trimEnd();
  return before.length === 0 || REGEX_PRECEDER.test(before.at(-1)) || REGEX_KEYWORD.test(before);
};

const regexLiteralEnd = (line, index) => {
  let inClass = false;
  for (let cursor = index + 1; cursor < line.length; cursor += 1) {
    const character = line[cursor];
    if (character === "\\") {
      cursor += 1;
    } else if (character === "[") {
      inClass = true;
    } else if (character === "]") {
      inClass = false;
    } else if (character === "/" && !inClass) {
      return cursor;
    }
  }
  return -1;
};

// Finds the first backtick that opens a template literal. A backtick inside a
// quoted string, a regex literal, or a comment opens nothing. Without this
// scan, the rest of the file reads as one template and hides every comment.
// Returns the last index of the slash construct at `index`, or -1 when the
// rest of the line is a comment or an unclosed block comment.
const slashConstructEnd = (line, index) => {
  const next = line[index + 1];
  if (next === "/") {
    return -1;
  }
  if (next === "*") {
    const close = line.indexOf("*/", index + 2);
    return close === -1 ? -1 : close + 1;
  }
  if (!startsRegexLiteral(line, index)) {
    return index;
  }
  const close = regexLiteralEnd(line, index);
  return close === -1 ? index : close;
};

// Returns the last index of the construct at `index` that cannot hold a
// template opener, or -1 when nothing after it can.
const skippedConstructEnd = (line, index) => {
  const character = line[index];
  if (character === '"' || character === "'") {
    return closingDelimiterIndex(line, character, index + 1, true);
  }
  return character === "/" ? slashConstructEnd(line, index) : index;
};

const templateOpenerIndex = (line, start) => {
  for (let index = start; index < line.length; index += 1) {
    if (line[index] === "`") {
      return index;
    }
    index = skippedConstructEnd(line, index);
    if (index === -1) {
      return -1;
    }
  }
  return -1;
};

const multilineOpener = (path, line, start) => {
  const extension = extname(path).toLowerCase();
  const candidates = [];

  if (JAVASCRIPT_EXTENSIONS.has(extension)) {
    const index = templateOpenerIndex(line, start);
    if (index !== -1) {
      candidates.push({ index, length: 1, delimiter: "`", escaped: true });
    }
  }

  if (extension === ".py") {
    for (const delimiter of ['"""', "'''"]) {
      const index = line.indexOf(delimiter, start);
      if (index !== -1) {
        candidates.push({ index, length: delimiter.length, delimiter, escaped: false });
      }
    }
  }

  if (extension === ".rs") {
    const rawString = /(?:br|r)(#*)"/gu;
    rawString.lastIndex = start;
    for (const match of line.matchAll(rawString)) {
      if (match.index > 0 && /[A-Za-z0-9_]/u.test(line[match.index - 1])) {
        continue;
      }
      candidates.push({
        index: match.index,
        length: match[0].length,
        delimiter: `"${match[1]}`,
        escaped: false,
      });
      break;
    }
  }

  return candidates.toSorted((left, right) => left.index - right.index)[0] ?? null;
};

const closingDelimiterIndex = (line, delimiter, start, escaped) => {
  let index = line.indexOf(delimiter, start);
  if (!escaped) {
    return index;
  }

  while (index !== -1) {
    let backslashes = 0;
    for (let cursor = index - 1; cursor >= 0 && line[cursor] === "\\"; cursor -= 1) {
      backslashes += 1;
    }
    if (backslashes % 2 === 0) {
      return index;
    }
    index = line.indexOf(delimiter, index + delimiter.length);
  }
  return index;
};

const maskMultilineStrings = (path, line, initialState) => {
  const masked = [...line];
  let state = initialState;
  let cursor = 0;

  while (cursor < line.length) {
    if (state === null) {
      const opener = multilineOpener(path, line, cursor);
      if (opener === null) {
        break;
      }
      const contentStart = opener.index + opener.length;
      const close = closingDelimiterIndex(line, opener.delimiter, contentStart, opener.escaped);
      const end = close === -1 ? line.length : close + opener.delimiter.length;
      masked.fill(" ", opener.index, end);
      if (close === -1) {
        state = { delimiter: opener.delimiter, escaped: opener.escaped };
        break;
      }
      cursor = end;
      continue;
    }

    const close = closingDelimiterIndex(line, state.delimiter, cursor, state.escaped);
    const end = close === -1 ? line.length : close + state.delimiter.length;
    masked.fill(" ", cursor, end);
    if (close === -1) {
      break;
    }
    state = null;
    cursor = end;
  }

  return { line: masked.join(""), state };
};

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

const lineCommentBodies = (line, includeDocs) => {
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
      const docComment =
        directComment && (line.startsWith("///", index) || line.startsWith("//!", index));
      if (docComment && !includeDocs) {
        return [];
      }
      bodies.push(line.slice(index + (docComment ? 3 : 2)).trim());
      index += 1;
    }
  }

  return bodies;
};

const commentBodies = (line, includeDocs) => {
  const trimmed = line.trimStart();
  const docBlock =
    trimmed.startsWith("/**") || trimmed.startsWith("/*!") || trimmed.startsWith("*");
  if (docBlock && !includeDocs) {
    return [];
  }

  const direct = trimmed.match(/^(?:#|\/\*+!?|\*|<!--)\s*(.*?)(?:\s*\*\/|\s*-->)?$/u);
  return [...(direct === null ? [] : [direct[1].trim()]), ...lineCommentBodies(line, includeDocs)];
};

const isNarratorBody = (body) => {
  if (!NARRATOR_START.test(body) && !STEP_START.test(body)) {
    return false;
  }

  return !KEEPER_KEYWORD.test(body) && !TOOL_DIRECTIVE.test(body) && !EXPLANATION_SIGNAL.test(body);
};

const isHistoryBody = (body) => HISTORY_PHRASE.test(body) && !TOOL_DIRECTIVE.test(body);

// Narration checks skip rustdoc and JSDoc, which may walk through API usage.
// History checks include them, because a contract must describe the present.
const commentKind = (line) => {
  if (commentBodies(line, false).some(isNarratorBody)) {
    return "narration";
  }
  return commentBodies(line, true).some(isHistoryBody) ? "history" : null;
};

export const scanSourceText = (path, source) => {
  if (!isSourcePath(path)) {
    return [];
  }

  const findings = [];
  let multilineState = null;
  for (const [index, line] of source.split(/\r?\n/u).entries()) {
    const masked = maskMultilineStrings(path, line, multilineState);
    multilineState = masked.state;
    const kind = commentKind(masked.line);
    if (kind !== null) {
      findings.push({ path, line: index + 1, text: line.trim(), kind });
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
      const kind = path !== null && isSourcePath(path) ? commentKind(line.slice(1)) : null;
      if (kind !== null) {
        findings.push({ path, line: newLine, text: line.slice(1).trim(), kind });
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

const scanDiff = (args, readSource) => {
  const candidates = scanUnifiedDiff(git(["diff", "--no-ext-diff", "--unified=0", ...args]));
  const findingLines = new Map();

  return candidates.filter((candidate) => {
    if (!findingLines.has(candidate.path)) {
      findingLines.set(
        candidate.path,
        new Set(scanSourceText(candidate.path, readSource(candidate.path)).map(({ line }) => line)),
      );
    }
    return findingLines.get(candidate.path).has(candidate.line);
  });
};

const usage = () => {
  console.error(
    "Usage: node scripts/check-comment-quality.mjs (--all | --staged | --working-tree) [--claude-hook]",
  );
  console.error(
    "       node scripts/check-comment-quality.mjs --claude-pre-tool < hook-input.json",
  );
};

const FINDING_GUIDANCE = {
  narration: {
    title: "Narrator-style comments found:",
    advice: "Remove routine narration or replace it with non-obvious rationale.",
  },
  history: {
    title: "Comments that describe the edit history found:",
    advice:
      "Describe the code as it is now, for a reader who did not see the edit. For a regression, state what breaks without the code. Put history in the commit message.",
  },
};

const reportFindings = (findings, formatLocation) => {
  for (const kind of Object.keys(FINDING_GUIDANCE)) {
    const matching = findings.filter((finding) => finding.kind === kind);
    if (matching.length === 0) {
      continue;
    }
    console.error(FINDING_GUIDANCE[kind].title);
    for (const finding of matching) {
      console.error(`  ${formatLocation(finding)}${finding.text}`);
    }
    console.error(FINDING_GUIDANCE[kind].advice);
  }
};

const readHookInput = () => {
  try {
    return JSON.parse(readFileSync(0, "utf8"));
  } catch {
    return null;
  }
};

// Returns the file content after the edit, or null when the edit cannot apply.
// The tool itself rejects such an edit, so the guard has nothing to judge.
const applyProposedEdit = (toolInput, current) => {
  if (typeof toolInput.content === "string") {
    return toolInput.content;
  }
  if (current === null) {
    return null;
  }
  const edits = Array.isArray(toolInput.edits) ? toolInput.edits : [toolInput];
  let result = current;
  for (const edit of edits) {
    if (typeof edit?.old_string !== "string" || typeof edit.new_string !== "string") {
      return null;
    }
    if (!result.includes(edit.old_string)) {
      return null;
    }
    result =
      edit.replace_all === true
        ? result.split(edit.old_string).join(edit.new_string)
        : result.replace(edit.old_string, () => edit.new_string);
  }
  return result;
};

const findingCounts = (findings) => {
  const counts = new Map();
  for (const { text } of findings) {
    counts.set(text, (counts.get(text) ?? 0) + 1);
  }
  return counts;
};

// The guard scans the whole file before and after the edit, so string
// masking sees the full context, and only comments that the edit adds count.
// Malformed input passes, because a guard must not stop unrelated tool use.
export const scanProposedEdit = (hookInput, readCurrent) => {
  const toolInput = hookInput?.tool_input;
  const path = toolInput?.file_path;
  if (typeof path !== "string" || !isSourcePath(path)) {
    return [];
  }
  const current = readCurrent(path);
  const after = applyProposedEdit(toolInput, current);
  if (after === null) {
    return [];
  }
  const existing = findingCounts(current === null ? [] : scanSourceText(path, current));
  return scanSourceText(path, after).filter(({ text }) => {
    const remaining = existing.get(text) ?? 0;
    existing.set(text, remaining - 1);
    return remaining <= 0;
  });
};

const readIfPresent = (path) => (existsSync(path) ? readFileSync(path, "utf8") : null);

// Git reports the root through symbolic links, for example `/tmp` as
// `/private/tmp` on macOS. A new file has no real path yet, so resolve its
// nearest existing ancestor.
const realPath = (path) => {
  if (existsSync(path)) {
    return realpathSync(path);
  }
  const parent = dirname(path);
  return parent === path ? path : join(realPath(parent), basename(path));
};

// The guard covers tracked and untracked repository files, the same scope as
// the Stop hook. Ignored scratch files and other repositories stay out.
const isGuardedPath = (path) => {
  try {
    const root = git(["rev-parse", "--show-toplevel"]).trim();
    const absolute = realPath(resolve(path));
    const inside = relative(root, absolute);
    if (inside === "" || inside.startsWith("..") || isAbsolute(inside)) {
      return false;
    }
    execFileSync("git", ["check-ignore", "--quiet", "--", absolute], { stdio: "ignore" });
    return false;
  } catch (error) {
    return error?.status === 1;
  }
};

const runPreTool = () => {
  const hookInput = readHookInput();
  const path = hookInput?.tool_input?.file_path;
  if (typeof path !== "string" || !isGuardedPath(path)) {
    return;
  }
  const findings = scanProposedEdit(hookInput, readIfPresent);
  if (findings.length === 0) {
    return;
  }
  reportFindings(findings, () => "");
  process.exitCode = 2;
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
  if (args.size === 1 && args.has("--claude-pre-tool")) {
    runPreTool();
    return;
  }
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
    findings = scanDiff(["--cached", "--diff-filter=ACMR"], (path) => git(["show", `:${path}`]));
  } else {
    findings = [
      ...scanDiff(["HEAD", "--diff-filter=ACMR"], (path) => readFileSync(path, "utf8")),
      ...scanUntrackedFiles(),
    ];
  }

  if (findings.length === 0) {
    return;
  }

  reportFindings(findings, (finding) => `${finding.path}:${finding.line}: `);
  process.exitCode = args.has("--claude-hook") ? 2 : 1;
};

const invokedPath =
  process.argv[1] === undefined ? null : pathToFileURL(resolve(process.argv[1])).href;
if (invokedPath === import.meta.url) {
  run();
}
