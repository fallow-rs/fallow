import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";

const PS_MAX_BUFFER = 8 * 1024 * 1024;
const MS_PER_SECOND = 1000;
const SECONDS_PER_MINUTE = 60;
const SECONDS_PER_HOUR = 3600;
const SECONDS_PER_DAY = 86_400;

/**
 * Parses a `ps` CPU time (`[DD-][HH:]MM:SS[.ss]`, for example `836:35.10`
 * on macOS) to milliseconds, or null.
 */
export const parsePsCpuTime = (value) => {
  const match = /^(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)$/u.exec(String(value).trim());
  if (!match) return null;
  const [, days = "0", hours = "0", minutes, seconds] = match;
  const total =
    Number(days) * SECONDS_PER_DAY +
    Number(hours) * SECONDS_PER_HOUR +
    Number(minutes) * SECONDS_PER_MINUTE +
    Number(seconds);
  return Math.round(total * MS_PER_SECOND);
};

/** The pids of `rootPid` and its descendants in `rows` of `[pid, ppid]`. */
export const processTreePids = (rootPid, rows) => {
  const children = new Map();
  rows.forEach(([pid, ppid]) => {
    const siblings = children.get(ppid) ?? [];
    siblings.push(pid);
    children.set(ppid, siblings);
  });
  const tree = new Set();
  const pending = [rootPid];
  while (pending.length > 0) {
    const pid = pending.pop();
    if (tree.has(pid)) continue;
    tree.add(pid);
    pending.push(...(children.get(pid) ?? []));
  }
  return tree;
};

const psRows = (fields) => {
  const result = spawnSync("ps", ["-axo", fields], { encoding: "utf8", maxBuffer: PS_MAX_BUFFER });
  if (result.status !== 0) return null;
  return result.stdout
    .trim()
    .split("\n")
    .map((line) => line.trim().split(/\s+/u));
};

// Linux `ps` prints CPU time in whole seconds, so the kernel counters give the
// resolution that a run of a few seconds needs.
let clockTicksPerSecond = null;
const clockTicks = () => {
  if (clockTicksPerSecond === null) {
    const result = spawnSync("getconf", ["CLK_TCK"], { encoding: "utf8" });
    const ticks = Number(result.stdout?.trim());
    clockTicksPerSecond = Number.isFinite(ticks) && ticks > 0 ? ticks : 100;
  }
  return clockTicksPerSecond;
};
const linuxCpuMs = (pid) => {
  try {
    const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
    const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
    const ticks = Number(fields[11]) + Number(fields[12]);
    return Number.isFinite(ticks) ? (ticks * MS_PER_SECOND) / clockTicks() : null;
  } catch {
    return null;
  }
};

/**
 * One sample of the process tree of `rootPid`: its total RSS in KiB and the
 * CPU time of each process in milliseconds. Returns null where the platform
 * gives no such sample.
 */
const sampleProcessTree = (rootPid, platform = process.platform) => {
  if (platform === "win32") return null;
  const rows = psRows(platform === "darwin" ? "pid=,ppid=,rss=,time=" : "pid=,ppid=,rss=");
  if (rows === null) return null;
  const parsed = rows
    .map(([pid, ppid, rss, time]) => [Number(pid), Number(ppid), Number(rss), time])
    .filter(([pid, ppid, rss]) => [pid, ppid, rss].every(Number.isFinite));
  const tree = processTreePids(rootPid, parsed);
  const members = parsed.filter(([pid]) => tree.has(pid));
  const cpuMsByPid = new Map(
    members.map(([pid, , , time]) => [
      pid,
      platform === "darwin" ? parsePsCpuTime(time) : linuxCpuMs(pid),
    ]),
  );
  return {
    rssKb: members.reduce((total, [, , rss]) => total + rss, 0),
    cpuMsByPid,
  };
};

/**
 * Keeps the highest CPU time seen for each process. A process that exits
 * keeps its last sample, so the sum is the CPU time of the tree up to the
 * last sample.
 */
export const recordCpuSample = (maxCpuMsByPid, sample) => {
  sample?.cpuMsByPid.forEach((cpuMs, pid) => {
    if (Number.isFinite(cpuMs))
      maxCpuMsByPid.set(pid, Math.max(maxCpuMsByPid.get(pid) ?? 0, cpuMs));
  });
};

/** The CPU time of the process tree in milliseconds, or null without samples. */
export const processTreeCpuMs = (maxCpuMsByPid) =>
  maxCpuMsByPid.size === 0 ? null : [...maxCpuMsByPid.values()].reduce((sum, ms) => sum + ms, 0);

const TIME_BINARY = "/usr/bin/time";
const TIME_REPORT_LINE = /^(real|user|sys)\s+(\d+(?:\.\d+)?)$/u;

/**
 * The command that reports the CPU time of `binary` and its waited child
 * processes: `/usr/bin/time -p` when it exists. Sampling loses the CPU time
 * after the last sample, because a run ends before its final sample.
 */
export const timedCommand = (binary, args, platform = process.platform) =>
  platform !== "win32" && existsSync(TIME_BINARY)
    ? { binary: TIME_BINARY, args: ["-p", binary, ...args], timed: true }
    : { binary, args, timed: false };

/**
 * Splits the `time -p` report from the end of `stderr`. Returns the stderr of
 * the timed command and its user plus system time in milliseconds, or null.
 */
export const splitTimeReport = (stderr) => {
  const lines = stderr.replace(/\n$/u, "").split("\n");
  const values = {};
  let start = lines.length;
  while (start > 0 && Object.keys(values).length < 3) {
    const match = TIME_REPORT_LINE.exec(lines[start - 1].trim());
    if (!match) break;
    values[match[1]] = Number(match[2]);
    start -= 1;
  }
  if (!["real", "user", "sys"].every((key) => Number.isFinite(values[key]))) {
    return { stderr, cpuMs: null };
  }
  const rest = lines.slice(0, start).join("\n");
  return {
    stderr: rest === "" ? "" : `${rest}\n`,
    cpuMs: Math.round((values.user + values.sys) * MS_PER_SECOND),
  };
};

/**
 * Starts `binary` through `timedCommand`. A timed command runs in its own
 * process group, so `kill` also stops the process that it times.
 */
export const spawnTimed = (binary, args, options) => {
  const command = timedCommand(binary, args);
  const child = spawn(command.binary, command.args, { ...options, detached: command.timed });
  const kill = () => (command.timed ? process.kill(-child.pid, "SIGKILL") : child.kill("SIGKILL"));
  return { child, kill };
};

/**
 * Samples the process tree of `pid` every `intervalMs`. `stop` returns the
 * peak RSS in KiB (or null) and the sampled CPU time in milliseconds (or null).
 */
export const startUsageMonitor = (pid, intervalMs) => {
  let peakRssKb = 0;
  const maxCpuMsByPid = new Map();
  const sample = () => {
    const usage = sampleProcessTree(pid);
    if (usage !== null) peakRssKb = Math.max(peakRssKb, usage.rssKb);
    recordCpuSample(maxCpuMsByPid, usage);
  };
  sample();
  const timer = setInterval(sample, intervalMs);
  return {
    stop: () => {
      clearInterval(timer);
      sample();
      return { peakRssKb: peakRssKb || null, sampledCpuMs: processTreeCpuMs(maxCpuMsByPid) };
    },
  };
};
