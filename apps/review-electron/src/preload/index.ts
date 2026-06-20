import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
};

contextBridge.exposeInMainWorld("fallow", api);

export type FallowApi = typeof api;
