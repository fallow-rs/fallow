import type { WalkthroughDocument } from "../model/walkthrough";
import { factsForFile } from "./enrich";

/** A component/element selection posted by the in-page picker. */
export type Selection = {
  file: string;
  line: number;
  column?: number;
  component?: string;
};

/** The grounded inspector card shown for a selection. */
export type InspectorCard = {
  file: string;
  line: number;
  component: string | null;
  facts: string[];
};

/** Join a picker selection to grounded Fallow facts from the latest review. */
export const buildInspectorCard = (
  doc: WalkthroughDocument | null,
  sel: Selection,
): InspectorCard => ({
  file: sel.file,
  line: sel.line,
  component: sel.component ?? null,
  facts: doc ? factsForFile(doc, sel.file) : ["no review loaded yet"],
});
