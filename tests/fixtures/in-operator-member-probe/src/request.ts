import { detailOf } from "./probe";

export class RequestError extends Error {
  get displayDetail(): string {
    return "The request failed.";
  }

  get retryHint(): string {
    return "Try again later.";
  }

  get unusedRequestDetail(): string {
    return "never read";
  }
}

export const describe = (cause: unknown): string => detailOf(cause) ?? "unknown";
