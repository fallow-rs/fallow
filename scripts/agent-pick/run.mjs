#!/usr/bin/env node
// Asks coding agent CLIs neutral cleanup questions and records which tools
// they recommend. See scripts/agent-pick/README.md.
//
// Usage: node scripts/agent-pick/run.mjs [--runs N] [--agents claude,codex]
//   [--prompts id,id] [--out DIR] [--timeout SECONDS] [--dry-run]

import { spawn, spawnSync } from "node:child_process";
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { AGENTS } from "./agents.mjs";
import { classifyAnswer } from "./classify.mjs";
import { createFixture } from "./fixture.mjs";
import { formatSummary, summarize } from "./summary.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "../..");
const DEFAULT_TIMEOUT_SECONDS = 300;
const USAGE = `Usage: node scripts/agent-pick/run.mjs [options]

  --runs N             Runs per prompt and agent (default 1)
  --agents LIST        Comma-separated agents: ${Object.keys(AGENTS).join(",")} (default all)
  --prompts LIST       Comma-separated prompt ids (default all)
  --out DIR            Output directory (default .plans/agent-pick/<date>/)
  --timeout SECONDS    Time limit per agent call (default ${DEFAULT_TIMEOUT_SECONDS})
  --dry-run            Print the commands and run nothing
`;

/** Load the prompt set. */
export const loadPrompts = (path = join(HERE, "prompts.json")) =>
  JSON.parse(readFileSync(path, "utf8"));

const splitList = (value) =>
  value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);

const positiveInteger = (value, flag) => {
  const number = Number(value);
  return Number.isInteger(number) && number > 0
    ? { ok: true, data: number }
    : { ok: false, error: `${flag} needs a positive integer, not "${value}".` };
};

/**
 * Parse and validate the command-line options.
 * @param {string[]} argv
 * @param {{ prompts: Array<{ id: string }> }} promptSet
 * @param {Date} [now]
 */
export const parseOptions = (argv, promptSet, now = new Date()) => {
  let values;
  try {
    ({ values } = parseArgs({
      args: argv,
      options: {
        runs: { type: "string", default: "1" },
        agents: { type: "string" },
        prompts: { type: "string" },
        out: { type: "string" },
        timeout: { type: "string", default: String(DEFAULT_TIMEOUT_SECONDS) },
        "dry-run": { type: "boolean", default: false },
        help: { type: "boolean", default: false },
      },
      strict: true,
    }));
  } catch (error) {
    return { ok: false, error: error.message };
  }
  if (values.help) return { ok: true, data: { help: true } };
  const runs = positiveInteger(values.runs, "--runs");
  if (!runs.ok) return runs;
  const timeout = positiveInteger(values.timeout, "--timeout");
  if (!timeout.ok) return timeout;
  const agents = values.agents ? splitList(values.agents) : Object.keys(AGENTS);
  const unknownAgents = agents.filter((agent) => !Object.hasOwn(AGENTS, agent));
  if (unknownAgents.length > 0 || agents.length === 0) {
    return { ok: false, error: `Unknown agent: ${unknownAgents.join(", ") || "(empty list)"}.` };
  }
  const knownIds = promptSet.prompts.map(({ id }) => id);
  const promptIds = values.prompts ? splitList(values.prompts) : knownIds;
  const unknownPrompts = promptIds.filter((id) => !knownIds.includes(id));
  if (unknownPrompts.length > 0 || promptIds.length === 0) {
    return {
      ok: false,
      error: `Unknown prompt id: ${unknownPrompts.join(", ") || "(empty list)"}. Known: ${knownIds.join(", ")}.`,
    };
  }
  const date = now.toISOString().slice(0, 10);
  return {
    ok: true,
    data: {
      help: false,
      runs: runs.data,
      timeoutMs: timeout.data * 1000,
      agents: [...new Set(agents)],
      promptIds: [...new Set(promptIds)],
      outDir: resolve(values.out ?? join(REPO_ROOT, ".plans", "agent-pick", date)),
      dryRun: values["dry-run"],
    },
  };
};

/**
 * Refuse an output directory inside the repository that git does not ignore,
 * so that results never land in tracked files.
 * @param {string} outDir
 * @param {string} [repoRoot]
 */
export const checkOutDir = (outDir, repoRoot = REPO_ROOT) => {
  const inside = relative(repoRoot, outDir);
  if (inside.startsWith("..") || isAbsolute(inside)) return { ok: true, data: outDir };
  const probe = join(inside, "results.jsonl");
  const ignored = spawnSync("git", ["check-ignore", "-q", "--no-index", probe], {
    cwd: repoRoot,
  });
  return ignored.status === 0
    ? { ok: true, data: outDir }
    : { ok: false, error: `${inside} is inside the repository and git does not ignore it.` };
};

const whichBinary = (binary) => {
  const found = spawnSync("which", [binary], { encoding: "utf8" });
  return found.status === 0 ? found.stdout.trim() : null;
};

const cliVersion = (binary) => {
  const version = spawnSync(binary, ["--version"], { encoding: "utf8", timeout: 30_000 });
  return version.status === 0 ? version.stdout.trim() : null;
};

