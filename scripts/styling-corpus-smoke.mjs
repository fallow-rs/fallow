#!/usr/bin/env node
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { cargoFallowBin } from "./cargo-target.mjs";
import { parseOptions, parsePositiveInt } from "./lib/options.mjs";
import { parseJsonReport, resolveFallowBin, runTool } from "./lib/tool-run.mjs";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DEFAULT_CACHE_DIR = join(homedir(), ".cache", "fallow", "styling-corpus");
const DEFAULT_OUT_DIR = join(REPO_ROOT, "target", "styling-corpus-smoke");
const DEFAULT_BASELINE = join(
  REPO_ROOT,
  "scripts",
  "fixtures",
  "styling-corpus-smoke-baseline.json",
);
const SAMPLE_PATH_LIMIT = 5;
const STYLEX_EXAMPLE_TOKENS_PATH = "examples/example-nextjs/app/globalTokens.stylex.ts";
const STYLEX_NESTED_TOKENS_PATH = "examples/example-nextjs/components/NestedTokensDemo.stylex.ts";

const CORPUS = [
  {
    name: "tailwindcss",
    repo: "tailwindlabs/tailwindcss",
    ref: "main",
    stacks: ["tailwind", "css"],
  },
  {
    name: "stylex",
    repo: "facebook/stylex",
    ref: "main",
    stacks: ["stylex", "css-in-js"],
  },
  {
    name: "vanilla-extract",
    repo: "vanilla-extract-css/vanilla-extract",
    ref: "master",
    stacks: ["vanilla-extract", "css-in-js", "css-modules"],
  },
  {
    name: "pandacss",
    repo: "chakra-ui/panda",
    ref: "main",
    stacks: ["pandacss", "css-in-js"],
  },
  {
    name: "styled-components",
    repo: "styled-components/styled-components",
    ref: "main",
    stacks: ["styled-components", "css-in-js"],
  },
  {
    name: "emotion",
    repo: "emotion-js/emotion",
    ref: "main",
    stacks: ["emotion", "css-in-js"],
  },
  {
    name: "shadcn-admin",
    repo: "satnaing/shadcn-admin",
    ref: "main",
    stacks: ["shadcn", "cva", "tailwind", "css"],
  },
  {
    name: "shadcn-vite",
    repo: "dan5py/react-vite-shadcn-ui",
    ref: "main",
    stacks: ["shadcn", "cva", "tailwind", "css"],
  },
  {
    name: "ant-design",
    repo: "ant-design/ant-design",
    ref: "master",
    stacks: ["less", "css-modules", "react"],
  },
  {
    name: "bootstrap",
    repo: "twbs/bootstrap",
    ref: "main",
    stacks: ["sass", "css"],
  },
  {
    name: "vue-core",
    repo: "vuejs/core",
    ref: "main",
    stacks: ["vue", "sfc", "template-heavy"],
  },
  {
    name: "svelte",
    repo: "sveltejs/svelte",
    ref: "main",
    stacks: ["svelte", "template-heavy"],
  },
  {
    name: "astro",
    repo: "withastro/astro",
    ref: "main",
    stacks: ["astro", "template-heavy"],
  },
];

const COMMANDS = [
  {
    id: "health-css",
    args: ["health", "--css", "--format", "json", "--quiet", "--max-crap", "10000"],
  },
  {
    id: "health-css-production",
    args: ["health", "--css", "--production", "--format", "json", "--quiet", "--max-crap", "10000"],
  },
  {
    id: "audit-css-deep",
    args: ["audit", "--css-deep", "--format", "json", "--quiet", "--base", "HEAD~1"],
  },
];

const REQUIRED_STACKS = [
  "tailwind",
  "stylex",
  "vanilla-extract",
  "pandacss",
  "styled-components",
  "emotion",
  "shadcn",
  "cva",
  "css-modules",
  "sass",
  "less",
  "vue",
  "svelte",
  "astro",
  "template-heavy",
];

