import { BrowserWindow } from "electron";

export function openWindows(): void {
  new BrowserWindow({
    webPreferences: {
      nodeIntegration: true,
      webSecurity: false,
      contextIsolation: false,
    },
  });
}
