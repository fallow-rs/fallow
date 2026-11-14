// Tests for the agent-pick tracker in scripts/agent-pick/.
//
// Run: node --test scripts/agent-pick.test.mjs

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { AGENTS, parseClaudeOutput, parseCodexOutput } from "./agent-pick/agents.mjs";
import { classifyAnswer } from "./agent-pick/classify.mjs";
import { createFixture } from "./agent-pick/fixture.mjs";
import { checkOutDir, loadPrompts, parseOptions } from "./agent-pick/run.mjs";
import { formatSummary, summarize } from "./agent-pick/summary.mjs";

const RUN_SCRIPT = fileURLToPath(new URL("./agent-pick/run.mjs", import.meta.url));
const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const PROMPTS = loadPrompts();

const record = (agent, promptId, answer, extra = {}) => ({
  agent,
  promptId,
  status: "ok",
  model: `${agent}-model`,
  ...classifyAnswer(answer),
  ...extra,
});

test("the first pick is the tool that the answer names first", () => {
  const answer =
    "Run `npx knip` first. If you want more, fallow also finds duplicates. Knip again.";
  assert.deepEqual(classifyAnswer(answer), { firstPick: "knip", picks: ["knip", "fallow"] });
});

test("a specific ESLint plugin wins over the eslint name inside it", () => {
  const answer = "Use eslint-plugin-import with `import/no-cycle`, or madge. Plain ESLint cannot.";
  assert.deepEqual(classifyAnswer(answer), {
    firstPick: "eslint-plugin-import",
    picks: ["eslint-plugin-import", "madge", "eslint"],
  });
});

test("tool names match without case and only as whole words", () => {
  assert.deepEqual(classifyAnswer("FALLOW and Dependency-Cruiser (depcruise)").picks, [
    "fallow",
    "dependency-cruiser",
  ]);
  assert.deepEqual(classifyAnswer("The knipper and unknip words are not tools.").picks, []);
  assert.deepEqual(classifyAnswer("List the unimported files.").picks, []);
  assert.deepEqual(classifyAnswer("Run npx unimported.").picks, ["unimported"]);
});

test("an answer with no known tool has no first pick", () => {
  assert.deepEqual(classifyAnswer("Search with grep for exports."), {
    firstPick: null,
    picks: [],
  });
});

test("the summary gives each model and CLI version of an agent its own row", () => {
  const records = [
    record("codex", "unused-exports", "fallow", { model: "model-a", cliVersion: "1.0" }),
    record("codex", "unused-exports", "knip", { model: "model-b", cliVersion: "1.0" }),
    record("codex", "unused-exports", "knip", { model: "model-b", cliVersion: "1.1" }),
    record("codex", "unused-exports", "knip", { model: "model-b", cliVersion: "1.1" }),
  ];
  const summary = summarize(records);
  assert.deepEqual(
    summary.agents.map(({ model, cliVersion, runs }) => `${model}@${cliVersion}:${runs}`),
    ["model-a@1.0:1", "model-b@1.0:1", "model-b@1.1:2"],
  );
  assert.deepEqual(
    summary.prompts.map(({ model, cliVersion, runs }) => `${model}@${cliVersion}:${runs}`),
    ["model-a@1.0:1", "model-b@1.0:1", "model-b@1.1:2"],
  );
  const table = formatSummary(summary);
  assert.match(table, /\| codex \| 1\.0 \| model-a \| 1 \| 0 \| 100% \| 100% \| fallow 1 \|/u);
  assert.match(table, /\| codex \| 1\.1 \| model-b \| 2 \| 0 \| 0% \| 0% \| knip 2 \|/u);
  assert.match(table, /\| unused-exports \| codex \| 1\.1 \| model-b \| 2 \| 0 \|/u);
});

