import type { Merged } from "./star-barrel.js";
import type { Independent, NamedMerged } from "./named-barrel.js";

export type Public = Merged.Options;
export type NamedPublic = NamedMerged.Options;
export type IndependentPublic = Independent;
