// Command lines and output parsers for the agent CLIs that the tracker calls.
// Each command gets the prompt on standard input, so a variadic flag such as
// `--tools` cannot take the prompt as one of its values.

import { readFileSync } from "node:fs";
import { join } from "node:path";

/** The file name that `codex exec -o` writes the last agent message to. */
const CODEX_LAST_MESSAGE = "codex-last-message.txt";

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

const findModel = (value) => {
  if (value === null || typeof value !== "object") return null;
  if (typeof value.model === "string" && value.model.length > 0) return value.model;
  for (const child of Object.values(value)) {
    const model = findModel(child);
    if (model) return model;
  }
  return null;
};

/**
 * Parse the JSONL events of `codex exec --json`. The text comes from the
 * last-message file when it exists, and else from the last agent message event.
 * @param {string} stdout
 * @param {string | null} lastMessage
 * @returns {{ ok: true, data: { text: string, model: string | null, costUsd: null } }
 *   | { ok: false, error: string }}
 */
export const parseCodexOutput = (stdout, lastMessage) => {
  const events = stdout
    .split("\n")
    .filter((line) => line.trim().startsWith("{"))
    .flatMap((line) => {
      try {
        return [JSON.parse(line)];
      } catch {
        return [];
      }
    });
  const failure = events.find((event) => event.type === "error" || event.type === "turn.failed");
  const messages = events
    .map((event) => event.item)
    .filter((item) => item?.type === "agent_message" && typeof item.text === "string");
  const text = lastMessage?.trim() ? lastMessage : messages.at(-1)?.text;
  if (!text) {
    const detail = failure?.message ?? failure?.error?.message;
    return { ok: false, error: `codex returned no answer${detail ? `: ${detail}` : ""}` };
  }
  return { ok: true, data: { text, model: findModel(events), costUsd: null } };
};

/**
 * The supported agents. `args` receives the run directory, which holds the
 * fixture project in `fixture/` and is a safe place for CLI side files.
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
    parse: (stdout) => parseClaudeOutput(stdout),
  },
  codex: {
    binary: "codex",
    args: (runDir) => [
      "exec",
      "--json",
      // The read-only sandbox blocks file writes and network for every
      // command that the agent runs.
      "--sandbox",
      "read-only",
      "--ephemeral",
      "--skip-git-repo-check",
      "--color",
      "never",
      "-o",
      join(runDir, CODEX_LAST_MESSAGE),
      "-",
    ],
    parse: (stdout, runDir) => {
      let lastMessage = null;
      try {
        lastMessage = readFileSync(join(runDir, CODEX_LAST_MESSAGE), "utf8");
      } catch {
        lastMessage = null;
      }
      return parseCodexOutput(stdout, lastMessage);
    },
  },
};
