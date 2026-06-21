import type { WalkthroughFile } from "../../../model/walkthrough";

/** Visual weight for the fan-in metric: a hub (many importers) draws the eye. */
export type SignalTone = "hub" | "elevated" | "muted";

/**
 * Structured review signal for one file, parsed from the engine's focus score
 * and reason. Fan-in (how many modules import this file) is the blast-radius
 * signal worth surfacing; fan-out and "isolated" are low-signal and recede.
 */
export type FileSignal = {
  fanIn: number;
  fanOut: number;
  security: boolean;
  riskZone: boolean;
  deprioritized: boolean;
  isolated: boolean;
  fanInTone: SignalTone;
};

const IMPORTERS = /(\d+)\s+importers?/;
const FAN_OUT = /fan-out\s+(\d+)/;
const HUB = 6;
const ELEVATED = 2;

/** Grade a fan-in count so only genuine hubs earn an accent color. */
export const fanInTone = (fanIn: number): SignalTone =>
  fanIn >= HUB ? "hub" : fanIn >= ELEVATED ? "elevated" : "muted";

/** Deterministic per-file signal derived purely from the Fallow focus entry. */
export const deriveFileSignal = (file: WalkthroughFile): FileSignal => {
  const importers = IMPORTERS.exec(file.reason);
  const fanOut = FAN_OUT.exec(file.reason);
  const fanIn = importers ? Number(importers[1]) : 0;
  return {
    fanIn,
    fanOut: fanOut ? Number(fanOut[1]) : 0,
    security: file.score.securityTaint > 0,
    riskZone: file.score.riskZone > 0,
    deprioritized: file.deprioritized,
    isolated: /isolated change/.test(file.reason),
    fanInTone: fanInTone(fanIn),
  };
};
