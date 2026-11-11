// Command lines and output parsers for the agent CLIs that the tracker calls.
// Each command gets the prompt on standard input, so a variadic flag such as
// `--tools` cannot take the prompt as one of its values.

import { readFileSync } from "node:fs";
import { join } from "node:path";

/** The file name that `codex exec -o` writes the last agent message to. */
const CODEX_LAST_MESSAGE = "codex-last-message.txt";

/** The codex features that load local extensions into the agent context. */
const CODEX_DISABLED_FEATURES = ["plugins", "remote_plugin", "apps", "memories", "hooks"];

/**
 * Pick the model that produced the most output tokens. Claude Code can call
 * a small model for internal steps, and that model is not the one that wrote
 * the answer.
 */
const mainClaudeModel = (modelUsage) => {
  const entries = Object.entries(modelUsage ?? {});
  if (entries.length === 0) return null;
  return entries.toSorted(
    ([, left], [, right]) => (right?.outputTokens ?? 0) - (left?.outputTokens ?? 0),
  )[0][0];
};

/**
 * Parse the single JSON object of `claude -p --output-format json`.
 * @param {string} stdout
 * @returns {{ ok: true, data: { text: string, model: string | null, costUsd: number | null } }
 *   | { ok: false, error: string }}
 */
export const parseClaudeOutput = (stdout) => {
  let result;
  try {
    result = JSON.parse(stdout);
  } catch {
    return { ok: false, error: "claude did not print a JSON result" };
  }
  if (result?.is_error || typeof result?.result !== "string") {
    const detail = typeof result?.result === "string" ? result.result : result?.subtype;
    return { ok: false, error: `claude returned an error: ${detail ?? "no result"}` };
  }
  return {
    ok: true,
    data: {
      text: result.result,
      model: mainClaudeModel(result.modelUsage),
      costUsd: typeof result.total_cost_usd === "number" ? result.total_cost_usd : null,
    },
  };
};

/** The line that opens and closes the settings header of `codex exec`. */
const CODEX_HEADER_RULE = "--------";
const CODEX_MODEL_LINE = /^model: (?<model>\S+)$/u;
const CODEX_ERROR_LINE = /^ERROR: (?<message>.+)$/u;

/**
 * Read the model from the settings header that `codex exec` prints to
 * standard error. The `--json` events carry no model, so the codex call does
 * not use `--json`. Only the lines between the first two rules count, because
 * the prompt echo after the header can hold any text.
 */
const codexHeaderModel = (stderr) => {
  const lines = stderr.split("\n").map((line) => line.trimEnd());
  const open = lines.indexOf(CODEX_HEADER_RULE);
  if (open === -1) return null;
  const close = lines.indexOf(CODEX_HEADER_RULE, open + 1);
  if (close === -1) return null;
  for (const line of lines.slice(open + 1, close)) {
    const match = CODEX_MODEL_LINE.exec(line);
    if (match) return match.groups.model;
  }
  return null;
};

/**
 * Parse the output of `codex exec` without `--json`. The text comes from the
 * last-message file when it exists, and else from standard output, which holds
 * only the last agent message.
 * @param {{ stdout: string, stderr: string }} output
 * @param {string | null} lastMessage
 * @returns {{ ok: true, data: { text: string, model: string | null, costUsd: null } }
 *   | { ok: false, error: string }}
 */
export const parseCodexOutput = ({ stdout, stderr }, lastMessage) => {
  const text = lastMessage?.trim() ? lastMessage : stdout.trim();
  if (!text) {
    const detail = stderr
      .split("\n")
      .map((line) => CODEX_ERROR_LINE.exec(line.trim())?.groups.message)
      .findLast(Boolean);
    return { ok: false, error: `codex returned no answer${detail ? `: ${detail}` : ""}` };
  }
  return { ok: true, data: { text, model: codexHeaderModel(stderr), costUsd: null } };
};

/**
 * The supported agents. `args` receives the run directory, which holds the
 * fixture project in `fixture/` and is a safe place for CLI side files.
 * `parse` receives the standard output and standard error of the call.
 */
export const AGENTS = {
  claude: {
    binary: "claude",
    args: () => [
      "-p",
      "--output-format",
      "json",
      "--no-session-persistence",
      // The answer must reflect the agent itself, not the plugins, skills,
      // memory and MCP servers of the machine that runs the tracker.
      "--safe-mode",
      "--strict-mcp-config",
      // Read-only tools let the agent look at the fixture. Without Bash and
      // the edit tools it cannot install, run or change anything.
      "--tools",
      "Read,Glob,Grep",
      "--permission-mode",
      "dontAsk",
      "--max-budget-usd",
      "1",
    ],
    parse: ({ stdout }) => parseClaudeOutput(stdout),
  },
  codex: {
    binary: "codex",
    args: (runDir) => [
      "exec",
      // The read-only sandbox blocks file writes and network for every
      // command that the agent runs.
      "--sandbox",
      "read-only",
      "--ephemeral",
      "--skip-git-repo-check",
      // The answer must reflect the agent itself, not the config, plugins,
      // apps, memories and hooks of the machine that runs the tracker.
      "--ignore-user-config",
      "--ignore-rules",
      ...CODEX_DISABLED_FEATURES.flatMap((feature) => ["--disable", feature]),
      "--color",
      "never",
      "-o",
      join(runDir, CODEX_LAST_MESSAGE),
      "-",
    ],
    parse: (output, runDir) => {
      let lastMessage = null;
      try {
        lastMessage = readFileSync(join(runDir, CODEX_LAST_MESSAGE), "utf8");
      } catch {
        lastMessage = null;
      }
      return parseCodexOutput(output, lastMessage);
    },
  },
};
