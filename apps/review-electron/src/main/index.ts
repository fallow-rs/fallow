import { join } from "node:path";
import { app, BrowserWindow, ipcMain } from "electron";
import { runReview } from "./review";

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

void app.whenReady().then(() => {
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