const shellQuote = (value) =>
  /^[\w./:=,-]+$/u.test(value) ? value : `'${value.replaceAll("'", "'\\''")}'`;

/** Build the full prompt that the agent receives on standard input. */
const buildPrompt = (promptSet, prompt) => `${prompt.text}\n\n${promptSet.instruction}`;

const runCommand = (binary, args, { cwd, input, timeoutMs }) =>
  new Promise((done) => {
    const child = spawn(binary, args, { cwd, stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
    }, timeoutMs);
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (error) => {
      clearTimeout(timer);
      done({ status: null, stdout, stderr: error.message, timedOut });
    });
    child.on("close", (status) => {
      clearTimeout(timer);
      done({ status, stdout, stderr, timedOut });
    });
    child.stdin.end(input);
  });

const callAgent = async ({ agentId, prompt, promptSet, run, timeoutMs, version }) => {
  const agent = AGENTS[agentId];
  const fixture = createFixture();
  const started = Date.now();
  try {
    const result = await runCommand(agent.binary, agent.args(fixture.runDir), {
      cwd: fixture.fixtureDir,
      input: buildPrompt(promptSet, prompt),
      timeoutMs,
    });
    const parsed = agent.parse(result, fixture.runDir);
    const base = {
      timestamp: new Date(started).toISOString(),
      agent: agentId,
      cliVersion: version,
      promptId: prompt.id,
      category: prompt.category,
      run,
      durationMs: Date.now() - started,
    };
    if (!parsed.ok) {
      const reason = result.timedOut
        ? `timed out after ${timeoutMs / 1000}s`
        : [parsed.error, result.stderr.trim().split("\n").at(-1)].filter(Boolean).join("; ");
      return { ...base, status: "error", error: reason, model: null, firstPick: null, picks: [] };
    }
    return {
      ...base,
      status: "ok",
      model: parsed.data.model,
      costUsd: parsed.data.costUsd,
      ...classifyAnswer(parsed.data.text),
      answer: parsed.data.text,
    };
  } finally {
    fixture.remove();
  }
};

const printDryRun = (options, promptSet, available) => {
  for (const agentId of options.agents) {
    const agent = AGENTS[agentId];
    const status = available[agentId] ? `found at ${available[agentId]}` : "not found, would skip";
    console.log(`# ${agentId}: ${status}`);
    for (const id of options.promptIds) {
      const prompt = promptSet.prompts.find((entry) => entry.id === id);
      const args = agent.args("<run-dir>").map(shellQuote).join(" ");
      const input = shellQuote(buildPrompt(promptSet, prompt));
      console.log(`(cd <run-dir>/fixture && printf %s ${input} | ${agent.binary} ${args})`);
    }
  }
  console.log(
    `# ${options.runs} run(s) each, results to ${options.outDir}. Dry run: nothing was run.`,
  );
};

const execute = async (options, promptSet) => {
  const available = Object.fromEntries(
    options.agents.map((agentId) => [agentId, whichBinary(AGENTS[agentId].binary)]),
  );
  if (options.dryRun) {
    printDryRun(options, promptSet, available);
    return 0;
  }
  const outDir = checkOutDir(options.outDir);
  if (!outDir.ok) {
    console.error(outDir.error);
    return 2;
  }
  mkdirSync(outDir.data, { recursive: true });
  const resultsPath = join(outDir.data, "results.jsonl");
  const skipped = options.agents
    .filter((agentId) => !available[agentId])
    .map((agentId) => ({ agent: agentId, reason: `${AGENTS[agentId].binary} is not on PATH` }));
  const records = [];
  for (const agentId of options.agents.filter((id) => available[id])) {
    const version = cliVersion(AGENTS[agentId].binary);
    for (const id of options.promptIds) {
      const prompt = promptSet.prompts.find((entry) => entry.id === id);
      for (let run = 1; run <= options.runs; run += 1) {
        process.stderr.write(`${agentId} ${id} run ${run}/${options.runs}\n`);
        const record = await callAgent({
          agentId,
          prompt,
          promptSet,
          run,
          timeoutMs: options.timeoutMs,
          version,
        });
        records.push(record);
        appendFileSync(resultsPath, `${JSON.stringify(record)}\n`);
        const outcome = record.status === "ok" ? `first pick ${record.firstPick}` : record.error;
        process.stderr.write(`  ${outcome}\n`);
      }
    }
  }
  const summary = summarize(records, skipped);
  writeFileSync(join(outDir.data, "summary.json"), `${JSON.stringify(summary, null, 2)}\n`);
  const table = formatSummary(summary);
  writeFileSync(join(outDir.data, "summary.md"), table);
  console.log(table);
  console.log(`Results: ${resultsPath}`);
  return records.some((record) => record.status !== "ok") ? 1 : 0;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const promptSet = loadPrompts();
  const options = parseOptions(process.argv.slice(2), promptSet);
  if (!options.ok) {
    console.error(`${options.error}\n\n${USAGE}`);
    process.exitCode = 2;
  } else if (options.data.help) {
    console.log(USAGE);
  } else {
    process.exitCode = await execute(options.data, promptSet);
  }
}
