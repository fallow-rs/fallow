import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = join(REPO_ROOT, "benchmarks", "cli-instructions.sh");
const PROJECTS = ["preact", "zod", "vue-core"];
const COMMANDS = ["dead-code", "audit"];
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
    // shell-words quoting wraps the tilde in single quotes.
    assert.equal(exec.includes("--base 'HEAD~1'"), name.includes(" audit "), `${name} audit base`);
  }
});

test("counters reads the dead-code work counters from --performance", () => {
  const work = mkdtempSync(join(tmpdir(), "fallow-cli-instructions-"));
  const fake = join(work, "fake-fallow");
  writeFileSync(
    fake,
    [
      "#!/usr/bin/env bash",
      'if [[ "$1" != "dead-code" ]]; then echo "unexpected $1" >&2; exit 3; fi',
      "echo '{\"findings\": []}'",
      "echo 'progress line' >&2",
      'echo \'{"total_ms": 1.5, "counters": {"files_read": 7, "oxc_resolve_calls": 11}}\' >&2',
    ].join("\n"),
  );
  chmodSync(fake, 0o755);

  const result = run(["counters", "--fallow-bin", fake, "--work-dir", work]);
  assert.equal(result.status, 0, result.stderr);

  const entries = JSON.parse(result.stdout);
  const expected = PROJECTS.flatMap((project) =>
    STATES.flatMap((state) => [
      { name: `cli ${project} dead-code (${state}): files_read`, unit: "count", value: 7 },
      { name: `cli ${project} dead-code (${state}): oxc_resolve_calls`, unit: "count", value: 11 },
    ]),
  );
  assert.deepEqual(entries, expected);
});
