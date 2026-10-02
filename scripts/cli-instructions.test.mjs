import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = join(REPO_ROOT, "benchmarks", "cli-instructions.sh");
const PROJECTS = ["preact", "zod", "vue-core"];
const COMMANDS = ["dead-code"];
const STATES = ["cold", "warm"];

const run = (args) => spawnSync("bash", [SCRIPT, ...args], { encoding: "utf8" });

/** Parse the generated codspeed.yml into `{ name, exec }` pairs. */
const parseConfig = (text) => {
  const entries = [];
  const lines = text.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const name = lines[index].match(/^ {2}- name: (.*)$/);
    if (!name) {
      continue;
    }
    const exec = lines[index + 1].match(/^ {4}exec: (.*)$/);
    assert.ok(exec, `benchmark ${name[1]} has an exec line`);
    entries.push({ name: JSON.parse(name[1]), exec: JSON.parse(exec[1]) });
  }
  return entries;
};

test("config lists one benchmark per project, command and cache state", () => {
  const work = mkdtempSync(join(tmpdir(), "fallow-cli-instructions-"));
  const result = run(["config", "--fallow-bin", "/opt/fallow", "--work-dir", work]);
  assert.equal(result.status, 0, result.stderr);

  const entries = parseConfig(result.stdout);
  const expected = PROJECTS.flatMap((project) =>
    COMMANDS.flatMap((command) => STATES.map((state) => `cli ${project} ${command} (${state})`)),
  );
  assert.deepEqual(
    entries.map((entry) => entry.name),
    expected,
  );

  for (const { name, exec } of entries) {
    const args = exec.split(" ");
    assert.equal(args[0], "/opt/fallow", name);
    assert.ok(exec.includes("--threads 1"), `${name} pins one thread`);
    assert.ok(
      exec.includes(`--config ${join(work, "fallow-bench.json")}`),
      `${name} uses the bench config`,
    );
    assert.equal(args.includes("--no-cache"), name.endsWith("(cold)"), `${name} cache flag`);
  }
});

// The CPU simulation rejects a measured process that starts a child process.
// `benchmark_dead_code_run_starts_no_git_process` in
// crates/cli/tests/integration/check_tests.rs proves that dead-code starts
// none under these settings.
test("the benchmark runs turn off the git-backed next steps", () => {
  const workflow = readFileSync(
    join(REPO_ROOT, ".github", "workflows", "bench-cli-instructions.yml"),
    "utf8",
  );
  assert.match(workflow, /^ {2}FALLOW_SUGGESTIONS: 'off'$/m);
  assert.match(readFileSync(SCRIPT, "utf8"), /^export FALLOW_SUGGESTIONS=off$/m);
});
