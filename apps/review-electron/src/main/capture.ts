import { BrowserWindow } from "electron";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { shotPath } from "./shots";
import { describeLoadError } from "./errors";

export type Capture = { dataUrl: string; path: string };

/**
 * Load `url` in a hidden window and capture it to a PNG under `.fallow-review/
 * shots/`. Returns a data URL (for the renderer canvas) + the saved path.
 */
export const captureUrl = async (root: string, url: string, at: number): Promise<Capture> => {
  const win = new BrowserWindow({ width: 1024, height: 768, show: false });
  try {
    await win.loadURL(url).catch((e: unknown) => {
      throw describeLoadError(e, url);
    });
    await new Promise<void>((resolve) => setTimeout(resolve, 400));
    const image = await win.webContents.capturePage();
    const path = shotPath(root, at);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, image.toPNG());
    return { dataUrl: image.toDataURL(), path };
  } finally {
    win.destroy();
  }
};
