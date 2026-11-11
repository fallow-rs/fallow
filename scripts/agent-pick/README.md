# Agent pick tracker

The tracker measures which tools coding agents recommend for tasks that fallow
covers. It asks each agent CLI a set of neutral requests. No request names a
tool. The tracker then records the tools that each answer names.

## What it measures

For each prompt, agent and run, the tracker records these fields:

- `agent` and `cliVersion`: the CLI and its version.
- `model`: the model id that the CLI output reports.
- `promptId` and `category`: the request from `prompts.json`.
- `firstPick`: the known tool that the answer names first.
- `picks`: all known tools that the answer names, in order of first appearance.
- `answer`: the full answer text, so that a later classifier change can read it again.

`classify.mjs` holds the list of known tools. The classifier matches names
only. It does not see negation, so "do not use ts-prune" counts as a ts-prune
mention. Read the `answer` field when a count looks wrong.

## How a run works

1. The tracker finds each CLI with `which`. It skips a missing CLI and records
   the reason in the summary.
2. For each call, the tracker creates a small TypeScript project in a new
   temporary directory. It removes the directory after the call.
3. The agent gets the request plus an instruction to answer with a
   recommendation only.
4. The CLI flags block tool execution and file writes:
   - `claude -p` gets only the read-only `Read`, `Glob` and `Grep` tools. It
     also gets `--safe-mode` and `--strict-mcp-config`, so local plugins,
     skills, memory and MCP servers do not change the answer.
   - `codex exec` runs in the `read-only` sandbox with `--ephemeral`.

## How to run it

```bash
# Unit tests (no agent calls)
node --test scripts/agent-pick.test.mjs

# Print the commands, run nothing
node scripts/agent-pick/run.mjs --dry-run

# One smoke call
node scripts/agent-pick/run.mjs --runs 1 --agents claude --prompts unused-exports

# Full matrix, three runs each
node scripts/agent-pick/run.mjs --runs 3
```

Options:

- `--runs N`: runs per prompt and agent (default 1).
- `--agents claude,codex`: the agents to call (default all).
- `--prompts id,id`: the prompt ids from `prompts.json` (default all).
- `--out DIR`: the output directory (default `.plans/agent-pick/<date>/`).
- `--timeout SECONDS`: the time limit per call (default 300).
- `--dry-run`: print the commands and run nothing.

The tracker writes `results.jsonl`, `summary.json` and `summary.md` to the
output directory. It refuses an output directory inside the repository that
git does not ignore, so results never go into tracked files.

## What it costs

Each call is a paid agent request on the account that the CLI uses. The
full matrix is the number of prompts, times the number of agents, times
`--runs`. Ten prompts, two agents and three runs make 60 paid calls. A
`claude` call has a budget limit of 1 USD through `--max-budget-usd`.
`summary.json` and `results.jsonl` hold the `costUsd` of each `claude` call.
`codex` does not report a cost. Use `--dry-run` and a small `--prompts` list
before a full run.
