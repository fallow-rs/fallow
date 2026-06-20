import { join } from "node:path";
import { app, BrowserWindow, ipcMain } from "electron";
import { runReview, runGuide, validateWalkthrough } from "./review";
import { appendFeedItem } from "./feed";
import { buildAgentWalkthrough } from "./agentWalkthrough";
import { captureUrl } from "./capture";
import { saveAnnotatedShot, type SaveAnnotation } from "./shots";
import { startInspectServer } from "./inspectServer";
import type { FeedItem } from "../model/agent";
import type { WalkthroughDocument } from "../model/walkthrough";

let mainWindow: BrowserWindow | null = null;
let latestDoc: WalkthroughDocument | null = null;

const createWindow = (): BrowserWindow => {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    title: "Fallow Review",
    backgroundColor: "#0e0c0a",
    webPreferences: {
      preload: join(__dirname, "../preload/index.js"),
      sandbox: false,
      webviewTag: true,
    },
  });

  const devUrl = process.env["ELECTRON_RENDERER_URL"];
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

void app.whenReady().then(() => {
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

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
