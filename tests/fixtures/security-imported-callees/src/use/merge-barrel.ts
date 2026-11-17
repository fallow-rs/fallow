import { merge } from "../barrel";

export const apply = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);
