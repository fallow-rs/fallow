// Reports GitHub Actions timings for recent completed workflow runs, so CI
// changes can be measured. It reads the runs and their jobs with `gh api`.
//
// Per workflow: run queue time (run created to first job start), run time
// (first job start to last job end), wall time (run created to last job end),
// and the number of jobs that used a runner per pull request run.
// Per job: queue time (job created to job start) and run time.
// Each metric prints p50 and p90 in minutes.
//
// Usage: node scripts/ci-metrics.mjs [--limit 20] [--workflow ci.yml]
//        [--event pull_request] [--repo owner/name] [--json]

import { execFile } from "node:child_process";
import { realpathSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);

const DEFAULT_LIMIT = 20;
const USAGE = `Usage: node scripts/ci-metrics.mjs [--limit 20] [--workflow ci.yml]
       [--event pull_request] [--repo owner/name] [--json]`;
const MAX_PER_PAGE = 100;
const PARALLEL_REQUESTS = 8;
const MS_PER_MINUTE = 60_000;

const gh = async (args) => {
  const { stdout } = await run("gh", args, { maxBuffer: 64 * 1024 * 1024 });
  return stdout;
};

const ghJson = async (path) => JSON.parse(await gh(["api", path]));

export const parseArguments = (argv) => {
  const options = {
    limit: DEFAULT_LIMIT,
    workflow: undefined,
    event: undefined,
    repo: undefined,
    json: false,
    help: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === "--json") {
      options.json = true;
      continue;
    }
    if (flag === "--help" || flag === "-h") {
      options.help = true;
      continue;
    }
    const value = argv[index + 1];
    index += 1;
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === "--limit") options.limit = Number(value);
    else if (flag === "--workflow") options.workflow = value;
    else if (flag === "--event") options.event = value;
    else if (flag === "--repo") options.repo = value;
    else throw new Error(`unknown argument: ${flag}`);
  }
  if (!Number.isInteger(options.limit) || options.limit < 1)
    throw new Error("--limit must be a positive integer");
  return options;
};

/** Returns the value at quantile `q` (0..1) with the nearest-rank method, or null for no values. */
export const quantile = (values, q) => {
  if (values.length === 0) return null;
  const sorted = values.toSorted((a, b) => a - b);
  const rank = Math.max(1, Math.ceil(q * sorted.length));
  return sorted[rank - 1];
};

const minutes = (from, to) => (Date.parse(to) - Date.parse(from)) / MS_PER_MINUTE;

/** Jobs that used a runner. Skipped jobs, and jobs cancelled in the queue, never get one. */
const ranOnRunner = (job) =>
  Boolean(job.runner_name) && job.started_at && job.completed_at && job.conclusion !== "skipped";

/** Computes the timings of one run from its jobs. Returns null when no job used a runner. */
export const runTimings = (workflowRun, jobs) => {
  const used = jobs.filter(ranOnRunner);
  if (used.length === 0) return null;
  const firstStart = used.map((job) => job.started_at).toSorted()[0];
  const lastEnd = used
    .map((job) => job.completed_at)
    .toSorted()
    .at(-1);
  // A re-run keeps `created_at` of the first attempt. `run_started_at` is the
  // start of the latest attempt, which is the attempt the jobs belong to.
  const created = workflowRun.run_started_at ?? workflowRun.created_at;
  return {
    queue: minutes(created, firstStart),
    run: minutes(firstStart, lastEnd),
    wall: minutes(created, lastEnd),
    jobs: used.length,
    jobTimings: used.map((job) => ({
      name: job.name,
      queue: minutes(job.created_at, job.started_at),
      run: minutes(job.started_at, job.completed_at),
    })),
  };
};

const stats = (values) => ({
  n: values.length,
  p50: quantile(values, 0.5),
  p90: quantile(values, 0.9),
});

