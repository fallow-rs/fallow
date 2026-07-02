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

const VALUE_OPTION_SETTERS = {
  "--cache-dir": (opts, value) => {
    opts.cacheDir = value;
  },
  "--out-dir": (opts, value) => {
    opts.outDir = value;
  },
  "--fallow-bin": (opts, value) => {
    opts.fallowBin = value;
  },
  "--baseline": (opts, value) => {
    opts.baseline = value;
  },
  "--project": (opts, value) => {
    opts.projects.push(value);
  },
  "--max-projects": (opts, value) => {
    opts.maxProjects = Number(value);
  },
  "--timeout-ms": (opts, value) => {
    opts.timeoutMs = Number(value);
  },
};

const FLAG_OPTION_SETTERS = {
  "--refresh": (opts) => {
    opts.refresh = true;
  },
  "--skip-clone": (opts) => {
    opts.skipClone = true;
  },
  "--fail-on-spikes": (opts) => {
    opts.failOnSpikes = true;
  },
  "--list": (opts) => {
    opts.list = true;
  },
  "--help": (opts) => {
    opts.help = true;
  },
  "-h": (opts) => {
    opts.help = true;
  },
};

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

  for (let index = 0; index < argv.length; index += 1) {
    index = applyArg(argv, index, opts);
  }

  opts.cacheDir = resolve(opts.cacheDir);
  opts.outDir = resolve(opts.outDir);
  opts.baseline = resolve(opts.baseline);
  if (!Number.isFinite(opts.timeoutMs) || opts.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive number");
  }
  return opts;
};

const applyArg = (argv, index, opts) => {
  const arg = argv[index];
  const inline = parseInlineValue(arg);
  if (inline) {
    setValueOption(opts, inline.name, inline.value);
    return index;
  }
  if (VALUE_OPTION_SETTERS[arg]) {
    setValueOption(opts, arg, readNextValue(argv, index, arg));
    return index + 1;
  }
  if (FLAG_OPTION_SETTERS[arg]) {
    FLAG_OPTION_SETTERS[arg](opts);
    return index;
  }
  throw new Error(`Unknown argument: ${arg}`);
};

const parseInlineValue = (arg) => {
  const separator = arg.indexOf("=");
  if (separator === -1) return null;
  const name = arg.slice(0, separator);
  if (!VALUE_OPTION_SETTERS[name]) return null;
  return { name, value: arg.slice(separator + 1) };
};

const readNextValue = (argv, index, arg) => {
  const nextIndex = index + 1;
  if (nextIndex >= argv.length) throw new Error(`Missing value for ${arg}`);
  return argv[nextIndex];
};

const setValueOption = (opts, name, value) => {
  const setter = VALUE_OPTION_SETTERS[name];
  if (!setter) throw new Error(`Unknown argument: ${name}`);
  setter(opts, value);
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
    if (check.status === 0) {
      const hasPathSeparator = candidate.includes("/") || candidate.includes("\\");
      return hasPathSeparator ? resolve(candidate) : candidate;
    }
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
  });
  const parsed = parseJson(proc.stdout || "");
  const findings = parsed.ok ? collectStylingFindings(parsed.value) : [];
  return {
    id: command.id,
    args: fullArgs,
    status: proc.status,
    signal: proc.signal || null,
    timed_out: Boolean(proc.error && proc.error.code === "ETIMEDOUT"),
    spawn_error: proc.error ? proc.error.message : null,
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

const commandStatusLabel = (command) => {
  if (command.status !== null) return String(command.status);
  if (command.timed_out) return "timeout";
  if (command.spawn_error) return `spawn error: ${command.spawn_error}`;
  return command.signal || "signal";
};

const renderMarkdown = (results) => {
  const lines = renderMarkdownHeader(results);
  appendStackCoverage(lines, results.stack_coverage);
  appendSpikes(lines, results.spikes);
  appendProjects(lines, results.projects);
  return `${lines.join("\n")}\n`;
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

  const fallowBin = findFallowBin(opts);
  mkdirSync(opts.cacheDir, { recursive: true });
  mkdirSync(opts.outDir, { recursive: true });
  const baseline = loadBaseline(opts.baseline);
  const results = initialResults(opts, fallowBin, corpus);

  for (const entry of corpus) {
    results.projects.push(runProject(entry, opts, fallowBin));
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
