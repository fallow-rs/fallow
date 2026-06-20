"use strict";
const node_path = require("node:path");
const electron = require("electron");
const node_child_process = require("node:child_process");
const node_util = require("node:util");
const toScore = (s) => ({
  fanIo: s?.fan_io ?? 0,
  securityTaint: s?.security_taint ?? 0,
  riskZone: s?.risk_zone ?? 0,
  changeShape: s?.change_shape ?? 0,
  total: s?.total ?? 0
});
const buildCleared = (brief) => {
  const out = [];
  const dead = brief.summary?.dead_code_issues ?? 0;
  const dupes = brief.summary?.duplication_clone_groups ?? 0;
  const cx = brief.summary?.complexity_findings ?? 0;
  if (dead > 0) out.push({ kind: "dead-code", label: "dead-code findings", count: dead });
  if (dupes > 0) out.push({ kind: "duplication", label: "duplication clone groups", count: dupes });
  if (cx > 0) out.push({ kind: "complexity", label: "complexity findings", count: cx });
  return out;
};
const buildFocus = (brief) => {
  const changedFiles = brief.changed_files_count ?? brief.triage?.files ?? 0;
  const riskClass = brief.triage?.risk_class ?? "unknown";
  const verdict = brief.verdict ?? "unknown";
  return {
    verdict,
    changedFiles,
    baseRef: brief.base_ref ?? "",
    baseDescription: brief.base_description ?? "",
    riskClass,
    reviewEffort: brief.triage?.review_effort ?? "unknown",
    headline: `${changedFiles} changed files, ${riskClass} risk, verdict ${verdict}`
  };
};
const toWalkthroughDocument = (brief) => {
  const factByFile = /* @__PURE__ */ new Map();
  const addFact = (e, deprioritized) => {
    factByFile.set(e.file, {
      path: e.file,
      attention: e.score?.total ?? 0,
      label: e.label ?? (deprioritized ? "not-prioritized" : "review-here"),
      reason: e.reason ?? "",
      deprioritized,
      score: toScore(e.score)
    });
  };
  (brief.focus?.review_here ?? []).forEach((e) => addFact(e, false));
  (brief.focus?.deprioritized ?? []).forEach((e) => addFact(e, true));
  const fileFor = (path) => factByFile.get(path) ?? {
    path,
    attention: 0,
    label: "unscored",
    reason: "",
    deprioritized: false,
    score: { fanIo: 0, securityTaint: 0, riskZone: 0, changeShape: 0, total: 0 }
  };
  const order = brief.partition?.order ?? [];
  const orderIndex = (dir) => {
    const i = order.indexOf(dir);
    return i === -1 ? Number.MAX_SAFE_INTEGER : i;
  };
  const stages = (brief.partition?.units ?? []).map((unit) => ({ unit, idx: orderIndex(unit.module_dir) })).sort((a, b) => a.idx - b.idx).map(({ unit }, i) => ({
    moduleDir: unit.module_dir,
    order: i,
    files: (unit.files ?? []).map(fileFor)
  }));
  const decisions = (brief.decisions?.decisions ?? []).filter((d) => typeof d["signal_id"] === "string" && d["signal_id"].length > 0).map((d) => ({
    signalId: d["signal_id"],
    question: typeof d["question"] === "string" ? d["question"] : "",
    raw: d
  }));
  return {
    schemaVersion: brief.schema_version ?? 0,
    focus: buildFocus(brief),
    stages,
    decisions,
    cleared: buildCleared(brief),
    coordinationGaps: brief.impact_closure?.coordination_gap ?? [],
    weakening: brief.weakening ?? [],
    graphSnapshotHash: brief.graph_snapshot_hash ?? null
  };
};
const run = node_util.promisify(node_child_process.execFile);
const fallowBin = () => process.env["FALLOW_BIN"] ?? "fallow";
const runReview = async (root) => {
  const { stdout } = await run(fallowBin(), ["review", "--format", "json"], {
    cwd: root ?? process.cwd(),
    maxBuffer: 64 * 1024 * 1024
  });
  return toWalkthroughDocument(JSON.parse(stdout));
};
const createWindow = () => {
  const win = new electron.BrowserWindow({
    width: 1400,
    height: 900,
    title: "Fallow Review",
    backgroundColor: "#0e0c0a",
    webPreferences: {
      preload: node_path.join(__dirname, "../preload/index.js"),
      sandbox: false
    }
  });
  const devUrl = process.env["ELECTRON_RENDERER_URL"];
  if (devUrl) {
    void win.loadURL(devUrl);
  } else {
    void win.loadFile(node_path.join(__dirname, "../renderer/index.html"));
  }
};
electron.ipcMain.handle("review:get", (_event, root) => runReview(root));
void electron.app.whenReady().then(() => {
  createWindow();
  electron.app.on("activate", () => {
    if (electron.BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});
electron.app.on("window-all-closed", () => {
  if (process.platform !== "darwin") electron.app.quit();
});