const OPTION_SPEC = {
  "--cache-dir": { key: "cacheDir", type: "string" },
  "--out-dir": { key: "outDir", type: "string" },
  "--fallow-bin": { key: "fallowBin", type: "string" },
  "--baseline": { key: "baseline", type: "string" },
  "--project": { key: "projects", type: "string", repeat: true },
  "--max-projects": { key: "maxProjects", type: "positiveInt" },
  "--timeout-ms": { key: "timeoutMs", type: "positiveInt" },
  "--refresh": { key: "refresh", type: "flag" },
  "--skip-clone": { key: "skipClone", type: "flag" },
  "--fail-on-spikes": { key: "failOnSpikes", type: "flag" },
  "--list": { key: "list", type: "flag" },
  "--help": { key: "help", type: "flag" },
  "-h": { key: "help", type: "flag" },
};

const parseArgs = (argv) => {
  const defaults = {
    cacheDir: process.env.FALLOW_STYLING_CORPUS_CACHE || DEFAULT_CACHE_DIR,
    outDir: DEFAULT_OUT_DIR,
    fallowBin: process.env.FALLOW_BIN || "",
    baseline: DEFAULT_BASELINE,
    projects: [],
    maxProjects: null,
    timeoutMs: parsePositiveInt(
      "FALLOW_STYLING_CORPUS_TIMEOUT_MS",
      process.env.FALLOW_STYLING_CORPUS_TIMEOUT_MS || 120_000,
    ),
    refresh: false,
    skipClone: false,
    failOnSpikes: false,
    list: false,
    help: false,
  };

  const opts = parseOptions(argv, OPTION_SPEC, defaults);
  opts.cacheDir = resolve(opts.cacheDir);
  opts.outDir = resolve(opts.outDir);
  opts.baseline = resolve(opts.baseline);
  return opts;
};

const usage = () => `Usage: node scripts/styling-corpus-smoke.mjs [options]

Options:
  --cache-dir DIR       Corpus clone cache. Default: ${DEFAULT_CACHE_DIR}
  --out-dir DIR         Output directory. Default: target/styling-corpus-smoke
  --fallow-bin PATH     fallow binary. Default: FALLOW_BIN, target, then PATH
  --baseline PATH       Spike baseline or allowlist JSON
  --project NAME        Run one corpus project. Repeatable
  --max-projects N      Run only the first N selected projects
  --timeout-ms N        Per-command timeout. Default: 120000
  --refresh             Re-clone selected cached projects
  --skip-clone          Use existing cache only
  --fail-on-spikes      Exit nonzero when non-allowlisted spikes are found
  --list                Print corpus entries and exit
`;

const selectedCorpus = (opts) => {
  let selected = CORPUS;
  if (opts.projects.length > 0) {
    const wanted = new Set(opts.projects);
    selected = CORPUS.filter((entry) => wanted.has(entry.name));
    const found = new Set(selected.map((entry) => entry.name));
    const missing = opts.projects.filter((name) => !found.has(name));
    if (missing.length > 0) throw new Error(`Unknown project(s): ${missing.join(", ")}`);
  }
  if (opts.maxProjects !== null) selected = selected.slice(0, opts.maxProjects);
  return selected;
};

const run = (cmd, args, options = {}) =>
  runTool(cmd, args, { maxBuffer: 16 * 1024 * 1024, ...options });

const projectDir = (cacheDir, entry) => join(cacheDir, entry.name);

const cloneProject = (entry, dest, opts) => {
  if (opts.refresh && existsSync(dest)) {
    rmSync(dest, { recursive: true, force: true });
  }
  if (existsSync(join(dest, ".git"))) return { ok: true, cached: true };
  if (opts.skipClone) return { ok: false, error: "missing cache and --skip-clone was set" };

  mkdirSync(dirname(dest), { recursive: true });
  const clone = run("git", [
    "clone",
    "--depth",
    "20",
    "--single-branch",
    "--branch",
    entry.ref,
    `https://github.com/${entry.repo}.git`,
    dest,
  ]);
  if (!clone.ok) {
    return { ok: false, error: (clone.stderr || clone.stdout || clone.reason).trim() };
  }
  return { ok: true, cached: false };
};

const gitHead = (dir) => {
  const out = run("git", ["-C", dir, "rev-parse", "HEAD"]);
  return out.ok ? out.stdout.trim() : "";
};

