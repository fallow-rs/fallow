import { contextBridge, ipcRenderer } from "electron";
import type { WalkthroughDocument } from "../model/walkthrough";
import type { FeedItem, Guide, InlineFraming } from "../model/agent";
import type { Capture } from "../main/capture";
import type { SaveAnnotation } from "../main/shots";
import type { InspectorCard } from "../main/inspect";
import type { FileDiff } from "../main/diff";
import type { TradeOffRunResult } from "../main/agentRun";
import type { TradeOffValidation } from "../main/tradeoffValidation";
import type { TradeOffEnvelope } from "../model/tradeoff";
import type { ReviewContext } from "../model/reviewContext";
import type { AppConfig } from "../main/config";

const api = {
  getReview: (root?: string): Promise<WalkthroughDocument> =>
    ipcRenderer.invoke("review:get", root),
  getGuide: (root?: string): Promise<Guide> => ipcRenderer.invoke("review:guide", root),
  appendFeed: (item: FeedItem): Promise<void> => ipcRenderer.invoke("feed:append", item),
  getCapturedFraming: (): Promise<InlineFraming[]> => ipcRenderer.invoke("framing:captured"),
  getTradeoffs: (): Promise<TradeOffEnvelope | null> => ipcRenderer.invoke("tradeoffs:get"),
  validateTradeoffs: (): Promise<TradeOffValidation | null> =>
    ipcRenderer.invoke("tradeoffs:validate"),
  getReviewContext: (): Promise<ReviewContext | null> => ipcRenderer.invoke("reviewContext:get"),
  runTradeoffs: (id: string): Promise<TradeOffRunResult> => ipcRenderer.invoke("tradeoffs:run", id),
  capture: (url: string): Promise<Capture> => ipcRenderer.invoke("shot:capture", url),
  saveShot: (payload: SaveAnnotation): Promise<string> => ipcRenderer.invoke("shot:save", payload),
  getDiff: (base: string, file: string): Promise<FileDiff> =>
    ipcRenderer.invoke("diff:get", base, file),
  getAllDiffs: (base: string): Promise<{ patch: string }> => ipcRenderer.invoke("diff:all", base),
  getConfig: (): Promise<AppConfig> => ipcRenderer.invoke("config:get"),
  onInspectSelection: (cb: (card: InspectorCard) => void): void => {
    // Single consumer (the app shell). Drop any prior listener before adding, so a
    // StrictMode double-mount or hot-reload re-registration cannot accumulate.
    ipcRenderer.removeAllListeners("inspect:selection");
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
