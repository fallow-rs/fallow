import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { FeedItem, Guide, InlineFraming } from "../model/agent";
import type { Capture } from "../main/capture";
import type { SaveAnnotation } from "../main/shots";
import type { InspectorCard } from "../main/inspect";
import type { FileDiff } from "../main/diff";
import type { AgentBackend } from "../main/backends";
import type { AgentRunResult } from "../main/agentRun";
import type { AppConfig } from "../main/config";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
  getGuide: (root?: string): Promise<Guide> => ipcRenderer.invoke("review:guide", root),
  appendFeed: (item: FeedItem): Promise<void> => ipcRenderer.invoke("feed:append", item),
  getCapturedFraming: (): Promise<InlineFraming[]> => ipcRenderer.invoke("framing:captured"),
  validate: (hash: string, items: FeedItem[]): Promise<unknown> =>
    ipcRenderer.invoke("review:validate", hash, items),
  capture: (url: string): Promise<Capture> => ipcRenderer.invoke("shot:capture", url),
  saveShot: (payload: SaveAnnotation): Promise<string> => ipcRenderer.invoke("shot:save", payload),
  getDiff: (base: string, file: string): Promise<FileDiff> =>
    ipcRenderer.invoke("diff:get", base, file),
  getAllDiffs: (base: string): Promise<{ patch: string }> => ipcRenderer.invoke("diff:all", base),
  listBackends: (): Promise<AgentBackend[]> => ipcRenderer.invoke("agent:backends"),
  runAgent: (id: string): Promise<AgentRunResult> => ipcRenderer.invoke("agent:run", id),
  getConfig: (): Promise<AppConfig> => ipcRenderer.invoke("config:get"),
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
