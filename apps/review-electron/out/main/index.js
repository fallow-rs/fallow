"use strict";
const node_path = require("node:path");
const electron = require("electron");
const node_child_process = require("node:child_process");
const promises = require("node:fs/promises");
const node_os = require("node:os");
const node_util = require("node:util");
const node_http = require("node:http");
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
const at = (root) => root ?? process.cwd();
const MAX_BUFFER = 64 * 1024 * 1024;
const runReview = async (root) => {
  const { stdout } = await run(fallowBin(), ["review", "--format", "json"], {
    cwd: at(root),
    maxBuffer: MAX_BUFFER
  });
  return toWalkthroughDocument(JSON.parse(stdout));
};
const runGuide = async (root) => {
  const { stdout } = await run(fallowBin(), ["review", "--walkthrough-guide", "--format", "json"], {
    cwd: at(root),
    maxBuffer: MAX_BUFFER
  });
  const g = JSON.parse(stdout);
  return {
    graphSnapshotHash: g.graph_snapshot_hash ?? "",
    emittedSignalIds: g.digest?.decisions?.emitted_signal_ids ?? [],
    order: g.direction?.order ?? []
  };
};
const validateWalkthrough = async (payload, root) => {
  const file = node_path.join(node_os.tmpdir(), `fallow-agent-wt-${process.pid}-${Date.now()}.json`);
  await promises.writeFile(file, JSON.stringify(payload), "utf8");
  const { stdout } = await run(fallowBin(), ["review", "--walkthrough-file", file, "--format", "json"], {
    cwd: at(root),
    maxBuffer: MAX_BUFFER
  });
  return JSON.parse(stdout);
};
const feedPath = (root) => node_path.join(root, ".fallow-review", "feed.jsonl");
const appendFeedItem = async (root, item) => {
  const path = feedPath(root);
  await promises.mkdir(node_path.dirname(path), { recursive: true });
  await promises.appendFile(path, `${JSON.stringify(item)}
`, "utf8");
};
const buildAgentWalkthrough = (graphSnapshotHash, items) => ({
  graph_snapshot_hash: graphSnapshotHash,
  judgments: items.filter((i) => i.target.kind === "signal_id" && i.target.value.length > 0).map((i) => {
    const judgment = { signal_id: i.target.value, framing: i.note };
    if (i.verdict) judgment.concern = i.verdict;
    return judgment;
  })
});
const decodePngDataUrl = (dataUrl) => {
  const match = /^data:image\/png;base64,(.+)$/s.exec(dataUrl);
  const b64 = match?.[1];
  if (b64 === void 0) throw new Error("expected a base64 png data url");
  return Buffer.from(b64, "base64");
};
const shotPath = (root, at2) => node_path.join(root, ".fallow-review", "shots", `shot-${at2}.png`);
const saveAnnotatedShot = async (root, payload, at2) => {
  const png = decodePngDataUrl(payload.annotatedDataUrl);
  const path = shotPath(root, at2);
  await promises.mkdir(node_path.dirname(path), { recursive: true });
  await promises.writeFile(path, png);
  await appendFeedItem(root, {
    target: { kind: "file_line", value: payload.target ?? "screenshot" },
    note: payload.note,
    imageRef: path,
    at: new Date(at2).toISOString()
  });
  return path;
};
const captureUrl = async (root, url, at2) => {
  const win = new electron.BrowserWindow({ width: 1024, height: 768, show: false });
  try {
    await win.loadURL(url);
    await new Promise((resolve) => setTimeout(resolve, 400));
    const image = await win.webContents.capturePage();
    const path = shotPath(root, at2);
    await promises.mkdir(node_path.dirname(path), { recursive: true });
    await promises.writeFile(path, image.toPNG());
    return { dataUrl: image.toDataURL(), path };
  } finally {
    win.destroy();
  }
};
const factsForFile = (doc, file) => {
  for (const stage of doc.stages) {
    const found = stage.files.find((f) => f.path === file);
    if (!found) continue;
    const facts = [`stage ${stage.order + 1}: ${stage.moduleDir}`];
    if (found.reason) facts.push(found.reason);
    facts.push(`attention ${found.attention}${found.deprioritized ? " (deprioritized)" : ""}`);
    return facts;
  }
  return ["no Fallow signal for this file in the current review"];
};
const buildInspectorCard = (doc, sel) => ({
  file: sel.file,
  line: sel.line,
  component: sel.component ?? null,
  facts: doc ? factsForFile(doc, sel.file) : ["no review loaded yet"]
});
const INSPECT_PORT = 7787;
const SELECT_PATH = "/fallow-select";
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "POST, OPTIONS",
  "access-control-allow-headers": "content-type"
};
const startInspectServer = (getDoc, send, root) => {
  const server = node_http.createServer((req, res) => {
    if (req.method === "OPTIONS") {
      res.writeHead(204, CORS).end();
      return;
    }
    if (req.method !== "POST" || req.url !== SELECT_PATH) {
      res.writeHead(404, CORS).end();
      return;
    }
    let body = "";
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      void (async () => {
        try {
          const sel = JSON.parse(body);
          const card = buildInspectorCard(getDoc(), sel);
          send(card);
          await appendFeedItem(root, {
            target: { kind: "component", value: sel.component ?? `${sel.file}:${sel.line}` },
            note: `inspected ${sel.component ?? sel.file}`,
            at: (/* @__PURE__ */ new Date()).toISOString()
          });
          res.writeHead(200, { "content-type": "application/json", ...CORS }).end(JSON.stringify(card));
        } catch (err) {
          res.writeHead(400, CORS).end(String(err));
        }
      })();
    });
  });
  server.listen(INSPECT_PORT, "127.0.0.1");
  return server;
};
let mainWindow = null;
let latestDoc = null;
const createWindow = () => {
  const win = new electron.BrowserWindow({
    width: 1400,
    height: 900,
    title: "Fallow Review",
    backgroundColor: "#0e0c0a",
    webPreferences: {
      preload: node_path.join(__dirname, "../preload/index.js"),
      sandbox: false,
      webviewTag: true
    }
  });
  const devUrl = process.env["ELECTRON_RENDERER_URL"];
  if (devUrl) {
    void win.loadURL(devUrl);
  } else {
    void win.loadFile(node_path.join(__dirname, "../renderer/index.html"));
  }
  return win;
};
electron.ipcMain.handle("review:get", async (_event, root) => {
  latestDoc = await runReview(root);
  return latestDoc;
});
electron.ipcMain.handle("review:guide", (_event, root) => runGuide(root));
electron.ipcMain.handle("feed:append", (_event, item) => appendFeedItem(process.cwd(), item));
electron.ipcMain.handle(
  "review:validate",
  (_event, hash, items) => validateWalkthrough(buildAgentWalkthrough(hash, items))
);
electron.ipcMain.handle("shot:capture", (_event, url) => captureUrl(process.cwd(), url, Date.now()));
electron.ipcMain.handle(
  "shot:save",
  (_event, payload) => saveAnnotatedShot(process.cwd(), payload, Date.now())
);
void electron.app.whenReady().then(() => {
  mainWindow = createWindow();
  startInspectServer(
    () => latestDoc,
    (card) => mainWindow?.webContents.send("inspect:selection", card),
    process.cwd()
  );
  electron.app.on("activate", () => {
    if (electron.BrowserWindow.getAllWindows().length === 0) mainWindow = createWindow();
  });
});
electron.app.on("window-all-closed", () => {
  if (process.platform !== "darwin") electron.app.quit();
});
