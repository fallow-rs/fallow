export class BaseException {
  // Used only via `e instanceof BaseException` narrowing in app.ts: must be credited.
  getMessage(): string {
    return "base";
  }

  // Never called anywhere: must still report as an unused class member.
  unusedHelper(): void {}
}
