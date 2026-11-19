export function useRef<T>(value: T | null): { current: T | null } {
  return { current: value };
}

export function useEffect(effect: () => void | (() => void), _deps: unknown[]): void {
  effect();
}
