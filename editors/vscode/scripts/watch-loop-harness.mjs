#!/usr/bin/env node
// Drives a real VS Code with the Fallow extension, saves `.fallowrc.json`
// once, and counts the LSP traffic during an idle window after the save. A
// save must start one analysis run, not a loop of runs (#5278).
//
// Usage:
//   node scripts/watch-loop-harness.mjs --bin <dir with fallow and fallow-lsp> \
//     --out <dir> [--label run] [--workspace <project>] [--extension <dir>]
//     [--idle 60] [--touch <ms>] [--edit gui|fs] [--source wire|trace]
//     [--vscode-version stable] [--vscode-cache <dir>]
//     [--settings '<json merged into the user settings>'] [--run-analysis] [--fix]
//     [--max-runs <n>] [--max-cancels <n>] [--max-error-lines <n>]
//
// The harness copies the workspace, so the run does not change the project.
// --touch rewrites the config with the same bytes at that interval, like a
// virus scanner or a sync client that touches a file without a change.
// --source wire (default on macOS and Linux) starts the LSP through a bash
// wrapper that copies both stdio directions to files. --source trace (default
// on Windows) reads the `fallow.trace.server` lines from the Output channel log.
// The process exits with 1 when a count is over its --max-* limit.
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { downloadAndUnzipVSCode } from "@vscode/test-electron";
import { _electron } from "playwright-core";

import {
  countErrorLines,
  countKind,
  eventsAfter,
  eventsFromTrace,
  eventsFromWire,
  limitFailures,
} from "./watch-loop-events.mjs";

const extensionRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const isWindows = process.platform === "win32";
const { values: opts } = parseArgs({
  options: {
    label: { type: "string", default: "run" },
    bin: { type: "string" },
    workspace: {
      type: "string",
      default: path.join(extensionRoot, "test/integration/fixtures/real-workspace"),
    },
    extension: { type: "string", default: extensionRoot },
    out: { type: "string" },
    idle: { type: "string", default: "60" },
    "vscode-version": { type: "string", default: "stable" },
    "vscode-cache": {
      type: "string",
      default: path.join(os.tmpdir(), "fallow-vscode-test-cache"),
    },
    "first-timeout": { type: "string", default: "300" },
    edit: { type: "string", default: "gui" },
    source: { type: "string", default: isWindows ? "trace" : "wire" },
    settings: { type: "string", default: "{}" },
    "run-analysis": { type: "boolean", default: false },
    fix: { type: "boolean", default: false },
    touch: { type: "string", default: "0" },
    "max-runs": { type: "string" },
    "max-cancels": { type: "string" },
    "max-error-lines": { type: "string" },
  },
});
for (const key of ["bin", "out"]) {
  if (!opts[key]) throw new Error(`--${key} is required`);
}
const optionalNumber = (value) => (value === undefined ? undefined : Number(value));

const outDir = path.resolve(opts.out, opts.label);
fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });
const workspace = path.join(outDir, "workspace");
fs.cpSync(path.resolve(opts.workspace), workspace, {
  recursive: true,
  filter: (source) => !/[\\/](?:node_modules|\.fallow)$/.test(source),
});
const configFile = path.join(workspace, ".fallowrc.json");
if (!fs.existsSync(configFile)) fs.writeFileSync(configFile, "{}\n");
const idleMs = Number(opts.idle) * 1000;
const mod = process.platform === "darwin" ? "Meta" : "Control";
const exe = isWindows ? ".exe" : "";
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, Math.max(0, ms)));
const log = (message) => {
  const line = `[${new Date().toISOString()}] ${message}`;
  console.log(line);
  fs.appendFileSync(path.join(outDir, "harness.log"), `${line}\n`);
};

