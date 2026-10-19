import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { devNull, tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import test from "node:test";

import { collectAdvisoryIds, findStaleIgnores, parseIgnoredIds } from "./check-osv-ignores.mjs";

const TOML = `# comment
[[IgnoredVulns]]
id = "GHSA-aaaa-aaaa-aaaa"
ignoreUntil = 2027-01-15T00:00:00Z
reason = "Development dependency."

[[PackageOverrides]]
ignore = true
reason = "Fixture."

[[IgnoredVulns]]
id = "RUSTSEC-2026-0001"
reason = "Not reachable."
`;

const REPORT = {
  results: [
    {
      source: { path: "/repo/Cargo.lock", type: "lockfile" },
      packages: [
        {
          package: { name: "zip", version: "2.0.0", ecosystem: "crates.io" },
          vulnerabilities: [{ id: "RUSTSEC-2026-0001", aliases: ["GHSA-bbbb-bbbb-bbbb"] }],
          groups: [{ ids: ["RUSTSEC-2026-0001"], aliases: ["GHSA-bbbb-bbbb-bbbb", "CVE-2026-1"] }],
        },
      ],
    },
  ],
};

test("parseIgnoredIds reads only IgnoredVulns ids", () => {
  assert.deepEqual(parseIgnoredIds(TOML), ["GHSA-aaaa-aaaa-aaaa", "RUSTSEC-2026-0001"]);
  assert.deepEqual(parseIgnoredIds("[[PackageOverrides]]\nignore = true\n"), []);
});

test("collectAdvisoryIds includes ids, aliases and group aliases", () => {
  assert.deepEqual([...collectAdvisoryIds(REPORT)].toSorted(), [
    "CVE-2026-1",
    "GHSA-bbbb-bbbb-bbbb",
    "RUSTSEC-2026-0001",
  ]);
  assert.equal(collectAdvisoryIds({}).size, 0);
});

test("findStaleIgnores reports an id that no report contains", () => {
  assert.deepEqual(findStaleIgnores(parseIgnoredIds(TOML), [REPORT]), ["GHSA-aaaa-aaaa-aaaa"]);
});

test("findStaleIgnores accepts an ignore that matches an alias", () => {
  assert.deepEqual(findStaleIgnores(["CVE-2026-1", "GHSA-bbbb-bbbb-bbbb"], [REPORT]), []);
});

test("findStaleIgnores merges the reports of all lockfiles in a directory", () => {
  const second = {
    results: [{ packages: [{ vulnerabilities: [{ id: "GHSA-aaaa-aaaa-aaaa" }] }] }],
  };
  assert.deepEqual(findStaleIgnores(parseIgnoredIds(TOML), [REPORT, second]), []);
});

test("findStaleIgnores reports every ignore when the scan finds nothing", () => {
  assert.deepEqual(findStaleIgnores(["GHSA-aaaa-aaaa-aaaa"], [{ results: [] }]), [
    "GHSA-aaaa-aaaa-aaaa",
  ]);
});

const SCRIPT = resolve(import.meta.dirname, "check-osv-ignores.mjs");

// The fake osv-scanner prints one report for every lockfile, so the start
// test needs neither the network nor a real scanner.
const runInFixtureRepo = (toml, scanner = `cat "$REPORT"\nexit 1`) => {
  const root = mkdtempSync(join(tmpdir(), "check-osv-ignores-test-"));
  try {
    const bin = join(root, "bin");
    const repo = join(root, "repo");
    mkdirSync(bin);
    mkdirSync(join(repo, "app"), { recursive: true });
    writeFileSync(join(root, "report.json"), JSON.stringify(REPORT));
    writeFileSync(
      join(bin, "osv-scanner"),
      `#!/bin/sh\nREPORT="${join(root, "report.json")}"\n${scanner}\n`,
    );
    chmodSync(join(bin, "osv-scanner"), 0o755);
    writeFileSync(join(repo, "app", "osv-scanner.toml"), toml);
    writeFileSync(join(repo, "app", "Cargo.lock"), "");
    const env = {
      ...process.env,
      GIT_CONFIG_GLOBAL: devNull,
      GIT_CONFIG_NOSYSTEM: "1",
      PATH: `${bin}${delimiter}${process.env.PATH}`,
    };
    const git = (...args) =>
      execFileSync("git", ["-c", "maintenance.auto=false", "-c", "gc.auto=0", ...args], {
        cwd: repo,
        env,
      });
    git("init", "--quiet");
    git("add", ".");
    return spawnSync(process.execPath, [SCRIPT], { cwd: repo, env, encoding: "utf8" });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
};

test(
  "the script fails on a stale ignore and names the file and id",
  { skip: process.platform === "win32" && "the fake scanner is a shell script" },
  () => {
    const result = runInFixtureRepo(TOML);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /app\/osv-scanner\.toml: GHSA-aaaa-aaaa-aaaa matches no advisory/u);
    assert.doesNotMatch(result.stderr, /RUSTSEC-2026-0001/u);
  },
);

test(
  "the script passes when every ignore matches an advisory",
  { skip: process.platform === "win32" && "the fake scanner is a shell script" },
  () => {
    const result = runInFixtureRepo('[[IgnoredVulns]]\nid = "CVE-2026-1"\nreason = "x"\n');
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /All IgnoredVulns entries match an advisory/u);
  },
);

test(
  "the script reports every ignore as stale for a lockfile without packages",
  { skip: process.platform === "win32" && "the fake scanner is a shell script" },
  () => {
    const result = runInFixtureRepo(
      TOML,
      'echo "No package sources found, --help for usage information." >&2\nexit 128',
    );
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /GHSA-aaaa-aaaa-aaaa matches no advisory/u);
    assert.match(result.stderr, /RUSTSEC-2026-0001 matches no advisory/u);
  },
);

test(
  "the script fails loudly when the scanner fails",
  { skip: process.platform === "win32" && "the fake scanner is a shell script" },
  () => {
    const result = runInFixtureRepo(TOML, 'echo "network error" >&2\nexit 127');
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /osv-scanner failed on .*\(exit 127\)/u);
  },
);
