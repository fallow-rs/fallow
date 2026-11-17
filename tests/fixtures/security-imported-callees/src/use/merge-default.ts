import merge from "../merge-default";

export const apply = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);
