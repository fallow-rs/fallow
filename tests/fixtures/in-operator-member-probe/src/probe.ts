import { TimeoutError } from "./timeout";

export type ProbeResult = string | undefined;

export const detailOf = (cause: unknown): string | undefined =>
  typeof cause === "object" &&
  cause !== null &&
  "displayDetail" in cause &&
  typeof cause.displayDetail === "string"
    ? cause.displayDetail
    : undefined;

export const hasRetryHint = (cause: object): boolean => Reflect.has(cause, "retryHint");

export const timeout = (): TimeoutError => new TimeoutError();
