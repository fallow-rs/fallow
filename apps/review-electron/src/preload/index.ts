import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { FeedItem, Guide } from "../model/agent";
import type { Capture } from "../main/capture";
import type { SaveAnnotation } from "../main/shots";
import type { InspectorCard } from "../main/inspect";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
  getGuide: (root?: string): Promise<Guide> => ipcRenderer.invoke("review:guide", root),
  appendFeed: (item: FeedItem): Promise<void> => ipcRenderer.invoke("feed:append", item),
  validate: (hash: string, items: FeedItem[]): Promise<unknown> =>
    ipcRenderer.invoke("review:validate", hash, items),
  capture: (url: string): Promise<Capture> => ipcRenderer.invoke("shot:capture", url),
  saveShot: (payload: SaveAnnotation): Promise<string> => ipcRenderer.invoke("shot:save", payload),
  onInspectSelection: (cb: (card: InspectorCard) => void): void => {
    ipcRenderer.on("inspect:selection", (_event, card: InspectorCard) => cb(card));
  },
};

contextBridge.exposeInMainWorld("fallow", api);

export type FallowApi = typeof api;
