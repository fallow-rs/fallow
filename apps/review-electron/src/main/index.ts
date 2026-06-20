import { join } from "node:path";
import { app, BrowserWindow, ipcMain } from "electron";
import { runReview, runGuide, validateWalkthrough } from "./review";
import { appendFeedItem } from "./feed";
import { buildAgentWalkthrough } from "./agentWalkthrough";
import type { FeedItem } from "../model/agent";

const createWindow = (): void => {
  const win = new BrowserWindow({
    width: 1400,
    height: 900,
    title: "Fallow Review",
    backgroundColor: "#0e0c0a",
    webPreferences: {
      preload: join(__dirname, "../preload/index.js"),
      sandbox: false,
    },
  });

  const devUrl = process.env["ELECTRON_RENDERER_URL"];
  if (devUrl) {
    void win.loadURL(devUrl);
  } else {
    void win.loadFile(join(__dirname, "../renderer/index.html"));
  }
};

ipcMain.handle("review:get", (_event, root: string | undefined) => runReview(root));
ipcMain.handle("review:guide", (_event, root: string | undefined) => runGuide(root));
ipcMain.handle("feed:append", (_event, item: FeedItem) => appendFeedItem(process.cwd(), item));
ipcMain.handle("review:validate", (_event, hash: string, items: FeedItem[]) =>
  validateWalkthrough(buildAgentWalkthrough(hash, items)),
);

void app.whenReady().then(() => {
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
