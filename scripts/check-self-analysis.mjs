#!/usr/bin/env node
// Run the dogfood steps of the CI job "Fallow Self Analysis" locally.
//
// The job runs the branch binary on the VS Code extension and the npm wrapper
// and fails on an error-severity finding. A detection change that reports
// more can pass every other local gate and still fail this job. The steps are
// read from the workflow, so this check and CI run the same commands.

import { spawnSync } from "node:child_process";
import { readFileSync, realpathSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const WORKFLOW = ".github/workflows/ci.yml";
const JOB_NAME = "name: Fallow Self Analysis";
const BINARY_STEP = /^\s*run: \.\/target\/debug\/fallow (.+)$/;

const indentOf = (line) => line.length - line.trimStart().length;

/** The fallow arguments of each `--root` dogfood step of the job. */
export const selfAnalysisCommands = (workflowText) => {
  const lines = workflowText.split("\n");
  const nameIndex = lines.findIndex((line) => line.trim() === JOB_NAME);
  if (nameIndex < 1) {
    throw new Error(`${WORKFLOW} has no job with "${JOB_NAME}"`);
  }
  const jobIndent = indentOf(lines[nameIndex - 1]);
  const commands = [];
  for (const line of lines.slice(nameIndex + 1)) {
    if (line.trim() !== "" && indentOf(line) <= jobIndent) {
      break;
    }
    const step = BINARY_STEP.exec(line);
    if (step === null) {
      continue;
    }
    const args = step[1].trim().split(/\s+/);
    if (args.includes("--root")) {
      commands.push(args);
    }
  }
  return commands;
};

const runInherit = (command, args) => {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error !== undefined) {
    process.stderr.write(`[self-analysis] ${command} did not start: ${result.error.message}\n`);
  }
  return result.status ?? 1;
};

/**
 * Run each command with the current sources. Returns the first failing exit
 * code, 1 when there is no command, or 0.
 */
export const runSelfAnalysis = (commands, run = runInherit) => {
  if (commands.length === 0) {
    process.stderr.write(`[self-analysis] ${WORKFLOW} has no --root dogfood step to run\n`);
    return 1;
  }
  for (const args of commands) {
    process.stderr.write(`[self-analysis] fallow ${args.join(" ")}\n`);
    const status = run("cargo", ["run", "--quiet", "--bin", "fallow", "--", ...args]);
    if (status !== 0) {
      return status;
    }
  }
  return 0;
};

const isDirectInvocation =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(realpathSync(path.resolve(process.argv[1]))).href;

if (isDirectInvocation) {
  const commands = selfAnalysisCommands(readFileSync(WORKFLOW, "utf8"));
  process.exit(runSelfAnalysis(commands));
}
