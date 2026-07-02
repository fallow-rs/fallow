#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(SCRIPT_DIR, "..");
const DEFAULT_OUT_DIR = join(REPO_ROOT, "target", "styling-pr-smoke");
const DEFAULT_CLONE_DEPTH = 1000;
const DEFAULT_MAX_PRS = 24;
const DEFAULT_PRS_PER_REPO = 2;
const DEFAULT_PR_LOOKBACK = 15;

const CANDIDATES = [
  { repo: "Guria/modern-stack", evidence: "fallow.toml" },
  { repo: "Janhouse/traefik-proxy-admin", evidence: ".gitea workflow / fallow skill" },
  { repo: "PathableAI-org/SpecAble", evidence: ".husky/pre-commit" },
  { repo: "WeGotWorkspace/wegotworkspace", evidence: "fallow.toml" },
  { repo: "browseros-ai/BrowserOS", evidence: "CLAUDE.md fallow" },
  { repo: "ddboy19912/sivraj", evidence: "apps/web/.fallowrc.jsonc.bak" },
  { repo: "CoreBunch/Instatic", evidence: "fallow search hit" },
  { repo: "sanity-io/sdk", evidence: "fallow-baselines" },
  { repo: "mirumee/nimara-ecommerce", evidence: "package.json fallow" },
  { repo: "atomicstrata/atomicmemory", evidence: "scripts/ci/code-health.mjs" },
  { repo: "lightsound/tanstack-start-start", evidence: "AGENTS.md fallow" },
  { repo: "NWACus/web", evidence: "docs/fallow.md" },
  { repo: "rmccorkl/TubeSage", evidence: "eslint.config.mjs fallow" },
  { repo: "metrists/metrists", evidence: "packages/*/.fallowrc.json" },
  { repo: "Aurora-AI/Elysiancorpfront", evidence: "public/evidence/micro-steps.txt" },
  { repo: "ueberdosis/tiptap", evidence: "AGENTS.md fallow audit" },
  { repo: "open-gsd/gsd-core", evidence: "src/fallow-runner.cts" },
  { repo: "callstack/agent-device", evidence: "package.json fallow audit" },
  { repo: "filiphsps/commerce", evidence: ".claude/hooks/fallow-check.sh" },
  { repo: "AWeber-Imbi/imbi-ui", evidence: ".husky/pre-commit fallow audit" },
  { repo: "schuettc/now-playing", evidence: ".claude/hooks/fallow-gate.sh" },
  { repo: "everr-labs/everr", evidence: "lefthook.yml fallow" },
  {
    repo: "LubomirGeorgiev/cloudflare-workers-nextjs-saas-template",
    evidence: "AGENTS.md fallow audit",
  },
  { repo: "christianpeirson/argos-edge", evidence: ".husky/pre-push fallow" },
  { repo: "rejifald/movar", evidence: "scripts/metrics-gate.mts" },
  { repo: "walterlow/freecut", evidence: "scripts/check-fallow-changed-health.mjs" },
  { repo: "cliftonc/drizzle-cube", evidence: "quality-gate skill fallow" },
  { repo: "trillium/massage", evidence: "scripts/fallow-prepush.sh" },
  { repo: "electather/nama", evidence: ".vite-hooks/pre-push" },
  { repo: "BlakePetersen/petersen-pack", evidence: "apps/luna/.husky/pre-push" },
];

const VALUE_OPTION_SETTERS = {
  "--out-dir": (opts, value) => {
    opts.outDir = value;
  },
  "--cache-dir": (opts, value) => {
    opts.cacheDir = value;
  },
  "--fallow-bin": (opts, value) => {
    opts.fallowBin = value;
  },
  "--max-prs": (opts, value) => {
    opts.maxPrs = Number(value);
  },
  "--prs-per-repo": (opts, value) => {
    opts.prsPerRepo = Number(value);
  },
  "--pr-lookback": (opts, value) => {
    opts.prLookback = Number(value);
  },
  "--clone-depth": (opts, value) => {
    opts.cloneDepth = Number(value);
  },
  "--timeout-ms": (opts, value) => {
    opts.timeoutMs = Number(value);
  },
};