// --- Binaries ------------------------------------------------------------------
const binDir = path.join(outDir, "bin");
fs.mkdirSync(binDir);
const realLsp = path.resolve(opts.bin, `fallow-lsp${exe}`);
fs.copyFileSync(path.resolve(opts.bin, `fallow${exe}`), path.join(binDir, `fallow${exe}`));
const wireDir = path.join(outDir, "wire");
fs.mkdirSync(wireDir);
if (opts.source === "wire") {
  // The extension runs `--version` to check the binary. Only the server
  // process goes through the copies of the two stream directions.
  fs.writeFileSync(
    path.join(binDir, "fallow-lsp"),
    `#!/bin/bash
case "$1" in --version|-V|--help) exec "${realLsp}" "$@";; esac
exec 2> >(tee -a "${wireDir}/stderr.log" >&2)
tee -a "${wireDir}/client-to-server.log" | "${realLsp}" "$@" | tee -a "${wireDir}/server-to-client.log"
`,
    { mode: 0o755 },
  );
} else {
  fs.copyFileSync(realLsp, path.join(binDir, `fallow-lsp${exe}`));
}

// --- Event sources -------------------------------------------------------------
const userDataDir = path.join(outDir, "user-data");
const findOutputLogs = () => {
  const found = [];
  const walk = (dir) => {
    if (!fs.existsSync(dir)) return;
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith("Fallow.log")) found.push(full);
    }
  };
  walk(path.join(userDataDir, "logs"));
  return found;
};
const readOrEmpty = (file) => (fs.existsSync(file) ? fs.readFileSync(file) : Buffer.alloc(0));
const readEvents = () =>
  opts.source === "wire"
    ? eventsFromWire(
        readOrEmpty(path.join(wireDir, "client-to-server.log")),
        readOrEmpty(path.join(wireDir, "server-to-client.log")),
      )
    : findOutputLogs().flatMap((file) => eventsFromTrace(fs.readFileSync(file, "utf8")));

// --- VS Code -------------------------------------------------------------------
const vscodeExecutable = await downloadAndUnzipVSCode({
  cachePath: path.resolve(opts["vscode-cache"]),
  version: opts["vscode-version"],
});
fs.mkdirSync(path.join(userDataDir, "User"), { recursive: true });
fs.writeFileSync(
  path.join(userDataDir, "User", "settings.json"),
  JSON.stringify(
    {
      "fallow.lspPath": path.join(binDir, `fallow-lsp${exe}`),
      "fallow.autoDownload": false,
      "fallow.trace.server": "verbose",
      "security.workspace.trust.enabled": false,
      "workbench.startupEditor": "none",
      "extensions.autoUpdate": false,
      "extensions.autoCheckUpdates": false,
      "update.mode": "none",
      "telemetry.telemetryLevel": "off",
      "git.enabled": false,
      "window.restoreWindows": "none",
      "window.dialogStyle": "custom",
      ...JSON.parse(opts.settings),
    },
    null,
    2,
  ),
);

log(`VS Code ${vscodeExecutable}`);
const app = await _electron.launch({
  executablePath: vscodeExecutable,
  args: [
    workspace,
    `--extensionDevelopmentPath=${path.resolve(opts.extension)}`,
    `--user-data-dir=${userDataDir}`,
    `--extensions-dir=${path.join(outDir, "extensions")}`,
    "--disable-workspace-trust",
    "--skip-welcome",
    "--skip-release-notes",
    "--disable-telemetry",
    "--new-window",
    ...(opts.source === "trace" ? ["--log", "trace"] : []),
  ],
  env: { ...process.env, VSCODE_SKIP_PRELAUNCH: "1" },
  timeout: 180_000,
});
const page = await app.firstWindow();
await page.waitForSelector(".monaco-workbench", { timeout: 180_000 });
log("workbench ready");

