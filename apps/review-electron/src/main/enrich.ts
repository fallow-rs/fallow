import type { WalkthroughDocument } from "../model/walkthrough";

/**
 * Grounded facts for a selected file, derived from the latest review document:
 * its review stage, the focus reason, and the attention weight. This is what
 * turns a generic "go to source" inspector into a grounded one.
 */
export const factsForFile = (doc: WalkthroughDocument, file: string): string[] => {
  for (const stage of doc.stages) {
    const found = stage.files.find((f) => f.path === file);
    if (!found) continue;
    const facts = [`stage ${stage.order + 1}: ${stage.moduleDir}`];
    if (found.reason) facts.push(found.reason);
    facts.push(`attention ${found.attention}${found.deprioritized ? " (deprioritized)" : ""}`);
    return facts;
  }
  return ["no Fallow signal for this file in the current review"];
};
