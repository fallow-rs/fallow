import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { FeedItem, Guide } from "../model/agent";
import type { Capture } from "../main/capture";
import type { SaveAnnotation } from "../main/shots";
import type { InspectorCard } from "../main/inspect";
import type { FileDiff } from "../main/diff";
import type { AgentBackend } from "../main/backends";
import type { AgentRunResult } from "../main/agentRun";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
  getGuide: (root?: string): Promise<Guide> => ipcRenderer.invoke("review:guide", root),
  appendFeed: (item: FeedItem): Promise<void> => ipcRenderer.invoke("feed:append", item),
  validate: (hash: string, items: FeedItem[]): Promise<unknown> =>
    ipcRenderer.invoke("review:validate", hash, items),
  capture: (url: string): Promise<Capture> => ipcRenderer.invoke("shot:capture", url),
  saveShot: (payload: SaveAnnotation): Promise<string> => ipcRenderer.invoke("shot:save", payload),
  getDiff: (base: string, file: string): Promise<FileDiff> =>
    ipcRenderer.invoke("diff:get", base, file),
  listBackends: (): Promise<AgentBackend[]> => ipcRenderer.invoke("agent:backends"),
  runAgent: (id: string): Promise<AgentRunResult> => ipcRenderer.invoke("agent:run", id),
  onInspectSelection: (cb: (card: InspectorCard) => void): void => {
    ipcRenderer.on("inspect:selection", (_event, card: InspectorCard) => cb(card));
  },
};

export type FallowApi = typeof api;

// contextBridge only works with contextIsolation on; fall back defensively.
if (process.contextIsolated) {
  try {
    contextBridge.exposeInMainWorld("fallow", api);
  } catch (error) {
    console.error(error);
  }
} else {
  (globalThis as unknown as { fallow: FallowApi }).fallow = api;
}
