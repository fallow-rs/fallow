import { merge } from "../merge";

export const apply = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);
