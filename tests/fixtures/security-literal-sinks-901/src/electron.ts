import { BrowserWindow } from "electron";

export function openWindows(): void {
  new BrowserWindow({
    webPreferences: {
      nodeIntegration: true,
    },
  });
  new BrowserWindow({
    webPreferences: {
      webSecurity: false,
    },
  });
  new BrowserWindow({
    webPreferences: {
      contextIsolation: false,
    },
  });
}