const readUi = async () =>
  page.evaluate(() => {
    const text = (el) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
    const status = [...document.querySelectorAll(".statusbar-item")]
      .filter((el) => /fallow/i.test(el.id))
      .map((el) => ({ id: el.id, text: text(el), aria: el.getAttribute("aria-label") }));
    const toasts = [...document.querySelectorAll(".notification-list-item-message")]
      .map((el) => text(el))
      .filter(Boolean);
    const rows = [...document.querySelectorAll(".pane-body .monaco-list-row")]
      .map((el) => el.getAttribute("aria-label") ?? text(el))
      .filter((value) => value && !/^\s*$/.test(value))
      .slice(0, 60);
    return { status, toasts, rows };
  });

// Sample the status bar and the notifications once per second, so the report
// shows each value that a user could see and for how long.
const timeline = [];
const sessionStart = Date.now();
const sampling = new AbortController();
const sampler = (async () => {
  while (!sampling.signal.aborted) {
    try {
      const ui = await readUi();
      const entry = {
        s: Math.round((Date.now() - sessionStart) / 1000),
        status: ui.status.map((item) => item.text).join(" | "),
        toasts: ui.toasts,
      };
      const prev = timeline.at(-1);
      if (
        !prev ||
        prev.status !== entry.status ||
        JSON.stringify(prev.toasts) !== JSON.stringify(entry.toasts)
      ) {
        timeline.push(entry);
      }
    } catch {
      // The window can be busy or closing.
    }
    await sleep(1000);
  }
})();

const runCommand = async (title) => {
  await page.keyboard.press("F1");
  await page.waitForSelector(".quick-input-widget input", { timeout: 10_000 });
  await page.keyboard.type(title, { delay: 10 });
  await sleep(700);
  await page.keyboard.press("Enter");
  await sleep(800);
};

const shot = async (name) => {
  const file = path.join(outDir, `${name}.png`);
  await page.screenshot({ path: file });
  log(`screenshot ${file}`);
};

const waitQuiet = async (deadline, quietSeconds = 8) => {
  let last = -1;
  for (let quiet = 0; quiet < quietSeconds && Date.now() < deadline;) {
    const n = readEvents().length;
    quiet = n === last ? quiet + 1 : 0;
    last = n;
    await sleep(1000);
  }
};

// --- First analysis --------------------------------------------------------------
const firstDeadline = Date.now() + Number(opts["first-timeout"]) * 1000;
while (Date.now() < firstDeadline && countKind(readEvents(), "complete") === 0) await sleep(1000);
await waitQuiet(firstDeadline + 60_000);
log(`first analysis settled: ${JSON.stringify({ complete: countKind(readEvents(), "complete") })}`);
await runCommand("Fallow: Open Sidebar");
await sleep(1500);
if (opts["run-analysis"]) {
  await runCommand("Fallow: Run Analysis");
  await waitQuiet(Date.now() + 180_000, 10);
  await sleep(1500);
}
if (opts.fix) {
  await runCommand("Fallow: Auto-Fix");
  const toast = page.locator(".notification-toast", { hasText: "Apply these fixes?" });
  await toast.waitFor({ timeout: 20_000 });
  await toast.getByRole("button", { name: "Yes" }).click();
  log("fix confirmed");
  await waitQuiet(Date.now() + 240_000, 15);
  await sleep(1500);
  log(`after fix: ${JSON.stringify(await readUi())}`);
  await shot("00-after-fix");
}
const uiBefore = await readUi();
fs.writeFileSync(path.join(outDir, "ui-before-edit.json"), JSON.stringify(uiBefore, null, 2));
await shot("01-before-edit");

// --- Save the config once ------------------------------------------------------
const sinceSave = eventsAfter(readEvents());
const editStart = Date.now();
if (opts.edit === "fs") {
  fs.appendFileSync(configFile, "\n");
} else {
  await page.keyboard.press(`${mod}+P`);
  await page.waitForSelector(".quick-input-widget input", { timeout: 10_000 });
  await page.keyboard.type(".fallowrc.json", { delay: 20 });
  await sleep(1500);
  await page.keyboard.press("Enter");
  await sleep(1500);
  await page.keyboard.press(`${mod}+End`);
  await page.keyboard.press("Enter");
  await sleep(300);
  await page.keyboard.press(`${mod}+S`);
}
log(`config saved via ${opts.edit}`);