const loadBaseline = (path) => {
  if (!existsSync(path)) return { version: 1, counts: {}, allowlist: [] };
  const parsed = JSON.parse(readFileSync(path, "utf8"));
  return {
    version: parsed.version || 1,
    counts: parsed.counts || {},
    allowlist: parsed.allowlist || parsed.allowed_spikes || [],
  };
};

const collectObjectArrayEntries = (value, arrayKey) => {
  const entries = [];
  const visit = (node, key = "") => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) {
      if (key === arrayKey) {
        for (const item of node) {
          if (item && typeof item === "object") entries.push(item);
        }
      }
      for (const item of node) visit(item);
      return;
    }
    for (const [childKey, child] of Object.entries(node)) visit(child, childKey);
  };
  visit(value);
  return entries;
};

const collectStylingFindings = (value) => collectObjectArrayEntries(value, "styling_findings");

const collectTokenConsumers = (value) => collectObjectArrayEntries(value, "token_consumers");

const summarizeStyleXThemeCoverage = (value) => {
  const consumers = collectTokenConsumers(value).filter(
    (entry) => typeof entry.token === "string" && !entry.token.startsWith("--"),
  );
  const locations = consumers.flatMap((entry) =>
    Array.isArray(entry.consumers) ? entry.consumers : [],
  );
  return {
    token_definitions: consumers.length,
    stable_conditional_definitions: consumers.filter(
      (entry) =>
        entry.definition_path === STYLEX_EXAMPLE_TOKENS_PATH &&
        entry.token === "globalTokens.surfaceBg",
    ).length,
    nested_token_definitions: consumers.filter(
      (entry) =>
        entry.definition_path === STYLEX_NESTED_TOKENS_PATH &&
        entry.token === "nestedTokens.surface.bg",
    ).length,
    member_consumers: locations.filter((location) => location.kind === "js-member").length,
    theme_consumers: locations.filter((location) => location.kind === "js-call").length,
    official_nested_theme_call_samples: countOfficialStyleXNestedThemeCallSamples(consumers),
    phantom_stable_default_tokens: consumers.filter(
      (entry) =>
        entry.definition_path === STYLEX_EXAMPLE_TOKENS_PATH &&
        entry.token === "globalTokens.surfaceBg.default",
    ).length,
  };
};

const countOfficialStyleXNestedThemeCallSamples = (consumers) => {
  const locations = consumers
    .filter(
      (entry) =>
        entry.definition_path === STYLEX_NESTED_TOKENS_PATH && entry.namespace === "nestedTokens",
    )
    .flatMap((entry) => (Array.isArray(entry.consumers) ? entry.consumers : []))
    .filter(
      (location) => location.kind === "js-call" && location.path === STYLEX_NESTED_TOKENS_PATH,
    );
  return new Set(locations.map((location) => `${location.path || ""}\0${location.line || 0}`)).size;
};

const commandProblem = (command) => {
  if (command.timed_out) return "command timed out";
  if (command.spawn_error) return `command failed to start: ${command.spawn_error}`;
  if (command.parse_error) return `output was invalid JSON: ${command.parse_error}`;
  if (command.status === null || command.status > 1) {
    return `command failed with status ${commandStatusLabel(command)}`;
  }
  return null;
};

const styleXCommandFailure = (command) => {
  if (!command) return "StyleX health command did not run";
  const failure = commandProblem(command);
  return failure ? `StyleX health ${failure}` : null;
};

const projectRan = (project) =>
  !project.error && project.commands.some((command) => commandProblem(command) === null);

const normalizeFinding = (finding) => ({
  code: String(finding.code || "unknown"),
  sub_kind: String(finding.sub_kind || finding.kind || "unknown"),
  confidence: String(finding.confidence || finding.severity || "unknown"),
  path: typeof finding.path === "string" ? finding.path : "",
});

const groupFindings = (findings) => {
  const groups = new Map();
  for (const finding of findings.map(normalizeFinding)) {
    const key = [finding.code, finding.sub_kind, finding.confidence].join("|");
    const current = groups.get(key) || {
      code: finding.code,
      sub_kind: finding.sub_kind,
      confidence: finding.confidence,
      count: 0,
      sample_paths: [],
    };
    current.count += 1;
    if (finding.path && current.sample_paths.length < SAMPLE_PATH_LIMIT) {
      current.sample_paths.push(finding.path);
    }
    groups.set(key, current);
  }
  return [...groups.values()].toSorted(
    (a, b) =>
      b.count - a.count || a.code.localeCompare(b.code) || a.sub_kind.localeCompare(b.sub_kind),
  );
};

