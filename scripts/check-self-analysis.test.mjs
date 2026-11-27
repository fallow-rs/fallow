import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { runSelfAnalysis, selfAnalysisCommands } from "./check-self-analysis.mjs";

const workflow = (steps) => `jobs:
  self-analysis:
    name: Fallow Self Analysis
    steps:
${steps}
  other:
    name: Other
    steps:
      - name: Not part of the job
        run: ./target/debug/fallow dead-code --root elsewhere --quiet
`;

test("the dogfood steps of the Self Analysis job are the commands", () => {
  const text = workflow(`      - name: Build fallow binary
        run: cargo build --bin fallow
      - name: Dead-code dogfood
        run: ./target/debug/fallow dead-code --root editors/vscode --format json --quiet
      - name: File length
        run: ./target/debug/fallow audit --config scripts/fallow-file-length.json --base HEAD^1 --quiet
      - name: Health dogfood
        run: ./target/debug/fallow health --root npm/fallow --quiet --report-only`);

  assert.deepEqual(selfAnalysisCommands(text), [
    ["dead-code", "--root", "editors/vscode", "--format", "json", "--quiet"],
    ["health", "--root", "npm/fallow", "--quiet", "--report-only"],
  ]);
});

test("a workflow without the job is an error", () => {
  assert.throws(() => selfAnalysisCommands("jobs: {}\n"), /Fallow Self Analysis/);
});

test("the real workflow has dogfood steps for both packages", () => {
  const commands = selfAnalysisCommands(readFileSync(".github/workflows/ci.yml", "utf8"));
  const roots = new Set(commands.map((args) => args[args.indexOf("--root") + 1]));
  assert.deepEqual([...roots].toSorted(), ["editors/vscode", "npm/fallow"]);
  assert.ok(commands.some((args) => args[0] === "dead-code"));
});

test("each command runs through cargo, and the first failure stops the run", () => {
  const calls = [];
  const exitCode = runSelfAnalysis(
    [
      ["dead-code", "--root", "a"],
      ["dupes", "--root", "a"],
    ],
    (command, args) => {
      calls.push([command, ...args]);
      return 1;
    },
  );
  assert.equal(exitCode, 1);
  assert.deepEqual(calls, [
    ["cargo", "run", "--quiet", "--bin", "fallow", "--", "dead-code", "--root", "a"],
  ]);
});

test("a job without dogfood steps fails, so the check never passes empty", () => {
  const exitCode = runSelfAnalysis([], () => 0);
  assert.equal(exitCode, 1);
});
