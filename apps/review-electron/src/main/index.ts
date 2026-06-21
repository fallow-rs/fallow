import { join } from "node:path";
import { app, BrowserWindow, ipcMain, session, type WebContents } from "electron";
import { electronApp, optimizer } from "@electron-toolkit/utils";
import { runReview, runGuide, validateWalkthrough } from "./review";
import { appendFeedItem } from "./feed";
import { buildAgentWalkthrough } from "./agentWalkthrough";
import { captureUrl } from "./capture";
import { saveAnnotatedShot, type SaveAnnotation } from "./shots";
import { getFileDiff } from "./diff";
import { startInspectServer } from "./inspectServer";
import type { FeedItem } from "../model/agent";
import type { WalkthroughDocument } from "../model/walkthrough";

const PROD_CSP =
  "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'";

const rendererDevUrl = (): string | undefined => process.env["ELECTRON_RENDERER_URL"];

let mainWindow: BrowserWindow | null = null;
let latestDoc: WalkthroughDocument | null = null;

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

ipcMain.handle("review:get", async (_event, root: string | undefined) => {
  latestDoc = await runReview(root);
  return latestDoc;
});
ipcMain.handle("review:guide", (_event, root: string | undefined) => runGuide(root));
ipcMain.handle("feed:append", (_event, item: FeedItem) => appendFeedItem(process.cwd(), item));
ipcMain.handle("review:validate", (_event, hash: string, items: FeedItem[]) =>
  validateWalkthrough(buildAgentWalkthrough(hash, items)),
);
ipcMain.handle("shot:capture", (_event, url: string) => captureUrl(process.cwd(), url, Date.now()));
ipcMain.handle("shot:save", (_event, payload: SaveAnnotation) =>
  saveAnnotatedShot(process.cwd(), payload, Date.now()),
);
ipcMain.handle("diff:get", (_event, base: string, file: string) =>
  getFileDiff(process.cwd(), base, file),
);

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
      process.cwd(),
    );

    app.on("activate", () => {
      if (BrowserWindow.getAllWindows().length === 0) mainWindow = createWindow();
    });
  });
}

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