const spikeKey = (project, command, code, subKind, confidence) =>
  `${project}:${command}:${code}:${subKind}:${confidence}`;

const issueCodeKey = (project, command, code) => `${project}:${command}:${code}:*:all`;

const computeSpikes = (results, baseline) => {
  const allowlist = new Set(baseline.allowlist);
  const spikes = [];
  for (const project of results.projects) {
    for (const command of project.commands) {
      spikes.push(...issueCodeSpikes(project, command, baseline, allowlist));
      spikes.push(...highConfidenceSpikes(project, command, baseline, allowlist));
    }
  }
  return spikes;
};

const issueCodeSpikes = (project, command, baseline, allowlist) =>
  [...issueCodeCounts(project, command)].flatMap(([key, count]) => {
    const previous = Number(baseline.counts[key] || 0);
    if (count <= previous || allowlist.has(key)) return [];
    return [
      {
        scope: "issue-code",
        key,
        project: project.name,
        command: command.id,
        code: key.split(":")[2],
        previous,
        current: count,
      },
    ];
  });

const issueCodeCounts = (project, command) => {
  const counts = new Map();
  for (const group of command.finding_groups) {
    const key = issueCodeKey(project.name, command.id, group.code);
    counts.set(key, (counts.get(key) || 0) + group.count);
  }
  return counts;
};

const highConfidenceSpikes = (project, command, baseline, allowlist) =>
  command.finding_groups.filter(isHighConfidenceGroup).flatMap((group) => {
    const key = spikeKey(project.name, command.id, group.code, group.sub_kind, group.confidence);
    const previous = Number(baseline.counts[key] || 0);
    if (group.count <= previous || allowlist.has(key)) return [];
    return [
      {
        scope: "high-confidence-sub-kind",
        key,
        project: project.name,
        command: command.id,
        code: group.code,
        sub_kind: group.sub_kind,
        confidence: group.confidence,
        previous,
        current: group.count,
      },
    ];
  });

const isHighConfidenceGroup = (group) => group.confidence === "high";

const runFallowCommand = (fallowBin, entry, dir, command, opts) => {
  const fullArgs = [...command.args, "--root", dir];
  const proc = run(fallowBin, fullArgs, {
    cwd: dir,
    timeout: opts.timeoutMs,
    env: { ...process.env, FALLOW_QUIET: "1" },
    okStatuses: [0, 1],
  });
  const parsed = parseJsonReport(proc.stdout);
  const findings = parsed.ok ? collectStylingFindings(parsed.value) : [];
  const stylexThemeCoverage =
    entry.name === "stylex" && command.id === "health-css" && parsed.ok
      ? summarizeStyleXThemeCoverage(parsed.value)
      : null;
  return {
    id: command.id,
    args: fullArgs,
    status: proc.status,
    signal: proc.signal,
    timed_out: proc.timedOut,
    spawn_error: proc.spawnError,
    parse_error: parsed.ok ? null : parsed.error,
    stderr_sample: proc.stderr.trim().slice(0, 2000),
    finding_groups: groupFindings(findings),
    total_styling_findings: findings.length,
    stylex_theme_coverage: stylexThemeCoverage,
    project: entry.name,
  };
};

