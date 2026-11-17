import { merge } from "../merge";
import lodashMerge from "lodash.merge";

export const shallow = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  merge(base, input);

export function apply(base: Record<string, unknown>, input: Record<string, unknown>): unknown {
  const merge = lodashMerge;
  return merge(base, input);
}
