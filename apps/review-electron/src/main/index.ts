import { join } from "node:path";
import { watch } from "node:fs";
import { app, BrowserWindow, ipcMain, session, type WebContents } from "electron";
import { electronApp, optimizer } from "@electron-toolkit/utils";
import { loadConfig, configPath } from "./config";
import { runReview, runGuide } from "./review";
import { appendFeedItem } from "./feed";
import { readCapturedFraming } from "./capturedFraming";
import { captureUrl } from "./capture";
import { saveAnnotatedShot, type SaveAnnotation } from "./shots";
import { getFileDiff, getAllDiffs } from "./diff";
import { runTradeoffElicitation } from "./agentRun";
import { readPersistedTradeoffs } from "./tradeoffs";
import { validateTradeoffs } from "./tradeoffValidation";
import { readReviewContext } from "./reviewContext";
import { startInspectServer } from "./inspectServer";
import type { FeedItem } from "../model/agent";
import type { WalkthroughDocument } from "../model/walkthrough";

const PROD_CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'";

const rendererDevUrl = (): string | undefined => process.env["ELECTRON_RENDERER_URL"];

let mainWindow: BrowserWindow | null = null;
let latestDoc: WalkthroughDocument | null = null;
let appConfig = loadConfig();
// A config-provided binary wins over the ambient PATH for `fallow`.
if (appConfig.fallowBin) process.env["FALLOW_BIN"] = appConfig.fallowBin;

const createWindow = (): BrowserWindow => {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    show: false,
    title: "Fallow Review",
    backgroundColor: "#0e0c0a",
    webPreferences: {
      preload: join(__dirname, "../preload/index.js"),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      webviewTag: true,
    },
  });

  win.on("ready-to-show", () => win.show());

  const devUrl = rendererDevUrl();
  if (devUrl) {
    void win.loadURL(devUrl);
  } else {
    void win.loadFile(join(__dirname, "../renderer/index.html"));
  }
  return win;
};

// The repo under review. Defaults to the launch cwd; `FALLOW_REVIEW_ROOT` points
// the app at any checkout (e.g. a PR worktree) without a cwd dance.
const reviewRoot = (): string => process.env.FALLOW_REVIEW_ROOT?.trim() || process.cwd();

ipcMain.handle("review:get", async (_event, root: string | undefined) => {
  latestDoc = await runReview(root ?? reviewRoot());
  return latestDoc;
});
ipcMain.handle("review:guide", (_event, root: string | undefined) =>
  runGuide(root ?? reviewRoot()),
);
ipcMain.handle("feed:append", (_event, item: FeedItem) => appendFeedItem(reviewRoot(), item));
ipcMain.handle("framing:captured", () => readCapturedFraming(reviewRoot()));
ipcMain.handle("shot:capture", (_event, url: string) => captureUrl(reviewRoot(), url, Date.now()));
ipcMain.handle("shot:save", (_event, payload: SaveAnnotation) =>
  saveAnnotatedShot(reviewRoot(), payload, Date.now()),
);
ipcMain.handle("diff:get", (_event, base: string, file: string) =>
  getFileDiff(reviewRoot(), base, file),
);
ipcMain.handle("diff:all", (_event, base: string) => getAllDiffs(reviewRoot(), base));
ipcMain.handle("tradeoffs:get", () => readPersistedTradeoffs(reviewRoot()));
ipcMain.handle("tradeoffs:validate", () => validateTradeoffs(reviewRoot()));
ipcMain.handle("reviewContext:get", () => readReviewContext(reviewRoot()));
ipcMain.handle("tradeoffs:run", (_event, id: string) => runTradeoffElicitation(reviewRoot(), id));
ipcMain.handle("config:get", () => appConfig);

/**
 * Harden every webContents (security checklist): deny popups, block off-app
 * navigation for the app shell, and strip privileges from any attached <webview>.
 */
const hardenContents = (contents: WebContents): void => {
  contents.setWindowOpenHandler(() => ({ action: "deny" }));
  contents.on("will-attach-webview", (_event, webPreferences) => {
    delete webPreferences.preload;
    webPreferences.nodeIntegration = false;
    webPreferences.contextIsolation = true;
  });
  if (contents.getType() === "window") {
    contents.on("will-navigate", (event, url) => {
      const devUrl = rendererDevUrl();
      const allowed = devUrl ? url.startsWith(devUrl) : url.startsWith("file://");
      if (!allowed) event.preventDefault();
    });
  }
};

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on("second-instance", () => {
    if (!mainWindow) return;
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.focus();
  });

  void app.whenReady().then(() => {
    electronApp.setAppUserModelId("dev.fallow.review");
    app.on("browser-window-created", (_event, window) => optimizer.watchWindowShortcuts(window));
    app.on("web-contents-created", (_event, contents) => hardenContents(contents));

    // Deny all permission requests (camera, geolocation, notifications, ...).
    session.defaultSession.setPermissionRequestHandler((_wc, _permission, callback) =>
      callback(false),
    );
    session.defaultSession.setPermissionCheckHandler(() => false);

    // Strict CSP for the loaded-from-file app document; the vite dev server and
    // the <webview> guest (external review targets) are left untouched.
    if (!rendererDevUrl()) {
      session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
        if (details.url.startsWith("file://")) {
          callback({
            responseHeaders: { ...details.responseHeaders, "Content-Security-Policy": [PROD_CSP] },
          });
          return;
        }
        callback({ responseHeaders: details.responseHeaders });
      });
    }

    mainWindow = createWindow();
    startInspectServer(
      () => latestDoc,
      (card) => mainWindow?.webContents.send("inspect:selection", card),
      reviewRoot(),
      appConfig.inspectPort,
    );

    // Hot-reload the JSONC config on change (best-effort; ignored if absent).
    try {
      watch(configPath(), () => {
        appConfig = loadConfig();
      });
    } catch {
      /* no config file to watch */
    }

    app.on("activate", () => {
      if (BrowserWindow.getAllWindows().length === 0) mainWindow = createWindow();
    });
  });
}

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
