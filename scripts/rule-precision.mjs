#!/usr/bin/env node
/**
 * Measures and records the precision of fallow rules on a pinned public corpus.
 *
 *   sample   run fallow on each corpus project, write review sheets
 *   record   merge the verdicts of a review into the ledger
 *   summary  write the summary from the ledger
 *   check    fail when the ledger is malformed or the summary is stale
 *
 * See docs/rule-precision.md.
 */
import { execFileSync, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import {
  buildRow,
  combineReviews,
  findingAnchor,
  findingsByRule,
  ledgerErrors,
  renderSummary,
  reviewView,
  seededSample,
  upsertRow,
} from "./rule-precision/ledger.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LEDGER = join(REPO_ROOT, "benchmarks/rule-precision.json");
const SUMMARY = join(REPO_ROOT, "benchmarks/rule-precision-summary.md");
const DEFAULT_SAMPLE_SIZE = 30;
const SNIPPET_RADIUS = 8;
const MAX_REPORT_BYTES = 1 << 30;

const fail = (message) => {
  console.error(`rule-precision: ${message}`);
  process.exit(2);
};

const parseJson = (text, source) => {
  try {
    return JSON.parse(text);
  } catch (error) {
    return fail(`${source} is not valid JSON: ${error.message}`);
  }
};

const readJson = (path) => {
  if (!existsSync(path)) fail(`${path} does not exist`);
  return parseJson(readFileSync(path, "utf8"), path);
};
const writeJson = (path, value) => writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);

// Tests point this at a local directory of bare repositories.
const GIT_BASE = process.env.RULE_PRECISION_GIT_BASE ?? "https://github.com/";
const INSTALL_MARKER = "rule-precision-installed";

const git = (dir, args) => spawnSync("git", ["-C", dir, ...args], { encoding: "utf8" });

const gitOrFail = (dir, args) => {
  const run = git(dir, args);
  if (run.status !== 0) fail(`git ${args.join(" ")} in ${dir} failed:\n${run.stderr}`);
  return run.stdout.trim();
};

// The checkout must be its own repository with the expected origin. Without
// this check, git in a plain folder inside another repository acts on that
// outer repository.
const assertOwnCheckout = (dir, url) => {
  const top = git(dir, ["rev-parse", "--show-toplevel"]);
  if (top.status !== 0 || realpathSync(top.stdout.trim()) !== realpathSync(dir)) {
    fail(`${dir} exists but is not its own git checkout; remove it or pass another --corpus-dir`);
  }
  const origin = git(dir, ["remote", "get-url", "origin"]).stdout.trim();
  if (origin !== url) fail(`${dir} has origin ${origin || "(none)"}, expected ${url}`);
};

const ensureCheckout = (dir, project) => {
  const url = `${GIT_BASE}${project.repo}.git`;
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
    gitOrFail(dir, ["init", "-q"]);
    gitOrFail(dir, ["remote", "add", "origin", url]);
  }
  assertOwnCheckout(dir, url);
  const head = git(dir, ["rev-parse", "--verify", "-q", "HEAD"]);
  if (head.status !== 0 || head.stdout.trim() !== project.commit) {
    gitOrFail(dir, ["fetch", "-q", "--depth", "1", "origin", project.commit]);
    gitOrFail(dir, ["checkout", "-q", "--detach", project.commit]);
  }
  if (gitOrFail(dir, ["status", "--porcelain"]) !== "") {
    fail(`${dir} has local changes or untracked files; reset the checkout first`);
  }
  installDependencies(dir, project.commit);
};

// Users run fallow with installed dependencies: tsconfig `extends` and
// workspace links resolve through node_modules. Install scripts stay off,
// because the corpus is third-party code.
const INSTALLERS = [
  ["pnpm-lock.yaml", "pnpm", ["install", "--frozen-lockfile", "--ignore-scripts"]],
  ["package-lock.json", "npm", ["ci", "--ignore-scripts", "--no-audit", "--no-fund"]],
  ["yarn.lock", "yarn", ["install", "--immutable", "--mode=skip-build"]],
  ["bun.lock", "bun", ["install", "--frozen-lockfile", "--ignore-scripts"]],
];

