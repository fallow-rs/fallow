import { merge } from "../merge";

export const shallow = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);

export const apply = (
  merge: (a: Record<string, unknown>, b: Record<string, unknown>) => unknown,
  base: Record<string, unknown>,
  input: Record<string, unknown>,
): unknown => merge(base, input);