const styleXContractFailures = (projects) => {
  const project = projects.find((entry) => entry.name === "stylex");
  if (!project) return [];
  if (project.error) return [`StyleX project failed: ${project.error}`];
  const command = project.commands.find((entry) => entry.id === "health-css");
  const commandFailure = styleXCommandFailure(command);
  if (commandFailure) return [commandFailure];
  const coverage = command?.stylex_theme_coverage;
  if (!coverage) return ["StyleX health output did not expose theme coverage"];

  const failures = [];
  if (coverage.token_definitions === 0) failures.push("no StyleX token definitions detected");
  if (coverage.stable_conditional_definitions === 0) {
    failures.push("stable StyleX conditional token shape regressed");
  }
  if (coverage.nested_token_definitions === 0) {
    failures.push("no nested StyleX token definitions detected");
  }
  if (coverage.member_consumers === 0) failures.push("no StyleX member consumers detected");
  if (coverage.theme_consumers === 0) failures.push("no StyleX theme consumers detected");
  if (coverage.official_nested_theme_call_samples === 0) {
    failures.push("official nested StyleX theme produced no theme-call sample");
  }
  if (coverage.phantom_stable_default_tokens !== 0) {
    failures.push("stable conditional value was exposed as a phantom .default token");
  }
  return failures;
};

const stackCoverage = (projects) => {
  const covered = new Set();
  for (const project of projects) {
    for (const stack of project.stacks) covered.add(stack);
  }
  return REQUIRED_STACKS.map((stack) => ({ stack, covered: covered.has(stack) }));
};

const commandStatusLabel = (command) => {
  if (command.status !== null) return String(command.status);
  if (command.timed_out) return "timeout";
  if (command.spawn_error) return `spawn error: ${command.spawn_error}`;
  return command.signal || "signal";
};

const renderMarkdown = (results) => {
  const lines = renderMarkdownHeader(results);
  appendStackCoverage(lines, results.stack_coverage);
  appendStyleXThemeContract(lines, results);
  appendSpikes(lines, results.spikes);
  appendProjects(lines, results.projects);
  return `${lines.join("\n")}\n`;
};

const appendStyleXThemeContract = (lines, results) => {
  const project = results.projects.find((entry) => entry.name === "stylex");
  if (!project) return;
  lines.push("", "## StyleX Theme Contract", "");
  if (project.error) {
    lines.push(`Failed: ${project.error}`);
    return;
  }
  const coverage = project.commands.find(
    (entry) => entry.id === "health-css",
  )?.stylex_theme_coverage;
  if (!coverage) {
    lines.push("Failed: StyleX health output did not expose theme coverage.");
    return;
  }
  lines.push(
    `Status: ${results.stylex_contract_failures.length === 0 ? "pass" : "fail"}`,
    "",
    `Token definitions: ${coverage.token_definitions}`,
    `Stable conditional definitions: ${coverage.stable_conditional_definitions}`,
    `Nested token definitions: ${coverage.nested_token_definitions}`,
    `Member consumers: ${coverage.member_consumers}`,
    `Theme consumers: ${coverage.theme_consumers}`,
    `Official nested theme-call samples: ${coverage.official_nested_theme_call_samples}`,
    `Phantom stable \`.default\` tokens: ${coverage.phantom_stable_default_tokens}`,
  );
  for (const failure of results.stylex_contract_failures) lines.push(`- ${failure}`);
};

const renderMarkdownHeader = (results) => [
  "# Styling Corpus Smoke",
  "",
  `Generated: ${results.generated_at}`,
  `Fallow: \`${results.fallow_bin}\``,
  `Cache: \`${results.cache_dir}\``,
];

const appendStackCoverage = (lines, coverage) => {
  lines.push("", "## Stack Coverage", "", "| Stack | Covered |", "| --- | --- |");
  for (const item of coverage) {
    lines.push(`| ${item.stack} | ${item.covered ? "yes" : "no"} |`);
  }
};

const appendSpikes = (lines, spikes) => {
  lines.push("", "## Spikes", "");
  if (spikes.length === 0) {
    lines.push("No non-allowlisted spikes.");
    return;
  }
  lines.push("| Scope | Project | Command | Code | Sub-kind | Previous | Current |");
  lines.push("| --- | --- | --- | --- | --- | ---: | ---: |");
  for (const spike of spikes) {
    lines.push(
      `| ${spike.scope} | ${spike.project} | ${spike.command} | ${spike.code} | ${spike.sub_kind || "*"} | ${spike.previous} | ${spike.current} |`,
    );
  }
};

const appendProjects = (lines, projects) => {
  lines.push("", "## Projects", "");
  for (const project of projects) {
    appendProject(lines, project);
  }
};

