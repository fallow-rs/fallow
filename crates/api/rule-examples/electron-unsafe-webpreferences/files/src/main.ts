import { app, BrowserWindow } from "electron";

app.whenReady().then(() => {
  const window = new BrowserWindow({ webPreferences: { nodeIntegration: true } });
  window.loadURL("https://shop.internal/orders");
});
