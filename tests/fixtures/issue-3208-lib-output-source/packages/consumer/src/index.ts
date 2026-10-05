import { thing } from "@repro/uses-lib";
import { feature } from "@repro/uses-lib/feature";

export { relative } from "./relative";

export const doubled = thing * 2;
export const enabled = feature === "on";
