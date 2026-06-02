import { ClipsService } from "./service";

// `useMemo` returns the factory's product directly, so `svc` is a ClipsService
// instance and `svc.analyze()` is a real use of ClipsService.analyze.
export function page(): string {
  const svc = useMemo(() => new ClipsService(), []);
  return svc.analyze();
}

// Minimal useMemo shim so the fixture is self-contained.
declare function useMemo<T>(factory: () => T, deps: unknown[]): T;
