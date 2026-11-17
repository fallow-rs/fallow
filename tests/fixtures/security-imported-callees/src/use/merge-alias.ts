import { assignAll as merge } from "../shallow";

export const apply = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);
