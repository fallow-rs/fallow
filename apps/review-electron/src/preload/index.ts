import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { FeedItem, Guide } from "../model/agent";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
  getGuide: (root?: string): Promise<Guide> => ipcRenderer.invoke("review:guide", root),
  appendFeed: (item: FeedItem): Promise<void> => ipcRenderer.invoke("feed:append", item),
  validate: (hash: string, items: FeedItem[]): Promise<unknown> =>
    ipcRenderer.invoke("review:validate", hash, items),
};

contextBridge.exposeInMainWorld("fallow", api);

export type FallowApi = typeof api;