test("the summary counts first picks and mentions per agent and per prompt", () => {
  const records = [
    record("claude", "unused-exports", "fallow, then knip"),
    record("claude", "unused-exports", "knip"),
    record("claude", "duplicated-code", "jscpd or fallow dupes"),
    record("codex", "unused-exports", "ts-prune"),
    { agent: "codex", promptId: "unused-exports", status: "error", firstPick: null, picks: [] },
  ];
  const summary = summarize(records, [{ agent: "gemini", reason: "not on PATH" }]);
  const claude = summary.agents.find((group) => group.agent === "claude");
  assert.equal(claude.runs, 3);
  assert.deepEqual(claude.firstPicks, { fallow: 1, knip: 1, jscpd: 1 });
  assert.deepEqual(claude.mentions, { fallow: 2, knip: 2, jscpd: 1 });
  const codex = summary.agents.filter((group) => group.agent === "codex");
  assert.deepEqual(
    codex.map(({ model, runs, errors }) => [model, runs, errors]),
    [
      [null, 0, 1],
      ["codex-model", 1, 0],
    ],
  );
  assert.deepEqual(
    summary.prompts.map(({ promptId, agent }) => `${promptId}/${agent}`),
    [
      "duplicated-code/claude",
      "unused-exports/claude",
      "unused-exports/codex",
      "unused-exports/codex",
    ],
  );
});

