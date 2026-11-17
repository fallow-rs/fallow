import * as utils from "../merge";

export const apply = (base: Record<string, unknown>, input: Record<string, unknown>): unknown =>
  utils.merge(base, input);