const FLAG_OPTION_SETTERS = {
  "--run-only": (opts) => {
    opts.runOnly = true;
  },
  "--select-only": (opts) => {
    opts.selectOnly = true;
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
    outDir: DEFAULT_OUT_DIR,
    cacheDir: "",
    fallowBin: process.env.FALLOW_BIN || "",
    maxPrs: DEFAULT_MAX_PRS,
    prsPerRepo: DEFAULT_PRS_PER_REPO,
    prLookback: DEFAULT_PR_LOOKBACK,
    cloneDepth: DEFAULT_CLONE_DEPTH,
    timeoutMs: Number(process.env.FALLOW_STYLING_PR_TIMEOUT_MS || 180_000),
    runOnly: false,
    selectOnly: false,
    list: false,
    help: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    index = applyArg(argv, index, opts);
  }

  opts.outDir = resolve(opts.outDir);
  opts.cacheDir = resolve(opts.cacheDir || join(opts.outDir, "repos"));
  opts.fallowBin = resolveFallowBin(opts.fallowBin);
  for (const [name, value] of [
    ["--max-prs", opts.maxPrs],
    ["--prs-per-repo", opts.prsPerRepo],
    ["--pr-lookback", opts.prLookback],
    ["--clone-depth", opts.cloneDepth],
    ["--timeout-ms", opts.timeoutMs],
  ]) {
    if (!Number.isFinite(value) || value <= 0) {
      throw new Error(`${name} must be a positive number`);
    }
  }
  if (opts.runOnly && opts.selectOnly) {
    throw new Error("--run-only and --select-only are mutually exclusive");
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

const resolveFallowBin = (configured) => {
  if (configured) return resolve(configured);
  const local = join(REPO_ROOT, "target", "debug", "fallow");
  if (existsSync(local)) return local;
  return "fallow";
};

const usage = () => `Usage: node scripts/styling-pr-smoke.mjs [options]

Options:
  --out-dir DIR         Output directory. Default: target/styling-pr-smoke
  --cache-dir DIR       Clone cache. Default: <out-dir>/repos
  --fallow-bin PATH     fallow binary. Default: FALLOW_BIN, target/debug/fallow, then PATH
  --max-prs N           Max selected frontend styling PRs. Default: ${DEFAULT_MAX_PRS}
  --prs-per-repo N      Max selected PRs per repo. Default: ${DEFAULT_PRS_PER_REPO}
  --pr-lookback N       Recent PRs inspected per repo. Default: ${DEFAULT_PR_LOOKBACK}
  --clone-depth N       Git clone/fetch depth. Default: ${DEFAULT_CLONE_DEPTH}
  --timeout-ms N        Per audit timeout. Default: 180000
  --run-only            Reuse selected-prs.json, skip GitHub PR selection
  --select-only         Refresh selected-prs.json, skip clone/audit
  --list                Print candidate repos and exit
`;

const run = (cmd, args, opts = {}) =>
  spawnSync(cmd, args, {
    encoding: "utf8",
    timeout: opts.timeout ?? 120_000,
    maxBuffer: 32 * 1024 * 1024,
    ...opts,
  });

const runJson = (cmd, args, opts = {}) => {
  const proc = run(cmd, args, opts);
  if (proc.status !== 0) {
    return { ok: false, error: (proc.stderr || proc.stdout || `${cmd} failed`).trim() };
  }
  try {
    return { ok: true, value: JSON.parse(proc.stdout || "[]") };
  } catch (error) {
    return { ok: false, error: error.message };
  }
};

const frontendFile = (path) =>
  /\.(css|scss|sass|less|pcss|tsx|jsx|vue|svelte|astro|html|mdx)$/i.test(path) ||
  /(^|\/)(app|apps|components|pages|src|styles|theme|ui|web)\//i.test(path);

const stylingFile = (path) =>
  /\.(css|scss|sass|less|pcss|module\.(css|scss|sass)|vue|svelte|astro)$/i.test(path) ||
  /(tailwind|panda|stylex|stitches|styled|emotion|theme|tokens|styles?)/i.test(path);

const selectPrs = (opts) => {
  const selected = [];
  const repoSummaries = [];

  for (const candidate of CANDIDATES) {
    if (selected.length >= opts.maxPrs) break;
    console.error(`== ${candidate.repo} ==`);
    const [owner, name] = candidate.repo.split("/");
    const prs = runJson(
      "gh",
      [
        "api",
        `repos/${owner}/${name}/pulls?state=all&per_page=${opts.prLookback}&sort=updated&direction=desc`,
      ],
      { timeout: 30_000 },
    );
    if (!prs.ok) {
      repoSummaries.push({ ...candidate, status: "pr-list-failed", error: prs.error });
      continue;
    }

    const repoSelected = [];
    for (const pr of prs.value) {
      if (selected.length >= opts.maxPrs || repoSelected.length >= opts.prsPerRepo) break;
      const files = runJson(
        "gh",
        ["api", `repos/${owner}/${name}/pulls/${pr.number}/files?per_page=100`],
        { timeout: 30_000 },
      );
      if (!files.ok) continue;
      const paths = (files.value || []).map((file) => file.filename).filter(Boolean);
      const frontend = paths.filter(frontendFile);
      const styling = paths.filter(stylingFile);
      if (frontend.length === 0 || styling.length === 0) continue;
      const item = {
        repo: candidate.repo,
        evidence: candidate.evidence,
        number: pr.number,
        title: pr.title,
        url: pr.html_url,
        state: pr.state,
        updatedAt: pr.updated_at,
        baseRefName: pr.base?.ref || "",
        headRefName: pr.head?.ref || "",
        frontendFiles: frontend.slice(0, 12),
        stylingFiles: styling.slice(0, 12),
        changedFileCount: paths.length,
      };
      selected.push(item);
      repoSelected.push(item);
    }
    repoSummaries.push({
      ...candidate,
      status: "ok",
      prCount: prs.value.length,
      selected: repoSelected.map((pr) => pr.number),
    });
  }

  return {
    generated_at: new Date().toISOString(),
    selected,
    repoSummaries,
  };
};

const slug = (repo) => repo.replace("/", "__");

export const remoteBranchRefspec = (branch) => `refs/heads/${branch}:refs/remotes/origin/${branch}`;

const fetchRemoteBranch = (dir, branch, opts) =>
  run(
    "git",
    ["-C", dir, "fetch", "origin", remoteBranchRefspec(branch), "--depth", String(opts.cloneDepth)],
    { timeout: 120_000 },
  );

const ensureClone = (repo, baseRef, opts) => {
  const dir = join(opts.cacheDir, slug(repo));
  if (!existsSync(join(dir, ".git"))) {
    mkdirSync(dirname(dir), { recursive: true });
    const clone = run(
      "git",
      [
        "clone",
        "--depth",
        String(opts.cloneDepth),
        "--single-branch",
        "--branch",
        baseRef,
        `https://github.com/${repo}.git`,
        dir,
      ],
      { timeout: 180_000 },
    );
    if (clone.status !== 0) {
      return { ok: false, dir, error: (clone.stderr || clone.stdout || "clone failed").trim() };
    }
  }
  const fetchBase = fetchRemoteBranch(dir, baseRef, opts);
  if (fetchBase.status !== 0) {
    return {
      ok: false,
      dir,
      error: (fetchBase.stderr || fetchBase.stdout || "fetch base failed").trim(),
    };
  }
  return { ok: true, dir };
};

const checkoutPr = (dir, pr, opts) => {
  const branch = `pr-${pr.number}`;
  const checkoutBase = run("git", ["-C", dir, "checkout", "--quiet", `origin/${pr.baseRefName}`], {
    timeout: 60_000,
  });
  if (checkoutBase.status !== 0) {
    return {
      ok: false,
      error: (checkoutBase.stderr || checkoutBase.stdout || "checkout base failed").trim(),
    };
  }
  const fetch = run(
    "git",
    [
      "-C",
      dir,
      "fetch",
      "origin",
      `pull/${pr.number}/head:refs/heads/${branch}`,
      "--depth",
      String(opts.cloneDepth),
      "--force",
    ],
    { timeout: 120_000 },
  );
  if (fetch.status !== 0) {
    return { ok: false, error: (fetch.stderr || fetch.stdout || "fetch PR failed").trim() };
  }
  const checkout = run("git", ["-C", dir, "checkout", "--quiet", branch], { timeout: 60_000 });
  if (checkout.status !== 0) {
    return { ok: false, error: (checkout.stderr || checkout.stdout || "checkout failed").trim() };
  }
  return { ok: true, branch };
};

const collectStylingFindings = (value) => {
  const findings = [];
  const visit = (node, key = "") => {
    if (!node || typeof node !== "object") return;
    if (Array.isArray(node)) {
      if (key === "styling_findings") {
        findings.push(...node.filter((item) => item && typeof item === "object"));
      }
      for (const item of node) visit(item);
      return;
    }
    for (const [childKey, child] of Object.entries(node)) visit(child, childKey);
  };
  visit(value);
  return findings;
};

const groupFindings = (findings) => {
  const map = new Map();
  for (const finding of findings) {
    const key = [
      finding.code || "unknown",
      finding.sub_kind || "unknown",
      finding.confidence || "unknown",
    ].join("|");
    const current = map.get(key) || {
      code: finding.code || "unknown",
      sub_kind: finding.sub_kind || "unknown",
      confidence: finding.confidence || "unknown",
      count: 0,
      examples: [],
    };
    current.count += 1;
    if (current.examples.length < 5) {
      current.examples.push({
        path: finding.path || "",
        line: finding.line || null,
        value: finding.value || "",
        disposition: finding.agent_disposition || "",
      });
    }
    map.set(key, current);
  }
  return [...map.values()].toSorted((a, b) => b.count - a.count || a.code.localeCompare(b.code));
};

const runAudit = (dir, pr, opts) => {
  const proc = run(
    opts.fallowBin,
    ["audit", "--root", dir, "--base", `origin/${pr.baseRefName}`, "--format", "json", "--quiet"],
    { cwd: REPO_ROOT, timeout: opts.timeoutMs, env: { ...process.env, FALLOW_QUIET: "1" } },
  );
  let parsed = null;
  let parseError = null;
  try {
    parsed = JSON.parse(proc.stdout || "{}");
  } catch (error) {
    parseError = error.message;
  }
  const findings = parsed ? collectStylingFindings(parsed) : [];
  return {
    status: proc.status,
    signal: proc.signal || null,
    timed_out: Boolean(proc.error && proc.error.code === "ETIMEDOUT"),
    spawn_error: proc.error?.message || null,
    parse_error: parseError,
    stderr_sample: (proc.stderr || "").trim().slice(0, 2000),
    stdout_error: parsed?.error ? parsed.message || "" : "",
    verdict: parsed?.verdict || null,
    summary: parsed?.summary || null,
    styling_findings: findings.length,
    groups: groupFindings(findings),
  };
};

const runSelectedPrs = (selected, opts) => {
  const results = [];
  for (const pr of selected) {
    console.error(`== ${pr.repo}#${pr.number} ==`);
    const clone = ensureClone(pr.repo, pr.baseRefName, opts);
    if (!clone.ok) {
      results.push({ ...pr, ok: false, stage: "clone", error: clone.error });
      continue;
    }
    const checkout = checkoutPr(clone.dir, pr, opts);
    if (!checkout.ok) {
      results.push({ ...pr, ok: false, stage: "checkout", error: checkout.error });
      continue;
    }
    const audit = runAudit(clone.dir, pr, opts);
    results.push({
      ...pr,
      ok: audit.parse_error === null && !audit.stdout_error,
      stage: "audit",
      repo_dir: clone.dir,
      audit,
    });
  }
  return results;
};

const writeReport = (results, opts) => {
  const report = {
    generated_at: new Date().toISOString(),
    fallow_bin: opts.fallowBin,
    count: results.length,
    valid_audits: results.filter((result) => result.ok).length,
    repos_covered: new Set(results.map((result) => result.repo)).size,
    results,
  };
  writeFileSync(join(opts.outDir, "pr-smoke-results.json"), `${JSON.stringify(report, null, 2)}\n`);
  writeFileSync(join(opts.outDir, "pr-smoke-report.md"), `${renderMarkdownReport(report)}\n`);
  return report;
};

const renderMarkdownReport = (report) => {
  const lines = [
    "# Styling PR Smoke",
    "",
    `Generated: ${report.generated_at}`,
    `Fallow: \`${report.fallow_bin}\``,
    "",
    "## Summary",
    "",
    `- Selected PRs: ${report.count}`,
    `- Valid audit JSON results: ${report.valid_audits}`,
    `- Repos covered: ${report.repos_covered}`,
    "",
    "## Results",
    "",
    "| Repo PR | State | Audit | Styling findings | Top styling groups |",
    "| --- | --- | ---: | ---: | --- |",
  ];

  for (const result of report.results) {
    const href = `[${result.repo}#${result.number}](${result.url})`;
    if (!result.ok) {
      const reason = String(
        result.error || result.audit?.stdout_error || result.audit?.parse_error || "",
      )
        .replace(/\n/g, " ")
        .slice(0, 180);
      lines.push(`| ${href} | ${result.state} | ${result.stage}: failed | - | ${reason} |`);
      continue;
    }
    const groups = result.audit.groups
      .slice(0, 3)
      .map((group) => `${group.code}/${group.sub_kind}/${group.confidence}: ${group.count}`)
      .join("<br>");
    lines.push(
      `| ${href} | ${result.state} | ${result.audit.verdict || result.audit.status} | ${result.audit.styling_findings} | ${groups} |`,
    );
  }

  lines.push("", "## Review Notes", "");
  lines.push(
    "- Treat high-confidence structural styling groups as action candidates only when the examples point at changed files.",
  );
  lines.push(
    "- Treat low-confidence `raw-style-value`, dead-surface, and semantic token-drift groups as review-first signals.",
  );
  lines.push(
    "- A zero-styling result on a frontend PR is useful: it shows default audit added no styling noise for that change.",
  );
  return lines.join("\n");
};

const main = () => {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    console.log(usage());
    return;
  }
  if (opts.list) {
    console.log(JSON.stringify(CANDIDATES, null, 2));
    return;
  }
  mkdirSync(opts.outDir, { recursive: true });
  mkdirSync(opts.cacheDir, { recursive: true });

  let selection = null;
  if (opts.runOnly) {
    selection = JSON.parse(readFileSync(join(opts.outDir, "selected-prs.json"), "utf8"));
  } else {
    selection = selectPrs(opts);
    writeFileSync(
      join(opts.outDir, "selected-prs.json"),
      `${JSON.stringify(selection, null, 2)}\n`,
    );
  }

  if (opts.selectOnly) {
    console.log(
      JSON.stringify(
        {
          selected: selection.selected.length,
          repos: new Set(selection.selected.map((pr) => pr.repo)).size,
        },
        null,
        2,
      ),
    );
    return;
  }

  const results = runSelectedPrs(selection.selected, opts);
  const report = writeReport(results, opts);
  console.log(JSON.stringify({ count: report.count, valid_audits: report.valid_audits }, null, 2));
};

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