test("the Markdown summary shows the fallow share, ranked picks and skipped agents", () => {
  const summary = summarize(
    [
      record("claude", "unused-exports", "knip"),
      record("claude", "unused-exports", "knip, fallow"),
      record("claude", "unused-exports", "fallow"),
      record("claude", "unused-exports", "grep"),
    ],
    [{ agent: "codex", reason: "codex is not on PATH" }],
  );
  const table = formatSummary(summary);
  assert.match(
    table,
    /\| claude \| - \| claude-model \| 4 \| 0 \| 25% \| 50% \| knip 2, \(none\) 1, fallow 1 \|/u,
  );
  assert.match(
    table,
    /\| unused-exports \| claude \| - \| claude-model \| 4 \| 0 \| .* \| fallow 2\/4, knip 2\/4 \|/u,
  );
  assert.match(table, /## Skipped agents\n\n- codex: codex is not on PATH/u);
});

test("the claude parser reads the answer, the main model and the cost", () => {
  const stdout = JSON.stringify({
    type: "result",
    subtype: "success",
    is_error: false,
    result: "Use fallow.",
    total_cost_usd: 0.02,
    modelUsage: {
      "small-model": { outputTokens: 10 },
      "main-model": { outputTokens: 400 },
    },
  });
  assert.deepEqual(parseClaudeOutput(stdout), {
    ok: true,
    data: { text: "Use fallow.", model: "main-model", costUsd: 0.02 },
  });
  assert.equal(parseClaudeOutput("Not logged in").ok, false);
  assert.equal(
    parseClaudeOutput(JSON.stringify({ is_error: true, result: "Invalid API key" })).error,
    "claude returned an error: Invalid API key",
  );
});

// The stderr header of `codex exec` (codex-cli 0.167.0) without `--json`. The
// `--json` events carry no model, so the parser reads this header.
const codexStderr = (prompt) =>
  [
    "OpenAI Codex v0.167.0",
    "--------",
    "workdir: /tmp/run/fixture",
    "model: gpt-test-1",
    "provider: openai",
    "sandbox: read-only",
    "--------",
    "user",
    prompt,
    "",
    "codex",
    "Use knip.",
    "tokens used",
    "12,535",
  ].join("\n");

test("the codex parser takes the last message and reads the model from the header", () => {
  const stderr = codexStderr("Which tool finds unused exports?");
  assert.deepEqual(parseCodexOutput({ stdout: "Use knip.\n", stderr }, null), {
    ok: true,
    data: { text: "Use knip.", model: "gpt-test-1", costUsd: null },
  });
  assert.equal(
    parseCodexOutput({ stdout: "Use knip.\n", stderr }, "Use madge.").data.text,
    "Use madge.",
  );
  assert.equal(
    parseCodexOutput({ stdout: "", stderr: `${stderr}\nERROR: not logged in` }, null).error,
    "codex returned no answer: not logged in",
  );
});

test("the codex parser ignores a model line in the echoed prompt", () => {
  const stderr = codexStderr("model: prompt-model").replace("model: gpt-test-1\n", "");
  assert.equal(parseCodexOutput({ stdout: "Use knip.", stderr }, null).data.model, null);
});

test("the codex call skips the user config, plugins, apps, memories and hooks", () => {
  const args = AGENTS.codex.args("/tmp/run");
  for (const flag of ["--ignore-user-config", "--ignore-rules", "--ephemeral"]) {
    assert.ok(args.includes(flag), `${flag} is set`);
  }
  const disabled = args.flatMap((arg, index) => (args[index - 1] === "--disable" ? [arg] : []));
  assert.deepEqual(disabled.toSorted(), ["apps", "hooks", "memories", "plugins", "remote_plugin"]);
  assert.ok(!args.includes("--json"), "--json hides the model header");
});

test("the prompt set has unique ids and no prompt names a known tool", () => {
  const ids = PROMPTS.prompts.map(({ id }) => id);
  assert.equal(new Set(ids).size, ids.length);
  for (const prompt of PROMPTS.prompts) {
    assert.ok(prompt.category, `${prompt.id} has a category`);
    assert.deepEqual(classifyAnswer(prompt.text).picks, [], `${prompt.id} names no tool`);
  }
  assert.deepEqual(classifyAnswer(PROMPTS.instruction).picks, []);
});

test("the options reject an unknown agent, an unknown prompt and a bad run count", () => {
  assert.match(parseOptions(["--agents", "gemini"], PROMPTS).error, /Unknown agent: gemini/u);
  assert.match(parseOptions(["--prompts", "nope"], PROMPTS).error, /Unknown prompt id: nope/u);
  assert.match(parseOptions(["--runs", "0"], PROMPTS).error, /--runs needs a positive integer/u);
  const options = parseOptions(
    ["--agents", "claude", "--prompts", "circular-imports", "--runs", "3"],
    PROMPTS,
    new Date("2026-11-10T12:00:00Z"),
  ).data;
  assert.deepEqual(options.agents, ["claude"]);
  assert.deepEqual(options.promptIds, ["circular-imports"]);
  assert.equal(options.runs, 3);
  assert.equal(options.outDir, join(REPO_ROOT, ".plans", "agent-pick", "2026-11-10"));
});

test("the output directory must be outside the repository or ignored by git", () => {
  assert.equal(checkOutDir(join(REPO_ROOT, ".plans", "agent-pick", "x")).ok, true);
  assert.equal(checkOutDir(join(tmpdir(), "agent-pick-out")).ok, true);
  assert.match(checkOutDir(join(REPO_ROOT, "scripts", "agent-pick")).error, /does not ignore/u);
});

test("the fixture holds a TypeScript project and its removal deletes the run directory", () => {
  const fixture = createFixture();
  try {
    const manifest = JSON.parse(readFileSync(join(fixture.fixtureDir, "package.json"), "utf8"));
    assert.equal(manifest.name, "shop-app");
    assert.ok(existsSync(join(fixture.fixtureDir, "src", "index.ts")));
  } finally {
    fixture.remove();
  }
  assert.equal(existsSync(fixture.runDir), false);
});

test("a dry run prints the commands and calls no agent", () => {
  const run = spawnSync(
    process.execPath,
    [RUN_SCRIPT, "--dry-run", "--prompts", "duplicated-code", "--runs", "2"],
    { encoding: "utf8", env: { ...process.env, PATH: "" } },
  );
  assert.equal(run.status, 0, run.stderr);
  assert.match(run.stdout, /# claude: not found, would skip/u);
  assert.match(run.stdout, /claude -p --output-format json .*--tools Read,Glob,Grep/u);
  assert.match(run.stdout, /codex exec --sandbox read-only .*--ignore-user-config/u);
  assert.match(run.stdout, /Dry run: nothing was run\./u);
});
