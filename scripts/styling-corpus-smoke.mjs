#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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

const parseArgs = (argv) => {
  const opts = {
    cacheDir: process.env.FALLOW_STYLING_CORPUS_CACHE || DEFAULT_CACHE_DIR,
    outDir: DEFAULT_OUT_DIR,
    fallowBin: process.env.FALLOW_BIN || "",
    baseline: DEFAULT_BASELINE,
    projects: [],
    maxProjects: 0,
    timeoutMs: Number(process.env.FALLOW_STYLING_CORPUS_TIMEOUT_MS || 120_000),
    refresh: false,
    skipClone: false,
    failOnSpikes: false,
    list: false,
    help: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      i += 1;
      if (i >= argv.length) throw new Error(`Missing value for ${arg}`);
      return argv[i];
    };
    if (arg === "--cache-dir") opts.cacheDir = next();
    else if (arg.startsWith("--cache-dir=")) opts.cacheDir = arg.slice("--cache-dir=".length);
    else if (arg === "--out-dir") opts.outDir = next();
    else if (arg.startsWith("--out-dir=")) opts.outDir = arg.slice("--out-dir=".length);
    else if (arg === "--fallow-bin") opts.fallowBin = next();
    else if (arg.startsWith("--fallow-bin=")) opts.fallowBin = arg.slice("--fallow-bin=".length);
    else if (arg === "--baseline") opts.baseline = next();
    else if (arg.startsWith("--baseline=")) opts.baseline = arg.slice("--baseline=".length);
    else if (arg === "--project") opts.projects.push(next());
    else if (arg.startsWith("--project=")) opts.projects.push(arg.slice("--project=".length));
    else if (arg === "--max-projects") opts.maxProjects = Number(next());
    else if (arg.startsWith("--max-projects=")) {
      opts.maxProjects = Number(arg.slice("--max-projects=".length));
    } else if (arg === "--timeout-ms") opts.timeoutMs = Number(next());
    else if (arg.startsWith("--timeout-ms="))
      opts.timeoutMs = Number(arg.slice("--timeout-ms=".length));
    else if (arg === "--refresh") opts.refresh = true;
    else if (arg === "--skip-clone") opts.skipClone = true;
    else if (arg === "--fail-on-spikes") opts.failOnSpikes = true;
    else if (arg === "--list") opts.list = true;
    else if (arg === "--help" || arg === "-h") opts.help = true;
    else throw new Error(`Unknown argument: ${arg}`);
  }

  opts.cacheDir = resolve(opts.cacheDir);
  opts.outDir = resolve(opts.outDir);
  opts.baseline = resolve(opts.baseline);
  if (!Number.isFinite(opts.timeoutMs) || opts.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }
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
  if (opts.maxProjects > 0) selected = selected.slice(0, opts.maxProjects);
  return selected;
};

const findFallowBin = (opts) => {
  const candidates = [
    opts.fallowBin,
    join(REPO_ROOT, "target", "release", "fallow"),
    join(REPO_ROOT, "target", "debug", "fallow"),
    "fallow",
  ].filter(Boolean);
  for (const candidate of candidates) {
    const check = spawnSync(candidate, ["--version"], { encoding: "utf8" });
    if (check.status === 0) return candidate;
  }
  throw new Error("fallow binary not found. Build fallow or pass --fallow-bin PATH");
};

const run = (cmd, args, options = {}) =>
  spawnSync(cmd, args, {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    ...options,
  });

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
  if (clone.status !== 0) {
    return { ok: false, error: (clone.stderr || clone.stdout || "git clone failed").trim() };
  }
  return { ok: true, cached: false };
};

const gitHead = (dir) => {
  const out = run("git", ["-C", dir, "rev-parse", "HEAD"]);
  return out.status === 0 ? out.stdout.trim() : "";
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

const parseJson = (stdout) => {
  try {
    return { ok: true, value: JSON.parse(stdout) };
  } catch (error) {
    return { ok: false, error: error.message };
  }
};

const collectStylingFindings = (value) => {
  const findings = [];
  const visit = (node, key = "") => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) {
      if (key === "styling_findings") {
        for (const item of node) {
          if (item && typeof item === "object") findings.push(item);
        }
      }
      for (const item of node) visit(item);
      return;
    }
    for (const [childKey, child] of Object.entries(node)) visit(child, childKey);
  };
  visit(value);
  return findings;
};

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
  return [...groups.values()].sort(
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
      for (const group of command.finding_groups) {
        const issueKey = issueCodeKey(project.name, command.id, group.code);
        const issueBaseline = Number(baseline.counts[issueKey] || 0);
        if (group.count > issueBaseline && !allowlist.has(issueKey)) {
          spikes.push({
            scope: "issue-code",
            key: issueKey,
            project: project.name,
            command: command.id,
            code: group.code,
            previous: issueBaseline,
            current: group.count,
          });
        }
        if (group.confidence !== "high") continue;
        const key = spikeKey(
          project.name,
          command.id,
          group.code,
          group.sub_kind,
          group.confidence,
        );
        const previous = Number(baseline.counts[key] || 0);
        if (group.count > previous && !allowlist.has(key)) {
          spikes.push({
            scope: "high-confidence-sub-kind",
            key,
            project: project.name,
            command: command.id,
            code: group.code,
            sub_kind: group.sub_kind,
            confidence: group.confidence,
            previous,
            current: group.count,
          });
        }
      }
    }
  }
  return spikes;
};

