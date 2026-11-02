export class TimeoutError extends Error {
  get displayDetail(): string {
    return "The request timed out.";
  }

  get unusedTimeoutDetail(): string {
    return "never read";
  }
}