// A marker in the git directory records the commit of the last install that
// succeeded, so an install that failed halfway runs again.
const installDependencies = (dir, commit) => {
  const marker = join(dir, gitOrFail(dir, ["rev-parse", "--git-dir"]), INSTALL_MARKER);
  if (existsSync(marker) && readFileSync(marker, "utf8").trim() === commit) return;
  // An install that fails halfway leaves node_modules in an unknown state, so
  // no earlier marker may survive it.
  rmSync(marker, { force: true });
  const installer = INSTALLERS.find(([lockfile]) => existsSync(join(dir, lockfile)));
  if (installer) {
    const [, bin, args] = installer;
    console.error(`rule-precision: ${dir}: ${bin} ${args.join(" ")}`);
    const run = spawnSync(bin, args, { cwd: dir, stdio: ["ignore", "ignore", "inherit"] });
    if (run.status !== 0) fail(`${bin} ${args.join(" ")} in ${dir} exited with ${run.status}`);
  }
  writeFileSync(marker, `${commit}\n`);
};

const snippet = (root, anchor) => {
  if (!anchor.path) return null;
  const file = join(root, anchor.path);
  if (!existsSync(file) || !statSync(file).isFile()) return null;
  const lines = readFileSync(file, "utf8").split("\n");
  const line = anchor.line ?? 1;
  const start = Math.max(1, line - SNIPPET_RADIUS);
  const end = Math.min(lines.length, line + SNIPPET_RADIUS);
  return lines
    .slice(start - 1, end)
    .map((text, i) => `${String(start + i).padStart(5)}${start + i === line ? ">" : " "} ${text}`)
    .join("\n");
};

// Exit code 1 means that a gate failed on findings, which is the normal case
// for a corpus project. Only exit code 2 is an execution error.
const FINDINGS_EXIT = 1;

const runFallow = (bin, cwd, command, extraArgs) => {
  const run = spawnSync(bin, [command, ...extraArgs, "--format", "json", "--quiet", "--no-cache"], {
    cwd,
    maxBuffer: MAX_REPORT_BYTES,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, FALLOW_TELEMETRY: "off" },
  });
  if (run.status !== 0 && run.status !== FINDINGS_EXIT) {
    fail(`fallow ${command} in ${cwd} exited with ${run.status}:\n${run.stderr}`);
  }
  return parseJson(run.stdout.toString(), `the output of fallow ${command} in ${cwd}`);
};

// The reviewer judges a finding against the rule's own claim, so the sample
// carries the `fallow explain` text of the rule.
const NO_CONTRACT = "NO CONTRACT";

const explainRule = (bin, rule) => {
  const run = spawnSync(bin, ["explain", rule], { env: { ...process.env, NO_COLOR: "1" } });
  if (run.status === 0) return run.stdout.toString();
  console.error(
    `rule-precision: warning: fallow explain ${rule} has no text; write the claim to ${rule}/contract.txt before the review`,
  );
  return `${NO_CONTRACT}: fallow explain ${rule} has no text. Write the claim of the rule here before the review.\n`;
};

const fallowVersion = (bin) =>
  execFileSync(bin, ["--version"])
    .toString()
    .trim()
    .replace(/^fallow\s+/, "");

const reviewItem = (name, dir, finding) => {
  const anchor = findingAnchor(finding);
  return {
    project: name,
    id: finding.finding_id,
    repo_root: dir,
    anchor,
    finding: reviewView(finding),
    snippet: snippet(dir, anchor),
  };
};

// Adds the sample of one project to the per-rule entries.
const sampleProject = ({ perRule, name, dir, project, report, size, wanted }) => {
  for (const [rule, findings] of findingsByRule(report)) {
    if (wanted && !wanted.has(rule)) continue;
    const picked = seededSample(findings, size, `${rule}:${name}:${project.commit}`);
    const entry = perRule.get(rule) ?? { projects: [], items: [], all: {} };
    entry.all[name] = findings.map(findingAnchor);
    entry.projects.push({
      project: name,
      commit: project.commit,
      findings: findings.length,
      sampled_ids: picked.map((f) => f.finding_id),
    });
    entry.items.push(...picked.map((finding) => reviewItem(name, dir, finding)));
    perRule.set(rule, entry);
  }
};

const writeRuleSample = ({ out, rule, entry, command, extraArgs, version, size, bin }) => {
  const ruleDir = join(out, rule);
  mkdirSync(ruleDir, { recursive: true });
  writeJson(join(ruleDir, "sample.json"), {
    rule,
    command,
    args: extraArgs,
    fallow_version: version,
    sample_size: size,
    projects: entry.projects,
  });
  writeJson(join(ruleDir, "review.json"), entry.items);
  // A file that only other reported files import is still unreachable. The
  // reviewer needs every finding of the rule, not only the sample, to see it.
  writeJson(join(ruleDir, "all-findings.json"), entry.all);
  writeFileSync(join(ruleDir, "contract.txt"), explainRule(bin, rule));
  console.log(`${rule}\t${entry.items.length} sampled\t${ruleDir}`);
};

