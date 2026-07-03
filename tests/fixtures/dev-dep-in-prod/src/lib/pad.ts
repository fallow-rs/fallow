// Production helper reachable from the entry point. Uses a real dependency
// (`left-pad`) at runtime and a devDependency (`type-fest`) TYPE-ONLY, which
// must NOT be flagged because type imports are erased at build time.
import leftPad from "left-pad";
import type { Simplify } from "type-fest";

export type Padded = Simplify<{ value: string }>;

export const pad = (value: unknown): string => leftPad(String(value), 4);
