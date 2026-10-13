import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { test } from "node:test";

const workflow = readFileSync(new URL("../workflows/release.yml", import.meta.url), "utf8");
const targets = [
  "universal",
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-arm64",
  "win32-x64",
];
const providers = [
  { name: "VS Code Marketplace", secret: "VSCE_PAT", mode: "VSCE_AUTH_MODE", flag: "--oidc" },
  { name: "Open VSX", secret: "OVSX_PAT", mode: "OVSX_AUTH_MODE", flag: "--trusted-publishing" },
];

const publishStep = (name) => {
  const start = workflow.indexOf(`      - name: Publish every target to ${name}\n`);
  assert.notEqual(start, -1);
  const next = workflow.indexOf("\n  vscode-", start);
  return workflow.slice(start, next === -1 ? undefined : next);
};

const execute = (t, provider, mode, identity = true, pat = true) => {
  const cwd = mkdtempSync(join(tmpdir(), "fallow-publisher-auth-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  mkdirSync(join(cwd, "bin"));
  mkdirSync(join(cwd, "vscode-targets"));
  const files = targets.map((target) => `fallow-vscode-1.2.3-${target}.vsix`);
  writeFileSync(
    join(cwd, "vscode-targets/inventory.json"),
    JSON.stringify({ entries: files.map((file) => ({ file })) }),
  );
  for (const file of files)
    writeFileSync(join(cwd, "vscode-targets", file), "prepared fixture bytes");
  writeFileSync(
    join(cwd, "bin/npx"),
    `#!${process.execPath}\nimport { appendFileSync } from "node:fs";\nappendFileSync("calls.jsonl", JSON.stringify({ args: process.argv.slice(2), patPresent: Boolean(process.env[${JSON.stringify(provider.secret)}]) }) + "\\n");\n`,
    { mode: 0o755 },
  );
  const step = publishStep(provider.name);
  const block = step.match(/        run: \|\n([\s\S]+)/u);
  assert.ok(block);
  const script = block[1]
    .split("\n")
    .map((line) => line.slice(10))
    .join("\n");
  const result = spawnSync("bash", ["-euo", "pipefail", "-c", script], {
    cwd,
    encoding: "utf8",
    env: {
      HOME: cwd,
      PATH: `${cwd}/bin:${dirname(process.execPath)}:/opt/homebrew/bin:/usr/bin:/bin`,
      RELEASE_TAG: "v1.2.3",
      GITHUB_ACTIONS: "true",
      [provider.mode]: mode,
      [provider.secret]: pat ? "synthetic-legacy-pat" : "",
      ACTIONS_ID_TOKEN_REQUEST_URL: identity
        ? "https://token.actions.githubusercontent.com/synthetic"
        : "",
      ACTIONS_ID_TOKEN_REQUEST_TOKEN: identity ? "synthetic-oidc-request" : "",
    },
  });
  let calls = [];
  try {
    calls = readFileSync(join(cwd, "calls.jsonl"), "utf8")
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  return { result, calls, files };
};

for (const provider of providers) {
  test(`${provider.name} keeps explicit legacy publication and prepared target bytes`, (t) => {
    const { result, calls, files } = execute(t, provider, "pat");
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(
      calls.map(({ args }) => args.find((arg) => arg.endsWith(".vsix"))),
      files.map((file) => `vscode-targets/${file}`),
    );
    assert.ok(calls.every(({ args, patPresent }) => patPresent && !args.includes(provider.flag)));
  });

  test(`${provider.name} explicitly selects OIDC without an inherited legacy PAT`, (t) => {
    const { result, calls, files } = execute(t, provider, "oidc");
    assert.equal(result.status, 0, result.stderr);
    assert.equal(calls.length, files.length);
    assert.ok(calls.every(({ args, patPresent }) => args.includes(provider.flag) && !patPresent));
    assert.match(
      publishStep(provider.name),
      new RegExp(
        `${provider.secret}: \\$\\{\\{ vars\\.${provider.mode} != 'oidc' && secrets\\.${provider.secret} \\|\\| '' \\}\\}`,
        "u",
      ),
    );
  });

  for (const [mode, identity, pat] of [
    ["invalid", true, true],
    ["oidc", false, true],
    ["pat", true, false],
  ]) {
    test(`${provider.name} rejects incomplete ${mode} authorization before upload`, (t) => {
      const { result, calls } = execute(t, provider, mode, identity, pat);
      assert.notEqual(result.status, 0);
      assert.match(result.stdout, /::error::/u);
      assert.deepEqual(calls, []);
    });
  }
}