const appendProject = (lines, project) => {
  lines.push(`### ${project.name}`, "");
  lines.push(`Repo: \`${project.repo}\` at \`${project.ref}\``);
  lines.push(`Commit: \`${project.commit || "unknown"}\``);
  lines.push(`Stacks: ${project.stacks.map((s) => `\`${s}\``).join(", ")}`);
  if (project.error) {
    lines.push(`Error: ${project.error}`, "");
    return;
  }
  lines.push("", "| Command | Status | Styling findings | Top groups |");
  lines.push("| --- | ---: | ---: | --- |");
  for (const command of project.commands) {
    lines.push(
      `| ${command.id} | ${commandStatusLabel(command)} | ${command.total_styling_findings} | ${topGroups(command)} |`,
    );
  }
  lines.push("");
};

const topGroups = (command) =>
  command.finding_groups
    .slice(0, 3)
    .map((group) => `${group.code}/${group.sub_kind}/${group.confidence}: ${group.count}`)
    .join("<br>");

const initialResults = (opts, fallowBin, corpus) => ({
  schema_version: 2,
  generated_at: new Date().toISOString(),
  fallow_bin: fallowBin,
  cache_dir: opts.cacheDir,
  baseline: opts.baseline,
  commands: COMMANDS.map((command) => ({ id: command.id, args: command.args })),
  corpus: corpus.map((entry) => ({
    name: entry.name,
    repo: entry.repo,
    ref: entry.ref,
    stacks: entry.stacks,
  })),
  stack_coverage: stackCoverage(corpus),
  projects: [],
  spikes: [],
  stylex_contract_failures: [],
});

const runProject = (entry, opts, fallowBin) => {
  const dest = projectDir(opts.cacheDir, entry);
  console.error(`== ${entry.name} (${entry.repo} @ ${entry.ref}) ==`);
  const clone = cloneProject(entry, dest, opts);
  const project = {
    name: entry.name,
    repo: entry.repo,
    ref: entry.ref,
    stacks: entry.stacks,
    path: dest,
    commit: clone.ok ? gitHead(dest) : "",
    cached: clone.ok ? clone.cached : false,
    commands: [],
    error: clone.ok ? null : clone.error,
  };
  if (!clone.ok) {
    console.error(`  skip: ${clone.error}`);
    return project;
  }
  for (const command of COMMANDS) {
    console.error(`  ${command.id}`);
    project.commands.push(runFallowCommand(fallowBin, entry, dest, command, opts));
  }
  return project;
};

const main = () => {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    console.log(usage());
    return 0;
  }
  const corpus = selectedCorpus(opts);
  if (opts.list) {
    for (const entry of corpus) {
      console.log(`${entry.name}\t${entry.repo}\t${entry.ref}\t${entry.stacks.join(",")}`);
    }
    return 0;
  }

  const fallowBin = resolveFallowBin(
    opts.fallowBin
      ? [opts.fallowBin]
      : [cargoFallowBin(REPO_ROOT, "release"), cargoFallowBin(REPO_ROOT, "debug"), "fallow"],
  );
  mkdirSync(opts.cacheDir, { recursive: true });
  mkdirSync(opts.outDir, { recursive: true });
  const baseline = loadBaseline(opts.baseline);
  const results = initialResults(opts, fallowBin, corpus);

  for (const entry of corpus) {
    results.projects.push(runProject(entry, opts, fallowBin));
  }

  results.stylex_contract_failures = styleXContractFailures(results.projects);
  results.spikes = computeSpikes(results, baseline);
  const jsonPath = join(opts.outDir, "styling-corpus-smoke.json");
  const markdownPath = join(opts.outDir, "styling-corpus-smoke.md");
  writeFileSync(jsonPath, `${JSON.stringify(results, null, 2)}\n`);
  writeFileSync(markdownPath, renderMarkdown(results));
  console.error(`JSON: ${jsonPath}`);
  console.error(`Markdown: ${markdownPath}`);

  if (opts.failOnSpikes && results.spikes.length > 0) return 2;
  if (results.stylex_contract_failures.length > 0) return 3;
  if (!results.projects.some(projectRan)) {
    console.error("styling corpus smoke: no project gave a usable fallow result");
    return 1;
  }
  return 0;
};

try {
  process.exitCode = main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