const runFallowCommand = (fallowBin, entry, dir, command, opts) => {
  const fullArgs = [...command.args, "--root", dir];
  const proc = run(fallowBin, fullArgs, {
    cwd: dir,
    timeout: opts.timeoutMs,
    env: { ...process.env, FALLOW_QUIET: "1" },
  });
  const parsed = parseJson(proc.stdout || "");
  const findings = parsed.ok ? collectStylingFindings(parsed.value) : [];
  return {
    id: command.id,
    args: fullArgs,
    status: proc.status,
    signal: proc.signal || null,
    timed_out: Boolean(proc.error && proc.error.code === "ETIMEDOUT"),
    parse_error: parsed.ok ? null : parsed.error,
    stderr_sample: (proc.stderr || "").trim().slice(0, 2000),
    finding_groups: groupFindings(findings),
    total_styling_findings: findings.length,
    project: entry.name,
  };
};

const stackCoverage = (projects) => {
  const covered = new Set();
  for (const project of projects) {
    for (const stack of project.stacks) covered.add(stack);
  }
  return REQUIRED_STACKS.map((stack) => ({ stack, covered: covered.has(stack) }));
};

const renderMarkdown = (results) => {
  const lines = [
    "# Styling Corpus Smoke",
    "",
    `Generated: ${results.generated_at}`,
    `Fallow: \`${results.fallow_bin}\``,
    `Cache: \`${results.cache_dir}\``,
    "",
    "## Stack Coverage",
    "",
    "| Stack | Covered |",
    "| --- | --- |",
  ];
  for (const item of results.stack_coverage) {
    lines.push(`| ${item.stack} | ${item.covered ? "yes" : "no"} |`);
  }
  lines.push("", "## Spikes", "");
  if (results.spikes.length === 0) {
    lines.push("No non-allowlisted spikes.");
  } else {
    lines.push("| Scope | Project | Command | Code | Sub-kind | Previous | Current |");
    lines.push("| --- | --- | --- | --- | --- | ---: | ---: |");
    for (const spike of results.spikes) {
      lines.push(
        `| ${spike.scope} | ${spike.project} | ${spike.command} | ${spike.code} | ${spike.sub_kind || "*"} | ${spike.previous} | ${spike.current} |`,
      );
    }
  }
  lines.push("", "## Projects", "");
  for (const project of results.projects) {
    lines.push(`### ${project.name}`, "");
    lines.push(`Repo: \`${project.repo}\` at \`${project.ref}\``);
    lines.push(`Commit: \`${project.commit || "unknown"}\``);
    lines.push(`Stacks: ${project.stacks.map((s) => `\`${s}\``).join(", ")}`);
    if (project.error) {
      lines.push(`Error: ${project.error}`, "");
      continue;
    }
    lines.push("", "| Command | Status | Styling findings | Top groups |");
    lines.push("| --- | ---: | ---: | --- |");
    for (const command of project.commands) {
      const top = command.finding_groups
        .slice(0, 3)
        .map((group) => `${group.code}/${group.sub_kind}/${group.confidence}: ${group.count}`)
        .join("<br>");
      lines.push(
        `| ${command.id} | ${command.status ?? "signal"} | ${command.total_styling_findings} | ${top || ""} |`,
      );
    }
    lines.push("");
  }
  return `${lines.join("\n")}\n`;
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

  const fallowBin = findFallowBin(opts);
  mkdirSync(opts.cacheDir, { recursive: true });
  mkdirSync(opts.outDir, { recursive: true });
  const baseline = loadBaseline(opts.baseline);
  const results = {
    schema_version: 1,
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
  };

  for (const entry of corpus) {
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
    if (clone.ok) {
      for (const command of COMMANDS) {
        console.error(`  ${command.id}`);
        project.commands.push(runFallowCommand(fallowBin, entry, dest, command, opts));
      }
    } else {
      console.error(`  skip: ${clone.error}`);
    }
    results.projects.push(project);
  }

  results.spikes = computeSpikes(results, baseline);
  const jsonPath = join(opts.outDir, "styling-corpus-smoke.json");
  const markdownPath = join(opts.outDir, "styling-corpus-smoke.md");
  writeFileSync(jsonPath, `${JSON.stringify(results, null, 2)}\n`);
  writeFileSync(markdownPath, renderMarkdown(results));
  console.error(`JSON: ${jsonPath}`);
  console.error(`Markdown: ${markdownPath}`);

  if (opts.failOnSpikes && results.spikes.length > 0) return 2;
  if (results.projects.every((project) => project.error)) return 1;
  return 0;
};

try {
  process.exitCode = main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
