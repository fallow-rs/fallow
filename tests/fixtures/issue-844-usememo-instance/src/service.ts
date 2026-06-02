export class ClipsService {
  // Called only via a useMemo-bound instance in app.ts: must be credited.
  analyze(): string {
    return "ok";
  }

  // Never called anywhere: must still report as an unused class member.
  unusedHelper(): void {}
}