const sample = (values) => {
  const bin = resolve(values["fallow-bin"] ?? fail("sample needs --fallow-bin"));
  const corpusDir = values["corpus-dir"] ?? fail("sample needs --corpus-dir");
  const out = values.out ?? fail("sample needs --out");
  const { command } = values;
  const extraArgs = values.args ? values.args.split(" ").filter(Boolean) : [];
  const size = Number(values["sample-size"]);
  if (!Number.isInteger(size) || size < 1)
    fail("--sample-size must be a whole number of 1 or more");
  if (existsSync(out) && readdirSync(out).length > 0) {
    fail(`${out} is not empty; give each sample run its own --out`);
  }
  const wanted = values.rules ? new Set(values.rules.split(",")) : null;
  const version = fallowVersion(bin);
  const perRule = new Map();
  for (const [name, project] of Object.entries(readJson(LEDGER).corpus)) {
    const dir = join(corpusDir, name);
    ensureCheckout(dir, project);
    console.error(`rule-precision: ${name}: fallow ${command}`);
    const report = runFallow(bin, dir, command, extraArgs);
    sampleProject({ perRule, name, dir, project, report, size, wanted });
  }
  for (const [rule, entry] of perRule) {
    writeRuleSample({ out, rule, entry, command, extraArgs, version, size, bin });
  }
};

const writeSummary = (ledger) => writeFileSync(SUMMARY, renderSummary(ledger));

const record = (values) => {
  const ruleDir = values.in ?? fail("record needs --in <rule sample dir>");
  const verdictPaths = values.verdicts ?? fail("record needs --verdicts (once per review)");
  const contract = join(ruleDir, "contract.txt");
  if (existsSync(contract) && readFileSync(contract, "utf8").startsWith(NO_CONTRACT)) {
    fail(`${contract} has no claim of the rule; write it, and review the sample with it`);
  }
  const combined = combineReviews(verdictPaths.map(readJson));
  if (!combined.ok) fail(`the reviews do not combine:\n  ${combined.errors.join("\n  ")}`);
  const result = buildRow({
    sample: readJson(join(ruleDir, "sample.json")),
    verdicts: combined.verdicts,
    reviews: combined.reviews,
    agreement: combined.agreement,
  });
  if (!result.ok) fail(`verdicts do not match the sample:\n  ${result.errors.join("\n  ")}`);
  const ledger = upsertRow(readJson(LEDGER), result.row);
  const errors = ledgerErrors(ledger);
  if (errors.length > 0) fail(`ledger is invalid:\n  ${errors.join("\n  ")}`);
  writeJson(LEDGER, ledger);
  writeSummary(ledger);
  console.log(`recorded ${result.row.command} ${result.row.rule}`);
};

const check = () => {
  const ledger = readJson(LEDGER);
  const errors = ledgerErrors(ledger);
  if (errors.length > 0) fail(errors.join("\n  "));
  const summary = existsSync(SUMMARY) ? readFileSync(SUMMARY, "utf8") : "";
  if (summary !== renderSummary(ledger)) {
    errors.push(
      "benchmarks/rule-precision-summary.md is stale: run `node scripts/rule-precision.mjs summary`",
    );
  }
  if (errors.length > 0) fail(errors.join("\n  "));
  console.log("rule-precision: ledger and summary are valid");
};

const parseCli = () => {
  try {
    return parseArgs({
      allowPositionals: true,
      options: {
        "fallow-bin": { type: "string" },
        "corpus-dir": { type: "string" },
        out: { type: "string" },
        command: { type: "string", default: "dead-code" },
        args: { type: "string" },
        rules: { type: "string" },
        "sample-size": { type: "string", default: String(DEFAULT_SAMPLE_SIZE) },
        in: { type: "string" },
        verdicts: { type: "string", multiple: true },
      },
    });
  } catch (error) {
    return fail(error.message);
  }
};
const { positionals, values } = parseCli();

const subcommands = {
  sample: () => sample(values),
  record: () => record(values),
  summary: () => {
    const ledger = readJson(LEDGER);
    const errors = ledgerErrors(ledger);
    if (errors.length > 0) fail(errors.join("\n  "));
    writeSummary(ledger);
  },
  check,
};
const run = subcommands[positionals[0]];
if (!run) fail(`usage: rule-precision.mjs <${Object.keys(subcommands).join("|")}> [options]`);
run();