// --- Idle window -----------------------------------------------------------------
const touchMs = Number(opts.touch);
const toucher =
  touchMs > 0
    ? setInterval(() => fs.writeFileSync(configFile, fs.readFileSync(configFile)), touchMs)
    : null;
const buckets = [];
const bucketMs = 10_000;
for (let t = bucketMs; t <= idleMs; t += bucketMs) {
  await sleep(editStart + t - Date.now());
  const after = sinceSave(readEvents());
  buckets.push({
    secondsAfterSave: t / 1000,
    didChangeWatchedFiles: countKind(after, "watched"),
    didSave: countKind(after, "didSave"),
    analysisRuns: countKind(after, "running"),
    analysisComplete: countKind(after, "complete"),
    cancelled: countKind(after, "cancelled"),
  });
  log(`t+${t / 1000}s ${JSON.stringify(buckets.at(-1))}`);
}
if (toucher) clearInterval(toucher);

const after = sinceSave(readEvents());
const uriCounts = {};
for (const event of after.filter((e) => e.kind === "watched")) {
  for (const uri of event.uris) {
    const key = decodeURIComponent(uri).replace(/file:.*?\/workspace\//i, "<ws>/");
    uriCounts[key] = (uriCounts[key] ?? 0) + 1;
  }
}
const totals = readEvents()
  .filter((e) => e.kind === "complete")
  .map((e) => e.total);

await runCommand("Fallow: Show Output Channel");
await sleep(2000);
const uiAfter = await readUi();
await shot("02-after-idle-output");
await page.keyboard.press("Escape");

sampling.abort();
await sampler;
const outputLogs = findOutputLogs();
const errorLines = outputLogs.reduce(
  (sum, file) => sum + countErrorLines(fs.readFileSync(file, "utf8")),
  0,
);
const afterSave = { ...buckets.at(-1), errorLines };
const failures = limitFailures(
  {
    analysisRuns: countKind(after, "running"),
    cancelled: countKind(after, "cancelled"),
    errorLines,
  },
  {
    maxRuns: optionalNumber(opts["max-runs"]),
    maxCancels: optionalNumber(opts["max-cancels"]),
    maxErrorLines: optionalNumber(opts["max-error-lines"]),
  },
);
const summary = {
  label: opts.label,
  platform: process.platform,
  source: opts.source,
  edit: opts.edit,
  touchMs,
  idleSeconds: idleMs / 1000,
  afterSave,
  failures,
  buckets,
  watchedEventUris: uriCounts,
  cancelledTexts: after
    .filter((e) => e.kind === "cancelled")
    .map((e) => e.text)
    .slice(0, 5),
  analysisCompleteTotals: totals,
  uiBefore,
  uiAfter,
  timeline,
};
fs.writeFileSync(path.join(outDir, "summary.json"), JSON.stringify(summary, null, 2));

outputLogs.forEach((file, index) =>
  fs.copyFileSync(file, path.join(outDir, `output-${index}-Fallow.log`)),
);
await app.close().catch(() => {});
for (const dir of [userDataDir, path.join(outDir, "extensions"), workspace]) {
  fs.rmSync(dir, { recursive: true, force: true });
}
console.log(JSON.stringify({ afterSave, failures, watchedEventUris: uriCounts, totals }, null, 2));
if (process.env["GITHUB_STEP_SUMMARY"]) {
  const verdict = failures.length > 0 ? `FAIL ${failures.join("; ")}` : "pass";
  fs.appendFileSync(
    process.env["GITHUB_STEP_SUMMARY"],
    `- ${opts.label} on ${process.platform}: ${verdict}. ${JSON.stringify(afterSave)}\n`,
  );
}
if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  process.exit(1);
}