/** Groups run timings into per-workflow and per-job statistics. */
export const summarize = (entries) => {
  const workflows = new Map();
  for (const { workflowRun, timings } of entries) {
    if (timings === null) continue;
    const key = workflowRun.name;
    if (!workflows.has(key)) workflows.set(key, { runs: [], jobs: new Map() });
    const bucket = workflows.get(key);
    bucket.runs.push({ event: workflowRun.event, ...timings });
    for (const job of timings.jobTimings) {
      if (!bucket.jobs.has(job.name)) bucket.jobs.set(job.name, []);
      bucket.jobs.get(job.name).push(job);
    }
  }
  return [...workflows].map(([name, bucket]) => {
    const prRuns = bucket.runs.filter((entry) => entry.event === "pull_request");
    return {
      workflow: name,
      runs: bucket.runs.length,
      queue: stats(bucket.runs.map((entry) => entry.queue)),
      run: stats(bucket.runs.map((entry) => entry.run)),
      wall: stats(bucket.runs.map((entry) => entry.wall)),
      jobsPerPrRun: stats(prRuns.map((entry) => entry.jobs)),
      jobs: [...bucket.jobs]
        .map(([job, values]) => ({
          job,
          queue: stats(values.map((value) => value.queue)),
          run: stats(values.map((value) => value.run)),
        }))
        .toSorted((a, b) => (b.run.p90 ?? 0) - (a.run.p90 ?? 0)),
    };
  });
};

const mapLimited = async (items, limit, worker) => {
  const results = Array.from({ length: items.length });
  let next = 0;
  const lanes = Array.from({ length: Math.min(limit, items.length) }, async () => {
    while (next < items.length) {
      const index = next;
      next += 1;
      results[index] = await worker(items[index]);
    }
  });
  await Promise.all(lanes);
  return results;
};

const fetchRuns = async ({ repo, workflow, event, limit }) => {
  const base = workflow
    ? `repos/${repo}/actions/workflows/${workflow}/runs`
    : `repos/${repo}/actions/runs`;
  const runs = [];
  for (let page = 1; runs.length < limit; page += 1) {
    const query = new URLSearchParams({
      status: "completed",
      per_page: String(MAX_PER_PAGE),
      page: String(page),
    });
    if (event) query.set("event", event);
    const body = await ghJson(`${base}?${query}`);
    // A cancelled run (for example a PR run replaced by a new push) has no
    // complete timings, so it does not count toward the limit.
    runs.push(
      ...body.workflow_runs.filter((workflowRun) => workflowRun.conclusion !== "cancelled"),
    );
    if (body.workflow_runs.length < MAX_PER_PAGE) break;
  }
  return runs.slice(0, limit);
};

const fetchJobs = async (repo, workflowRun) => {
  const body = await ghJson(
    `repos/${repo}/actions/runs/${workflowRun.id}/jobs?filter=latest&per_page=${MAX_PER_PAGE}`,
  );
  return body.jobs;
};

const format = (value) => (value === null ? "-" : value.toFixed(1));
const pair = (stat) => `${format(stat.p50)}/${format(stat.p90)}`;

const printText = (summary, options) => {
  const lines = [
    `Last ${options.limit} completed runs in ${options.repo}` +
      `${options.workflow ? `, workflow ${options.workflow}` : ""}${options.event ? `, event ${options.event}` : ""}.`,
    "Times in minutes as p50/p90. Queue = created to first start. Wall = created to last end.",
    "",
  ];
  for (const entry of summary.toSorted((a, b) => (b.wall.p90 ?? 0) - (a.wall.p90 ?? 0))) {
    lines.push(
      `${entry.workflow} (${entry.runs} runs): queue ${pair(entry.queue)}, run ${pair(entry.run)}, ` +
        `wall ${pair(entry.wall)}, jobs per PR run ${pair(entry.jobsPerPrRun)}`,
    );
    for (const job of entry.jobs) {
      lines.push(`    ${job.job} (${job.run.n}): queue ${pair(job.queue)}, run ${pair(job.run)}`);
    }
  }
  console.log(lines.join("\n"));
};

const main = async (options) => {
  const repo =
    options.repo ??
    (await gh(["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])).trim();
  const resolved = { ...options, repo };
  const runs = await fetchRuns(resolved);
  const entries = await mapLimited(runs, PARALLEL_REQUESTS, async (workflowRun) => ({
    workflowRun,
    timings: runTimings(workflowRun, await fetchJobs(repo, workflowRun)),
  }));
  const summary = summarize(entries);
  if (options.json)
    console.log(JSON.stringify({ repo, limit: options.limit, workflows: summary }, null, 2));
  else printText(summary, resolved);
};

// `process.argv[1]` keeps symlinks while Node realpaths the ESM entry, so
// compare real paths. An unresolvable value means this module was imported.
const realPathOrNull = (value) => {
  try {
    return value === undefined ? null : realpathSync(value);
  } catch {
    return null;
  }
};

if (realPathOrNull(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const options = parseArguments(process.argv.slice(2));
    if (options.help) console.log(USAGE);
    else await main(options);
  } catch (error) {
    console.error(`ci-metrics: ${error.message}`);
    process.exitCode = 1;
  }
}
